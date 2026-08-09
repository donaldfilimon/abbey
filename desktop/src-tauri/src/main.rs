// Abbey desktop client — a read-only Tauri 2 surface over Abbey's shared
// application core.
//
// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod commands;
mod ipc;

use abbey::edition::ACTIVE;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Requirement, not decoration: the safe and personal editions must
            // never share a bundle identity. `tauri.conf.json` is generated
            // from `abbey::edition` by `desktop/codegen`, and this check is the
            // runtime half — a personal-edition binary packaged with the safe
            // identifier refuses to start rather than installing over it.
            let configured = commands::configured_identifier(&app.handle());
            let expected = ACTIVE.bundle_id();
            if configured != expected {
                return Err(format!(
                    "bundle identity mismatch: this binary is the {} edition ({expected}) but was \
                     packaged as {configured}. Regenerate with `cd desktop && bun run codegen` \
                     and build the personal edition with \
                     `--features personal-edition --config tauri.personal.conf.json`.",
                    ACTIVE.slug()
                )
                .into());
            }
            Ok(())
        })
        // The complete IPC surface. Adding a command means adding it here, in
        // review, next to this comment.
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::app_claims,
            commands::app_routes,
            commands::app_connection,
            commands::app_bundle_identity,
        ])
        .run(tauri::generate_context!())
        .expect("abbey-desktop failed to start");
}

#[cfg(test)]
mod tests {
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
        assert_eq!(permissions, vec!["core:default".to_owned()]);
    }
}
