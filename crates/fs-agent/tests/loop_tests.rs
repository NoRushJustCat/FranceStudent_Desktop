//! Tests de la boucle sans réseau : backend, outils, approbateur et stockage factices.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::{
    AgentEvent, AgentLoop, Approval, ApprovalRequest, Approver, BackendError, ChatBackend,
    ChatSnapshot, EventSink, LoopPolicy, PendingCall, RunControl, RunRequest, RunState, RunStatus,
    RunStore, StoreError, Terminal, ToolContext, ToolExecution, ToolExecutor, ToolSpec, TurnInput,
    TurnOutcome,
};
use fs_api::{StreamOptions, ToolCall};
use serde_json::json;

// ---------- double de stockage ----------

#[derive(Default, Clone)]
struct MemoryStore {
    runs: Arc<Mutex<HashMap<String, RunState>>>,
}

#[async_trait]
impl RunStore for MemoryStore {
    async fn save(&self, state: &RunState) -> Result<(), StoreError> {
        self.runs
            .lock()
            .unwrap()
            .insert(state.run_id.clone(), state.clone());
        Ok(())
    }

    async fn load(&self, run_id: &str) -> Result<Option<RunState>, StoreError> {
        Ok(self.runs.lock().unwrap().get(run_id).cloned())
    }

    async fn list_open(&self) -> Result<Vec<RunState>, StoreError> {
        Ok(self
            .runs
            .lock()
            .unwrap()
            .values()
            .filter(|state| !state.status.is_terminal())
            .cloned()
            .collect())
    }

    async fn delete(&self, run_id: &str) -> Result<(), StoreError> {
        self.runs.lock().unwrap().remove(run_id);
        Ok(())
    }
}

// ---------- double de backend ----------

struct FakeBackend {
    script: Mutex<VecDeque<Result<TurnOutcome, BackendError>>>,
    requests: Mutex<Vec<TurnInput>>,
    snapshot: Mutex<ChatSnapshot>,
    usage: Mutex<Option<fs_agent::ContextUsage>>,
}

impl FakeBackend {
    fn new(script: Vec<TurnOutcome>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(script.into_iter().map(Ok).collect()),
            requests: Mutex::new(Vec::new()),
            snapshot: Mutex::new(ChatSnapshot::default()),
            usage: Mutex::new(None),
        })
    }

    fn with_errors(script: Vec<Result<TurnOutcome, BackendError>>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(script.into()),
            requests: Mutex::new(Vec::new()),
            snapshot: Mutex::new(ChatSnapshot::default()),
            usage: Mutex::new(None),
        })
    }

    fn requests(&self) -> Vec<TurnInput> {
        self.requests.lock().unwrap().clone()
    }

    fn set_snapshot(&self, snapshot: ChatSnapshot) {
        *self.snapshot.lock().unwrap() = snapshot;
    }

    fn set_usage(&self, usage: fs_agent::ContextUsage) {
        *self.usage.lock().unwrap() = Some(usage);
    }
}

#[async_trait]
impl ChatBackend for FakeBackend {
    async fn turn(
        &self,
        input: &TurnInput,
        _control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<TurnOutcome, BackendError> {
        self.requests.lock().unwrap().push(input.clone());
        sink.emit(AgentEvent::Delta {
            text: "delta".to_string(),
        });
        self.script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(BackendError::Other("script épuisé".to_string())))
    }

    async fn fetch_chat(&self, _chat_id: &str) -> Result<ChatSnapshot, BackendError> {
        Ok(self.snapshot.lock().unwrap().clone())
    }

    async fn chat_usage(
        &self,
        _chat_id: &str,
    ) -> Result<Option<fs_agent::ContextUsage>, BackendError> {
        Ok(*self.usage.lock().unwrap())
    }
}

fn completed(chat_id: &str, message_id: &str, calls: Vec<ToolCall>) -> TurnOutcome {
    TurnOutcome {
        chat_id: Some(chat_id.to_string()),
        message_id: Some(message_id.to_string()),
        content: "réponse".to_string(),
        tool_calls: calls,
        prompt_tokens: Some(10),
        output_tokens: Some(5),
        terminal: Terminal::Completed,
        last_event_id: Some("7".to_string()),
    }
}

fn call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        call_id: id.to_string(),
        name: name.to_string(),
        arguments: json!({ "path": "a.txt" }),
    }
}

// ---------- double d'outils ----------

struct FakeTools {
    names: Vec<String>,
    output: String,
    executed: Mutex<Vec<String>>,
}

impl FakeTools {
    fn new(names: &[&str], output: &str) -> Arc<Self> {
        Arc::new(Self {
            names: names.iter().map(|n| n.to_string()).collect(),
            output: output.to_string(),
            executed: Mutex::new(Vec::new()),
        })
    }

    fn executed(&self) -> Vec<String> {
        self.executed.lock().unwrap().clone()
    }
}

#[async_trait]
impl ToolExecutor for FakeTools {
    fn spec(&self, name: &str) -> Option<ToolSpec> {
        self.names
            .iter()
            .any(|n| n == name)
            .then(|| ToolSpec::new(name, fs_agent::Risk::ReadOnly))
    }

    async fn execute(&self, call: &ToolCall, _ctx: &ToolContext) -> ToolExecution {
        self.executed.lock().unwrap().push(call.name.clone());
        ToolExecution::ok(self.output.clone())
    }
}

// ---------- double d'approbateur ----------

struct FixedApprover {
    decision: Approval,
    confirm: bool,
}

#[async_trait]
impl Approver for FixedApprover {
    async fn decide(&self, _request: &ApprovalRequest) -> Approval {
        self.decision.clone()
    }

    async fn confirm(&self, _request: &ApprovalRequest) -> bool {
        self.confirm
    }
}

// ---------- utilitaires ----------

type Events = Arc<Mutex<Vec<AgentEvent>>>;

fn collector() -> (Events, impl FnMut(AgentEvent) -> bool + Send) {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let events = events.clone();
        move |event: AgentEvent| {
            events.lock().unwrap().push(event);
            true
        }
    };
    (events, sink)
}

fn request(run_id: &str, content: &str) -> RunRequest {
    RunRequest {
        run_id: run_id.to_string(),
        chat_id: None,
        agent_id: 1,
        content: content.to_string(),
        workdir: None,
        auto_accept: false,
        eco: false,
        tools: vec!["read_file".to_string()],
        options: StreamOptions::default(),
    }
}

fn policy() -> LoopPolicy {
    LoopPolicy {
        retry_delay_ms: 0,
        ..LoopPolicy::default()
    }
}

fn engine(
    backend: Arc<FakeBackend>,
    tools: Arc<FakeTools>,
    approver: Arc<FixedApprover>,
    store: MemoryStore,
) -> AgentLoop {
    AgentLoop::new(backend, tools, approver, Arc::new(store), policy())
}

fn engine_with_policy(
    backend: Arc<FakeBackend>,
    tools: Arc<FakeTools>,
    approver: Arc<dyn Approver>,
    store: MemoryStore,
    policy: LoopPolicy,
) -> AgentLoop {
    AgentLoop::new(backend, tools, approver, Arc::new(store), policy)
}

/// Refuse la continuation au seuil de requêtes.
struct BudgetDenier(FixedApprover);

#[async_trait]
impl Approver for BudgetDenier {
    async fn decide(&self, request: &ApprovalRequest) -> Approval {
        self.0.decide(request).await
    }

    async fn confirm(&self, request: &ApprovalRequest) -> bool {
        self.0.confirm(request).await
    }

    async fn confirm_budget(&self, _run_id: &str, _requests: u32) -> bool {
        false
    }
}

fn has_event(events: &Events, test: impl Fn(&AgentEvent) -> bool) -> bool {
    events.lock().unwrap().iter().any(test)
}

// ---------- scénarios ----------

#[tokio::test]
async fn preserves_user_text_and_completes() {
    let backend = FakeBackend::new(vec![completed("chat-1", "msg-1", vec![])]);
    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let store = MemoryStore::default();
    let loop_ = engine(backend.clone(), tools, approver, store.clone());
    let control = RunControl::new();
    let (events, mut sink) = collector();

    let content = "Corrige le bug\nligne 2";
    let state = loop_
        .run(request("run-1", content), &control, &mut sink)
        .await
        .unwrap();

    assert_eq!(state.status, RunStatus::Done);
    let requests = backend.requests();
    assert_eq!(requests.len(), 1);
    match &requests[0] {
        TurnInput::Start { content: sent, .. } => assert_eq!(sent, content),
        other => panic!("tour inattendu : {other:?}"),
    }
    assert!(has_event(&events, |e| matches!(e, AgentEvent::Done { .. })));
    assert!(store.runs.lock().unwrap().contains_key("run-1"));
}

#[tokio::test]
async fn executes_tool_and_sends_results() {
    let backend = FakeBackend::new(vec![
        completed("chat-1", "msg-1", vec![call("c1", "read_file")]),
        completed("chat-1", "msg-2", vec![]),
    ]);
    let tools = FakeTools::new(&["read_file"], "contenu");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let store = MemoryStore::default();
    let loop_ = engine(backend.clone(), tools.clone(), approver, store);
    let control = RunControl::new();
    let (events, mut sink) = collector();

    let state = loop_
        .run(request("run-1", "lis a.txt"), &control, &mut sink)
        .await
        .unwrap();

    assert_eq!(state.status, RunStatus::Done);
    assert_eq!(tools.executed(), vec!["read_file"]);
    let requests = backend.requests();
    assert_eq!(requests.len(), 2);
    match &requests[1] {
        TurnInput::Results { results, .. } => {
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].call_id, "c1");
            assert_eq!(results[0].output, "contenu");
            assert!(!results[0].is_error);
        }
        other => panic!("tour inattendu : {other:?}"),
    }
    assert!(has_event(&events, |e| matches!(
        e,
        AgentEvent::ToolResult { is_error: false, .. }
    )));
}

#[tokio::test]
async fn denial_is_an_error_result() {
    let backend = FakeBackend::new(vec![
        completed("chat-1", "msg-1", vec![call("c1", "read_file")]),
        completed("chat-1", "msg-2", vec![]),
    ]);
    let tools = FakeTools::new(&["read_file"], "contenu");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Deny("refusé".to_string()),
        confirm: false,
    });
    let loop_ = engine(backend.clone(), tools.clone(), approver, MemoryStore::default());
    let control = RunControl::new();
    let (_events, mut sink) = collector();

    loop_
        .run(request("run-1", "lis"), &control, &mut sink)
        .await
        .unwrap();

    assert!(tools.executed().is_empty());
    match &backend.requests()[1] {
        TurnInput::Results { results, .. } => {
            assert!(results[0].is_error);
            assert!(results[0].output.contains("refusé"));
        }
        other => panic!("tour inattendu : {other:?}"),
    }
}

#[tokio::test]
async fn unknown_and_platform_tools_are_never_executed() {
    let backend = FakeBackend::new(vec![
        completed(
            "chat-1",
            "msg-1",
            vec![call("c1", "ghost"), call("c2", "mcp__server__tool")],
        ),
        completed("chat-1", "msg-2", vec![]),
    ]);
    let tools = FakeTools::new(&["read_file"], "contenu");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools.clone(), approver, MemoryStore::default());
    let control = RunControl::new();
    let (_events, mut sink) = collector();

    loop_
        .run(request("run-1", "appelle"), &control, &mut sink)
        .await
        .unwrap();

    assert!(tools.executed().is_empty());
    match &backend.requests()[1] {
        TurnInput::Results { results, .. } => {
            assert_eq!(results.len(), 2);
            assert!(results.iter().all(|r| r.is_error));
        }
        other => panic!("tour inattendu : {other:?}"),
    }
}

#[tokio::test]
async fn rejects_too_many_calls() {
    let calls: Vec<ToolCall> = (0..33)
        .map(|i| call(&format!("c{i}"), "read_file"))
        .collect();
    let backend = FakeBackend::new(vec![completed("chat-1", "msg-1", calls)]);
    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools.clone(), approver, MemoryStore::default());
    let control = RunControl::new();
    let (events, mut sink) = collector();

    let state = loop_
        .run(request("run-1", "test"), &control, &mut sink)
        .await
        .unwrap();

    assert_eq!(state.status, RunStatus::Failed);
    assert!(tools.executed().is_empty());
    assert!(has_event(&events, |e| matches!(
        e,
        AgentEvent::Failed { code, .. } if code == "PROTOCOL"
    )));
}

#[tokio::test]
async fn rejects_duplicate_call_ids() {
    let backend = FakeBackend::new(vec![completed(
        "chat-1",
        "msg-1",
        vec![call("same", "read_file"), call("same", "read_file")],
    )]);
    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools, approver, MemoryStore::default());
    let control = RunControl::new();
    let (_events, mut sink) = collector();

    let state = loop_
        .run(request("run-1", "test"), &control, &mut sink)
        .await
        .unwrap();
    assert_eq!(state.status, RunStatus::Failed);
}

#[tokio::test]
async fn truncates_large_tool_output() {
    let backend = FakeBackend::new(vec![
        completed("chat-1", "msg-1", vec![call("c1", "read_file")]),
        completed("chat-1", "msg-2", vec![]),
    ]);
    let tools = FakeTools::new(&["read_file"], &"x".repeat(50_000));
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools, approver, MemoryStore::default());
    let control = RunControl::new();
    let (_events, mut sink) = collector();

    loop_
        .run(request("run-1", "lis"), &control, &mut sink)
        .await
        .unwrap();

    match &backend.requests()[1] {
        TurnInput::Results { results, .. } => {
            let len = results[0].output.chars().count();
            assert!(len <= 40_000 + 80, "longueur = {len}");
            assert!(results[0].output.contains("tronquée"));
        }
        other => panic!("tour inattendu : {other:?}"),
    }
}

#[tokio::test]
async fn cancellation_before_start_does_not_call_backend() {
    let backend = FakeBackend::new(vec![completed("chat-1", "msg-1", vec![])]);
    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools, approver, MemoryStore::default());
    let control = RunControl::new();
    control.cancel();
    let (events, mut sink) = collector();

    let state = loop_
        .run(request("run-1", "test"), &control, &mut sink)
        .await
        .unwrap();

    assert_eq!(state.status, RunStatus::Cancelled);
    assert!(backend.requests().is_empty());
    assert!(has_event(&events, |e| matches!(e, AgentEvent::Cancelled)));
}

#[tokio::test]
async fn retryable_error_resumes_stream() {
    let backend = FakeBackend::with_errors(vec![
        Err(BackendError::Transport("coupure".to_string())),
        Ok(completed("chat-1", "msg-1", vec![])),
    ]);
    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools, approver, MemoryStore::default());
    let control = RunControl::new();
    let (events, mut sink) = collector();

    let mut req = request("run-1", "test");
    req.chat_id = Some("chat-1".to_string());
    let state = loop_.run(req, &control, &mut sink).await.unwrap();

    assert_eq!(state.status, RunStatus::Done);
    let requests = backend.requests();
    assert_eq!(requests.len(), 2);
    assert!(matches!(requests[1], TurnInput::Resume { .. }));
    assert!(has_event(&events, |e| matches!(e, AgentEvent::Notice { .. })));
}

#[tokio::test]
async fn resume_executes_pending_calls() {
    let backend = FakeBackend::new(vec![completed("chat-1", "msg-2", vec![])]);
    let tools = FakeTools::new(&["read_file"], "contenu");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let store = MemoryStore::default();

    let mut state = RunState::new(&request("run-1", "lis"), fs_agent::now_iso());
    state.chat_id = Some("chat-1".to_string());
    state.status = RunStatus::AwaitingTools;
    state.pending_calls = vec![PendingCall::new(
        "c1".to_string(),
        "read_file".to_string(),
        json!({ "path": "a.txt" }),
    )];
    store.save(&state).await.unwrap();

    let loop_ = engine(backend.clone(), tools.clone(), approver, store.clone());
    let control = RunControl::new();
    let (_events, mut sink) = collector();

    let state = loop_.resume("run-1", &control, &mut sink).await.unwrap();

    assert_eq!(state.status, RunStatus::Done);
    assert_eq!(tools.executed(), vec!["read_file"]);
    assert!(matches!(backend.requests()[0], TurnInput::Results { .. }));
}

#[tokio::test]
async fn cancellation_flag_is_observed_between_tools() {
    let backend = FakeBackend::new(vec![completed(
        "chat-1",
        "msg-1",
        vec![call("c1", "read_file"), call("c2", "read_file")],
    )]);
    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Ask,
        confirm: false,
    });
    let loop_ = engine(backend, tools.clone(), approver, MemoryStore::default());
    let control = RunControl::new();
    control.cancel();
    let (_events, mut sink) = collector();

    let state = loop_
        .run(request("run-1", "test"), &control, &mut sink)
        .await
        .unwrap();

    assert_eq!(state.status, RunStatus::Cancelled);
    assert!(tools.executed().is_empty());
}

#[tokio::test]
async fn chat_snapshot_default_is_empty() {
    let snapshot = ChatSnapshot::default();
    assert!(snapshot.pending_tool_calls.is_empty());
}

#[tokio::test]
async fn resume_never_replays_a_started_call() {
    let backend = FakeBackend::new(vec![]);
    let tools = FakeTools::new(&["read_file"], "contenu");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let store = MemoryStore::default();

    let mut state = RunState::new(&request("run-1", "lis"), fs_agent::now_iso());
    state.chat_id = Some("chat-1".to_string());
    state.status = RunStatus::Executing;
    let mut pending = PendingCall::new(
        "c1".to_string(),
        "read_file".to_string(),
        json!({ "path": "a.txt" }),
    );
    pending.decision = Some(fs_agent::Decision::Allowed);
    pending.started_at = Some(fs_agent::now_iso());
    state.pending_calls = vec![pending];
    store.save(&state).await.unwrap();

    let loop_ = engine(backend.clone(), tools.clone(), approver, store.clone());
    let control = RunControl::new();
    let (events, mut sink) = collector();

    let state = loop_.resume("run-1", &control, &mut sink).await.unwrap();

    assert!(tools.executed().is_empty());
    assert!(backend.requests().is_empty());
    assert_eq!(state.status, RunStatus::Executing);
    assert!(has_event(&events, |e| matches!(
        e,
        AgentEvent::RunInterrupted { .. }
    )));
}

#[tokio::test]
async fn resume_forcing_replays_a_started_call() {
    let backend = FakeBackend::new(vec![completed("chat-1", "msg-2", vec![])]);
    let tools = FakeTools::new(&["read_file"], "contenu");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let store = MemoryStore::default();

    let mut state = RunState::new(&request("run-1", "lis"), fs_agent::now_iso());
    state.chat_id = Some("chat-1".to_string());
    state.status = RunStatus::Executing;
    let mut pending = PendingCall::new(
        "c1".to_string(),
        "read_file".to_string(),
        json!({ "path": "a.txt" }),
    );
    pending.decision = Some(fs_agent::Decision::Allowed);
    pending.started_at = Some(fs_agent::now_iso());
    state.pending_calls = vec![pending];
    store.save(&state).await.unwrap();

    let loop_ = engine(backend.clone(), tools.clone(), approver, store);
    let control = RunControl::new();
    let (_events, mut sink) = collector();

    let state = loop_
        .resume_forcing("run-1", &control, &mut sink)
        .await
        .unwrap();

    assert_eq!(state.status, RunStatus::Done);
    assert_eq!(tools.executed(), vec!["read_file"]);
}

#[tokio::test]
async fn conflict_409_reconciles_pending_calls() {
    let backend = FakeBackend::with_errors(vec![
        Err(BackendError::Status {
            status: 409,
            body: "generation in progress".to_string(),
        }),
        Ok(completed("chat-1", "msg-2", vec![])),
    ]);
    backend.set_snapshot(ChatSnapshot {
        pending_tool_calls: vec![call("c1", "read_file")],
        last_event_id: None,
    });
    let tools = FakeTools::new(&["read_file"], "contenu");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools.clone(), approver, MemoryStore::default());
    let control = RunControl::new();
    let (_events, mut sink) = collector();

    let mut req = request("run-1", "lis");
    req.chat_id = Some("chat-1".to_string());
    let state = loop_.run(req, &control, &mut sink).await.unwrap();

    assert_eq!(state.status, RunStatus::Done);
    assert_eq!(tools.executed(), vec!["read_file"]);
    assert!(matches!(backend.requests()[1], TurnInput::Results { .. }));
}

#[tokio::test]
async fn conflict_409_without_pending_calls_finishes() {
    let backend = FakeBackend::with_errors(vec![Err(BackendError::Status {
        status: 409,
        body: "already done".to_string(),
    })]);
    let tools = FakeTools::new(&["read_file"], "contenu");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools.clone(), approver, MemoryStore::default());
    let control = RunControl::new();
    let (events, mut sink) = collector();

    let mut req = request("run-1", "lis");
    req.chat_id = Some("chat-1".to_string());
    let state = loop_.run(req, &control, &mut sink).await.unwrap();

    assert_eq!(state.status, RunStatus::Done);
    assert!(tools.executed().is_empty());
    assert!(has_event(&events, |e| matches!(e, AgentEvent::Done { .. })));
}

#[tokio::test]
async fn stream_ended_is_retried_only_once() {
    let failed = Terminal::Failed {
        code: "STREAM_ENDED".to_string(),
        message: "flux coupé".to_string(),
        retryable: true,
    };
    let mut outcome = completed("chat-1", "", vec![]);
    outcome.terminal = failed;
    let backend = FakeBackend::new(vec![
        outcome.clone(),
        outcome.clone(),
        outcome.clone(),
        outcome,
    ]);
    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools, approver, MemoryStore::default());
    let control = RunControl::new();
    let (_events, mut sink) = collector();

    let mut req = request("run-1", "test");
    req.chat_id = Some("chat-1".to_string());
    let state = loop_.run(req, &control, &mut sink).await.unwrap();

    assert_eq!(state.status, RunStatus::Failed);
    // Premier tour + une seule reprise.
    assert_eq!(backend.requests().len(), 2);
}

#[tokio::test]
async fn budget_warning_stops_before_executing_more_tools() {
    let backend = FakeBackend::new(vec![
        completed("chat-1", "msg-1", vec![call("c1", "read_file")]),
        completed("chat-1", "msg-2", vec![]),
    ]);
    let tools = FakeTools::new(&["read_file"], "contenu");
    let approver = Arc::new(BudgetDenier(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    }));
    let mut policy = policy();
    policy.budget_warning_at = 1;
    let loop_ = engine_with_policy(
        backend.clone(),
        tools.clone(),
        approver,
        MemoryStore::default(),
        policy,
    );
    let control = RunControl::new();
    let (events, mut sink) = collector();

    let state = loop_
        .run(request("run-1", "lis"), &control, &mut sink)
        .await
        .unwrap();

    assert_eq!(state.status, RunStatus::Cancelled);
    assert!(tools.executed().is_empty());
    assert!(backend.requests().len() == 1);
    assert!(has_event(&events, |e| matches!(
        e,
        AgentEvent::BudgetWarning { .. }
    )));
}

#[tokio::test]
async fn usage_is_reported_after_each_turn() {
    let backend = FakeBackend::new(vec![completed("chat-1", "msg-1", vec![])]);
    backend.set_usage(fs_agent::ContextUsage {
        percent: Some(42.0),
        used_tokens: Some(4200),
        max_tokens: Some(10_000),
    });
    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend, tools, approver, MemoryStore::default());
    let control = RunControl::new();
    let (events, mut sink) = collector();

    loop_
        .run(request("run-1", "test"), &control, &mut sink)
        .await
        .unwrap();

    assert!(has_event(&events, |e| matches!(
        e,
        AgentEvent::Usage { percent: Some(p), .. } if (*p - 42.0).abs() < f64::EPSILON
    )));
}

#[tokio::test]
async fn cancellation_during_stream_stops_immediately() {
    struct HangingBackend;

    #[async_trait]
    impl ChatBackend for HangingBackend {
        async fn turn(
            &self,
            _input: &TurnInput,
            control: &RunControl,
            _sink: &mut dyn EventSink,
        ) -> Result<TurnOutcome, BackendError> {
            while !control.is_cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            Ok(TurnOutcome {
                chat_id: Some("chat-1".to_string()),
                message_id: None,
                content: String::new(),
                tool_calls: Vec::new(),
                prompt_tokens: None,
                output_tokens: None,
                terminal: Terminal::Cancelled,
                last_event_id: None,
            })
        }
    }

    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = AgentLoop::new(
        Arc::new(HangingBackend),
        tools,
        approver,
        Arc::new(MemoryStore::default()),
        policy(),
    );
    let control = RunControl::new();
    let canceller = control.clone();
    let (events, mut sink) = collector();

    let started = std::time::Instant::now();
    let task = tokio::spawn(async move {
        let state = loop_
            .run(request("run-1", "test"), &control, &mut sink)
            .await
            .unwrap();
        (state, started.elapsed())
    });

    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    canceller.cancel();
    let (state, elapsed) = task.await.unwrap();

    assert_eq!(state.status, RunStatus::Cancelled);
    assert!(elapsed < std::time::Duration::from_secs(2), "durée = {elapsed:?}");
    assert!(has_event(&events, |e| matches!(e, AgentEvent::Cancelled)));
}

#[tokio::test]
async fn locked_chat_is_not_retried() {
    let failed = Terminal::Failed {
        code: "CHAT_LOCKED".to_string(),
        message: String::new(),
        retryable: true,
    };
    let mut outcome = completed("chat-1", "", vec![]);
    outcome.terminal = failed;
    let backend = FakeBackend::new(vec![outcome]);
    let tools = FakeTools::new(&["read_file"], "ok");
    let approver = Arc::new(FixedApprover {
        decision: Approval::Allow,
        confirm: true,
    });
    let loop_ = engine(backend.clone(), tools, approver, MemoryStore::default());
    let control = RunControl::new();
    let (events, mut sink) = collector();

    let mut req = request("run-1", "test");
    req.chat_id = Some("chat-1".to_string());
    let state = loop_.run(req, &control, &mut sink).await.unwrap();

    assert_eq!(state.status, RunStatus::Failed);
    assert_eq!(backend.requests().len(), 1);
    assert!(has_event(&events, |e| matches!(
        e,
        AgentEvent::Failed { code, retryable: false, .. } if code == "CHAT_LOCKED"
    )));
}
