//! Compatibilité avec les anciens chats : retrait à l'affichage du bloc
//! `[Contexte client]` que les versions précédentes joignaient au message.
//!
//! L'application n'injecte plus aucun contexte dans les messages : les MCP
//! passent par le tunnel France Student et le contexte par les instructions
//! personnalisées du compte.

use serde_json::Value;

/// Début de l'ancien bloc de contexte.
const CONTEXT_PREFIX: &str = "[Contexte client]";
/// Séparateur entre l'ancien bloc et le message de l'utilisateur.
const CONTEXT_SEP: &str = "\n\n---\n";

/// Si `cmd` est une commande MCP (`mcp …` ou `fs-mcp …`), renvoie ce qui suit le préfixe.
/// Conservé pour lire les anciennes entrées du journal des modifications.
pub fn mcp_rest(cmd: &str) -> Option<&str> {
    let cmd = cmd.trim();
    for prefix in ["fs-mcp", "mcp"] {
        if let Some(rest) = cmd.strip_prefix(prefix) {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return Some(rest.trim());
            }
        }
    }
    None
}

fn strip_context_text(s: &str) -> Option<&str> {
    let t = s.trim_start();
    if !t.starts_with(CONTEXT_PREFIX) {
        return None;
    }
    t.find(CONTEXT_SEP).map(|i| &t[i + CONTEXT_SEP.len()..])
}

/// Retire, dans tout un JSON (chat, liste de chats…), l'ancien bloc `[Contexte client]`.
pub fn strip_context(v: &mut Value) {
    match v {
        Value::String(s) => {
            if let Some(cleaned) = strip_context_text(s).map(str::to_string) {
                *s = cleaned;
            }
        }
        Value::Array(a) => {
            for x in a {
                strip_context(x);
            }
        }
        Value::Object(o) => {
            for x in o.values_mut() {
                strip_context(x);
            }
        }
        _ => {}
    }
}
