# Abbey 2026-10-27 integration gate (main-sync Task 6 and plan integrations)

**Result: pass.** `tools/ci/verify-pinned-siblings.sh` printed `check.sh: OK`, exit 0, on `5472d40271200c32c752cb6820e2885c695a1f59`. The tree was clean. The pinned siblings were ABI `63d2bc0a393c3c3837eadab0d18bc2fc1fff5c78` and WDBX `612d3570f9ab0d842519aa4dbc4cb977aa33c9e0`, unpacked from clean archives. The toolchain was Rust 1.99.1. Rust tests: 6334 passed, 0 failed, 6 ignored. The soft cross-checks for `x86_64-pc-windows-gnu` and `x86_64-unknown-linux-gnu` were skipped because those targets are not installed. Desktop frontend steps on `fb0a493`: typecheck, `test:models` (189 pass), build and verify:bundle all exited 0. This receipt's own commit is docs-only and is not covered by the gate.

## What the gated head integrates on top of `0ad2fef`

- **CI sibling pins.**
  - `7b9519b`/`5182bc3` became `63d2bc0`/`612d357`. Main had imported `abi_agent_host::local_approval` since `30e3b84`, and the old ABI pin lacked it, so the pinned gate failed with E0432.
  - The new pins are the sibling heads the 2026-10-26 security packet built against. They are on the siblings' local `main` but **not yet published**. CI cannot check them out until `../abi` and `../wdbx` are pushed, which is a Donald step.
- **Gate tool.** `verify-pinned-siblings.sh` now resolves its workspace past the macOS `/var` symlink. Before this, the Tauri updater tests always failed under that script.
- **Authority plan Tasks 1–2.**
  - The dead `src/app_core/v4_types.rs` is removed.
  - Migration 12 adds the v4 admin authority tables, and `CURRENT_SCHEMA_VERSION` is now 12.
  - An adversarial review found two blocking trigger defects, and both were fixed with regression tests: profile revision and revocation epoch could regress, and an automation's spec could change without a new revision. The deferred `current_attempt_id` foreign key was also added.
  - Evidence: `2026-10-27-abbey-authority-tools-plan-task2.md`.
- **Correctness plan.**
  - The tools `source_manifest.py` and `flake_repeat.py`.
  - The anchored-pattern regression (ignored until the corpus is re-vendored).
  - The review-thread triage de-duplication, which took open todos from 48 to 41.
  - The re-verification evidence for the implemented rows.
  - The per-cohort binary-build guard.
- **Desktop plan.**
  - Task 1 component tests.
  - Training and Cluster pinned as unavailable.
  - The flow inventory.
  - The performance sample evaluator.
  - The Accessibility drivers behind opt-in gate variables.
- **Other plan tasks.**
  - The federation module split.
  - The personal-edition `shell.exec` contract, validation only.
  - The Program 3 live capture contract draft.
  - The windows-foundation `5be3d25` review. It found two medium findings that must be fixed before merging: undocumented `unsafe` blocks, and absolute rather than idle pipe timeouts. `5be3d25` itself is not merged.
  - The closure readiness inventory.
- **Docs.** The `CLAUDE.md` lines Donald approved in session `9f48b716`, plus the runtime row updated to schema v12.

## Failed attempts on the way

1. `0ad2fef` with the old pins: E0432 at compile time.
2. With the new pins under the default `TMPDIR`: 5 `updater_cached_verification` failures. Tauri refuses to run a binary whose `current_exe` path goes through the `/var` symlink.
3. With `TMPDIR` set to its real path: `default_file_rejects_socket_without_removing_it` failed because the socket path exceeded SUN_LEN. The fix resolves only the workspace path and leaves `TMPDIR` alone.
4. `fb0a493`: `tests/legacy_conversation_migration.rs` still expected schema 11. Fixed in `5472d40`.

## Not established

This receipt does not establish exact-head CI (that needs publication), native Windows or Linux behaviour, live daemon or provider behaviour, or any trained-model or production qualification. No claim was promoted.
