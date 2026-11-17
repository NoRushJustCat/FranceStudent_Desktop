//! Adaptateurs Tauri de la boucle `fs-agent` : transport HTTP, outils locaux,
//! confirmations UI, persistance et traduction des événements.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use fs_agent::{
    AgentEvent, AgentLoop, Approval, ApprovalRequest, Approver, BackendError, ChatBackend,
    ChatSnapshot, ContextUsage, EventSink, FsRunStore, LoopPolicy, Origin, Risk, RunControl,
    RunRequest, RunState, RunStore, Terminal, ToolContext, ToolExecution, ToolExecutor, ToolSpec,
    TurnInput, TurnOutcome,
};
use fs_api::{ChatEvent, ToolCall};
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::{ask, changelog, client, config_dir, perm, term, AppState, UiEvent};

// ---------- sortie vers l'interface ----------

fn preview(s: &str) -> String {
    if s.chars().count() > 4000 {
        let mut t: String = s.chars().take(4000).collect();
        t.push_str("\n[…]");
        t
    } else {
        s.to_string()
    }
}

fn display_args(args: &Value) -> Value {
    let mut a = args.clone();
    if let Some(o) = a.as_object_mut() {
        for (from, to) in [
            ("old_text", "old_string"),
            ("new_text", "new_string"),
            ("query", "pattern"),
        ] {
            if let Some(v) = o.get(from).cloned() {
                o.entry(to).or_insert(v);
            }
        }
    }
    a
}

/// Émet les événements vers le canal Tauri et tient le journal des modifications.
pub struct TauriSink<'a> {
    app: &'a AppHandle,
    channel: &'a Channel<UiEvent>,
    control: RunControl,
    workdir: Option<PathBuf>,
    calls: HashMap<String, (String, Value)>,
}

impl<'a> TauriSink<'a> {
    pub fn new(
        app: &'a AppHandle,
        channel: &'a Channel<UiEvent>,
        control: RunControl,
        workdir: Option<PathBuf>,
    ) -> Self {
        Self {
            app,
            channel,
            control,
            workdir,
            calls: HashMap::new(),
        }
    }

    fn finish_tool(&mut self, call_id: &str, output: &str, is_error: bool, denied: bool) {
        let Some((name, args)) = self.calls.remove(call_id) else {
            return;
        };
        if name == "run_command" {
            if let Some(command) = args["command"].as_str() {
                term::emit_ai(
                    self.app,
                    command,
                    &clip(output, 20_000),
                    is_error,
                    self.workdir.as_deref(),
                );
            }
        }
        let Some(label) = changelog::label(&name, &args, None) else {
            return;
        };
        let status = if denied {
            "refusé"
        } else if is_error {
            "échec"
        } else {
            "ok"
        };
        if let Ok(dir) = config_dir(self.app) {
            changelog::record(&dir, &name, &label, status, self.workdir.as_deref());
        }
        let _ = self.channel.send(UiEvent::Notice {
            text: format!("✎ {label} · {status}"),
        });
    }
}

impl EventSink for TauriSink<'_> {
    fn emit(&mut self, event: AgentEvent) -> bool {
        match event {
            AgentEvent::Opened { chat_id } => {
                let _ = self.channel.send(UiEvent::Opened { chat_id });
            }
            AgentEvent::Title { chat_id, title } => {
                let _ = self.channel.send(UiEvent::Title { chat_id, title });
            }
            AgentEvent::Delta { text } => {
                let _ = self.channel.send(UiEvent::Delta { text });
            }
            AgentEvent::Reasoning { text } => {
                let _ = self.channel.send(UiEvent::Reasoning { text });
            }
            AgentEvent::ToolCall {
                call_id,
                name,
                origin,
                args,
                needs_confirm,
            } => {
                self.calls.insert(call_id.clone(), (name.clone(), args.clone()));
                let _ = self.channel.send(UiEvent::ToolCall {
                    call_id,
                    name,
                    origin: match origin {
                        Origin::Mcp => "mcp".to_string(),
                        Origin::Local => "local".to_string(),
                    },
                    args: display_args(&args),
                    needs_confirm,
                });
            }
            AgentEvent::ToolResult {
                call_id,
                output,
                is_error,
                denied,
            } => {
                self.finish_tool(&call_id, &output, is_error, denied);
                let _ = self.channel.send(UiEvent::ToolResult {
                    call_id,
                    output: preview(&output),
                    is_error,
                    denied,
                });
            }
            AgentEvent::Notice { text } => {
                let _ = self.channel.send(UiEvent::Notice { text });
            }
            AgentEvent::AwaitingTools { run_id, calls } => {
                let _ = self
                    .channel
                    .send(UiEvent::AwaitingTools { run_id, calls });
            }
            AgentEvent::Resumed { run_id } => {
                let _ = self.channel.send(UiEvent::Resumed { run_id });
            }
            AgentEvent::RunInterrupted { run_id, reason } => {
                let _ = self
                    .channel
                    .send(UiEvent::RunInterrupted { run_id, reason });
            }
            AgentEvent::Usage {
                chat_id,
                percent,
                used_tokens,
            } => {
                let _ = self.channel.send(UiEvent::Usage {
                    chat_id,
                    percent,
                    used_tokens,
                });
            }
            AgentEvent::BudgetWarning { run_id, requests } => {
                let _ = self
                    .channel
                    .send(UiEvent::BudgetWarning { run_id, requests });
            }
            AgentEvent::Done {
                chat_id,
                message_id,
                prompt_tokens,
                output_tokens,
                total_prompt_tokens,
                total_output_tokens,
                api_calls,
                eco,
            } => {
                let _ = self.channel.send(UiEvent::Done {
                    chat_id,
                    message_id,
                    prompt_tokens,
                    output_tokens,
                    total_prompt_tokens,
                    total_output_tokens,
                    api_calls,
                    eco,
                });
            }
            AgentEvent::Failed {
                code,
                message,
                retryable,
            } => {
                let _ = self
                    .channel
                    .send(UiEvent::Failed {
                        code,
                        message,
                        retryable,
                    });
            }
            AgentEvent::Cancelled => {
                let _ = self.channel.send(UiEvent::Cancelled);
            }
        }
        !self.control.is_cancelled()
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('…');
    t
}

// ---------- transport HTTP ----------

pub struct ApiBackend {
    app: AppHandle,
}

#[derive(Default)]
struct TurnAccumulator {
    chat_id: Option<String>,
    message_id: Option<String>,
    content: String,
    prompt_tokens: Option<u64>,
    output_tokens: Option<u64>,
    tool_calls: Vec<ToolCall>,
    terminal: Option<Terminal>,
}

/// Premier nombre trouvé parmi `keys` (accepte un nombre ou une chaîne numérique).
fn first_number(value: &Value, keys: &[&str]) -> Option<f64> {
    for key in keys {
        let Some(found) = value.get(*key) else {
            continue;
        };
        if let Some(number) = found.as_f64() {
            return Some(number);
        }
        if let Some(text) = found.as_str() {
            if let Ok(number) = text.trim_end_matches('%').trim().parse::<f64>() {
                return Some(number);
            }
        }
    }
    None
}

fn backend_error(error: fs_api::Error) -> BackendError {
    match error {
        fs_api::Error::Status { status, body } => BackendError::Status { status, body },
        fs_api::Error::Http(e) => BackendError::Transport(e.to_string()),
        fs_api::Error::Stream(e) => BackendError::Stream(e),
        fs_api::Error::Json(e) => BackendError::Other(e.to_string()),
        fs_api::Error::BadKey => BackendError::Other("jeton de connexion invalide".to_string()),
    }
}

#[async_trait]
impl ChatBackend for ApiBackend {
    async fn turn(
        &self,
        input: &TurnInput,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<TurnOutcome, BackendError> {
        let state = self.app.state::<AppState>();
        let c = client(&self.app, &state)
            .await
            .map_err(BackendError::Other)?;

        let mut acc = TurnAccumulator::default();
        let mut seen: HashSet<String> = HashSet::new();
        let mut handler = |event: ChatEvent| match event {
            ChatEvent::Opened { chat_id } => {
                acc.chat_id = Some(chat_id.clone());
                sink.emit(AgentEvent::Opened { chat_id });
            }
            ChatEvent::Title { chat_id, title } => {
                sink.emit(AgentEvent::Title { chat_id, title });
            }
            ChatEvent::Delta(text) => {
                acc.content.push_str(&text);
                sink.emit(AgentEvent::Delta { text });
            }
            ChatEvent::ReasoningDelta(text) => {
                sink.emit(AgentEvent::Reasoning { text });
            }
            ChatEvent::Completed { chat_id, message } => {
                acc.chat_id = Some(chat_id);
                acc.message_id = Some(message.id.clone());
                if !message.content.is_empty() {
                    acc.content = message.content;
                }
                let usage = message.token_usage.unwrap_or_default();
                acc.prompt_tokens = usage.prompt_token_count;
                acc.output_tokens = usage.candidates_token_count;
                acc.tool_calls = message.tool_calls;
                acc.terminal = Some(Terminal::Completed);
            }
            ChatEvent::Error(error) => {
                acc.terminal = Some(Terminal::Failed {
                    code: error.code,
                    message: error.message,
                    retryable: error.retryable,
                });
            }
            ChatEvent::Cancelled { .. } => {
                acc.terminal = Some(Terminal::Cancelled);
            }
            ChatEvent::Mcp(text) | ChatEvent::Notice(text) => {
                sink.emit(AgentEvent::Notice { text });
            }
            ChatEvent::Other(ty) => {
                if (ty.contains("mcp") || ty.contains("tool")) && seen.insert(ty.clone()) {
                    sink.emit(AgentEvent::Notice {
                        text: format!("France Student a émis un événement « {ty} »."),
                    });
                }
            }
        };

        let chat_hint = match input {
            TurnInput::Start { chat_id, .. } => chat_id.clone(),
            TurnInput::Results { chat_id, .. } | TurnInput::Resume { chat_id, .. } => {
                Some(chat_id.clone())
            }
        };

        let stream = async {
            match input {
                TurnInput::Start {
                    chat_id,
                    agent_id,
                    content,
                    tools,
                    options,
                } => {
                    let tools: Vec<Value> = tools.iter().map(|name| json!(name)).collect();
                    match chat_id {
                        Some(id) => {
                            c.send_message_with_options_id(
                                id,
                                content,
                                Some(*agent_id),
                                &tools,
                                &[],
                                options,
                                &mut handler,
                            )
                            .await
                        }
                        None => {
                            c.stream_chat_with_options_id(
                                *agent_id,
                                content,
                                &tools,
                                options,
                                &mut handler,
                            )
                            .await
                        }
                    }
                }
                TurnInput::Results {
                    chat_id,
                    results,
                    options,
                } => {
                    c.send_message_with_options_id(
                        chat_id,
                        "",
                        None,
                        &[],
                        results,
                        options,
                        &mut handler,
                    )
                    .await
                }
                TurnInput::Resume {
                    chat_id,
                    last_event_id,
                } => {
                    c.resume_stream(chat_id, last_event_id.as_deref(), &mut handler)
                        .await
                }
            }
        };

        // Annulation immédiate : on coupe la lecture SSE et on annule côté serveur.
        let cancel_flag = control.flag();
        let waiter = async move {
            loop {
                if cancel_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        };
        let mut cancelled = false;
        let last_event_id = tokio::select! {
            result = stream => result.map_err(backend_error)?,
            _ = waiter => {
                cancelled = true;
                None
            }
        };

        if cancelled {
            if let Some(chat_id) = acc.chat_id.clone().or(chat_hint) {
                let _ = c.cancel(&chat_id).await;
            }
            return Ok(TurnOutcome {
                chat_id: acc.chat_id,
                message_id: acc.message_id,
                content: acc.content,
                tool_calls: Vec::new(),
                prompt_tokens: acc.prompt_tokens,
                output_tokens: acc.output_tokens,
                terminal: Terminal::Cancelled,
                last_event_id: None,
            });
        }

        Ok(TurnOutcome {
            chat_id: acc.chat_id,
            message_id: acc.message_id,
            content: acc.content,
            tool_calls: acc.tool_calls,
            prompt_tokens: acc.prompt_tokens,
            output_tokens: acc.output_tokens,
            terminal: acc.terminal.unwrap_or_else(|| Terminal::Failed {
                code: "STREAM_ENDED".to_string(),
                message: "Le flux s'est arrêté avant la fin de la réponse.".to_string(),
                retryable: true,
            }),
            last_event_id,
        })
    }

    async fn cancel(&self, chat_id: &str) -> Result<(), BackendError> {
        let state = self.app.state::<AppState>();
        let c = client(&self.app, &state)
            .await
            .map_err(BackendError::Other)?;
        c.cancel(chat_id).await.map_err(backend_error)
    }

    async fn fetch_chat(&self, chat_id: &str) -> Result<ChatSnapshot, BackendError> {
        let state = self.app.state::<AppState>();
        let c = client(&self.app, &state)
            .await
            .map_err(BackendError::Other)?;
        let value = c.get_chat(chat_id).await.map_err(backend_error)?;
        let mut pending = Vec::new();
        if let Some(messages) = value.get("messages").and_then(Value::as_array) {
            for message in messages.iter().rev() {
                let Some(calls) = message.get("toolCalls").and_then(Value::as_array) else {
                    continue;
                };
                for call in calls {
                    if let Ok(call) = serde_json::from_value::<ToolCall>(call.clone()) {
                        pending.push(call);
                    }
                }
                if !pending.is_empty() {
                    break;
                }
            }
        }
        Ok(ChatSnapshot {
            pending_tool_calls: pending,
            last_event_id: None,
        })
    }

    async fn chat_usage(&self, chat_id: &str) -> Result<Option<ContextUsage>, BackendError> {
        let state = self.app.state::<AppState>();
        let c = client(&self.app, &state)
            .await
            .map_err(BackendError::Other)?;
        let value = match c.chat_usage(chat_id).await {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let used = first_number(&value, &["usedTokens", "used_tokens", "contextTokens"]);
        let max = first_number(
            &value,
            &["maxTokens", "max_tokens", "contextWindow", "contextLimit"],
        );
        let percent = first_number(
            &value,
            &["contextUsagePercent", "context_usage_percent", "usagePercent"],
        )
        .or_else(|| match (used, max) {
            (Some(used), Some(max)) if max > 0.0 => Some(used / max * 100.0),
            _ => None,
        });
        Ok(Some(ContextUsage {
            percent,
            used_tokens: used.map(|value| value as u64),
            max_tokens: max.map(|value| value as u64),
        }))
    }
}

// ---------- outils locaux ----------

pub struct WorkspaceTools {
    app: AppHandle,
    workdir: Option<PathBuf>,
}

#[async_trait]
impl ToolExecutor for WorkspaceTools {
    fn spec(&self, name: &str) -> Option<ToolSpec> {
        if !fs_tools::NAMES.contains(&name) {
            return None;
        }
        let risk = match fs_tools::risk(name)? {
            fs_tools::Risk::Safe => Risk::ReadOnly,
            fs_tools::Risk::Edit => Risk::Mutating,
            fs_tools::Risk::Command => Risk::Command,
        };
        Some(ToolSpec::new(name, risk))
    }

    async fn execute(&self, call: &ToolCall, _ctx: &ToolContext) -> ToolExecution {
        let Some(root) = self.workdir.as_ref() else {
            return ToolExecution::error(
                "Outil de fichiers disponible uniquement en mode Code.",
            );
        };
        let state = self.app.state::<AppState>();
        let result = if perm::current(&state) == perm::Mode::Full {
            fs_tools::execute_full(root, &call.name, &call.arguments).await
        } else {
            fs_tools::execute(root, &call.name, &call.arguments).await
        };
        match result {
            Ok(output) => ToolExecution::ok(output),
            Err(message) => ToolExecution::error(message),
        }
    }
}

// ---------- approbations ----------

pub struct UiApprover {
    app: AppHandle,
}

#[async_trait]
impl Approver for UiApprover {
    async fn decide(&self, request: &ApprovalRequest) -> Approval {
        let state = self.app.state::<AppState>();
        let mode = perm::current(&state);
        match perm::decide(
            mode,
            request.auto_accept,
            &request.name,
            &request.args,
            request.workdir.as_deref(),
        ) {
            perm::Verdict::Allow => Approval::Allow,
            perm::Verdict::Ask => Approval::Ask,
            perm::Verdict::Deny(message) => Approval::Deny(message),
        }
    }

    async fn confirm(&self, request: &ApprovalRequest) -> bool {
        let state = self.app.state::<AppState>();
        ask(&state, &request.run_id, &request.call_id).await
    }
}

// ---------- entrées publiques ----------

/// Préflight : budget et compatibilité du modèle avec les outils client.
pub async fn preflight(
    app: &AppHandle,
    agent_id: i64,
    tools: &[String],
) -> Result<(), (String, String)> {
    let state = app.state::<AppState>();
    let Ok(c) = client(app, &state).await else {
        return Ok(()); // l'absence de clé est signalée au premier tour
    };

    if let Ok(usage) = c.usage().await {
        if let Some(percent) = remaining_percent(&usage) {
            if percent <= 0.0 {
                return Err((
                    "BUDGET_EXHAUSTED".to_string(),
                    "Budget de chats épuisé : réessaie après la réinitialisation indiquée par France Student.".to_string(),
                ));
            }
        }
    }

    if !tools.is_empty() {
        if let Ok(agents) = c.agents().await {
            if let Some(agent) = agents.into_iter().find(|a| a.id == agent_id) {
                let model = format!("{} {}", agent.model_provider, agent.model).to_lowercase();
                if model.contains("google") || model.contains("gemini") {
                    return Err((
                        "MODEL_UNSUPPORTED".to_string(),
                        "Ce modèle ne gère pas les outils client. Choisis un modèle OpenAI ou Claude pour le mode Code.".to_string(),
                    ));
                }
            }
        }
    }

    Ok(())
}

fn remaining_percent(usage: &Value) -> Option<f64> {
    for path in [
        &["chatBudget", "remainingPercent"][..],
        &["chatBudget", "remaining_percent"][..],
        &["budget", "remainingPercent"][..],
        &["budget", "remaining_percent"][..],
        &["remainingPercent"][..],
        &["remaining_percent"][..],
    ] {
        let mut value = usage;
        let mut found = true;
        for key in path {
            match value.get(*key) {
                Some(next) => value = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            continue;
        }
        if let Some(percent) = value.as_f64() {
            return Some(percent);
        }
    }
    let remaining = usage
        .pointer("/chatBudget/remaining")
        .and_then(Value::as_f64)?;
    let limit = usage
        .pointer("/chatBudget/limit")
        .and_then(Value::as_f64)
        .filter(|limit| *limit > 0.0)?;
    Some(remaining / limit * 100.0)
}

pub async fn run(
    app: &AppHandle,
    request: RunRequest,
    control: RunControl,
    channel: &Channel<UiEvent>,
) -> Result<RunState, String> {
    let root = config_dir(app)?;
    let store = Arc::new(FsRunStore::new(root.join("runs")));
    let backend = Arc::new(ApiBackend { app: app.clone() });
    let tools = Arc::new(WorkspaceTools {
        app: app.clone(),
        workdir: request.workdir.clone(),
    });
    let approver = Arc::new(UiApprover { app: app.clone() });
    let policy = LoopPolicy::for_mode(request.eco);
    let engine = AgentLoop::new(backend, tools, approver, store, policy);
    let mut sink = TauriSink::new(app, channel, control.clone(), request.workdir.clone());
    engine
        .run(request, &control, &mut sink)
        .await
        .map_err(|error| error.to_string())
}

pub async fn resume(
    app: &AppHandle,
    run_id: &str,
    control: RunControl,
    channel: &Channel<UiEvent>,
) -> Result<RunState, String> {
    let root = config_dir(app)?;
    let store = FsRunStore::new(root.join("runs"));
    let state = store
        .load(run_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Run introuvable : {run_id}"))?;
    let workdir = state.workdir.as_ref().map(PathBuf::from);
    let policy = LoopPolicy::for_mode(state.eco);

    let store = Arc::new(store);
    let backend = Arc::new(ApiBackend { app: app.clone() });
    let tools = Arc::new(WorkspaceTools {
        app: app.clone(),
        workdir: workdir.clone(),
    });
    let approver = Arc::new(UiApprover { app: app.clone() });
    let engine = AgentLoop::new(backend, tools, approver, store, policy);
    let mut sink = TauriSink::new(app, channel, control.clone(), workdir);
    engine
        .resume(run_id, &control, &mut sink)
        .await
        .map_err(|error| error.to_string())
}

/// Charge un run persisté (reprise, abandon, contrôle d'exclusivité).
pub async fn load_run(app: &AppHandle, run_id: &str) -> Result<RunState, String> {
    let root = config_dir(app)?;
    let store = FsRunStore::new(root.join("runs"));
    store
        .load(run_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Run introuvable : {run_id}"))
}

pub async fn abandon(app: &AppHandle, run_id: &str) -> Result<RunState, String> {    let root = config_dir(app)?;
    let store = FsRunStore::new(root.join("runs"));
    let state = store
        .load(run_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Run introuvable : {run_id}"))?;
    if state.status.is_terminal() {
        return Ok(state);
    }
    let engine = AgentLoop::new(
        Arc::new(ApiBackend { app: app.clone() }),
        Arc::new(WorkspaceTools {
            app: app.clone(),
            workdir: state.workdir.as_ref().map(PathBuf::from),
        }),
        Arc::new(UiApprover { app: app.clone() }),
        Arc::new(store),
        LoopPolicy::for_mode(state.eco),
    );
    engine
        .abandon(run_id)
        .await
        .map_err(|error| error.to_string())
}

pub async fn list_open(app: &AppHandle) -> Result<Vec<RunState>, String> {
    let root = config_dir(app)?;
    let store = FsRunStore::new(root.join("runs"));
    store.list_open().await.map_err(|error| error.to_string())
}
