fn main() {
    // Every app command must be listed here AND granted in capabilities/*.json.
    // Without the explicit manifest, Tauri allows all registered app commands by default.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "get_app_status",
        "get_preferences",
        "set_preferences",
        "open_application",
        "get_speech_setup",
        "save_azure_speech",
        "clear_azure_speech",
        "get_speech_token",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to run tauri-build");
}
