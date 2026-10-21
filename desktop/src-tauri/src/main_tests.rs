use abbey::edition::Edition;
use serde_json::Value;
use std::path::Path;

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

fn window(config: &str) -> Value {
    serde_json::from_str::<Value>(config)
        .expect("tauri config is valid JSON")
        .pointer("/app/windows/0")
        .cloned()
        .expect("the config declares a main window")
}

fn bundle_icons(config: &str) -> Vec<String> {
    serde_json::from_str::<Value>(config)
        .expect("tauri config is valid JSON")
        .pointer("/bundle/icon")
        .and_then(Value::as_array)
        .expect("the config declares bundle.icon")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
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
        "lookup_turn",
        "update",
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
fn the_menu_keeps_apple_roles_and_one_user_initiated_handler() {
    const MAIN: &str = include_str!("main.rs");
    // Exactly one custom menu item exists in the whole bar. Every other entry
    // is an OS-provided role, so no menu item can reach webview state, the
    // daemon, or the filesystem.
    assert_eq!(
        MAIN.matches("MenuItemBuilder::").count(),
        1,
        "the menu bar must not grow a second custom item"
    );
    assert_eq!(
        MAIN.matches("MenuItem::with_id(").count(),
        0,
        "the menu bar must not build an unnamed custom item"
    );
    // The first submenu is the app menu: on macOS the OS reads the app menu's
    // name from the bundle, and only the first submenu becomes it.
    let builder = MAIN
        .split("MenuBuilder::new(handle)")
        .nth(1)
        .expect("the menu builder");
    let first_item = builder.split(".item(").nth(1).expect("the first item");
    assert!(
        first_item.contains("&app_menu"),
        "the app menu must be the first submenu: {first_item}"
    );
    // About carries the version the bundle declares — the same source the
    // doctor surface prints, so the menu introduces no second truth.
    assert!(
        MAIN.contains("let version = handle"),
        "the about version comes from the config"
    );
    assert!(
        MAIN.contains(".version"),
        "the about version comes from the config"
    );
    assert!(
        MAIN.contains("version: Some(version)"),
        "the about panel must show the configured version"
    );
    // The update check is user-initiated and singular: one call, inside the
    // menu handler, with no background scheduler and no automatic run.
    assert_eq!(
        MAIN.matches(".check()").count(),
        1,
        "one update check, one place"
    );
    assert!(
        MAIN.contains("event.id() == \"check-for-updates\""),
        "the update check must only run from the menu item"
    );
    for scheduler in ["set_interval", "set_timeout", "sleep("] {
        assert!(
            !MAIN.contains(scheduler),
            "the updater must not run from a background scheduler ({scheduler})"
        );
    }
}

#[test]
fn the_window_is_never_globally_transparent() {
    for (name, config) in [("safe", SAFE_CONFIG), ("personal", PERSONAL_CONFIG)] {
        let window = window(config);
        assert_eq!(
            window.get("transparent").and_then(Value::as_bool),
            Some(false),
            "the {name} edition must not enable a transparent window"
        );
        // A standard, visible title bar is what carries the native material;
        // an overlay title bar would put the webview over it.
        assert_eq!(
            window.get("titleBarStyle").and_then(Value::as_str),
            Some("Transparent"),
            "the {name} edition must keep a transparent title bar"
        );
    }
}

#[test]
fn the_two_editions_agree_on_the_window_except_the_title() {
    let mut safe = window(SAFE_CONFIG);
    let mut personal = window(PERSONAL_CONFIG);
    // The title is the one per-edition difference; everything else about the
    // window has to stay identical or the editions would drift apart.
    assert_eq!(safe.get("title").and_then(Value::as_str), Some("Abbey"));
    assert_eq!(
        personal.get("title").and_then(Value::as_str),
        Some("Abbey Personal")
    );
    for window in [&mut safe, &mut personal] {
        if let Some(object) = window.as_object_mut() {
            object.remove("title");
        }
    }
    assert_eq!(
        safe, personal,
        "the two editions' windows must differ only in their title"
    );
}

#[test]
fn the_window_title_names_the_edition_that_is_on_screen() {
    const MAIN: &str = include_str!("main.rs");
    // The title is taken from the configured product name, so the personal
    // edition can never present itself as "Abbey".
    assert!(
        MAIN.contains("app.config().product_name.clone()"),
        "the window title must come from the configured product name"
    );
    assert!(MAIN.contains("window.set_title(&product_name)"));
}

#[test]
fn every_declared_bundle_icon_exists_and_covers_both_platforms() {
    let icons = bundle_icons(SAFE_CONFIG);
    for required in [
        "icons/icon.icns",
        "icons/icon.png",
        "icons/icon.ico",
        "icons/master.png",
    ] {
        assert!(
            icons.iter().any(|icon| icon == required),
            "bundle.icon must reference {required}"
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(required);
        assert!(path.is_file(), "missing icon file: {}", path.display());
    }
}

#[test]
fn the_personal_edition_inherits_the_same_icon_chain() {
    // The overlay must not declare a divergent bundle: the personal bundle
    // carries the same icon chain as the safe one, by inheritance.
    let personal: Value =
        serde_json::from_str(PERSONAL_CONFIG).expect("tauri config is valid JSON");
    assert!(
        personal.get("bundle").is_none(),
        "the personal overlay must inherit bundle.icon instead of restating it"
    );
}

#[test]
fn the_native_appearance_is_wired_and_never_makes_the_window_transparent() {
    const MAIN: &str = include_str!("main.rs");
    const APPEARANCE: &str = include_str!("appearance.rs");
    assert!(
        MAIN.contains("appearance::apply(&window)"),
        "the native appearance module must be applied at startup"
    );
    // The appearance is macOS-only; every other target compiles the module
    // down to an opaque no-op.
    assert!(APPEARANCE.contains("#[cfg(target_os = \"macos\")]"));
    assert!(APPEARANCE.contains("unsupported()"));
    // Material is installed in the window frame only. The webview is never
    // made transparent: the module must not fade the window, and it must keep
    // an opaque safety panel behind the webview so the window can never
    // become a see-through hole.
    assert!(
        !APPEARANCE.contains("setAlphaValue"),
        "the appearance module must not fade the window"
    );
    assert!(
        APPEARANCE.contains("NSBox"),
        "the appearance module must keep an opaque panel behind the webview"
    );
    assert!(
        APPEARANCE.contains("setTransparent(false)"),
        "the safety panel behind the webview must stay opaque"
    );
    assert!(
        APPEARANCE.contains("setOpaque(true)"),
        "removing the material must restore the opaque window"
    );
    // The degrade path is part of the contract, not an afterthought.
    assert!(APPEARANCE.contains("reduce_transparency"));
    assert!(APPEARANCE.contains("increase_contrast"));
    assert!(APPEARANCE.contains("differentiate_without_color"));
    assert!(APPEARANCE.contains("invert_colors"));
    assert!(
        APPEARANCE.contains("native material unavailable: opaque"),
        "the opaque outcome must say so plainly"
    );
}

#[test]
fn the_window_keeps_its_titlebar_for_the_native_material() {
    // A transparent title bar is what frees the frame's title bar strip for
    // the native material: the webview then occupies the content layout rect
    // and the strip above it belongs to the window frame.
    for (name, config) in [("safe", SAFE_CONFIG), ("personal", PERSONAL_CONFIG)] {
        assert_eq!(
            window(config).get("titleBarStyle").and_then(Value::as_str),
            Some("Transparent"),
            "the {name} edition must keep a transparent title bar for the material"
        );
    }
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
