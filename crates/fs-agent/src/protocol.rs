//! Validation du contrat `client-tools.md` : nombre d'appels, unicité des
//! `callId`, troncature des sorties et budget cumulé.

use fs_api::{ToolCall, ToolResult};

use crate::model::PendingCall;
use crate::policy::LoopPolicy;

/// Erreur de protocole : le run passe en `Failed`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("{count} appels d'outils dans un tour (maximum {max}).")]
    TooManyCalls { count: usize, max: usize },
    #[error("appel d'outil sans `callId` (index {index}).")]
    MissingCallId { index: usize },
    #[error("`callId` dupliqué : {0}.")]
    DuplicateCallId(String),
    #[error("{got} résultats pour {expected} appels.")]
    ResultCount { expected: usize, got: usize },
    #[error("résultat sans appel correspondant : {0}.")]
    ExtraResult(String),
    #[error("appel sans résultat : {0}.")]
    MissingResult(String),
}

/// Vérifie les appels reçus dans `completed.message.toolCalls`.
pub fn validate_calls(calls: &[ToolCall], max: usize) -> Result<(), ProtocolError> {
    if calls.len() > max {
        return Err(ProtocolError::TooManyCalls {
            count: calls.len(),
            max,
        });
    }
    let mut seen = std::collections::HashSet::with_capacity(calls.len());
    for (index, call) in calls.iter().enumerate() {
        if call.call_id.trim().is_empty() {
            return Err(ProtocolError::MissingCallId { index });
        }
        if !seen.insert(call.call_id.as_str()) {
            return Err(ProtocolError::DuplicateCallId(call.call_id.clone()));
        }
    }
    Ok(())
}

/// Tronque à `max` caractères (pas octets), en signalant la coupure.
pub fn clip_chars(input: &str, max: usize) -> (String, bool) {
    if input.chars().count() <= max {
        return (input.to_string(), false);
    }
    (input.chars().take(max).collect(), true)
}

/// Construit les résultats à envoyer : une réponse par appel, dans l'ordre,
/// tronquée à 40 k puis au budget cumulé de 120 k.
pub fn finalize_results(pending: &[PendingCall], policy: &LoopPolicy) -> Vec<ToolResult> {
    let mut out = Vec::with_capacity(pending.len());
    let mut used = 0usize;
    for call in pending {
        let raw = call.output.clone().unwrap_or_default();
        let (mut text, cut) = clip_chars(&raw, policy.max_output_chars);
        if cut {
            text.push_str(&format!(
                "\n[sortie tronquée à {} caractères]",
                policy.max_output_chars
            ));
        }

        let remaining = policy.max_results_chars.saturating_sub(used);
        if text.chars().count() > remaining {
            let marker = "\n[budget global de 120000 caractères atteint]";
            let marker_len = marker.chars().count();
            let keep = remaining.saturating_sub(marker_len);
            text = text.chars().take(keep).collect();
            if remaining >= marker_len {
                text.push_str(marker);
            }
        }
        used += text.chars().count();
        out.push(ToolResult {
            call_id: call.call_id.clone(),
            output: text,
            is_error: call.is_error,
        });
    }
    out
}

/// Vérifie qu'un envoi répond exactement une fois à chaque appel.
pub fn verify_results(calls: &[ToolCall], results: &[ToolResult]) -> Result<(), ProtocolError> {
    if calls.len() != results.len() {
        return Err(ProtocolError::ResultCount {
            expected: calls.len(),
            got: results.len(),
        });
    }
    let mut seen = std::collections::HashSet::with_capacity(results.len());
    for result in results {
        if !seen.insert(result.call_id.as_str()) {
            return Err(ProtocolError::DuplicateCallId(result.call_id.clone()));
        }
    }
    for call in calls {
        if !seen.contains(call.call_id.as_str()) {
            return Err(ProtocolError::MissingResult(call.call_id.clone()));
        }
    }
    if let Some(extra) = results.iter().find(|r| {
        !calls
            .iter()
            .any(|c| c.call_id.as_str() == r.call_id.as_str())
    }) {
        return Err(ProtocolError::ExtraResult(extra.call_id.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            call_id: id.to_string(),
            name: name.to_string(),
            arguments: json!({}),
        }
    }

    #[test]
    fn rejects_too_many_calls() {
        let calls: Vec<ToolCall> = (0..33)
            .map(|i| call(&format!("c{i}"), "read_file"))
            .collect();
        assert!(matches!(
            validate_calls(&calls, 32),
            Err(ProtocolError::TooManyCalls { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_ids() {
        let calls = vec![call("a", "read_file"), call("a", "read_file")];
        assert!(matches!(
            validate_calls(&calls, 32),
            Err(ProtocolError::DuplicateCallId(_))
        ));
    }

    #[test]
    fn clips_single_output() {
        let policy = LoopPolicy::default();
        let mut pending = vec![PendingCall::new("a".into(), "read_file".into(), json!({}))];
        pending[0].output = Some("x".repeat(50_000));
        let results = finalize_results(&pending, &policy);
        assert!(results[0].output.chars().count() <= 40_000 + 80);
        assert!(results[0].output.contains("tronquée"));
    }

    #[test]
    fn respects_total_budget() {
        let policy = LoopPolicy::default();
        let pending: Vec<PendingCall> = (0..4)
            .map(|i| {
                let mut p = PendingCall::new(format!("c{i}"), "read_file".into(), json!({}));
                p.output = Some("x".repeat(40_000));
                p
            })
            .collect();
        let results = finalize_results(&pending, &policy);
        let total: usize = results.iter().map(|r| r.output.chars().count()).sum();
        assert!(total <= 120_000, "total = {total}");
    }

    #[test]
    fn verifies_complete_answer_set() {
        let calls = vec![call("a", "read_file"), call("b", "read_file")];
        let results = vec![
            ToolResult {
                call_id: "a".into(),
                output: String::new(),
                is_error: false,
            },
            ToolResult {
                call_id: "b".into(),
                output: String::new(),
                is_error: false,
            },
        ];
        assert!(verify_results(&calls, &results).is_ok());
    }
}
