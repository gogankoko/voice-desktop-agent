fn main() {
    // Every app command must be listed here AND granted in capabilities/*.json.
    // Without the explicit manifest, Tauri allows all registered app commands by default.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "get_app_status",
        "get_preferences",
        "set_preferences",
        "get_speech_setup",
        "save_azure_speech",
        "clear_azure_speech",
        "get_speech_token",
        "speak_natural",
        "get_assistant_setup",
        "save_gemini_key",
        "clear_gemini_key",
        "run_agent",
        "cancel_agent",
        "pause_agent",
        "resume_agent",
        "confirm_agent",
        "get_diagnostics",
        "clear_diagnostics",
        "log_frontend_error",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to run tauri-build");
}
