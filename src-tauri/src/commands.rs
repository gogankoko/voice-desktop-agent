//! Tauri command handlers. Each handler is a thin adapter: it validates input,
//! delegates to a testable module, and returns a serialisable result.

use crate::status::{AppStatus, current_status};

/// Reports which subsystems are actually available in this build so the UI
/// never implies capabilities that do not exist yet.
#[tauri::command]
pub fn get_app_status() -> AppStatus {
    current_status()
}
