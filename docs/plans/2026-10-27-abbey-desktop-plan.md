# Abbey Desktop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close what the repository can close of `native-controls-proof`, `desktop-product` and `portable-package-services`: finish the desktop render-test debt, wire the Accessibility drivers into the client gate, add governed Tasks/Automations/Approvals surfaces on top of the durable v4 authority, and stage every owner-only step (Accessibility runner, notarisation, Windows/Linux hosts, signing) as a Donald step with its evidence file.

**Architecture:** The desktop never fabricates operational data: a surface becomes available only when the live grant set returned by `app_status` / the v3/v4 sessions includes what it needs (`desktop/src/surfaces.ts:1-13`). Each new operation is one narrow Tauri command, listed in `generate_handler!` (`desktop/src-tauri/src/main.rs:209`), mirrored in `COMMANDS` (`desktop/src/ipc/client.ts:45`), typed by codegen, rendered by a view, and covered by a bun test and a Rust command test.

**Tech Stack:** Tauri 2, React + TypeScript, bun, Rust 1.99.1 (`abbey-desktop`, `abbey-desktop-codegen` workspace members), macOS Accessibility (Swift AX drivers).

**Spec:** `tasks/completion.json` (`native-controls-proof`, `desktop-product`, `portable-package-services`); `docs/superpowers/plans/2026-10-22-abbey-completion.md` §6 "Complete desktop product"; `desktop/README.md` §§ "Views: real vs placeholder" (`:413-451`), "Unmet bars" (`:457+`); `docs/completion-plan-2026-10-22.md` WS-2, WS-8, WS-9, OB-1…OB-5.

## Global Constraints

Inherit `docs/plans/2026-10-27-abbey-completion-index.md` § Global Constraints. Desktop-specific (from `docs/agent-guide.md` "Commands and the gate" and `desktop/README.md`):
- The client gate is `desktop/check.sh` (bun typecheck, `bun test tests`, build, `verify:bundle`, clippy/tests for `abbey-desktop` in both editions, live scratch `abbeyd` reads, linked build). The root `./check.sh` runs only codegen `--check`. Run the two gates serially (shared per-checkout lock).
- Never hand-edit `desktop/src/ipc/generated.ts`, `generated.samples.ts`, `generated.editions.ts`; regenerate with `(cd desktop && bun run codegen)` after changing `src/app_core/` or `src/effective_model.rs`.
- The `generate_handler!` block stays comment-free (`desktop/scripts/verify-bundle.mjs:104-108` parses it). `COMMANDS` must equal it exactly (`tools/check_claims_sync.py` fails on drift).
- Signing capabilities stay exactly `core:default` + `updater:default`. The pinned updater public key (`desktop/src-tauri/tauri.conf.json:69-72`) is never regenerated to match a convenience key. No credential is manufactured or embedded.
- Opt-in macOS acceptance (`ABBEY_DESKTOP_*_ACCEPTANCE=1`) needs Accessibility permission and a packaged bundle; it is evidence only when it actually ran.

## Review Focus

- A view shown "available" because a grant is present while its commands still refuse → each new view test renders with the grant and asserts real data from a mocked command, plus a test with the grant absent.
- An Automations view that computes "next occurrence" in the browser with the local clock → must display the daemon's value only.
- A task card that invents a record when the daemon call fails → renders the error card (`desktop/src/views/ErrorCard.tsx`), never placeholder rows.
- The personal-edition bundle proved when the safe one was intended → `--expect-edition` is explicit in every acceptance step.
- An opt-in acceptance variable silently skipped when the prerequisite is missing → the wired steps must fail, not skip, when enabled without Accessibility permission.

## File map

- Create: `desktop/tests/command-palette.test.tsx`, `desktop/tests/error-boundary.test.tsx`, `desktop/tests/preferences.test.tsx`
- Modify: `desktop/check.sh:106-133` (two new opt-in steps); Create `desktop/scripts/tests/test_ax_drivers_wired.py`
- Create: `desktop/src-tauri/src/commands/tasks.rs`, `desktop/src-tauri/src/commands/automations.rs`, `desktop/src-tauri/src/commands/actions.rs` (inspect the existing `desktop/src-tauri/src/commands/` directory layout first and follow it)
- Modify: `desktop/src-tauri/src/main.rs:209-247`, `desktop/src/ipc/client.ts:45-91`, `desktop/src/surfaces.ts:75,114,138,152`
- Create: `desktop/src/views/TasksView.tsx`, `desktop/src/views/AutomationsView.tsx`, `desktop/src/views/ApprovalsView.tsx`; tests `desktop/tests/tasks-surface.test.tsx`, `automations-surface.test.tsx`, `approvals-surface.test.tsx`
- Modify: `desktop/src/views/RunsView.tsx` (withheld "Tasks" card replaced only when task grants exist)
- Modify: `desktop/README.md` §§ Views / Unmet bars (prose outside generated regions)
- Evidence: `docs/verification/YYYY-MM-DD-desktop-*.md`

---

### Task 1: Render tests for the untested components

`desktop/tests/` already has `models-view.test.tsx` and `update-check.test.tsx`; `CommandPalette`, `ErrorBoundary` (`desktop/src/components/`) and `usePreferences` (`desktop/src/hooks/usePreferences.ts`) have none.

**Files:** Create the three test files above.

- [ ] **Step 1:** Write `command-palette.test.tsx` (`opens on its shortcut, filters by typed text, closes on Escape, never lists an unavailable surface as runnable`), `error-boundary.test.tsx` (`renders the fallback with the error message and no stack in production mode`), `preferences.test.tsx` (`persists appearance/density and survives a storage that throws`).
- [ ] **Step 2:** `cd desktop && bun test tests/command-palette.test.tsx tests/error-boundary.test.tsx tests/preferences.test.tsx` → each assertion that encodes missing behaviour FAILS; assertions on existing behaviour pass (record which).
- [ ] **Step 3:** Fix any component defect the failing assertions reveal (minimal change).
- [ ] **Step 4:** Rerun → PASS; `bun run typecheck` → 0.
- [ ] **Step 5:** `./desktop/check.sh` → `desktop/check.sh: OK`. Commit `"Cover the command palette, error boundary and preferences with render tests"`.

### Task 2: Wire the Accessibility drivers into the client gate

`desktop/scripts/drive-surfaces-macos.py` and `drive-controls-macos.py` exist but no gate runs them (`grep -n "drive-" desktop/check.sh` → nothing).

**Files:** Modify `desktop/check.sh` (after the menu step, before `desktop/check.sh: OK`); Create `desktop/scripts/tests/test_ax_drivers_wired.py`

Current driver interface (verified): neither driver takes arguments; both read `ABBEY_DESKTOP_APP` (surfaces also `ABBEY_DESKTOP_BINARY`) and **exit 0 with `{"status":"skipped"}`** when not on macOS or when the bundle is missing (`desktop/scripts/drive-controls-macos.py:84-92`, `desktop/scripts/drive-surfaces-macos.py:122-135`). The AX trust check lives in the Swift driver (`desktop/scripts/routes-ax.swift:149`). A gate step that calls them as-is would pass by skipping.

- [ ] **Step 1:** Failing Python tests: `test_surface_driver_runs_when_its_variable_is_set` and `test_control_driver_runs_when_its_variable_is_set` (parse `desktop/check.sh`; assert a block guarded by `ABBEY_DESKTOP_SURFACE_ACCEPTANCE` runs `drive-surfaces-macos.py`, and one guarded by `ABBEY_DESKTOP_CONTROL_ACCEPTANCE` runs `drive-controls-macos.py`, each exporting an explicit `ABBEY_DESKTOP_APP` and `ABBEY_DESKTOP_DRIVER_REQUIRED=1`); `test_a_required_driver_fails_instead_of_skipping_without_a_bundle` (import each driver module, set `ABBEY_DESKTOP_DRIVER_REQUIRED=1` and a missing `ABBEY_DESKTOP_APP`, assert `main()` returns non-zero); `test_a_required_driver_refuses_the_wrong_edition_bundle` (driver checks the bundle identifier via `prove_packaged_macos.bundle_identifier` against `ABBEY_DESKTOP_EXPECT_EDITION`).
- [ ] **Step 2:** `python3 -m unittest discover -s desktop/scripts/tests -p 'test_*.py'` → FAIL.
- [ ] **Step 3:** In both drivers: when `ABBEY_DESKTOP_DRIVER_REQUIRED=1`, every "skipped" path returns 1, and the bundle's identifier must match `ABBEY_DESKTOP_EXPECT_EDITION` (`safe`/`personal`, reuse `EDITIONS_BY_IDENTIFIER` from `desktop/scripts/prove_packaged_macos.py`). Without the variable, behaviour is unchanged. Add the two guarded blocks to `desktop/check.sh`, mirroring the `ABBEY_DESKTOP_MENU_ACCEPTANCE` block (`desktop/check.sh:128-133`), defaulting the app path to `../target/release/bundle/macos/Abbey.app` and the edition to `${ABBEY_DESKTOP_PACKAGED_EDITION:-safe}`.
- [ ] **Step 4:** Rerun → PASS; `./desktop/check.sh` (variables unset) → OK.
- [ ] **Step 5:** Commit `"Wire the macOS Accessibility surface and control drivers behind opt-in gate variables"`.

### Task 3: **Donald step** — run the drivers on an Accessibility-enabled host (OB-5)

- [ ] **Step 1:** `./desktop/scripts/release-macos.sh` (staging bundles, ad-hoc signed) on a Mac where Donald has granted Accessibility to the driving terminal.
- [ ] **Step 2:** `ABBEY_DESKTOP_SURFACE_ACCEPTANCE=1 ABBEY_DESKTOP_CONTROL_ACCEPTANCE=1 ABBEY_DESKTOP_PACKAGED_ACCEPTANCE=1 ABBEY_DESKTOP_MENU_ACCEPTANCE=1 ./desktop/check.sh`, then again with `ABBEY_DESKTOP_PACKAGED_EDITION=personal`.
- [ ] **Step 3:** Evidence: `docs/verification/YYYY-MM-DD-desktop-native-controls.md` with host, macOS version, both editions' logs and the driver JSON.

### Task 4: Close `native-controls-proof`

- [ ] **Step 1:** If Task 3 passed for both editions: tick `tasks/todo.md:166` ("Not proven by the gate: the Accessibility control driver") with the evidence path; `tasks/completion.json` `native-controls-proof` `source`/`automated` passed, status `complete`.
- [ ] **Step 2:** `python3 tools/check_completion.py` → 0; `python3 tools/check_claims_sync.py --write` → check 0.
- [ ] **Step 3:** Commit `"Record native Accessibility control acceptance for both editions"`.

### Task 5: Decision gate — desktop transport for v4 admin commands

Precondition: authority-tools plan Tasks 7-8 merged (typed `V4DaemonSession` admin methods exist).

- [ ] **Step 1:** Read `desktop/src-tauri/src/backend/conversations.rs` and `backend/v3.rs` to see how existing v4/v3 sessions are opened and how a missing bearer fails closed (`a_configured_daemon_never_falls_back_to_the_in_process_core`).
- [ ] **Step 2:** Record whether admin commands can reuse the conversation session object (Branch R) or need a separately negotiated grant set (Branch S). Tasks 6-8 follow the recorded branch; the IPC command names below are the same in both.

### Task 6: Tasks surface

**Interfaces:** Tauri commands `v4_list_tasks(query) -> V4TaskPage`, `v4_read_task(id) -> V4Task`, `v4_task_events(query) -> V4TaskEventPage`, `v4_cancel_task(m) -> V4Task`, `v4_retry_task(r) -> V4Task` (create is deliberately absent until Task 9's confirmation design is reviewed).

- [ ] **Step 1: Failing tests:** Rust `desktop/src-tauri` command tests `task_commands_reject_without_task_grants` and `task_commands_forward_exact_queries_to_the_daemon_session`; bun `tasks-surface.test.tsx`: `renders tasks from the daemon page`, `shows the withheld reason when ReadTasks is not granted`, `cancel requires an explicit confirmation and sends once`, `an ambiguous cancel response keeps the id for status-only recovery`.
- [ ] **Step 2:** `cargo test --locked -p abbey-desktop task_commands` and `cd desktop && bun test tests/tasks-surface.test.tsx` → FAIL.
- [ ] **Step 3:** Implement commands; add names to `generate_handler!` (no comments) and `COMMANDS`; `(cd desktop && bun run codegen)`; add `TasksView.tsx`; replace the RunsView "Tasks / Unavailable" card only when `ReadTasks` is granted.
- [ ] **Step 4:** Rerun → PASS; `cd desktop && bun run typecheck && bun run build && bun run verify:bundle` → 0.
- [ ] **Step 5:** `./check.sh` then `./desktop/check.sh` → OK. Commit `"Add the governed Tasks surface over durable v4 task authority"`.

### Task 7: Automations surface

**Interfaces:** `v4_list_automations`, `v4_upsert_automation`, `v4_set_automation_enabled`, `v4_run_automation_now`.

- [ ] **Step 1: Failing tests:** `automations-surface.test.tsx`: `shows revision, schedule, timezone, enablement and the daemon's next occurrence`, `never computes next occurrence locally`, `run-now requires confirmation and records the returned task id`; Rust `automation_commands_reject_without_automation_grants`.
- [ ] **Step 2-4:** FAIL → implement (+ `surfaces.ts:75` entry becomes grant-derived) → PASS.
- [ ] **Step 5:** Both gates OK. Commit `"Add the Automations surface over durable automation authority"`.

### Task 8: Approvals surface (action decisions)

**Interfaces:** `v4_read_action`, `v4_decide_action`.

- [ ] **Step 1: Failing tests:** `approvals-surface.test.tsx`: `shows the exact action input and policy before a decision`, `a decision binds the action id, a fresh decision id and the kind`, `a second conflicting decision shows the daemon refusal`; Rust `decide_action_rejects_without_decide_grant`.
- [ ] **Step 2-4:** FAIL → implement in the Tools view (`surfaces.ts:114`) → PASS.
- [ ] **Step 5:** Both gates OK. Commit `"Add action approvals to the Tools surface"`.

### Task 9: Decision gate — task creation from the desktop

Creating a task delegates work under a route/profile/budget snapshot. Whether the desktop may originate it (vs. only manage tasks created by CLI/TUI) is a product decision.

- [ ] **Step 1:** **Donald step:** decide. Evidence: decision line in `docs/verification/YYYY-MM-DD-desktop-task-create-decision.md`.
- [ ] **Step 2 — if allowed:** TDD `v4_create_task` command + composer with explicit route/profile/budget display and confirmation (`create sends once and shows the immutable snapshot`); both gates; commit.

### Task 10: Training and Cluster stay honest until their authority exists

- [ ] **Step 1:** Test `unavailable-surface.test.tsx` gains `training and cluster remain unavailable while no training or worker grant is negotiated` (they flip only after the models-media-federation plan serves `ReadTraining`/`ReadWorkers`).
- [ ] **Step 2:** Run → PASS (pins current behaviour). Commit.

### Task 11: Remaining chat/product flows (enumerate, then one TDD cycle each)

§6 of the active plan lists: edition-matched onboarding/daemon startup, credentials outside WebView IPC, provider/workspace/profile configuration, durable multi-turn streamed chat, attachments/search/pin/unarchive/export, explicit retry, exact Stop recovery, memory curation, models/roles/permissions, diagnostics, backup/restore/retention, services/notifications, native menus/window state, accessibility, bounded history.

- [ ] **Step 1:** For each item, record in `docs/verification/YYYY-MM-DD-desktop-flow-inventory.md`: shipped (with test id) / missing / blocked-on-authority. Use `desktop/README.md:413-451` and `desktop/tests/` as the source.
- [ ] **Step 2:** For each **missing** item that needs no new daemon authority (e.g. pin, unarchive, export — README `:417` says they "remain unavailable"), one TDD cycle: failing bun test → implement → PASS → both gates → commit `"Add <flow> to the desktop"`. Items blocked on authority wait for their plan.

### Task 12: Performance and accessibility measurement (performance qualification, separate from correctness)

Bars from the active plan "Final qualification": cold start ≤5 s, warm interaction p95 ≤2 s, navigation feedback ≤100 ms, idle CPU ≤2 % of one core, 10,000-message history, keyboard/screen-reader, 200 % zoom, reduced motion, 24-hour soak.

- [ ] **Step 1:** Write a measurement script `desktop/scripts/measure_desktop_performance.py` with unit tests for its parsing (`test_p95_is_computed_over_recorded_samples`) — TDD as Task 2.
- [ ] **Step 2:** **Donald step:** run it on the named hardware for both editions; the 24-hour soak needs an unattended Mac. Evidence: `docs/verification/YYYY-MM-DD-desktop-performance.md` (hardware, dataset, sampling, results). A passing correctness gate is not this evidence.

### Task 13: **Donald steps** — notarised macOS release (OB-1…OB-4, OB-7)

Each step produces evidence in `docs/verification/YYYY-MM-DD-desktop-notarized-release.md`.
- [ ] **Step 1 (OB-1):** Donald installs the Developer ID Application certificate + key in the release keychain and sets `APPLE_SIGNING_IDENTITY` (`.github/workflows/desktop-release.yml:108`). Evidence: `codesign --verify --strict --verbose=2` on both editions.
- [ ] **Step 2 (OB-2):** Donald provides `APPLE_API_KEY_B64`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER` (`:109-111`). Evidence: `notarytool` submission id, stapling, `spctl -a -vv` acceptance for both editions.
- [ ] **Step 3 (OB-3):** Donald provides `ABBEY_UPDATER_SIGNING_KEY` (+ password) matching the pinned pubkey. Evidence: `cargo test --locked -p abbey-desktop --test updater_signature` against the released `.app.tar.gz`.
- [ ] **Step 4 (OB-7):** runner `gh` auth for the draft-release step. Evidence: draft release URL.
- [ ] **Step 5 (OB-4):** Donald publishes the draft release. Evidence: first-launch update check resolving `latest.json` / `latest-personal.json`.

### Task 14: `portable-package-services` — repo-side preparation

- [ ] **Step 1:** Inventory current packaging targets in `desktop/src-tauri/tauri.conf.json` / `tauri.personal.conf.json` and installers (`install.sh`, `install.ps1`, `tools/linux_user_service.py`). Record gaps per acceptance bullet in `docs/verification/YYYY-MM-DD-portable-packages-inventory.md`.
- [ ] **Step 2:** Failing test `desktop/scripts/tests/test_package_targets.py::test_each_edition_declares_distinct_identifiers_for_every_platform_bundle` → add missing Windows (MSI/NSIS) and Linux (deb/AppImage) bundle identifiers per edition → PASS → commit.
- [ ] **Step 3:** Failing Rust test for explicit X11/Wayland/portal capability detection (`linux_capture_is_unavailable_without_a_detected_portal`, `#[cfg(target_os = "linux")]`, compiled by the cross-compile guard only) → implement → commit. Linux runtime proof is OB-8.

### Task 15: **Donald steps** — Windows 11 ARM and Ubuntu ARM64 package journeys (OB-9, OB-8, OB-10)

- [ ] **Step 1 (OB-10):** Authenticode and Linux package signing keys available to the release job. Evidence: signed artifacts' verification output.
- [ ] **Step 2 (OB-9):** On Windows 11 ARM: install both editions with real PowerShell (`install.ps1`), coexist, open the WebView, chat/tasks/tools/automations journeys, service restart, uninstall with retained state. Evidence: `docs/verification/YYYY-MM-DD-windows-package-journey.md`.
- [ ] **Step 3 (OB-8):** On Ubuntu ARM64: same journeys with native packages and the user service. Evidence: `docs/verification/YYYY-MM-DD-ubuntu-package-journey.md`.
- [ ] **Step 4:** Ledger `portable-package-services` channels per evidence; `edition-windows-install-naming` (`src/claims/registry.rs:619`) Partial → Current only with Step 2's executed `install.ps1` for both editions (promotion rule).

### Task 16: Desktop claim status

- [ ] **Step 1:** After Tasks 6-8 and 11: registry `desktop-tauri-react` (`src/claims/registry.rs:645`) Proposed → Partial (governed tasks/automations closed; notarisation/Windows open) via promotion rule; sync; both gates; fresh-reviewer review; commit.
- [ ] **Step 2:** Current only after Task 13 (all five steps) and Task 15 Steps 2-3. `desktop-product` → `complete` only then.
