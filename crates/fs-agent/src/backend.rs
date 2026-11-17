//! Abstraction du transport : un « tour » de conversation (démarrage, résultats
//! d'outils ou reprise de flux). L'implémentation HTTP vit dans l'application.

use async_trait::async_trait;
use fs_api::{StreamOptions, ToolCall, ToolResult};

use crate::model::{AgentEvent, RunControl};

/// Récepteur d'événements. `false` = l'appelant a annulé : l'implémentation peut
/// cesser d'émettre, mais doit terminer proprement.
pub trait EventSink: Send {
    fn emit(&mut self, event: AgentEvent) -> bool;
}

impl<F> EventSink for F
where
    F: FnMut(AgentEvent) -> bool + Send,
{
    fn emit(&mut self, event: AgentEvent) -> bool {
        self(event)
    }
}

/// Ce qu'il faut envoyer au serveur pour ce tour.
#[derive(Debug, Clone)]
pub enum TurnInput {
    /// Premier tour d'un run : crée le chat si `chat_id` est absent.
    Start {
        chat_id: Option<String>,
        agent_id: i64,
        content: String,
        tools: Vec<String>,
        options: StreamOptions,
    },
    /// Renvoi des résultats du tour d'outils précédent.
    Results {
        chat_id: String,
        results: Vec<ToolResult>,
        options: StreamOptions,
    },
    /// Ré-attachement au flux d'une génération en cours.
    Resume {
        chat_id: String,
        last_event_id: Option<String>,
    },
}

/// Fin logique d'un tour.
#[derive(Debug, Clone)]
pub enum Terminal {
    Completed,
    Cancelled,
    Failed {
        code: String,
        message: String,
        retryable: bool,
    },
}

/// Résultat agrégé d'un tour, en plus des événements déjà émis.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub chat_id: Option<String>,
    pub message_id: Option<String>,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub prompt_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub terminal: Terminal,
    pub last_event_id: Option<String>,
}

/// Erreur de transport ou de protocole HTTP.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    #[error("HTTP {status} : {body}")]
    Status { status: u16, body: String },
    #[error("réseau : {0}")]
    Transport(String),
    #[error("flux SSE : {0}")]
    Stream(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Other(String),
}

impl BackendError {
    /// Une reprise de flux est-elle pertinente ?
    pub fn retryable(&self) -> bool {
        match self {
            BackendError::Transport(_) | BackendError::Stream(_) => true,
            BackendError::Status { status, .. } => matches!(status, 408 | 425 | 500..=599),
            BackendError::Unsupported(_) | BackendError::Other(_) => false,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            BackendError::Status { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// Consommation de contexte d'un chat (`GET /chats/{id}/usage`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ContextUsage {
    /// `contextUsagePercent` (0–100) si fourni.
    pub percent: Option<f64>,
    pub used_tokens: Option<u64>,
    pub max_tokens: Option<u64>,
}

/// Vue minimale d'un chat pour réconcilier un run interrompu.
#[derive(Debug, Clone, Default)]
pub struct ChatSnapshot {
    pub pending_tool_calls: Vec<ToolCall>,
    pub last_event_id: Option<String>,
}

/// Transport de conversation.
#[async_trait]
pub trait ChatBackend: Send + Sync {
    /// Un tour complet. L'implémentation doit interrompre la lecture du flux dès
    /// que `control` est annulé et terminer par `Terminal::Cancelled`.
    async fn turn(
        &self,
        input: &TurnInput,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<TurnOutcome, BackendError>;

    async fn cancel(&self, chat_id: &str) -> Result<(), BackendError> {
        let _ = chat_id;
        Ok(())
    }

    async fn fetch_chat(&self, chat_id: &str) -> Result<ChatSnapshot, BackendError> {
        let _ = chat_id;
        Ok(ChatSnapshot::default())
    }

    /// Consommation de contexte du chat, si le transport sait la lire.
    async fn chat_usage(&self, chat_id: &str) -> Result<Option<ContextUsage>, BackendError> {
        let _ = chat_id;
        Ok(None)
    }
}
