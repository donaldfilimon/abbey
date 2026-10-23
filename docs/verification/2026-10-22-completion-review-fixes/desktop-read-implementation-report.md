# Desktop read bridge implementation

Applied the prepared candidate after controller-confirmed RED (13 compile errors: 11 non-Future signatures and two missing helper references). Canonical commands.rs matched commands.before.rs before mutation. Frozen commands.rs matched that baseline plus the RED-only read_tests module registration; existing frozen tests matched prepared tests. Root AGENTS.md matched across trees; no nested desktop instructions were found.

## Files changed

- `desktop/src-tauri/src/commands.rs`: 31373 bytes; SHA-256 `b7175b46db1dc657d0e45ecf0b6376008f504e631ef9058a23f377e7f5d451b4`; canonical and frozen bytes identical.
- `desktop/src-tauri/src/commands/read_tests.rs`: 2060 bytes; SHA-256 `f8c55dbee2701c3cef55069e3c982b409a5a7a1263ad918fc7dd7a5c321a8790`; canonical and frozen bytes identical.

Eleven existing Tauri read commands now return futures and delegate blocking backend work to read_task via spawn_blocking: app_status, app_effective_model, app_claims, app_routes, app_run_status, app_run_events, app_v3_grants, app_memory_search, app_memory_metadata, app_claim_by_id, app_connection. Effective-model logic remains in a private synchronous helper so existing model-resolution tests retain their scope. app_connection now returns Result in Rust to represent sanitized worker failure; successful serialized ConnectionInfo payload remains unchanged. Existing effect/task helpers, command names, registration, backend selection, and grants remain unchanged.

Three regression tests check Future signatures without invoking daemon I/O, a stalled read yielding and running on another thread, and a worker panic returning a Transport error without its payload. Helper panic handling sanitizes the returned IPC error; the standard panic hook may still print a panic payload to process stderr, which is outside the returned IPC payload.

## Verification and limits

`CARGO_TARGET_DIR=<pending-desktop-read>/format-target ./tools/cargo.sh fmt --package abbey-desktop` succeeded in each tree. Both owned files remain byte-identical across trees. No Cargo build/test, commit, push, or external-service action was performed. Controller owns GREEN tests and broader verification; this report makes no gate/packaging/native acceptance claim. No generic effect dispatch was added.
