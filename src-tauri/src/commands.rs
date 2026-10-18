//! Tauri command handlers. Each handler is a thin adapter: it validates input,
//! delegates to a testable module, and returns a serialisable result.

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::agent;
use crate::agent::gemini::ChatModel as _;
use crate::agent::{AgentOutcome, AgentState, DesktopExecutor, gemini};

use crate::prefs::{Preferences, PrefsStore};
use crate::secrets;
use crate::speech::{self, SpeechSetup, SpeechState, SpeechToken};
use crate::status::{AppStatus, current_status};

/// Reports which subsystems are actually available in this build so the UI
/// never implies capabilities that do not exist yet.
#[tauri::command]
pub fn get_app_status() -> AppStatus {
    current_status(speech::setup().configured, assistant_setup().configured)
}

#[tauri::command]
pub fn get_preferences(store: State<'_, PrefsStore>) -> Result<Preferences, String> {
    store.load().map_err(|e| {
        crate::diag::log("command", format!("get_preferences failed: {e}"));
        "Your settings couldn't be loaded, so the defaults are being used.".to_string()
    })
}

/// `prefs` is deserialised with `deny_unknown_fields` and enum-typed values,
/// so anything outside the allowed set is rejected before reaching storage.
#[tauri::command]
pub fn set_preferences(
    store: State<'_, PrefsStore>,
    prefs: Preferences,
) -> Result<Preferences, String> {
    store.save(&prefs).map_err(|e| {
        crate::diag::log("command", format!("set_preferences failed: {e}"));
        "Your settings couldn't be saved.".to_string()
    })?;
    Ok(prefs)
}

#[tauri::command]
pub fn get_speech_setup() -> SpeechSetup {
    speech::setup()
}

/// The key is tested against Azure before it is stored, and is never returned.
#[tauri::command]
pub async fn save_azure_speech(
    state: State<'_, SpeechState>,
    region: String,
    key: String,
) -> Result<SpeechSetup, String> {
    speech::save(&state, &region, &key).await
}

#[tauri::command]
pub fn clear_azure_speech(state: State<'_, SpeechState>) -> Result<SpeechSetup, String> {
    speech::clear(&state)
}

/// Returns a short-lived (10 minute) Azure token, never the resource key.
#[tauri::command]
pub async fn get_speech_token(state: State<'_, SpeechState>) -> Result<SpeechToken, String> {
    speech::token(&state).await
}

/// Natural-voice audio (MP3 bytes) for a reply. The Azure key never leaves Rust.
#[tauri::command]
pub async fn speak_natural(
    state: State<'_, SpeechState>,
    text: String,
    voice: crate::prefs::Voice,
) -> Result<tauri::ipc::Response, String> {
    speech::synthesize(&state, &text, voice)
        .await
        .map(tauri::ipc::Response::new)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantSetup {
    pub configured: bool,
    pub model: &'static str,
}

fn assistant_setup() -> AssistantSetup {
    AssistantSetup {
        configured: matches!(secrets::load_gemini_key(), Ok(Some(_))),
        model: gemini::MODEL,
    }
}

#[tauri::command]
pub fn get_assistant_setup() -> AssistantSetup {
    assistant_setup()
}

/// Tested against Google (a free metadata call) before storing; never returned.
#[tauri::command]
pub async fn save_gemini_key(
    state: State<'_, AgentState>,
    key: String,
) -> Result<AssistantSetup, String> {
    let key = gemini::normalise_key(&key)?;
    gemini::test_key(&state.http, &key).await?;
    secrets::save_gemini_key(&key).map_err(|e| {
        crate::diag::log("command", format!("gemini key write failed: {e}"));
        "The key works, but Windows couldn't store it safely.".to_string()
    })?;
    Ok(assistant_setup())
}

#[tauri::command]
pub fn clear_gemini_key() -> Result<AssistantSetup, String> {
    secrets::delete_gemini_key().map_err(|e| {
        crate::diag::log("command", format!("gemini key delete failed: {e}"));
        "I couldn't remove the assistant key.".to_string()
    })?;
    Ok(assistant_setup())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentProgress {
    task_id: u32,
    stage: &'static str,
    tool: &'static str,
    detail: String,
}

/// Runs one request through the model and the validated tool registry.
/// `task_id` is echoed in progress events so the UI can ignore stale ones.
#[tauri::command]
pub async fn run_agent(
    app: AppHandle,
    state: State<'_, AgentState>,
    prefs: State<'_, PrefsStore>,
    task_id: u32,
    text: String,
) -> Result<AgentOutcome, String> {
    let loaded = prefs.load().ok();
    let practice = loaded.as_ref().map(|p| p.practice_mode).unwrap_or(false);
    state.files_enabled.store(
        loaded.as_ref().map(|p| p.file_search).unwrap_or(false),
        std::sync::atomic::Ordering::SeqCst,
    );
    let started = std::time::Instant::now();
    // Built-in commands ("open YouTube", "pause", "set a timer...") run instantly, without
    // the AI (and without a Gemini key); everything else goes to Gemini.
    if let Some(plan) = built_in_plan(&text).await {
        let model = agent::local::LocalModel::new(plan, local_clock());
        let result = state
            .run(
                &model,
                &DesktopExecutor,
                &text,
                practice,
                progress(&app, task_id),
            )
            .await;
        log_outcome("request", &result, started, model.model_id());
        return result;
    }
    let key = gemini_key().map_err(|_| {
        "Without the AI set up, I can only do built-in things like \"open YouTube\", \"pause\", \"next song\" or \"set a timer for 10 minutes\". For anything else, add a Gemini key in Settings.".to_string()
    })?;
    let model = gemini::Gemini::new(state.http.clone(), key, practice);
    let result = state
        .run(
            &model,
            &DesktopExecutor,
            &text,
            practice,
            progress(&app, task_id),
        )
        .await;
    log_outcome("request", &result, started, model.model_id());
    result
}

/// The built-in command for `text`, if it is one. Reads the Start menu only for "open X".
async fn built_in_plan(text: &str) -> Option<agent::local::Plan> {
    let apps = if agent::local::open_target(text).is_some() {
        tauri::async_runtime::spawn_blocking(crate::tools::apps::installed_apps)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    agent::local::plan(text, &apps)
}

fn local_clock() -> agent::local::Clock {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    agent::local::Clock {
        year: t.wYear,
        month: t.wMonth,
        day_of_week: t.wDayOfWeek,
        day: t.wDay,
        hour: t.wHour,
        minute: t.wMinute,
    }
}

/// The user's explicit answer to a confirmation question. Only the matching, unexpired
/// `confirmation_id` is accepted, once; approval runs exactly the action that was shown.
#[tauri::command]
pub async fn confirm_agent(
    app: AppHandle,
    state: State<'_, AgentState>,
    task_id: u32,
    confirmation_id: String,
    approved: bool,
) -> Result<AgentOutcome, String> {
    let (model_id, practice) = state
        .paused_info(&confirmation_id)
        .ok_or_else(|| "That question was already answered or has been cancelled.".to_string())?;
    let model = gemini::Gemini::resume(state.http.clone(), gemini_key()?, practice, model_id);
    crate::diag::log(
        "confirm",
        if approved {
            "user approved"
        } else {
            "user declined"
        },
    );
    let started = std::time::Instant::now();
    let result = state
        .confirm(
            &model,
            &DesktopExecutor,
            &confirmation_id,
            approved,
            progress(&app, task_id),
        )
        .await;
    log_outcome("confirm", &result, started, model.model_id());
    result
}

/// One technical summary line per request: which tools ran and how, never any text.
fn log_outcome(
    kind: &str,
    result: &Result<AgentOutcome, String>,
    started: std::time::Instant,
    model: &str,
) {
    let ms = started.elapsed().as_millis();
    let line = match result {
        Ok(o) => {
            let actions: Vec<String> = o
                .actions
                .iter()
                .map(|a| {
                    format!(
                        "{}:{}",
                        a.tool,
                        if !a.ok {
                            "failed"
                        } else if a.verified {
                            "verified"
                        } else {
                            "unverified"
                        }
                    )
                })
                .collect();
            format!(
                "{kind} done in {ms}ms model={model} actions=[{}] confirmation={} cancelled={}",
                actions.join(","),
                o.confirmation.is_some(),
                o.cancelled
            )
        }
        Err(e) => format!("{kind} failed in {ms}ms model={model}: {e}"),
    };
    crate::diag::log("agent", line);
}

/// Recent technical log lines for the Help screen (already redacted).
#[tauri::command]
pub fn get_diagnostics() -> String {
    crate::diag::recent(200)
}

#[tauri::command]
pub fn clear_diagnostics() {
    crate::diag::clear();
}

/// Lets the UI record its own errors. Length-limited and redacted like everything else.
#[tauri::command]
pub fn log_frontend_error(message: String) {
    let message: String = message.chars().take(1000).collect();
    crate::diag::log("ui", message);
}

fn gemini_key() -> Result<String, String> {
    secrets::load_gemini_key()
        .map_err(|_| "I couldn't read the assistant settings.".to_string())?
        .ok_or_else(|| "The assistant isn't set up yet. Add a Gemini key in Settings.".to_string())
}

fn progress(app: &AppHandle, task_id: u32) -> impl FnMut(agent::Progress) + '_ {
    move |p| {
        let _ = app.emit(
            "agent-progress",
            AgentProgress {
                task_id,
                stage: "executing",
                tool: p.tool,
                detail: p.detail,
            },
        );
    }
}

/// Pauses the current request after the step in progress. Returns whether one was running.
#[tauri::command]
pub fn pause_agent(state: State<'_, AgentState>) -> bool {
    let held = state.hold_current();
    if held {
        crate::diag::log("agent", "paused by user");
    }
    held
}

/// Continues a paused request. Returns whether one was running.
#[tauri::command]
pub fn resume_agent(state: State<'_, AgentState>) -> bool {
    state.release_current()
}

/// Stops the current request: no further model calls or tool actions start.
#[tauri::command]
pub fn cancel_agent(state: State<'_, AgentState>) {
    state.cancel_current();
}
