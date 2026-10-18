mod agent;
mod commands;
mod prefs;
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
            app.manage(prefs::PrefsStore::open(&dir.join("voice-agent.db"))?);
            app.manage(speech::SpeechState::new());
            app.manage(agent::AgentState::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::get_preferences,
            commands::set_preferences,
            commands::open_application,
            commands::get_speech_setup,
            commands::save_azure_speech,
            commands::clear_azure_speech,
            commands::get_speech_token,
            commands::get_assistant_setup,
            commands::save_gemini_key,
            commands::clear_gemini_key,
            commands::run_agent,
            commands::cancel_agent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
