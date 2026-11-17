//! Types publics de la boucle d'agent : requête, état persisté, événements.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fs_api::StreamOptions;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const RUN_STATE_VERSION: u32 = 1;

/// Demande de lancement d'un run.
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub run_id: String,
    pub chat_id: Option<String>,
    pub agent_id: i64,
    /// Texte exact saisi par l'utilisateur : jamais modifié par l'application.
    pub content: String,
    pub workdir: Option<PathBuf>,
    pub auto_accept: bool,
    pub eco: bool,
    /// Catalogue `clientTools` proposé au modèle (vide hors mode Code).
    pub tools: Vec<String>,
    pub options: StreamOptions,
}

/// État d'un run. Toute transition est persistée avant l'action suivante.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Preparing,
    Streaming,
    AwaitingTools,
    Executing,
    SendingResults,
    Resuming,
    Done,
    Failed,
    Cancelled,
    Abandoned,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunStatus::Done | RunStatus::Failed | RunStatus::Cancelled | RunStatus::Abandoned
        )
    }
}

/// Décision prise pour un appel d'outil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allowed,
    Denied,
    Unknown,
}

/// Appel d'outil en attente ou exécuté, conservé pour la reprise.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCall {
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
    #[serde(default)]
    pub decision: Option<Decision>,
    /// Posé juste avant l'exécution : un `started_at` sans `output` signifie que
    /// l'outil a peut-être eu un effet de bord. La reprise ne le relance jamais.
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub executed_at: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub denied: bool,
}

impl PendingCall {
    pub fn new(call_id: String, name: String, arguments: Value) -> Self {
        Self {
            call_id,
            name,
            arguments,
            decision: None,
            started_at: None,
            executed_at: None,
            output: None,
            is_error: false,
            denied: false,
        }
    }
}

/// Compteurs cumulés d'un run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Usage {
    pub api_calls: u32,
    pub prompt_tokens: u64,
    pub output_tokens: u64,
}

/// État complet persisté dans `<config>/runs/<run_id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub version: u32,
    pub run_id: String,
    #[serde(default)]
    pub chat_id: Option<String>,
    pub agent_id: i64,
    pub created_at: String,
    pub updated_at: String,
    pub status: RunStatus,
    #[serde(default)]
    pub step: u32,
    #[serde(default)]
    pub last_event_id: Option<String>,
    pub user_content: String,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub auto_accept: bool,
    #[serde(default)]
    pub eco: bool,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub images: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub enable_web_search: bool,
    #[serde(default)]
    pub pending_calls: Vec<PendingCall>,
    #[serde(default)]
    pub results_sent: bool,
    /// Le seuil de requêtes a déjà été signalé (une seule fois par run).
    #[serde(default)]
    pub budget_warned: bool,
    #[serde(default)]
    pub usage: Usage,
}

impl RunState {
    pub fn new(req: &RunRequest, now: String) -> Self {
        Self {
            version: RUN_STATE_VERSION,
            run_id: req.run_id.clone(),
            chat_id: req.chat_id.clone(),
            agent_id: req.agent_id,
            created_at: now.clone(),
            updated_at: now,
            status: RunStatus::Preparing,
            step: 0,
            last_event_id: None,
            user_content: req.content.clone(),
            workdir: req.workdir.as_ref().map(|p| p.display().to_string()),
            auto_accept: req.auto_accept,
            eco: req.eco,
            tools: req.tools.clone(),
            images: req.options.images.clone(),
            files: req.options.files.clone(),
            enable_web_search: req.options.enable_web_search,
            pending_calls: Vec::new(),
            results_sent: false,
            budget_warned: false,
            usage: Usage::default(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now_iso();
    }
}

/// Origine d'un appel d'outil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Outil local du catalogue `client-tools`.
    Local,
    /// Outil de plateforme (`mcp__…`) : jamais exécuté par la boucle client.
    Mcp,
}

/// Événements émis vers l'interface. Aucun ne contient d'instruction pour le modèle.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Opened { chat_id: String },
    Title { chat_id: String, title: String },
    Delta { text: String },
    Reasoning { text: String },
    ToolCall {
        call_id: String,
        name: String,
        origin: Origin,
        args: Value,
        needs_confirm: bool,
    },
    ToolResult {
        call_id: String,
        output: String,
        is_error: bool,
        denied: bool,
    },
    Notice { text: String },
    AwaitingTools { run_id: String, calls: usize },
    Resumed { run_id: String },
    RunInterrupted { run_id: String, reason: String },
    /// Consommation de contexte du chat après un tour (`GET /chats/{id}/usage`).
    Usage {
        chat_id: String,
        percent: Option<f64>,
        used_tokens: Option<u64>,
    },
    /// Seuil de requêtes atteint : l'interface peut demander « continuer ? ».
    BudgetWarning { run_id: String, requests: u32 },
    Done {
        chat_id: String,
        message_id: String,
        prompt_tokens: Option<u64>,
        output_tokens: Option<u64>,
        total_prompt_tokens: u64,
        total_output_tokens: u64,
        api_calls: u32,
        eco: bool,
    },
    Failed {
        code: String,
        message: String,
        retryable: bool,
    },
    Cancelled,
}

/// Drapeau d'annulation partagé entre la commande Tauri et la boucle.
#[derive(Clone, Default)]
pub struct RunControl {
    flag: Arc<AtomicBool>,
}

impl RunControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_flag(flag: Arc<AtomicBool>) -> Self {
        Self { flag }
    }

    pub fn flag(&self) -> Arc<AtomicBool> {
        self.flag.clone()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Horodatage ISO-8601 UTC, seconde près.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
