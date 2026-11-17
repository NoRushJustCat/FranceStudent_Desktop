//! Machine à états d'un run : streaming → appels → confirmation → exécution →
//! résultats → streaming, avec persistance et reprise.

use std::sync::Arc;
use std::time::Duration;

use fs_api::ToolCall;

use crate::approval::{Approval, ApprovalRequest, Approver};
use crate::backend::{
    BackendError, ChatBackend, ChatSnapshot, ContextUsage, EventSink, Terminal, TurnInput,
    TurnOutcome,
};
use crate::model::{
    now_iso, AgentEvent, Decision, Origin, PendingCall, RunControl, RunRequest, RunState,
    RunStatus,
};
use crate::policy::LoopPolicy;
use crate::protocol;
use crate::store::RunStore;
use crate::tools::{parse_arguments, ToolContext, ToolExecution, ToolExecutor};
use crate::AgentError;

/// Suite à donner après un tour terminé.
enum Step {
    /// Envoyer ce tour (résultats d'outils) puis continuer.
    Continue(TurnInput),
    /// Le run est terminal (fait, annulé ou en échec) : rendre l'état.
    Stop,
}

/// Boucle d'agent indépendante de Tauri et du réseau.
pub struct AgentLoop {
    backend: Arc<dyn ChatBackend>,
    tools: Arc<dyn ToolExecutor>,
    approver: Arc<dyn Approver>,
    store: Arc<dyn RunStore>,
    policy: LoopPolicy,
}

impl AgentLoop {
    pub fn new(
        backend: Arc<dyn ChatBackend>,
        tools: Arc<dyn ToolExecutor>,
        approver: Arc<dyn Approver>,
        store: Arc<dyn RunStore>,
        policy: LoopPolicy,
    ) -> Self {
        Self {
            backend,
            tools,
            approver,
            store,
            policy,
        }
    }

    pub fn policy(&self) -> &LoopPolicy {
        &self.policy
    }

    /// Lance un run complet et renvoie son état final (persisté).
    pub async fn run(
        &self,
        request: RunRequest,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<RunState, AgentError> {
        let mut state = RunState::new(&request, now_iso());
        self.store.save(&state).await?;

        if control.is_cancelled() {
            return self.finish_cancelled(state, sink).await;
        }

        state.status = RunStatus::Streaming;
        state.step = 0;
        state.touch();
        self.store.save(&state).await?;

        let input = TurnInput::Start {
            chat_id: request.chat_id.clone(),
            agent_id: request.agent_id,
            content: request.content.clone(),
            tools: request.tools.clone(),
            options: request.options.clone(),
        };
        self.drive(state, input, &request, control, sink).await
    }

    /// Reprise sûre d'un run interrompu (redémarrage, coupure réseau, appel en
    /// attente). Un appel commencé sans sortie n'est **jamais** relancé : la
    /// boucle s'arrête et signale `RunInterrupted`.
    pub async fn resume(
        &self,
        run_id: &str,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<RunState, AgentError> {
        let mut state = self
            .store
            .load(run_id)
            .await?
            .ok_or_else(|| AgentError::NotFound(run_id.to_string()))?;
        if state.status.is_terminal() {
            return Ok(state);
        }

        sink.emit(AgentEvent::Resumed {
            run_id: state.run_id.clone(),
        });

        let request = self.request_from(&state);

        // Effet de bord possible : ne pas rejouer automatiquement.
        if let Some(call) = state
            .pending_calls
            .iter()
            .find(|call| call.started_at.is_some() && call.output.is_none())
        {
            sink.emit(AgentEvent::RunInterrupted {
                run_id: state.run_id.clone(),
                reason: format!(
                    "L'outil « {} » a peut-être déjà été exécuté avant l'interruption : \
                     il ne sera pas relancé automatiquement. Vérifie son effet, puis \
                     relance explicitement ou abandonne le run.",
                    call.name
                ),
            });
            return Ok(state);
        }

        // Outils en attente : les exécuter puis envoyer les résultats.
        if !state.pending_calls.is_empty()
            && matches!(state.status, RunStatus::AwaitingTools | RunStatus::Executing)
        {
            if !self
                .execute_pending(&mut state, &request, control, sink)
                .await?
            {
                return Ok(state);
            }
            let results = protocol::finalize_results(&state.pending_calls, &self.policy);
            let Some(chat_id) = state.chat_id.clone() else {
                return self
                    .fail(
                        state,
                        "NO_CHAT",
                        "Run sans chat : impossible d'envoyer les résultats.".to_string(),
                        false,
                        sink,
                    )
                    .await;
            };
            state.results_sent = true;
            state.status = RunStatus::SendingResults;
            state.touch();
            self.store.save(&state).await?;

            let input = TurnInput::Results {
                chat_id,
                results,
                options: request.options.clone(),
            };
            return self.drive(state, input, &request, control, sink).await;
        }

        // Résultats exécutés mais pas encore envoyés.
        if !state.pending_calls.is_empty() && !state.results_sent {
            if let Some(chat_id) = state.chat_id.clone() {
                let results = protocol::finalize_results(&state.pending_calls, &self.policy);
                state.status = RunStatus::SendingResults;
                state.results_sent = true;
                state.touch();
                self.store.save(&state).await?;
                let input = TurnInput::Results {
                    chat_id,
                    results,
                    options: request.options.clone(),
                };
                return self.drive(state, input, &request, control, sink).await;
            }
        }

        // Sinon : ré-attachement au flux en cours.
        let Some(chat_id) = state.chat_id.clone() else {
            state.status = RunStatus::Abandoned;
            state.touch();
            self.store.save(&state).await?;
            sink.emit(AgentEvent::RunInterrupted {
                run_id: state.run_id.clone(),
                reason: "Le chat n'a pas été créé : renvoie le message.".to_string(),
            });
            return Ok(state);
        };

        let input = TurnInput::Resume {
            chat_id,
            last_event_id: state.last_event_id.clone(),
        };
        self.drive(state, input, &request, control, sink).await
    }

    /// Reprise **explicite** après confirmation de l'utilisateur : relance aussi
    /// les appels interrompus en cours d'exécution (risque de double effet).
    pub async fn resume_forcing(
        &self,
        run_id: &str,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<RunState, AgentError> {
        if let Some(mut state) = self.store.load(run_id).await? {
            let mut changed = false;
            for call in &mut state.pending_calls {
                if call.started_at.is_some() && call.output.is_none() {
                    call.started_at = None;
                    changed = true;
                }
            }
            if changed {
                state.touch();
                self.store.save(&state).await?;
            }
        }
        self.resume(run_id, control, sink).await
    }

    /// Abandonne explicitement un run non terminal.
    pub async fn abandon(&self, run_id: &str) -> Result<RunState, AgentError> {
        let mut state = self
            .store
            .load(run_id)
            .await?
            .ok_or_else(|| AgentError::NotFound(run_id.to_string()))?;
        if !state.status.is_terminal() {
            state.status = RunStatus::Abandoned;
            state.touch();
            self.store.save(&state).await?;
        }
        Ok(state)
    }

    pub async fn list_open(&self) -> Result<Vec<RunState>, AgentError> {
        Ok(self.store.list_open().await?)
    }

    // ---------- interne ----------

    fn request_from(&self, state: &RunState) -> RunRequest {
        RunRequest {
            run_id: state.run_id.clone(),
            chat_id: state.chat_id.clone(),
            agent_id: state.agent_id,
            content: state.user_content.clone(),
            workdir: state.workdir.as_ref().map(std::path::PathBuf::from),
            auto_accept: state.auto_accept,
            eco: self.policy.eco,
            tools: state.tools.clone(),
            options: fs_api::StreamOptions {
                images: state.images.clone(),
                files: state.files.clone(),
                enable_web_search: state.enable_web_search,
            },
        }
    }

    async fn drive(
        &self,
        mut state: RunState,
        mut input: TurnInput,
        request: &RunRequest,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<RunState, AgentError> {
        let mut retries = 0u32;

        loop {
            if control.is_cancelled() {
                return self.finish_cancelled(state, sink).await;
            }

            if state.step >= self.policy.max_steps {
                return self
                    .fail(
                        state,
                        "MAX_STEPS",
                        format!(
                            "Arrêt après {} appels d'outils sur un seul message.",
                            self.policy.max_steps
                        ),
                        false,
                        sink,
                    )
                    .await;
            }

            match &input {
                TurnInput::Start { .. } => state.status = RunStatus::Streaming,
                TurnInput::Results { .. } => state.status = RunStatus::SendingResults,
                TurnInput::Resume { .. } => state.status = RunStatus::Resuming,
            }
            state.touch();
            self.store.save(&state).await?;

            let outcome = match self.backend.turn(&input, control, sink).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    // 409/404 = génération déjà en cours ou flux disparu : réconcilier.
                    if matches!(error.status(), Some(404) | Some(409)) {
                        if let Some(chat_id) = state.chat_id.clone() {
                            match self
                                .reconcile(&mut state, &chat_id, request, control, sink)
                                .await?
                            {
                                Step::Continue(next) => {
                                    input = next;
                                    continue;
                                }
                                Step::Stop => return Ok(state),
                            }
                        }
                    }

                    let (code, message, retryable) =
                        classify(backend_failure(&error, !request.tools.is_empty()));
                    if retryable
                        && state.chat_id.is_some()
                        && retries < self.retry_limit(&code)
                    {
                        retries += 1;
                        if !self
                            .retry_wait(&mut state, retries, control, sink)
                            .await?
                        {
                            return Ok(state);
                        }
                        input = TurnInput::Resume {
                            chat_id: state.chat_id.clone().unwrap_or_default(),
                            last_event_id: state.last_event_id.clone(),
                        };
                        continue;
                    }
                    return self.fail(state, code, message, retryable, sink).await;
                }
            };

            if let Some(chat_id) = outcome.chat_id.clone() {
                state.chat_id = Some(chat_id);
            }
            if outcome.last_event_id.is_some() {
                state.last_event_id = outcome.last_event_id.clone();
            }
            state.usage.api_calls += 1;
            state.usage.prompt_tokens += outcome.prompt_tokens.unwrap_or(0);
            state.usage.output_tokens += outcome.output_tokens.unwrap_or(0);
            state.touch();
            self.store.save(&state).await?;

            self.report_usage(&state, sink).await;

            match outcome.terminal.clone() {
                Terminal::Cancelled => return self.finish_cancelled(state, sink).await,
                Terminal::Failed {
                    code,
                    message,
                    retryable,
                } => {
                    let (code, message, retryable) = classify((code, message, retryable));
                    if retryable
                        && state.chat_id.is_some()
                        && retries < self.retry_limit(&code)
                    {
                        retries += 1;
                        if !self
                            .retry_wait(&mut state, retries, control, sink)
                            .await?
                        {
                            return Ok(state);
                        }
                        input = TurnInput::Resume {
                            chat_id: state.chat_id.clone().unwrap_or_default(),
                            last_event_id: state.last_event_id.clone(),
                        };
                        continue;
                    }
                    return self.fail(state, code, message, retryable, sink).await;
                }
                Terminal::Completed => {
                    retries = 0;
                    match self
                        .after_completed(&mut state, outcome, request, control, sink)
                        .await?
                    {
                        Step::Continue(next) => input = next,
                        Step::Stop => return Ok(state),
                    }
                }
            }
        }
    }

    /// Traite un tour `completed` : fin, ou exécution des outils + résultats.
    async fn after_completed(
        &self,
        state: &mut RunState,
        outcome: TurnOutcome,
        request: &RunRequest,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<Step, AgentError> {
        if outcome.tool_calls.is_empty() {
            let chat_id = state.chat_id.clone().unwrap_or_default();
            let message_id = outcome.message_id.clone().unwrap_or_default();
            state.status = RunStatus::Done;
            state.touch();
            self.store.save(state).await?;
            sink.emit(AgentEvent::Done {
                chat_id,
                message_id,
                prompt_tokens: outcome.prompt_tokens,
                output_tokens: outcome.output_tokens,
                total_prompt_tokens: state.usage.prompt_tokens,
                total_output_tokens: state.usage.output_tokens,
                api_calls: state.usage.api_calls,
                eco: self.policy.eco,
            });
            return Ok(Step::Stop);
        }

        let tool_calls = outcome.tool_calls;

        if !self.budget_ok(state, control, sink).await? {
            return Ok(Step::Stop);
        }

        if let Err(error) = protocol::validate_calls(&tool_calls, self.policy.max_results_per_turn)
        {
            return self
                .fail_here(state, "PROTOCOL", error.to_string(), false, sink)
                .await;
        }

        state.status = RunStatus::AwaitingTools;
        state.step += 1;
        state.pending_calls = tool_calls
            .iter()
            .map(|call| {
                PendingCall::new(
                    call.call_id.clone(),
                    call.name.clone(),
                    call.arguments.clone(),
                )
            })
            .collect();
        state.results_sent = false;
        state.touch();
        self.store.save(state).await?;
        sink.emit(AgentEvent::AwaitingTools {
            run_id: state.run_id.clone(),
            calls: state.pending_calls.len(),
        });

        if !self.execute_pending(state, request, control, sink).await? {
            return Ok(Step::Stop);
        }

        let results = protocol::finalize_results(&state.pending_calls, &self.policy);
        if let Err(error) = protocol::verify_results(&tool_calls, &results) {
            return self
                .fail_here(state, "PROTOCOL", error.to_string(), false, sink)
                .await;
        }

        let Some(chat_id) = state.chat_id.clone() else {
            return self
                .fail_here(
                    state,
                    "NO_CHAT",
                    "Réponse complète sans identifiant de chat.".to_string(),
                    false,
                    sink,
                )
                .await;
        };

        state.results_sent = true;
        state.status = RunStatus::SendingResults;
        state.touch();
        self.store.save(state).await?;

        Ok(Step::Continue(TurnInput::Results {
            chat_id,
            results,
            options: request.options.clone(),
        }))
    }

    /// 409 : relit le chat et réconcilie (appels en attente ou fin de génération).
    async fn reconcile(
        &self,
        state: &mut RunState,
        chat_id: &str,
        request: &RunRequest,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<Step, AgentError> {
        let snapshot: ChatSnapshot = match self.backend.fetch_chat(chat_id).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let (code, message, retryable) =
                    classify(backend_failure(&error, !request.tools.is_empty()));
                return self
                    .fail_here(state, code, message, retryable, sink)
                    .await;
            }
        };

        if snapshot.pending_tool_calls.is_empty() {
            // La génération s'est terminée sans que le flux soit lu.
            state.status = RunStatus::Done;
            state.touch();
            self.store.save(state).await?;
            sink.emit(AgentEvent::Done {
                chat_id: chat_id.to_string(),
                message_id: String::new(),
                prompt_tokens: None,
                output_tokens: None,
                total_prompt_tokens: state.usage.prompt_tokens,
                total_output_tokens: state.usage.output_tokens,
                api_calls: state.usage.api_calls,
                eco: self.policy.eco,
            });
            return Ok(Step::Stop);
        }

        if let Err(error) =
            protocol::validate_calls(&snapshot.pending_tool_calls, self.policy.max_results_per_turn)
        {
            return self
                .fail_here(state, "PROTOCOL", error.to_string(), false, sink)
                .await;
        }

        // Réutilise les sorties déjà obtenues pour les mêmes `callId`.
        let previous = std::mem::take(&mut state.pending_calls);
        state.pending_calls = snapshot
            .pending_tool_calls
            .iter()
            .map(|call| {
                let mut pending = PendingCall::new(
                    call.call_id.clone(),
                    call.name.clone(),
                    call.arguments.clone(),
                );
                if let Some(done) = previous
                    .iter()
                    .find(|p| p.call_id == call.call_id && p.output.is_some())
                {
                    pending.decision = done.decision;
                    pending.started_at = done.started_at.clone();
                    pending.executed_at = done.executed_at.clone();
                    pending.output = done.output.clone();
                    pending.is_error = done.is_error;
                    pending.denied = done.denied;
                }
                pending
            })
            .collect();
        state.status = RunStatus::AwaitingTools;
        state.results_sent = false;
        state.touch();
        self.store.save(state).await?;
        sink.emit(AgentEvent::AwaitingTools {
            run_id: state.run_id.clone(),
            calls: state.pending_calls.len(),
        });

        if !self.execute_pending(state, request, control, sink).await? {
            return Ok(Step::Stop);
        }

        let results = protocol::finalize_results(&state.pending_calls, &self.policy);
        if let Err(error) = protocol::verify_results(&snapshot.pending_tool_calls, &results) {
            return self
                .fail_here(state, "PROTOCOL", error.to_string(), false, sink)
                .await;
        }

        state.results_sent = true;
        state.status = RunStatus::SendingResults;
        state.touch();
        self.store.save(state).await?;

        Ok(Step::Continue(TurnInput::Results {
            chat_id: chat_id.to_string(),
            results,
            options: request.options.clone(),
        }))
    }

    /// `GET /chats/{id}/usage` après chaque tour ; jamais bloquant.
    async fn report_usage(&self, state: &RunState, sink: &mut dyn EventSink) {
        let Some(chat_id) = state.chat_id.clone() else {
            return;
        };
        let Ok(Some(usage)) = self.backend.chat_usage(&chat_id).await else {
            return;
        };
        let ContextUsage {
            percent,
            used_tokens,
            max_tokens,
        } = usage;
        let percent = percent.or_else(|| match (used_tokens, max_tokens) {
            (Some(used), Some(max)) if max > 0 => Some(used as f64 / max as f64 * 100.0),
            _ => None,
        });
        sink.emit(AgentEvent::Usage {
            chat_id,
            percent,
            used_tokens,
        });
    }

    /// Au-delà du seuil de requêtes, demande une fois « continuer ? ».
    async fn budget_ok(
        &self,
        state: &mut RunState,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<bool, AgentError> {
        if state.budget_warned || state.usage.api_calls < self.policy.budget_warning_at {
            return Ok(true);
        }
        if control.is_cancelled() {
            self.save_cancelled(state).await?;
            sink.emit(AgentEvent::Cancelled);
            return Ok(false);
        }
        state.budget_warned = true;
        state.touch();
        self.store.save(state).await?;
        sink.emit(AgentEvent::BudgetWarning {
            run_id: state.run_id.clone(),
            requests: state.usage.api_calls,
        });
        if self
            .approver
            .confirm_budget(&state.run_id, state.usage.api_calls)
            .await
        {
            return Ok(true);
        }
        self.save_cancelled(state).await?;
        sink.emit(AgentEvent::Cancelled);
        Ok(false)
    }

    /// Traite les appels du tour. `false` = arrêt (état déjà persisté).
    async fn execute_pending(
        &self,
        state: &mut RunState,
        request: &RunRequest,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<bool, AgentError> {
        let calls: Vec<ToolCall> = state
            .pending_calls
            .iter()
            .map(|pending| ToolCall {
                call_id: pending.call_id.clone(),
                name: pending.name.clone(),
                arguments: pending.arguments.clone(),
            })
            .collect();

        for (index, call) in calls.iter().enumerate() {
            if control.is_cancelled() {
                self.save_cancelled(state).await?;
                sink.emit(AgentEvent::Cancelled);
                return Ok(false);
            }

            // Appel déjà exécuté avant une interruption : on rejoue l'affichage.
            if state.pending_calls[index].output.is_some() {
                let pending = &state.pending_calls[index];
                sink.emit(AgentEvent::ToolCall {
                    call_id: call.call_id.clone(),
                    name: call.name.clone(),
                    origin: origin_of(&call.name),
                    args: parse_arguments(&call.arguments),
                    needs_confirm: false,
                });
                sink.emit(AgentEvent::ToolResult {
                    call_id: call.call_id.clone(),
                    output: pending.output.clone().unwrap_or_default(),
                    is_error: pending.is_error,
                    denied: pending.denied,
                });
                continue;
            }

            let args = parse_arguments(&call.arguments);
            let origin = origin_of(&call.name);

            // Les outils `mcp__*` sont exécutés par France Student, jamais ici.
            if origin == Origin::Mcp {
                let message =
                    "Outil de plateforme exécuté côté France Student : non exécuté localement."
                        .to_string();
                sink.emit(AgentEvent::ToolCall {
                    call_id: call.call_id.clone(),
                    name: call.name.clone(),
                    origin,
                    args: args.clone(),
                    needs_confirm: false,
                });
                self.store_pending(
                    state,
                    index,
                    Decision::Unknown,
                    message.clone(),
                    true,
                    false,
                    sink,
                )
                .await?;
                continue;
            }

            let Some(spec) = self.tools.spec(&call.name) else {
                let message = format!("Outil inconnu : {}.", call.name);
                sink.emit(AgentEvent::ToolCall {
                    call_id: call.call_id.clone(),
                    name: call.name.clone(),
                    origin,
                    args: args.clone(),
                    needs_confirm: false,
                });
                self.store_pending(
                    state,
                    index,
                    Decision::Unknown,
                    message,
                    true,
                    false,
                    sink,
                )
                .await?;
                continue;
            };

            let approval_request = ApprovalRequest {
                run_id: state.run_id.clone(),
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                args: args.clone(),
                workdir: request.workdir.clone(),
                auto_accept: request.auto_accept,
                risk: spec.risk,
            };

            let decision = if self.policy.confirm_read_only
                && spec.risk == crate::tools::Risk::ReadOnly
            {
                Approval::Ask
            } else {
                self.approver.decide(&approval_request).await
            };

            let needs_confirm = matches!(decision, Approval::Ask);
            sink.emit(AgentEvent::ToolCall {
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                origin,
                args: args.clone(),
                needs_confirm,
            });

            let (execution, decided, denied) = if let Approval::Deny(message) = decision {
                (ToolExecution::error(message), Decision::Denied, true)
            } else {
                let allowed = match decision {
                    Approval::Allow => true,
                    Approval::Ask => self.approver.confirm(&approval_request).await,
                    Approval::Deny(_) => unreachable!(),
                };
                if control.is_cancelled() {
                    self.save_cancelled(state).await?;
                    sink.emit(AgentEvent::Cancelled);
                    return Ok(false);
                }
                if allowed {
                    self.mark_started(state, index).await?;
                    (
                        self.execute_tool(call, state, request).await,
                        Decision::Allowed,
                        false,
                    )
                } else {
                    (
                        ToolExecution::error("Refusé par l'utilisateur.".to_string()),
                        Decision::Denied,
                        true,
                    )
                }
            };

            if control.is_cancelled() {
                self.save_cancelled(state).await?;
                sink.emit(AgentEvent::Cancelled);
                return Ok(false);
            }

            if let Some(notice) = execution.notice.as_ref() {
                sink.emit(AgentEvent::Notice {
                    text: notice.clone(),
                });
            }
            self.store_pending(
                state,
                index,
                decided,
                execution.output,
                execution.is_error,
                denied,
                sink,
            )
            .await?;
        }

        Ok(true)
    }

    /// Marque un appel comme commencé : un crash après ce point ne le rejoue pas.
    async fn mark_started(&self, state: &mut RunState, index: usize) -> Result<(), AgentError> {
        {
            let pending = &mut state.pending_calls[index];
            pending.decision = Some(Decision::Allowed);
            pending.started_at = Some(now_iso());
        }
        state.status = RunStatus::Executing;
        state.touch();
        self.store.save(state).await?;
        Ok(())
    }

    async fn execute_tool(
        &self,
        call: &ToolCall,
        state: &RunState,
        request: &RunRequest,
    ) -> ToolExecution {
        let context = ToolContext {
            run_id: state.run_id.clone(),
            call_id: call.call_id.clone(),
            workdir: request.workdir.clone(),
        };
        self.tools.execute(call, &context).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn store_pending(
        &self,
        state: &mut RunState,
        index: usize,
        decision: Decision,
        output: String,
        is_error: bool,
        denied: bool,
        sink: &mut dyn EventSink,
    ) -> Result<(), AgentError> {
        let call_id = state.pending_calls[index].call_id.clone();
        {
            let pending = &mut state.pending_calls[index];
            pending.decision = Some(decision);
            pending.output = Some(output.clone());
            pending.is_error = is_error;
            pending.denied = denied;
            pending.executed_at = Some(now_iso());
        }
        state.touch();
        self.store.save(state).await?;
        sink.emit(AgentEvent::ToolResult {
            call_id,
            output,
            is_error,
            denied,
        });
        Ok(())
    }

    async fn retry_wait(
        &self,
        state: &mut RunState,
        retries: u32,
        control: &RunControl,
        sink: &mut dyn EventSink,
    ) -> Result<bool, AgentError> {
        if control.is_cancelled() {
            self.save_cancelled(state).await?;
            sink.emit(AgentEvent::Cancelled);
            return Ok(false);
        }
        state.status = RunStatus::Resuming;
        state.touch();
        self.store.save(state).await?;
        sink.emit(AgentEvent::Notice {
            text: format!(
                "Connexion interrompue, reprise ({}/{})…",
                retries,
                self.policy.max_retries
            ),
        });
        if control.is_cancelled() {
            self.save_cancelled(state).await?;
            sink.emit(AgentEvent::Cancelled);
            return Ok(false);
        }
        let delay = self
            .policy
            .retry_delay_ms
            .saturating_mul(1u64 << (retries.saturating_sub(1)).min(10));
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        Ok(true)
    }

    fn retry_limit(&self, code: &str) -> u32 {
        if code == "STREAM_ENDED" {
            self.policy.max_stream_ended_retries
        } else {
            self.policy.max_retries
        }
    }

    async fn save_cancelled(&self, state: &mut RunState) -> Result<(), AgentError> {
        state.status = RunStatus::Cancelled;
        state.touch();
        self.store.save(state).await?;
        Ok(())
    }

    async fn finish_cancelled(
        &self,
        mut state: RunState,
        sink: &mut dyn EventSink,
    ) -> Result<RunState, AgentError> {
        self.save_cancelled(&mut state).await?;
        sink.emit(AgentEvent::Cancelled);
        Ok(state)
    }

    /// Comme `fail`, depuis un état emprunté (fin d'un tour intermédiaire).
    async fn fail_here(
        &self,
        state: &mut RunState,
        code: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
        sink: &mut dyn EventSink,
    ) -> Result<Step, AgentError> {
        let code = code.into();
        let message = message.into();
        state.status = RunStatus::Failed;
        state.touch();
        self.store.save(state).await?;
        sink.emit(AgentEvent::Failed {
            code,
            message,
            retryable,
        });
        Ok(Step::Stop)
    }

    async fn fail(
        &self,
        mut state: RunState,
        code: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
        sink: &mut dyn EventSink,
    ) -> Result<RunState, AgentError> {
        let code = code.into();
        let message = message.into();
        state.status = RunStatus::Failed;
        state.touch();
        self.store.save(&state).await?;
        sink.emit(AgentEvent::Failed {
            code,
            message,
            retryable,
        });
        Ok(state)
    }
}

fn origin_of(name: &str) -> Origin {
    if name.starts_with("mcp__") {
        Origin::Mcp
    } else {
        Origin::Local
    }
}

/// Codes connus : message dédié et jamais de reprise automatique.
fn classify((code, message, retryable): (String, String, bool)) -> (String, String, bool) {
    let dedicated = match code.as_str() {
        "CHAT_LOCKED" => Some("Ce chat est verrouillé. Déverrouille-le dans France Student."),
        "CHAT_DELETED" => Some("Ce chat a été supprimé : le message ne peut pas être envoyé."),
        "CHAT_ARCHIVED" => Some("Ce chat est archivé. Restaure-le avant de continuer."),
        "CHAT_BUSY" => Some("Une génération est déjà en cours dans ce chat."),
        "CONTENT_POLICY_VIOLATION" => {
            Some("Le serveur a refusé ce contenu (politique de contenu).")
        }
        "MCP_TOO_MANY_TOOLS" => Some(
            "Trop d'outils MCP sont exposés à ce chat : désactive quelques serveurs avant de réessayer.",
        ),
        "BUDGET_EXHAUSTED" => {
            Some("Budget de chats épuisé : réessaie après la réinitialisation indiquée.")
        }
        "TOOLS_FORBIDDEN" => Some(
            "Les outils client sont refusés pour ce compte (flag `2026_11_client_tools` absent).",
        ),
        "MODEL_UNSUPPORTED" => {
            Some("Ce modèle ne gère pas les outils client. Choisis un modèle OpenAI ou Claude.")
        }
        _ => None,
    };
    match dedicated {
        Some(text) if message.is_empty() => (code, text.to_string(), false),
        Some(text) => (code, format!("{text} Détail : {message}"), false),
        None => (code, message, retryable),
    }
}

/// Traduit une erreur de transport en code lisible pour l'interface.
fn backend_failure(error: &BackendError, tools_on: bool) -> (String, String, bool) {
    match error {
        BackendError::Status { status: 403, body } if tools_on => (
            "TOOLS_FORBIDDEN".to_string(),
            format!(
                "Outils client refusés (HTTP 403) : le flag `2026_11_client_tools` n'est probablement pas activé pour ton compte. Détail : {}",
                truncate(body, 300)
            ),
            false,
        ),
        BackendError::Status { status: 400, body }
            if body.contains("not supported by this model") =>
        {
            (
                "MODEL_UNSUPPORTED".to_string(),
                "Ce modèle ne gère pas les outils client (Gemini). Choisis un modèle OpenAI ou Claude."
                    .to_string(),
                false,
            )
        }
        BackendError::Status { status: 400, body } => (
            "BAD_TOOL_CALL".to_string(),
            format!("Requête refusée (HTTP 400) : {}", truncate(body, 600)),
            false,
        ),
        BackendError::Status { status: 409, body } => (
            "CHAT_BUSY".to_string(),
            format!("Génération déjà en cours (HTTP 409) : {}", truncate(body, 300)),
            false,
        ),
        BackendError::Status { status: 429, body } => (
            "RATE_LIMITED".to_string(),
            match reset_hint(body) {
                Some(reset) => format!("Quota atteint (HTTP 429), réessai possible à {reset}."),
                None => format!("Quota atteint (HTTP 429) : {}", truncate(body, 300)),
            },
            false,
        ),
        BackendError::Status { status, body } => (
            format!("HTTP_{status}"),
            format!("HTTP {status} : {}", truncate(body, 400)),
            matches!(*status, 408 | 425) || *status >= 500,
        ),
        BackendError::Transport(message) => ("NETWORK".to_string(), message.clone(), true),
        BackendError::Stream(message) => ("STREAM".to_string(), message.clone(), true),
        BackendError::Unsupported(message) => {
            ("UNSUPPORTED".to_string(), message.clone(), false)
        }
        BackendError::Other(message) => ("BACKEND".to_string(), message.clone(), false),
    }
}

/// `resetAt` / `reset_at` d'un corps d'erreur, s'il est présent.
fn reset_hint(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    for key in ["resetAt", "reset_at", "retryAfter", "retry_after"] {
        if let Some(found) = value.get(key) {
            return Some(match found {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            });
        }
    }
    None
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max).collect();
    out.push('…');
    out
}
