//! Limites et seuils d'un run.

/// Limite de résultats imposée par `client-tools.md`.
pub const MAX_RESULTS_PER_TURN: usize = 32;
/// Limite de caractères d'une sortie d'outil.
pub const MAX_OUTPUT_CHARS: usize = 40_000;
/// Limite cumulée d'un envoi de résultats.
pub const MAX_RESULTS_CHARS: usize = 120_000;

/// Politique appliquée par la boucle.
#[derive(Debug, Clone)]
pub struct LoopPolicy {
    /// Nombre maximal de tours d'outils pour un message utilisateur.
    pub max_steps: u32,
    pub max_results_per_turn: usize,
    pub max_output_chars: usize,
    pub max_results_chars: usize,
    /// Confirmer aussi les outils en lecture seule (désactivé pour les outils locaux).
    pub confirm_read_only: bool,
    /// Nombre de reprises de flux autorisées sur une erreur réessayable.
    pub max_retries: u32,
    /// `STREAM_ENDED` n'est repris qu'une fois (puis échec).
    pub max_stream_ended_retries: u32,
    /// À partir de ce nombre de requêtes dans un run, demander « continuer ? ».
    pub budget_warning_at: u32,
    /// Délai de base du backoff (doublé à chaque tentative).
    pub retry_delay_ms: u64,
    pub eco: bool,
}

impl Default for LoopPolicy {
    fn default() -> Self {
        Self {
            max_steps: 30,
            max_results_per_turn: MAX_RESULTS_PER_TURN,
            max_output_chars: MAX_OUTPUT_CHARS,
            max_results_chars: MAX_RESULTS_CHARS,
            confirm_read_only: false,
            max_retries: 3,
            max_stream_ended_retries: 1,
            budget_warning_at: 10,
            retry_delay_ms: 1_000,
            eco: false,
        }
    }
}

impl LoopPolicy {
    pub fn eco() -> Self {
        Self {
            max_steps: 10,
            max_output_chars: 5_000,
            budget_warning_at: 8,
            eco: true,
            ..Self::default()
        }
    }

    pub fn for_mode(eco: bool) -> Self {
        if eco {
            Self::eco()
        } else {
            Self::default()
        }
    }
}
