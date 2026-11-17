//! Décision d'autorisation avant l'exécution d'un outil.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Risk;

/// Demande soumise à l'approbateur.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub run_id: String,
    pub call_id: String,
    pub name: String,
    pub args: Value,
    pub workdir: Option<PathBuf>,
    pub auto_accept: bool,
    pub risk: Risk,
}

/// Décision de l'approbateur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    /// Exécuter sans demander.
    Allow,
    /// Demander à l'utilisateur.
    Ask,
    /// Refuser avec un message renvoyé au modèle.
    Deny(String),
}

/// Politique d'approbation : les modes de l'application vivent dans l'implémentation.
#[async_trait]
pub trait Approver: Send + Sync {
    async fn decide(&self, request: &ApprovalRequest) -> Approval;

    /// Appelé seulement si `decide` a renvoyé `Ask`.
    async fn confirm(&self, request: &ApprovalRequest) -> bool;

    /// Seuil de requêtes atteint dans un run : `false` arrête proprement le run.
    /// L'implémentation par défaut continue (l'interface a déjà été avertie).
    async fn confirm_budget(&self, _run_id: &str, _requests: u32) -> bool {
        true
    }
}

/// Approuve tout (tests, automatisation explicite).
pub struct ApproveAll;

#[async_trait]
impl Approver for ApproveAll {
    async fn decide(&self, _request: &ApprovalRequest) -> Approval {
        Approval::Allow
    }

    async fn confirm(&self, _request: &ApprovalRequest) -> bool {
        true
    }
}

/// Refuse tout (tests, mode strict).
pub struct DenyAll;

#[async_trait]
impl Approver for DenyAll {
    async fn decide(&self, _request: &ApprovalRequest) -> Approval {
        Approval::Deny("Refusé.".to_string())
    }

    async fn confirm(&self, _request: &ApprovalRequest) -> bool {
        false
    }
}
