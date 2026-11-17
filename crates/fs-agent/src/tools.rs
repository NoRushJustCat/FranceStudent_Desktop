//! Catalogue et exécution des outils locaux.

use std::path::PathBuf;

use async_trait::async_trait;
use fs_api::ToolCall;
use serde_json::Value;

/// Niveau de risque d'un outil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Lecture seule : jamais de confirmation.
    ReadOnly,
    /// Modifie des fichiers : confirmation selon le mode.
    Mutating,
    /// Exécute une commande : confirmation selon le mode.
    Command,
}

/// Description d'un outil local.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub risk: Risk,
}

impl ToolSpec {
    pub fn new(name: impl Into<String>, risk: Risk) -> Self {
        Self {
            name: name.into(),
            risk,
        }
    }
}

/// Résultat d'exécution d'un outil.
#[derive(Debug, Clone, Default)]
pub struct ToolExecution {
    pub output: String,
    pub is_error: bool,
    /// Information à afficher dans l'interface (journal des modifications).
    pub notice: Option<String>,
}

impl ToolExecution {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
            notice: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            output: message.into(),
            is_error: true,
            notice: None,
        }
    }
}

/// Contexte d'exécution.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub run_id: String,
    pub call_id: String,
    pub workdir: Option<PathBuf>,
}

/// Exécuteur d'outils locaux.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// `None` = nom hors catalogue : la boucle répond `isError: true`.
    fn spec(&self, name: &str) -> Option<ToolSpec>;

    async fn execute(&self, call: &ToolCall, ctx: &ToolContext) -> ToolExecution;
}

/// Arguments d'un appel : objet JSON ou chaîne JSON selon le fournisseur.
pub fn parse_arguments(raw: &Value) -> Value {
    match raw {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.clone(),
    }
}
