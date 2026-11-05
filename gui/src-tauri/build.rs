fn main() {
    #[cfg(feature = "app")]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "window_info",
            "pick_folder",
            "open_recent",
            "open_parent",
            "snapshot",
            "refresh",
            "round_detail",
            "preview_scope",
            "draft_text",
            "check_draft",
            "save_draft",
            "run",
            "command",
            "reveal",
            "settings",
            "set_settings",
            "set_api_key",
            "system",
            "set_guard",
            "force_close",
        ]),
    ))
    .expect("tauri-build failed");
}
