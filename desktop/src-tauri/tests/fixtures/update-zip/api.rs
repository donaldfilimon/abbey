//! Actual public updater API only. No downloads and no replacement extractor.
use std::path::{Path, PathBuf};
use tauri_plugin_updater::{Update, UpdaterExt};

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/update-zip")
        .join(name)
}

pub fn update(key: &str, signature: &str, version: &str, executable: &Path) -> Update {
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context.package_info_mut().name = "abbey-updater-fixture".into();
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({"pubkey": key.trim(), "requireSignedVersion": true}),
    );
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .expect("mock app with real updater plugin");
    app.updater_builder()
        .executable_path(executable)
        .target("fixture")
        .no_proxy()
        .configure_client(|_| panic!("offline test attempted network access"))
        .endpoints(vec![
            "https://example.invalid/never-requested".parse().unwrap(),
        ])
        .unwrap()
        .build()
        .unwrap()
        .restore_update(serde_json::json!({
            "version": version,
            "url": "https://example.invalid/never-requested.zip",
            "signature": signature.trim(),
        }))
        .expect("parse cached release")
        .expect("fixture release is newer than mock app")
}
