mod agent;
mod commands;
mod diag;
mod hotkey;
mod prefs;
mod reminders;
mod secrets;
mod speech;
mod status;
mod tools;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            if diag::init(&dir.join("logs")).is_ok() {
                std::panic::set_hook(Box::new(|info| diag::log("panic", info.to_string())));
            }
            diag::log(
                "app",
                format!(
                    "started version {} on {}",
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::OS
                ),
            );
            app.manage(prefs::PrefsStore::open(&dir.join("voice-agent.db"))?);
            app.manage(speech::SpeechState::new());
            app.manage(agent::AgentState::new());
            hotkey::start(app.handle().clone());
            reminders::start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::get_preferences,
            commands::set_preferences,
            commands::get_speech_setup,
            commands::save_azure_speech,
            commands::clear_azure_speech,
            commands::get_speech_token,
            commands::speak_natural,
            commands::get_assistant_setup,
            commands::save_gemini_key,
            commands::clear_gemini_key,
            commands::run_agent,
            commands::cancel_agent,
            commands::pause_agent,
            commands::resume_agent,
            commands::confirm_agent,
            commands::get_diagnostics,
            commands::clear_diagnostics,
            commands::log_frontend_error,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
