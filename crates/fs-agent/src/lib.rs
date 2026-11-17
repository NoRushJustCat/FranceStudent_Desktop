//! Boucle agent : stream → toolCalls → confirmation → toolResults.
//!
//! Le crate ne dépend ni de Tauri ni du réseau : le transport est fourni par
//! l'implémentation de [`backend::ChatBackend`]. Le contexte FS Code n'est
//! jamais ajouté au texte utilisateur.

pub mod approval;
pub mod backend;
pub mod model;
pub mod policy;
pub mod protocol;
pub mod store;
pub mod tools;

mod r#loop;

pub use approval::{Approval, ApprovalRequest, ApproveAll, Approver, DenyAll};
pub use backend::{
    BackendError, ChatBackend, ChatSnapshot, ContextUsage, EventSink, Terminal, TurnInput,
    TurnOutcome,
};
pub use model::{
    now_iso, AgentEvent, Decision, Origin, PendingCall, RunControl, RunRequest, RunState, RunStatus,
    Usage, RUN_STATE_VERSION,
};
pub use policy::{
    LoopPolicy, MAX_OUTPUT_CHARS, MAX_RESULTS_CHARS, MAX_RESULTS_PER_TURN,
};
pub use protocol::ProtocolError;
pub use r#loop::AgentLoop;
pub use store::{FsRunStore, RunStore, StoreError};
pub use tools::{Risk, ToolContext, ToolExecution, ToolExecutor, ToolSpec};

/// Erreur fatale de la boucle (stockage, run introuvable, etc.).
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("transport : {0}")]
    Backend(#[from] BackendError),
    #[error("protocole : {0}")]
    Protocol(#[from] ProtocolError),
    #[error("stockage : {0}")]
    Store(#[from] StoreError),
    #[error("run introuvable : {0}")]
    NotFound(String),
    #[error("{0}")]
    Other(String),
}
