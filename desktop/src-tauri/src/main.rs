// Abbey desktop client — an enumerated Tauri 2 surface over Abbey's shared
// application core.
//
// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod commands;
mod ipc;
mod v3_ipc;

use abbey::edition::ACTIVE;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        // Deliberate exception to the no-execution-plugin stance, wired with
        // eyes open: `tauri-plugin-updater` can download and replace this app
        // bundle from the pinned release endpoint in `tauri.conf.json`
        // (`plugins.updater`), and only after verifying the artifact against
        // the pinned Ed25519 public key there. No other endpoint is trusted and
        // no other plugin is registered. The webview may invoke it only through
        // `updater:default` in capabilities/default.json; the Doctor surface's
        // update row is the single trigger surface (desktop/src/components/
        // UpdateCheck.tsx), and it never installs without the user pressing it.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Native application menu: Apple-standard predefined items only. Every
        // entry is an OS-provided role (about/hide/quit/undo/copy/minimize...
        // operate on this window or open the about panel); there are no custom
        // handlers that reach webview state, the daemon, or the filesystem, in
        // keeping with the read-only command surface. The FIRST submenu becomes
        // the macOS app menu (the OS reads its name from the bundle, not from
        // the submenu title).
        .menu(|handle| {
            use tauri::menu::{AboutMetadata, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
            let version = handle
                .config()
                .version
                .clone()
                .unwrap_or_else(|| "unknown".to_owned());
            // Same version source the doctor surface prints from
            // `app_status`; the menu introduces no second truth.
            let about = AboutMetadata {
                version: Some(version),
                ..AboutMetadata::default()
            };
            let product_name = handle.config().product_name.as_deref().unwrap_or("Abbey");
            // Bounded: the only custom menu handler in the bar. Clicking it
            // runs one manual update check against the pinned updater
            // endpoint — the same code path the Doctor surface's UpdateCheck
            // uses. No auto-check, no background scheduler, and no install
            // without an explicit button press (that stays in UpdateCheck).
            let check_for_updates = MenuItemBuilder::with_id("check-for-updates", "Check for Updates…")
                .build(handle)?;
            let app_menu = SubmenuBuilder::new(handle, product_name)
                .about(Some(about))
                .item(&check_for_updates)
                .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .quit()
            .build()?;
            let edit_menu = SubmenuBuilder::new(handle, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            let window_menu = SubmenuBuilder::new(handle, "Window")
                .fullscreen()
                .minimize()
                .maximize()
                .separator()
                .bring_all_to_front()
                .separator()
                .close_window()
                .build()?;
            MenuBuilder::new(handle)
                .item(&app_menu)
                .item(&edit_menu)
                .item(&window_menu)
                .build()
        })
        .setup(|app| {
            // Requirement, not decoration: the safe and personal editions must
            // never share a bundle identity. `tauri.conf.json` is generated
            // from `abbey::edition` by `desktop/codegen`, and this check is the
            // runtime half — a personal-edition binary packaged with the safe
            // identifier refuses to start rather than installing over it.
            let configured = commands::configured_identifier(app.handle());
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
            // The window title must name the edition that is actually on
            // screen: the personal edition ships as "Abbey Personal", and a
            // title bar reading "Abbey" would misreport the running build.
            // The base config's window title is the safe edition's default and
            // the edition overlay replaces arrays wholesale, so the title is
            // applied here from the same product name the bundle and the
            // menu use.
            if let Some(window) = app.get_webview_window("main") {
                if let Some(product_name) = app.config().product_name.clone() {
                    let _ = window.set_title(&product_name);
                }
            }
            // Route the single custom menu item ("Check for Updates…") to the
            // updater plugin's manual check: user-initiated (menu click only),
            // verified against the pinned Ed25519 key, never installs silently.
            app.handle().on_menu_event(move |app_handle, event| {
                if event.id() == "check-for-updates" {
                    let handle = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        match handle.updater() {
                            Ok(updater) => {
                                if let Err(err) = updater.check().await {
                                    eprintln!("abbey-desktop: update check failed: {err}");
                                }
                            }
                            Err(err) => eprintln!("abbey-desktop: update check failed: {err}"),
                        }
                    });
                }
            });
            Ok(())
        })
        // The complete IPC surface. Adding a command means adding it here, in
        // review, next to this comment.
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::app_claims,
            commands::app_routes,
            commands::app_run_status,
            commands::app_run_events,
            commands::app_v3_grants,
            commands::app_memory_search,
            commands::app_memory_metadata,
            commands::app_models_list,
            commands::app_model_grants,
            commands::app_model_details,
            commands::app_model_download,
            commands::app_model_load,
            commands::app_model_unload,
            commands::app_model_download_status,
            commands::app_model_lifecycle_status,
            commands::app_claim_by_id,
            commands::app_connection,
            commands::app_bundle_identity,
            commands::app_conversation_grants,
            commands::app_conversations_list,
            commands::app_conversations_search,
            commands::app_conversation_read,
            commands::app_conversation_create,
            commands::app_conversation_rename,
            commands::app_conversation_archive,
            commands::app_conversation_delete,
            commands::app_conversation_messages,
            commands::app_conversation_content,
            commands::app_conversation_routes,
            commands::app_conversation_submit_turn,
            commands::app_conversation_cancel_turn,
        ])
        .run(tauri::generate_context!())
        .expect("abbey-desktop failed to start");
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
