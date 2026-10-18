//! Agent orchestration: one user request → bounded model/tool loop → outcome.
//!
//! Rust owns every decision that matters for safety: which tools exist, whether a
//! proposed call is valid and allowed, how many steps run, deduplication, cancellation.
//! The model only proposes.

pub mod gemini;
pub mod prompt;
pub mod registry;

use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::tools::open_app::{AllowedApp, OpenResult};
use gemini::{ChatModel, ModelError, ProposedCall};
use registry::{Refusal, ValidatedCall};

/// Model calls per request. Each tool round-trip uses one; this bounds runaway loops.
pub const MAX_MODEL_CALLS: usize = 4;
pub const TASK_TIMEOUT: Duration = Duration::from_secs(60);
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
    /// Plain-language description for the transcript.
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentOutcome {
    pub reply: String,
    /// Real actions performed during this request, as observed by Rust (not as claimed by the model).
    pub actions: Vec<ActionReport>,
    pub cancelled: bool,
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

fn cancelled(actions: Vec<ActionReport>) -> AgentOutcome {
    AgentOutcome {
        reply: String::new(),
        actions,
        cancelled: true,
    }
}

const UNSURE_REPLY: &str =
    "Sorry, I'm not sure how to help with that. Could you say it another way?";
const TOO_MANY_STEPS_REPLY: &str =
    "Sorry, I couldn't finish that. Could you ask again in a simpler way?";

/// Runs one request. Returns the outcome plus the text-only turns to append to history.
pub async fn run_task<M: ChatModel, E: ToolExecutor>(
    model: &M,
    exec: &E,
    history: &[Value],
    utterance: &str,
    cancel: &Cancel,
    mut on_executing: impl FnMut(&'static str),
) -> Result<(AgentOutcome, [Value; 2]), ModelError> {
    let mut input: Vec<Value> = history.to_vec();
    input.push(user_step(utterance));
    let mut actions = Vec::new();
    let mut done: HashSet<ValidatedCall> = HashSet::new();

    for _ in 0..MAX_MODEL_CALLS {
        if cancel.is_cancelled() {
            return Ok((cancelled(actions), [Value::Null, Value::Null]));
        }
        let turn = tokio::select! {
            r = model.respond(&input) => r?,
            _ = cancel.cancelled() => return Ok((cancelled(actions), [Value::Null, Value::Null])),
        };
        input.extend(turn.steps.iter().cloned());

        if turn.calls.is_empty() {
            let reply = if turn.text.is_empty() {
                UNSURE_REPLY.to_string()
            } else {
                turn.text
            };
            let turns = [user_step(utterance), model_text_step(&reply)];
            return Ok((
                AgentOutcome {
                    reply,
                    actions,
                    cancelled: false,
                },
                turns,
            ));
        }

        for call in &turn.calls {
            if cancel.is_cancelled() {
                return Ok((cancelled(actions), [Value::Null, Value::Null]));
            }
            let result = match registry::validate(&call.name, &call.arguments) {
                Err(Refusal(why)) => function_result(call, json!({ "error": why }), true),
                Ok(v) if !done.insert(v) => function_result(
                    call,
                    json!({ "error": "Already done during this request, so it was not repeated." }),
                    true,
                ),
                Ok(ValidatedCall::OpenApplication(app)) => {
                    on_executing("open_application");
                    match exec.open_application(app).await {
                        Ok(r) => {
                            actions.push(ActionReport {
                                tool: "open_application",
                                ok: true,
                                verified: r.verified,
                                summary: if r.verified {
                                    format!("Opened {} (checked: it's in front)", r.app_name)
                                } else {
                                    format!("Asked Windows to open {}, but couldn't confirm it appeared", r.app_name)
                                },
                            });
                            function_result(
                                call,
                                json!({ "verified": r.verified, "app_name": r.app_name }),
                                false,
                            )
                        }
                        Err(why) => {
                            actions.push(ActionReport {
                                tool: "open_application",
                                ok: false,
                                verified: false,
                                summary: why.clone(),
                            });
                            function_result(call, json!({ "error": why }), true)
                        }
                    }
                }
            };
            input.push(result);
        }
    }

    let turns = [user_step(utterance), model_text_step(TOO_MANY_STEPS_REPLY)];
    Ok((
        AgentOutcome {
            reply: TOO_MANY_STEPS_REPLY.into(),
            actions,
            cancelled: false,
        },
        turns,
    ))
}

/// App-wide agent state: one task at a time, cancellable, short in-memory history.
pub struct AgentState {
    pub http: reqwest::Client,
    history: Mutex<Vec<Value>>,
    current: Mutex<Option<Arc<Cancel>>>,
}

impl AgentState {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            history: Mutex::new(Vec::new()),
            current: Mutex::new(None),
        }
    }

    pub fn cancel_current(&self) {
        if let Some(c) = self.current.lock().expect("agent lock").as_ref() {
            c.cancel();
        }
    }

    pub async fn run<M: ChatModel, E: ToolExecutor>(
        &self,
        model: &M,
        exec: &E,
        utterance: &str,
        on_executing: impl FnMut(&'static str),
    ) -> Result<AgentOutcome, String> {
        let utterance = utterance.trim();
        if utterance.is_empty() || utterance.chars().count() > MAX_REQUEST_CHARS {
            return Err("That request was empty or too long.".into());
        }
        let cancel = {
            let mut cur = self.current.lock().expect("agent lock");
            if cur.is_some() {
                return Err(
                    "I'm still working on your last request. Press Stop to cancel it.".into(),
                );
            }
            let c = Arc::new(Cancel::default());
            *cur = Some(c.clone());
            c
        };
        let history = self.history.lock().expect("agent lock").clone();

        let result = tokio::time::timeout(
            TASK_TIMEOUT,
            run_task(model, exec, &history, utterance, &cancel, on_executing),
        )
        .await;
        *self.current.lock().expect("agent lock") = None;

        match result {
            Err(_) => Err("That took too long, so I stopped. Please try again.".into()),
            Ok(Err(e)) => Err(e.user_message()),
            Ok(Ok((outcome, turns))) => {
                if !outcome.cancelled {
                    let mut h = self.history.lock().expect("agent lock");
                    h.extend(turns);
                    let excess = h.len().saturating_sub(HISTORY_TURNS * 2);
                    h.drain(..excess);
                }
                Ok(outcome)
            }
        }
    }
}

#[cfg(test)]
mod tests;
