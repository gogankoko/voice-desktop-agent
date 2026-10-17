//! Tauri command handlers. Each handler is a thin adapter: it validates input,
//! delegates to a testable module, and returns a serialisable result.

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::agent::{AgentOutcome, AgentState, DesktopExecutor, gemini};

use crate::prefs::{Preferences, PrefsStore};
use crate::secrets;
use crate::speech::{self, SpeechSetup, SpeechState, SpeechToken};
use crate::status::{AppStatus, current_status};
use crate::tools::open_app::{self, AllowedApp, OpenResult};

/// Reports which subsystems are actually available in this build so the UI
/// never implies capabilities that do not exist yet.
#[tauri::command]
pub fn get_app_status() -> AppStatus {
    current_status(speech::setup().configured, assistant_setup().configured)
}

#[tauri::command]
pub fn get_preferences(store: State<'_, PrefsStore>) -> Result<Preferences, String> {
    store.load().map_err(|e| {
        eprintln!("get_preferences failed: {e}");
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
        eprintln!("set_preferences failed: {e}");
        "Your settings couldn't be saved.".to_string()
    })?;
    Ok(prefs)
}

/// Opens an allowlisted application. `app` is an enum, so the frontend cannot name
/// arbitrary programs. Runs off the main thread because verification polls for seconds.
#[tauri::command]
pub async fn open_application(app: AllowedApp) -> Result<OpenResult, String> {
    tauri::async_runtime::spawn_blocking(move || open_app::open(app))
        .await
        .map_err(|e| {
            eprintln!("open_application task failed: {e}");
            "Something went wrong while opening the app.".to_string()
        })?
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
    let key = key.trim();
    if !(20..=200).contains(&key.len())
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(
            "That doesn't look like a Gemini API key. Copy it from Google AI Studio.".into(),
        );
    }
    gemini::test_key(&state.http, key).await?;
    secrets::save_gemini_key(key).map_err(|e| {
        eprintln!("gemini key write failed: {e}");
        "The key works, but Windows couldn't store it safely.".to_string()
    })?;
    Ok(assistant_setup())
}

#[tauri::command]
pub fn clear_gemini_key() -> Result<AssistantSetup, String> {
    secrets::delete_gemini_key().map_err(|e| {
        eprintln!("gemini key delete failed: {e}");
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
}

/// Runs one request through the model and the validated tool registry.
/// `task_id` is echoed in progress events so the UI can ignore stale ones.
#[tauri::command]
pub async fn run_agent(
    app: AppHandle,
    state: State<'_, AgentState>,
    task_id: u32,
    text: String,
) -> Result<AgentOutcome, String> {
    let key = secrets::load_gemini_key()
        .map_err(|_| "I couldn't read the assistant settings.".to_string())?
        .ok_or_else(|| {
            "The assistant isn't set up yet. Add a Gemini key in Settings.".to_string()
        })?;
    let model = gemini::Gemini::new(state.http.clone(), key);
    state
        .run(&model, &DesktopExecutor, &text, |tool| {
            let _ = app.emit(
                "agent-progress",
                AgentProgress {
                    task_id,
                    stage: "executing",
                    tool,
                },
            );
        })
        .await
}

/// Stops the current request: no further model calls or tool actions start.
#[tauri::command]
pub fn cancel_agent(state: State<'_, AgentState>) {
    state.cancel_current();
}
