# Proposed Linux user-service bootstrap slice

Prepared 2026-10-23 by source inspection only. No canonical edits, builds, or service operations were performed.

## Recommendation and boundary

Implement an explicit Linux-only user-service installer/controller for already-installed, edition-matched binaries. Keep ordinary `install.sh` behavior unchanged. Scope: install/start/status/stop/restart/uninstall of one systemd user unit and private service configuration. This advances `linux-service-foundation`; it does not close that requirement, add native package distribution, or implement desktop automatic discovery/startup. Desktop currently chooses its daemon route only when bearer environment configuration exists (`desktop/src-tauri/src/backend/mod.rs::bearer_source`, `route`). Automatic credential discovery changes that policy and should follow separately.

## Exact existing invocation contracts

- CLI identity: `<installed-cli> edition --name` and `<installed-cli> edition --daemon-name`. Flags conflict; invoke separately. `edition` with no flags prints a descriptive report, unsuitable for filename discovery. Source: `src/cli.rs::Commands::Edition`, `src/edition.rs::cmd_edition`.
- Allowed identity pairs: `abbey` / `abbeyd`; `abbey-personal` / `abbey-personal-daemon`. Reject failed, empty, unknown or inconsistent probes; never fall back to safe names. Existing installer precedent: `install.sh`.
- Authenticated status grammar: `<installed-cli> daemon status [--json]`. Use `daemon status --json`, not top-level `status` (which follows backend passthrough behavior). `src/cli/args.rs::DaemonCmd::Status` defines the boolean `--json`; `src/commands.rs::run_daemon_command` maps it to `AppCommand::Status`, prints the serialized `AppEvent`, and returns zero on success. Readiness must validate authenticated status and edition, not merely systemd process activity or a human-readable substring.
- Daemon startup: execute the installed daemon absolute path with **no command-line arguments**. `src/bin/abbeyd.rs::run` has no argument parser: it loads `DaemonConfig::from_env`, then `RuntimeDaemonConfig::from_env`, starts `RuntimeHandler`, and serves. There are no `--socket`, `--token-file`, `--state-dir`, `--foreground` or `serve` options. Unix SIGINT/SIGTERM requests shutdown through `shutdown_on_signals`.
- Safe service environment: `ABBEYD_BEARER_TOKEN_FILE`, `ABBEYD_SOCKET_PATH`, `ABBEY_STATE_DIR`; personal: `ABBEY_PERSONAL_DAEMON_BEARER_TOKEN_FILE`, `ABBEY_PERSONAL_DAEMON_SOCKET_PATH`, `ABBEY_PERSONAL_STATE_DIR`. Inline bearer and bearer-file sources are mutually exclusive. Never place inline token bytes in unit content, argv or logs. Optional ABI provider binding is startup-only `ABBEY_ABI_BIN` / `ABBEY_PERSONAL_ABI_BIN`; omit by default rather than inventing provider authority.
- Resolve and pin the service working directory explicitly: `RuntimeDaemonConfig::from_env` captures `current_dir` as startup-owned workspace. State defaults are edition-scoped XDG state roots; sockets belong under `<state>/daemon/abbeyd.sock` or `<state>/daemon/abbey-personal-daemon.sock`. Reuse existing resolution, or set explicit consistent state/socket paths for both daemon and status probe.

## Security, isolation and retention

Use edition-specific unit names, private configuration directories, token paths, socket paths and state. Create a cryptographically random bearer once with owner-only permissions; preserve it on idempotent reinstall. Validate ownership/type/permissions before trusting existing files; reject symlinks, unsafe directories, unrelated existing units and unknown identity. Avoid partial replacement: stage/validate before atomic publication and preserve previous content on failure. Quote/escape absolute binary and filesystem paths correctly for systemd; do not interpolate caller input into shell execution. Set `UMask=0077`. Do not source executable shell configuration. Avoid silently inheriting conflicting inline credentials or broad model-runtime settings.

Uninstall stops/disables/removes only the installer-owned unit and service integration artifacts. Preserve credentials, runtime.sqlite, memory, chats, backups, model stores and audit history. Never remove another edition's files. Do not delete a pre-existing user unit that is not provably owned by this installer. Define failed start/readiness handling explicitly and report the failure without claiming service readiness.

## Existing foundations to reuse

`src/daemon/config.rs::{DaemonConfig::from_env,load_bearer_file}` validates owner-only regular credential files. `src/daemon/server/unix.rs::{serve,prepare_private_directory,read_frame}` supplies owner-only Unix endpoints and bounded framing. Existing typed clients supply correlation/authentication; existing runtime supplies cancellation and restart recovery. No transport changes or wire-schema changes are needed for this slice.

## Focused verification

1. Run installer/controller tests against a temporary HOME/XDG tree, fixture identity binaries and a fake systemctl that records exact arguments. Cover both editions, hostile path characters, missing tools/binaries, unknown identity, idempotent reinstall/token preservation, ownership/type/mode refusal, rollback on publication failure, readiness failure, and uninstall retention. Assert secret bytes never enter logs or units.
2. Use a real scratch daemon on the local Unix host to exercise authenticated status readiness and edition mismatch/refusal. Reuse `tests/daemon_runtime.rs::real_daemon_v2_is_idempotent_paged_cancellable_and_reopenable` for transport/cancel/reopen behavior rather than duplicating its protocol assertions. Mock tests prove controller behavior, not systemd behavior.
3. On native Ubuntu ARM64, perform install/start/status/cancel/restart/uninstall-retention with real systemd user services, both editions concurrently, private files and retained state. Record actual executed CI evidence. Native Ubuntu and Windows evidence is unavailable on this macOS host; local tests do not satisfy those acceptance fields.

Keep `tasks/completion.json` open and distinguish source/automated receipts from pending `ubuntu_arm64` and `native_ci`. Follow required root/desktop gates for any eventual canonical edits and obtain Abbey guardrails review. Windows named pipes/ACLs/peer restriction and Job Objects remain a separate larger platform slice.
