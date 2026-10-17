//! Tauri command handlers. Each handler is a thin adapter: it validates input,
//! delegates to a testable module, and returns a serialisable result.

use tauri::State;

use crate::prefs::{Preferences, PrefsStore};
use crate::speech::{self, SpeechSetup, SpeechState, SpeechToken};
use crate::status::{AppStatus, current_status};
use crate::tools::open_app::{self, AllowedApp, OpenResult};

/// Reports which subsystems are actually available in this build so the UI
/// never implies capabilities that do not exist yet.
#[tauri::command]
pub fn get_app_status() -> AppStatus {
    current_status(speech::setup().configured)
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
