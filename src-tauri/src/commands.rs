//! Tauri command handlers. Each handler is a thin adapter: it validates input,
//! delegates to a testable module, and returns a serialisable result.

use tauri::State;

use crate::prefs::{Preferences, PrefsStore};
use crate::status::{AppStatus, current_status};

/// Reports which subsystems are actually available in this build so the UI
/// never implies capabilities that do not exist yet.
#[tauri::command]
pub fn get_app_status() -> AppStatus {
    current_status()
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
