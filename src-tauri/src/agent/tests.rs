use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use super::gemini::{ChatModel, ModelError, ModelTurn, ProposedCall};
use super::*;

/// Replays scripted turns and records every input it was sent.
struct ScriptedModel {
    turns: Mutex<VecDeque<Result<ModelTurn, ModelError>>>,
    seen: Mutex<Vec<Vec<Value>>>,
    /// When set, `respond` never completes (to test cancellation).
    hang: bool,
}

impl ScriptedModel {
    fn new(turns: Vec<Result<ModelTurn, ModelError>>) -> Self {
        Self {
            turns: Mutex::new(turns.into()),
            seen: Mutex::new(vec![]),
            hang: false,
        }
    }
    fn hanging() -> Self {
        Self {
            hang: true,
            ..Self::new(vec![])
        }
    }
    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl ChatModel for ScriptedModel {
    async fn respond(&self, input: &[Value]) -> Result<ModelTurn, ModelError> {
        self.seen.lock().unwrap().push(input.to_vec());
        if self.hang {
            std::future::pending::<()>().await;
        }
        let next = self.turns.lock().unwrap().pop_front();
        // Running out of script = the model keeps asking for the same tool.
        next.unwrap_or_else(|| {
            Ok(call_turn(&[(
                "again",
                "open_application",
                json!({"app": "default_browser"}),
            )]))
        })
    }
}

#[derive(Default)]
struct FakeDesktop {
    opened: AtomicUsize,
    verified: bool,
    fail: Option<String>,
}

impl ToolExecutor for FakeDesktop {
    async fn open_application(&self, _app: AllowedApp) -> Result<OpenResult, String> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        match &self.fail {
            Some(e) => Err(e.clone()),
            None => Ok(OpenResult {
                app: AllowedApp::DefaultBrowser,
                verified: self.verified,
                app_name: "Microsoft Edge".into(),
            }),
        }
    }
}

fn text_turn(text: &str) -> ModelTurn {
    ModelTurn {
        steps: vec![json!({"type": "model_output", "content": [{"type": "text", "text": text}]})],
        calls: vec![],
        text: text.into(),
    }
}

fn call_turn(calls: &[(&str, &str, Value)]) -> ModelTurn {
    let mut steps = vec![json!({"type": "thought", "signature": "sig-123"})];
    let mut proposed = vec![];
    for (id, name, args) in calls {
        steps.push(json!({"type": "function_call", "id": id, "name": name, "arguments": args}));
        proposed.push(ProposedCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: args.clone(),
        });
    }
    ModelTurn {
        steps,
        calls: proposed,
        text: String::new(),
    }
}

fn browser() -> Value {
    json!({"app": "default_browser"})
}

fn run(model: &ScriptedModel, desk: &FakeDesktop) -> AgentOutcome {
    let cancel = Cancel::default();
    tauri::async_runtime::block_on(run_task(
        model,
        desk,
        &[],
        "open my browser",
        &cancel,
        |_| {},
    ))
    .unwrap()
    .0
}

#[test]
fn conversation_without_tools() {
    let model = ScriptedModel::new(vec![Ok(text_turn("Hello! How can I help?"))]);
    let desk = FakeDesktop::default();
    let out = run(&model, &desk);
    assert_eq!(out.reply, "Hello! How can I help?");
    assert!(out.actions.is_empty());
    assert_eq!(desk.opened.load(Ordering::SeqCst), 0);
}

#[test]
fn executes_valid_call_and_reports_what_rust_observed() {
    let model = ScriptedModel::new(vec![
        Ok(call_turn(&[("c1", "open_application", browser())])),
        Ok(text_turn("Your browser is open.")),
    ]);
    let desk = FakeDesktop {
        verified: true,
        ..Default::default()
    };
    let out = run(&model, &desk);
    assert_eq!(desk.opened.load(Ordering::SeqCst), 1);
    assert_eq!(out.actions.len(), 1);
    assert!(out.actions[0].verified);
    assert_eq!(out.reply, "Your browser is open.");
}

#[test]
fn replays_model_steps_verbatim_then_the_result() {
    let model = ScriptedModel::new(vec![
        Ok(call_turn(&[("c1", "open_application", browser())])),
        Ok(text_turn("Done.")),
    ]);
    let desk = FakeDesktop {
        verified: true,
        ..Default::default()
    };
    run(&model, &desk);
    let second = &model.seen.lock().unwrap()[1];
    assert_eq!(second[0]["type"], "user_input");
    assert_eq!(
        second[1],
        json!({"type": "thought", "signature": "sig-123"})
    );
    assert_eq!(second[2]["type"], "function_call");
    assert_eq!(second[3]["type"], "function_result");
    assert_eq!(second[3]["call_id"], "c1");
    assert_eq!(second[3]["result"]["verified"], true);
}

#[test]
fn unverified_outcome_is_reported_as_unverified() {
    let model = ScriptedModel::new(vec![
        Ok(call_turn(&[("c1", "open_application", browser())])),
        Ok(text_turn("I couldn't confirm it opened.")),
    ]);
    let desk = FakeDesktop {
        verified: false,
        ..Default::default()
    };
    let out = run(&model, &desk);
    assert!(!out.actions[0].verified);
    assert!(out.actions[0].summary.contains("couldn't confirm"));
    let result = &model.seen.lock().unwrap()[1][3];
    assert_eq!(result["result"]["verified"], false);
}

#[test]
fn invalid_or_injected_calls_are_refused_without_executing() {
    let model = ScriptedModel::new(vec![
        Ok(call_turn(&[
            ("c1", "run_shell", json!({"cmd": "del C:\\*"})),
            (
                "c2",
                "open_application",
                json!({"app": "default_browser", "url": "http://evil.example"}),
            ),
            ("c3", "open_application", json!({"app": "cmd.exe"})),
        ])),
        Ok(text_turn("Sorry, I can't do that.")),
    ]);
    let desk = FakeDesktop::default();
    let out = run(&model, &desk);
    assert_eq!(desk.opened.load(Ordering::SeqCst), 0);
    assert!(out.actions.is_empty());
    let input = &model.seen.lock().unwrap()[1];
    let results: Vec<&Value> = input
        .iter()
        .filter(|s| s["type"] == "function_result")
        .collect();
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r["is_error"] == true));
}

#[test]
fn duplicate_calls_in_one_request_run_once() {
    let model = ScriptedModel::new(vec![
        Ok(call_turn(&[
            ("c1", "open_application", browser()),
            ("c2", "open_application", browser()),
        ])),
        Ok(call_turn(&[("c3", "open_application", browser())])),
        Ok(text_turn("Done.")),
    ]);
    let desk = FakeDesktop {
        verified: true,
        ..Default::default()
    };
    let out = run(&model, &desk);
    assert_eq!(desk.opened.load(Ordering::SeqCst), 1);
    assert_eq!(out.actions.len(), 1);
}

#[test]
fn loop_is_bounded() {
    let model = ScriptedModel::new(vec![]); // keeps proposing a tool forever
    let desk = FakeDesktop {
        verified: true,
        ..Default::default()
    };
    let out = run(&model, &desk);
    assert_eq!(model.calls(), MAX_MODEL_CALLS);
    assert_eq!(desk.opened.load(Ordering::SeqCst), 1);
    assert_eq!(out.reply, TOO_MANY_STEPS_REPLY);
}

#[test]
fn tool_errors_are_reported_and_sent_back() {
    let model = ScriptedModel::new(vec![
        Ok(call_turn(&[("c1", "open_application", browser())])),
        Ok(text_turn("I couldn't find your browser.")),
    ]);
    let desk = FakeDesktop {
        fail: Some("I couldn't find your web browser.".into()),
        ..Default::default()
    };
    let out = run(&model, &desk);
    assert!(!out.actions[0].ok);
    assert_eq!(model.seen.lock().unwrap()[1][3]["is_error"], true);
}

#[test]
fn model_errors_propagate() {
    let model = ScriptedModel::new(vec![Err(ModelError::RateLimited)]);
    let desk = FakeDesktop::default();
    let cancel = Cancel::default();
    let r = tauri::async_runtime::block_on(run_task(&model, &desk, &[], "hi", &cancel, |_| {}));
    assert_eq!(r.unwrap_err(), ModelError::RateLimited);
}

#[test]
fn cancel_before_start_does_nothing() {
    let model = ScriptedModel::new(vec![Ok(call_turn(&[(
        "c1",
        "open_application",
        browser(),
    )]))]);
    let desk = FakeDesktop::default();
    let cancel = Cancel::default();
    cancel.cancel();
    let (out, _) =
        tauri::async_runtime::block_on(run_task(&model, &desk, &[], "hi", &cancel, |_| {}))
            .unwrap();
    assert!(out.cancelled);
    assert_eq!(model.calls(), 0);
    assert_eq!(desk.opened.load(Ordering::SeqCst), 0);
}

#[test]
fn cancel_interrupts_a_pending_model_request() {
    let model = ScriptedModel::hanging();
    let desk = FakeDesktop::default();
    let cancel = std::sync::Arc::new(Cancel::default());
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        c2.cancel();
    });
    let (out, _) =
        tauri::async_runtime::block_on(run_task(&model, &desk, &[], "hi", &cancel, |_| {}))
            .unwrap();
    assert!(out.cancelled);
    assert_eq!(desk.opened.load(Ordering::SeqCst), 0);
}

#[test]
fn state_rejects_concurrent_requests_and_keeps_short_history() {
    let state = AgentState::new();
    let desk = FakeDesktop::default();
    for i in 0..10 {
        let model = ScriptedModel::new(vec![Ok(text_turn(&format!("reply {i}")))]);
        tauri::async_runtime::block_on(state.run(&model, &desk, &format!("q {i}"), |_| {}))
            .unwrap();
    }
    let h = state.history.lock().unwrap();
    assert_eq!(h.len(), HISTORY_TURNS * 2);
    assert_eq!(h.last().unwrap()["content"][0]["text"], "reply 9");
    drop(h);

    *state.current.lock().unwrap() = Some(std::sync::Arc::new(Cancel::default()));
    let model = ScriptedModel::new(vec![Ok(text_turn("x"))]);
    let err =
        tauri::async_runtime::block_on(state.run(&model, &desk, "again", |_| {})).unwrap_err();
    assert!(err.contains("still working"));
    assert_eq!(model.calls(), 0);
}

#[test]
fn empty_and_oversized_requests_are_rejected() {
    let state = AgentState::new();
    let desk = FakeDesktop::default();
    let model = ScriptedModel::new(vec![]);
    for bad in ["   ".to_string(), "x".repeat(MAX_REQUEST_CHARS + 1)] {
        assert!(tauri::async_runtime::block_on(state.run(&model, &desk, &bad, |_| {})).is_err());
    }
    assert_eq!(model.calls(), 0);
}
