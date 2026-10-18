//! Agent orchestration: one user request → bounded model/tool loop → outcome.
//!
//! Rust owns every decision that matters for safety: which tools exist, whether a
//! proposed call is valid, whether it may run now or needs the user's explicit
//! confirmation, how many steps run, deduplication and cancellation. The model only proposes.
//!
//! A request that reaches a high-risk action pauses (`Drive::NeedsConfirmation`). The paused
//! request is kept here with a single-use confirmation id; only `AgentState::confirm` with
//! that id can run the stored action, and only within `CONFIRMATION_TTL`.

pub mod gemini;
pub mod prompt;
pub mod registry;

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::tools::Risk;
use crate::tools::open_app::{AllowedApp, OpenResult};
use gemini::{ChatModel, ModelError, ProposedCall};
use registry::{ConfirmationText, Refusal, ValidatedCall};

/// Model calls per request. Each tool round-trip uses one; this bounds runaway loops.
pub const MAX_MODEL_CALLS: usize = 4;
pub const TASK_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a confirmation question stays answerable.
pub const CONFIRMATION_TTL: Duration = Duration::from_secs(120);
/// Completed requests kept as context (text only, in memory, never saved).
const HISTORY_TURNS: usize = 6;
pub const MAX_REQUEST_CHARS: usize = 2000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionReport {
    pub tool: &'static str,
    /// The tool ran without an error.
    pub ok: bool,
    /// The intended outcome was observed afterwards.
    pub verified: bool,
    /// Practice mode: nothing real happened.
    pub practice: bool,
    /// Plain-language description for the transcript.
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmationRequest {
    pub id: String,
    #[serde(flatten)]
    pub text: ConfirmationText,
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentOutcome {
    pub reply: String,
    /// Real actions performed during this request, as observed by Rust (not as claimed by the model).
    pub actions: Vec<ActionReport>,
    pub cancelled: bool,
    /// Set when the request is paused waiting for the user's explicit yes or no.
    pub confirmation: Option<ConfirmationRequest>,
}

impl AgentOutcome {
    fn reply(reply: impl Into<String>, actions: Vec<ActionReport>) -> Self {
        Self {
            reply: reply.into(),
            actions,
            cancelled: false,
            confirmation: None,
        }
    }
}

/// Executes validated tool calls. A trait so tests never touch the real desktop.
pub trait ToolExecutor {
    fn open_application(
        &self,
        app: AllowedApp,
    ) -> impl Future<Output = Result<OpenResult, String>> + Send;
}

pub struct DesktopExecutor;

impl ToolExecutor for DesktopExecutor {
    async fn open_application(&self, app: AllowedApp) -> Result<OpenResult, String> {
        tauri::async_runtime::spawn_blocking(move || crate::tools::open_app::open(app))
            .await
            .map_err(|_| "Something went wrong while opening the app.".to_string())?
    }
}

#[derive(Default)]
pub struct Cancel {
    flag: AtomicBool,
    notify: Notify,
}

impl Cancel {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        let notified = self.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable(); // register before checking, so a cancel can't slip between
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

fn user_step(text: &str) -> Value {
    json!({"type": "user_input", "content": [{"type": "text", "text": text}]})
}

fn model_text_step(text: &str) -> Value {
    json!({"type": "model_output", "content": [{"type": "text", "text": text}]})
}

fn function_result(call: &ProposedCall, result: Value, is_error: bool) -> Value {
    json!({
        "type": "function_result",
        "call_id": call.id,
        "name": call.name,
        "result": result,
        "is_error": is_error,
    })
}

const UNSURE_REPLY: &str =
    "Sorry, I'm not sure how to help with that. Could you say it another way?";
pub const TOO_MANY_STEPS_REPLY: &str =
    "Sorry, I couldn't finish that. Could you ask again in a simpler way?";
const EXPIRED_REPLY: &str =
    "I waited a while for your answer, so to be safe I didn't do it. Just ask again if you'd like.";

/// A high-risk call waiting for the user's answer.
#[derive(Debug, Clone)]
pub struct PendingAction {
    pub call: ProposedCall,
    pub validated: ValidatedCall,
}

pub enum Drive {
    /// Finished: outcome plus the text-only turns to append to history.
    Finished(AgentOutcome, [Value; 2]),
    Cancelled(Vec<ActionReport>),
    NeedsConfirmation(PendingAction),
}

/// One request's progress. Kept between `drive` calls while paused for confirmation.
pub struct TaskRun {
    utterance: String,
    input: Vec<Value>,
    actions: Vec<ActionReport>,
    done: HashSet<ValidatedCall>,
    model_calls: usize,
    /// Calls from the latest model turn not yet handled.
    queue: VecDeque<ProposedCall>,
    practice: bool,
}

impl TaskRun {
    pub fn new(history: &[Value], utterance: &str, practice: bool) -> Self {
        let mut input = history.to_vec();
        input.push(user_step(utterance));
        Self {
            utterance: utterance.to_string(),
            input,
            actions: Vec::new(),
            done: HashSet::new(),
            model_calls: 0,
            queue: VecDeque::new(),
            practice,
        }
    }

    fn finish(&mut self, reply: String) -> Drive {
        let turns = [user_step(&self.utterance), model_text_step(&reply)];
        Drive::Finished(
            AgentOutcome::reply(reply, std::mem::take(&mut self.actions)),
            turns,
        )
    }

    /// Advances until the request finishes, is cancelled, or needs the user's confirmation.
    pub async fn drive<M: ChatModel, E: ToolExecutor>(
        &mut self,
        model: &M,
        exec: &E,
        cancel: &Cancel,
        on_executing: &mut impl FnMut(&'static str),
    ) -> Result<Drive, ModelError> {
        loop {
            while let Some(call) = self.queue.pop_front() {
                if cancel.is_cancelled() {
                    return Ok(Drive::Cancelled(std::mem::take(&mut self.actions)));
                }
                let result = match registry::validate(&call.name, &call.arguments, self.practice) {
                    Err(Refusal(why)) => function_result(&call, json!({ "error": why }), true),
                    Ok(v) if self.done.contains(&v) => function_result(
                        &call,
                        json!({ "error": "Already done during this request, so it was not repeated." }),
                        true,
                    ),
                    Ok(v) if v.risk() == Risk::High => {
                        // One confirmed action at a time: anything else proposed alongside it waits.
                        for other in self.queue.drain(..) {
                            self.input.push(function_result(
                                &other,
                                json!({ "error": "Not run: an earlier action is waiting for the user's confirmation. Propose it again afterwards if still needed." }),
                                true,
                            ));
                        }
                        return Ok(Drive::NeedsConfirmation(PendingAction {
                            call,
                            validated: v,
                        }));
                    }
                    Ok(v) => self.execute(&call, v, exec, on_executing).await,
                };
                self.input.push(result);
            }

            if self.model_calls >= MAX_MODEL_CALLS {
                return Ok(self.finish(TOO_MANY_STEPS_REPLY.into()));
            }
            if cancel.is_cancelled() {
                return Ok(Drive::Cancelled(std::mem::take(&mut self.actions)));
            }
            let turn = tokio::select! {
                r = model.respond(&self.input) => r?,
                _ = cancel.cancelled() => return Ok(Drive::Cancelled(std::mem::take(&mut self.actions))),
            };
            self.model_calls += 1;
            self.input.extend(turn.steps.iter().cloned());
            if turn.calls.is_empty() {
                let reply = if turn.text.is_empty() {
                    UNSURE_REPLY.to_string()
                } else {
                    turn.text
                };
                return Ok(self.finish(reply));
            }
            self.queue.extend(turn.calls);
        }
    }

    /// Records the user's answer to a pending confirmation. Approval runs exactly the stored call.
    pub async fn resolve<E: ToolExecutor>(
        &mut self,
        pending: PendingAction,
        approved: bool,
        exec: &E,
        on_executing: &mut impl FnMut(&'static str),
    ) {
        let result = if approved {
            self.execute(&pending.call, pending.validated, exec, on_executing)
                .await
        } else {
            function_result(
                &pending.call,
                json!({ "declined": true, "note": "The user said no, so nothing was done. Acknowledge briefly; don't try again unless asked." }),
                false,
            )
        };
        self.input.push(result);
    }

    async fn execute<E: ToolExecutor>(
        &mut self,
        call: &ProposedCall,
        v: ValidatedCall,
        exec: &E,
        on_executing: &mut impl FnMut(&'static str),
    ) -> Value {
        self.done.insert(v.clone());
        on_executing(v.tool_name());
        match v {
            ValidatedCall::OpenApplication(app) => match exec.open_application(app).await {
                Ok(r) => {
                    self.actions.push(ActionReport {
                        tool: "open_application",
                        ok: true,
                        verified: r.verified,
                        practice: false,
                        summary: if r.verified {
                            format!("Opened {} (checked: it's in front)", r.app_name)
                        } else {
                            format!(
                                "Asked Windows to open {}, but couldn't confirm it appeared",
                                r.app_name
                            )
                        },
                    });
                    function_result(
                        call,
                        json!({ "verified": r.verified, "app_name": r.app_name }),
                        false,
                    )
                }
                Err(why) => {
                    self.actions.push(ActionReport {
                        tool: "open_application",
                        ok: false,
                        verified: false,
                        practice: false,
                        summary: why.clone(),
                    });
                    function_result(call, json!({ "error": why }), true)
                }
            },
            ValidatedCall::PracticeSendMessage { to, .. } => {
                self.actions.push(ActionReport {
                    tool: "practice_send_message",
                    ok: true,
                    verified: true,
                    practice: true,
                    summary: format!(
                        "Practice only: pretended to send a message to {to}. Nothing was sent."
                    ),
                });
                function_result(
                    call,
                    json!({ "practice": true, "sent": false, "note": "Practice mode: nothing was actually sent. Tell the user this was only practice." }),
                    false,
                )
            }
        }
    }
}

/// Unique, unguessable-enough id for one confirmation question (local app; uniqueness is what matters).
fn new_confirmation_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("c{n}-{nanos:x}")
}

struct Paused {
    id: String,
    run: TaskRun,
    pending: PendingAction,
    asked_at: Instant,
    /// The model that produced this request's steps; resumed requests must use the same one.
    model_id: &'static str,
}

/// App-wide agent state: one task at a time, cancellable, short in-memory history,
/// at most one request paused for confirmation.
pub struct AgentState {
    pub http: reqwest::Client,
    history: Mutex<Vec<Value>>,
    current: Mutex<Option<Arc<Cancel>>>,
    paused: Mutex<Option<Paused>>,
}

impl AgentState {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            history: Mutex::new(Vec::new()),
            current: Mutex::new(None),
            paused: Mutex::new(None),
        }
    }

    /// Stop: cancels the running request and forgets any unanswered confirmation.
    pub fn cancel_current(&self) {
        if let Some(c) = self.current.lock().expect("agent lock").as_ref() {
            c.cancel();
        }
        self.paused.lock().expect("agent lock").take();
    }

    fn begin(&self) -> Result<Arc<Cancel>, String> {
        let mut cur = self.current.lock().expect("agent lock");
        if cur.is_some() {
            return Err("I'm still working on your last request. Press Stop to cancel it.".into());
        }
        let c = Arc::new(Cancel::default());
        *cur = Some(c.clone());
        Ok(c)
    }

    /// Turns a drive result into what the UI gets, storing paused requests.
    fn settle(
        &self,
        result: Result<Result<Drive, ModelError>, tokio::time::error::Elapsed>,
        run: TaskRun,
        model_id: &'static str,
    ) -> Result<AgentOutcome, String> {
        *self.current.lock().expect("agent lock") = None;
        match result {
            Err(_) => Err("That took too long, so I stopped. Please try again.".into()),
            Ok(Err(e)) => Err(e.user_message()),
            Ok(Ok(Drive::Cancelled(actions))) => Ok(AgentOutcome {
                reply: String::new(),
                actions,
                cancelled: true,
                confirmation: None,
            }),
            Ok(Ok(Drive::Finished(outcome, turns))) => {
                let mut h = self.history.lock().expect("agent lock");
                h.extend(turns);
                let excess = h.len().saturating_sub(HISTORY_TURNS * 2);
                h.drain(..excess);
                Ok(outcome)
            }
            Ok(Ok(Drive::NeedsConfirmation(pending))) => {
                let id = new_confirmation_id();
                let request = ConfirmationRequest {
                    id: id.clone(),
                    text: registry::confirmation_text(&pending.validated),
                    expires_in_secs: CONFIRMATION_TTL.as_secs(),
                };
                let mut run = run;
                let actions = std::mem::take(&mut run.actions);
                *self.paused.lock().expect("agent lock") = Some(Paused {
                    id,
                    run,
                    pending,
                    asked_at: Instant::now(),
                    model_id,
                });
                Ok(AgentOutcome {
                    reply: String::new(),
                    actions,
                    cancelled: false,
                    confirmation: Some(request),
                })
            }
        }
    }

    pub async fn run<M: ChatModel, E: ToolExecutor>(
        &self,
        model: &M,
        exec: &E,
        utterance: &str,
        practice: bool,
        mut on_executing: impl FnMut(&'static str),
    ) -> Result<AgentOutcome, String> {
        let utterance = utterance.trim();
        if utterance.is_empty() || utterance.chars().count() > MAX_REQUEST_CHARS {
            return Err("That request was empty or too long.".into());
        }
        if self.paused.lock().expect("agent lock").is_some() {
            return Err("Please answer the question on the screen first, or press Stop.".into());
        }
        let cancel = self.begin()?;
        let history = self.history.lock().expect("agent lock").clone();
        let mut run = TaskRun::new(&history, utterance, practice);
        let result = tokio::time::timeout(
            TASK_TIMEOUT,
            run.drive(model, exec, &cancel, &mut on_executing),
        )
        .await;
        self.settle(result, run, model.model_id())
    }

    /// How a paused request must resume (model, practice mode), if one is paused under `id`.
    pub fn paused_info(&self, id: &str) -> Option<(&'static str, bool)> {
        self.paused
            .lock()
            .expect("agent lock")
            .as_ref()
            .filter(|p| p.id == id)
            .map(|p| (p.model_id, p.run.practice))
    }

    /// The user's answer. Only the current, unexpired, matching id is accepted, once.
    pub async fn confirm<M: ChatModel, E: ToolExecutor>(
        &self,
        model: &M,
        exec: &E,
        id: &str,
        approved: bool,
        mut on_executing: impl FnMut(&'static str),
    ) -> Result<AgentOutcome, String> {
        let paused = {
            let mut slot = self.paused.lock().expect("agent lock");
            match slot.as_ref() {
                Some(p) if p.id == id => slot.take().expect("checked"),
                _ => return Err("That question was already answered or has been cancelled.".into()),
            }
        };
        if paused.asked_at.elapsed() > CONFIRMATION_TTL {
            return Ok(AgentOutcome::reply(EXPIRED_REPLY, vec![]));
        }
        let cancel = self.begin()?;
        let Paused {
            mut run, pending, ..
        } = paused;
        let result = tokio::time::timeout(TASK_TIMEOUT, async {
            run.resolve(pending, approved, exec, &mut on_executing)
                .await;
            run.drive(model, exec, &cancel, &mut on_executing).await
        })
        .await;
        self.settle(result, run, model.model_id())
    }

    #[cfg(test)]
    fn age_pending_for_test(&self, by: Duration) {
        if let Some(p) = self.paused.lock().unwrap().as_mut() {
            p.asked_at -= by;
        }
    }
}

#[cfg(test)]
mod tests;
