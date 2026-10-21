use abbey::edition::Edition;
use serde_json::Value;

fn identifier(config: &str) -> String {
    serde_json::from_str::<Value>(config)
        .expect("tauri config is valid JSON")
        .get("identifier")
        .and_then(Value::as_str)
        .expect("tauri config declares an identifier")
        .to_owned()
}

fn product_name(config: &str) -> String {
    serde_json::from_str::<Value>(config)
        .expect("tauri config is valid JSON")
        .get("productName")
        .and_then(Value::as_str)
        .expect("tauri config declares a productName")
        .to_owned()
}

const SAFE_CONFIG: &str = include_str!("../tauri.conf.json");
const PERSONAL_CONFIG: &str = include_str!("../tauri.personal.conf.json");

#[test]
fn bundle_identity_is_derived_from_the_edition_table() {
    assert_eq!(identifier(SAFE_CONFIG), Edition::Safe.bundle_id());
    assert_eq!(identifier(PERSONAL_CONFIG), Edition::Personal.bundle_id());
    assert_eq!(product_name(SAFE_CONFIG), Edition::Safe.product_name());
    assert_eq!(
        product_name(PERSONAL_CONFIG),
        Edition::Personal.product_name()
    );
}

#[test]
fn the_two_editions_never_share_a_bundle_identity() {
    assert_ne!(identifier(SAFE_CONFIG), identifier(PERSONAL_CONFIG));
    assert_ne!(product_name(SAFE_CONFIG), product_name(PERSONAL_CONFIG));
}

#[test]
fn the_declared_csp_is_strict() {
    let csp = serde_json::from_str::<Value>(SAFE_CONFIG)
        .expect("valid JSON")
        .pointer("/app/security/csp")
        .and_then(Value::as_str)
        .expect("a csp is declared")
        .to_owned();
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("script-src 'self'"));
    assert!(csp.contains("object-src 'none'"));
    // The two directives that would readmit remote or inline script.
    assert!(!csp.contains("unsafe-inline"));
    assert!(!csp.contains("unsafe-eval"));
    assert!(!csp.contains("http://") || csp.contains("http://ipc.localhost"));
}

#[test]
fn invoke_surface_has_only_enumerated_model_and_conversation_effects() {
    const MAIN: &str = include_str!("main.rs");
    let handler = MAIN
        .split("generate_handler![")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("generate_handler block");
    assert!(handler.contains("commands::app_run_status"));
    assert!(handler.contains("commands::app_memory_search"));
    assert!(handler.contains("commands::app_models_list"));
    assert!(handler.contains("commands::app_claim_by_id"));
    assert!(handler.contains("commands::app_memory_metadata"));
    assert!(handler.contains("commands::app_v3_grants"));
    for name in [
        "download",
        "load",
        "unload",
        "details",
        "grants",
        "download_status",
        "lifecycle_status",
    ] {
        assert!(handler.contains(&format!("commands::app_model_{name}")));
    }
    for name in [
        "grants",
        "read",
        "create",
        "rename",
        "archive",
        "delete",
        "messages",
        "content",
        "routes",
        "submit_turn",
        "cancel_turn",
    ] {
        assert!(handler.contains(&format!("commands::app_conversation_{name}")));
    }
    assert!(handler.contains("commands::app_conversations_list"));
    assert!(handler.contains("commands::app_conversations_search"));
    let lower = handler.to_ascii_lowercase();
    // v4 submit/cancel, content, and routes are enumerated daemon calls.
    assert!(lower.contains("submit_turn"));
    assert!(!lower.contains("::submit ") && !lower.contains("app_submit\""));
    assert!(
        !lower
            .replace("commands::app_conversation_cancel_turn", "")
            .contains("cancel")
    );
    assert!(!lower.contains("invoke"));
    assert!(!lower.contains("obsolete"));
    assert!(!lower.contains("approve"));
    assert!(!lower.contains("infer"));
}

#[test]
fn the_app_menu_offers_a_bounded_update_check() {
    const MAIN: &str = include_str!("main.rs");
    // The app menu exposes exactly one custom item: a bounded Check for Updates
    // that routes to the existing user-initiated updater.
    assert!(
        MAIN.contains("MenuItemBuilder::with_id(\"check-for-updates\""),
        "the app menu must expose a Check for Updates item"
    );
    assert!(
        MAIN.contains("on_menu_event"),
        "the Check for Updates item must be routed to the updater plugin"
    );
}

#[test]
fn no_execution_capable_plugin_is_declared() {
    // Comments are stripped first: this file's own manifest *names* the
    // plugins it refuses to depend on, and a naive substring match would
    // fail on the explanation rather than on a real dependency.
    const RAW_MANIFEST: &str = include_str!("../Cargo.toml");
    let manifest: String = RAW_MANIFEST
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    for plugin in [
        "tauri-plugin-shell",
        "tauri-plugin-fs",
        "tauri-plugin-http",
        "tauri-plugin-process",
        "tauri-plugin-opener",
        "tauri-plugin-dialog",
    ] {
        assert!(
            !manifest.contains(plugin),
            "{plugin} would give the webview a capability the desktop client must not have"
        );
    }
    const CAPABILITIES: &str = include_str!("../capabilities/default.json");
    let permissions = serde_json::from_str::<Value>(CAPABILITIES)
        .expect("valid JSON")
        .get("permissions")
        .and_then(Value::as_array)
        .expect("permissions array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    // `updater:default` is the one deliberate execution-adjacent permission.
    // It lets the webview ask the updater plugin to fetch and replace this app
    // bundle from the endpoint pinned in tauri.conf.json, and only after the
    // artifact verifies against the pinned Ed25519 public key there. Any other
    // addition to this list is a real capability grant and has to be reviewed
    // beside the no-execution-plugin denylist above, not added silently.
    assert_eq!(
        permissions,
        vec!["core:default".to_owned(), "updater:default".to_owned()]
    );
}
