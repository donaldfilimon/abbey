# Read-only identity-probe investigation

Confirmed actual cause, 2026-10-23. No canonical edits, Cargo, agents, real
profile writes, or service operations performed. Only scratch identity CLI
executions and external evidence writes.

## Source path

`tools/linux_user_service.py:102-108` uses HOME=target user home and a clean
PATH for both edition identity probes, with no edition state/config overrides.
The CLI parses args then unconditionally calls AbbeyState::load before command
dispatch (`src/entry.rs:28-44`). AbbeyState::load selects the active edition's
state override or default root and calls create_dir_all for root and by-cwd
(`src/state.rs:37-62`). No private creation mode is specified; the resulting
mode depends on caller umask. `edition --name` dispatch prints the compile-time
name only later (`src/edition.rs:352-360`). No suitable existing supported
identity-before-initialization bypass was found. The local TUI recipe shortcut
has a separate constrained grammar and is not an identity-probe API.

Default state is ~/.local/state/<edition> on Unix. Config may also be read in
AgentConfig default/backend resolution; config overrides are ABBEY_CONFIG and
ABBEY_PERSONAL_CONFIG (`src/edition.rs:84,101,235-240`; config::load).

## Actual scratch reproduction

Executed:
`python3 /Users/donald/.codex/verification/abbey-completion-20261022/linux-service-identity-probe.py`

Script APFS-clones the already qualified external debug/abbey executable into a
fresh temporary directory; runs only `edition --name`, with three-second timeout
and scratch HOME/cwd. It never starts a daemon or uses systemd. No Cargo build.
JSON receipt: linux-service-identity-probe.json. Both invocations exited 0,
stdout `abbey`, empty stderr.

1. HOME-only clean env created .local/state/abbey and by-cwd under scratch HOME,
   both mode0755 (plus parent0755 directories).
2. Same clean env with both editions' state/config overrides pointing into an
   owner-only temporary probe directory left scratch HOME completely empty.
   The CLI-created safe-state/by-cwd remained0755 but were confined within the
   0700 throwaway probe directory, outside intended service state.

This directly reproduces the controller's reported failure: an identity query
creates the future service root0755, and install correctly refuses that root
rather than silently changing/adopting it. Fixture identity commands omitted
this startup behavior, so the existing tests missed the interaction.

## Recommended bounded fix, not implemented in this investigation

Wrap both identity probes in one TemporaryDirectory lifetime. Construct a
separate probe environment from only fixed PATH plus a scratch HOME and
explicit variables for *both* editions before knowing compiled identity:

- ABBEY_STATE_DIR=<probe>/safe-state
- ABBEY_PERSONAL_STATE_DIR=<probe>/personal-state
- ABBEY_CONFIG=<probe>/safe-config.toml
- ABBEY_PERSONAL_CONFIG=<probe>/personal-config.toml
- HOME=<probe>/home (create0700)
- optionally XDG_CONFIG_HOME/XDG_STATE_HOME=<probe>/xdg-config or xdg-state for
  complete fallback containment, while explicit edition roots remain decisive.

Resolve tempfile root to a real path (macOS /var aliases) for predictable paths.
Do not copy any of those temporary values into self.env/service_env; those keep
target HOME and the explicit service root. Cleanup probes on success or failure.
Keep existing three-second/64KiB child bounds and identity pair validation.
Do not chmod or adopt an existing permissive service state root. A small
controller-only change avoids widening Rust CLI initialization scope.

## Meaningful regression

Replace identity fixture with a deliberately eager initializer that, before
printing either edition flag, resolves the active edition state override or
HOME/.local/state/<slug>, mkdirs root/by-cwd with0755, and records selected
HOME/state/config paths in a test-owned log. Under current controller:
constructing then installing fails on private state mode. Under fixed controller:
assert both probes saw explicit separate safe/personal scratch overrides and
scratch config paths, scratch roots were removed after probes, intended service
root did not exist immediately after constructor, install creates0700 service
state, and existing permissive service state remains refused/unmodified. Run
for both editions and an invalid identity failure to prove cleanup. Do not just
assert temporary environment keys exist: emulate eager state creation.

Controller can then rerun its actual already-built cloned CLI/daemon scratch
integration (no systemd acceptance); actual safe CLI identity-scoping success
is already demonstrated above. Native Ubuntu systemd acceptance remains pending.
