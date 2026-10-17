mod commands;
mod prefs;
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::get_preferences,
            commands::set_preferences,
            commands::open_application,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
