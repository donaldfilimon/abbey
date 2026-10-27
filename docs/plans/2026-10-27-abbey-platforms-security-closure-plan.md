# Abbey Platforms, Security and Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the repository side of the `platforms` milestone (`macos-ci`, `linux-ci` todos, `windows-foundation`, `linux-service-foundation`, `model-store-isolation`), the security work (`docs/plans/2026-10-25-compatible-security-pins.md` + `dependency-security-remediation`), `three-platform-artifact-ci`, and stage `final-closure` (Phase 11), with every runner, host, key and independent review written as a Donald step.

**Architecture:** Review-then-integrate the existing `abbey/windows-foundation` commit `5be3d25` through a task branch; spike `model-store-isolation` before choosing an Abbey-only or ABI fix; reconcile security packets against the lockfile instead of redoing them; harden the CI workflow guards so zero-job or skipped runs can never count; run the closure checklist last.

**Tech Stack:** Rust 1.99.1, GitHub Actions self-hosted runners (`.github/workflows/rust.yml`), `tools/ci/*`, `tools/security/*`, `cargo-audit`/`cargo-deny`, PowerShell (`install.ps1`, `tools/ci/qualify-windows.ps1`), systemd user services (`tools/linux_user_service.py`).

**Spec:** `tasks/completion.json` (`macos-ci`, `linux-ci`, `windows-foundation`, `linux-service-foundation`, `model-store-isolation`, `dependency-security-remediation`, `three-platform-artifact-ci`, `final-closure`); `docs/superpowers/plans/2026-10-22-abbey-completion.md` §2 "Three-platform foundation", "Final qualification"; `docs/completion-plan-2026-10-22.md` WS-13, WS-19, WS-20, OB-6…OB-10, OB-19; `docs/plans/2026-10-25-compatible-security-pins.md`.

## Global Constraints

Inherit `docs/plans/2026-10-27-abbey-completion-index.md` § Global Constraints. Plan-specific:
- "Runner registration and CI execution are different facts." "A zero-job `startup_failure` is infrastructure evidence, never source-test evidence" (`tools/ci/require_executed_run.py:3-16`). Cross-compilation is not runtime proof; `sh tools/ci/cross-compile-guard.sh` is a soft check.
- "Windows named pipes stay unsupported until implemented and proved." Every `unsafe` block needs an explicit `#[allow(unsafe_code)]` with a SAFETY comment (manifest lint).
- Security: "Preserve every existing source, vendor attribution, package identity, historical receipt and security exception. No advisory suppression or source-version relabeling." (`docs/plans/2026-10-25-compatible-security-pins.md:10`). The two scoped exceptions in `tools/security/run-dep-scan.sh:9-13` (RUSTSEC-2025-0141 bincode, RUSTSEC-2024-0436 paste) are not widened.
- Local green gates, hosted CI, runtime proof, signing, DAST, external review and multi-host are reported as separate facts — never as one "done".
- `.work/windows-foundation/abbey` is an existing manual worktree; integrate from it, never edit it in place without its owner's release.

Precondition: main-sync plan Task 6 done.

## Review Focus

- The Windows commit's `tasks/completion.json` and `.github/workflows/rust.yml` hunks were written against `d34e258`; replaying them over `8fdb4ed`+PR #108 can silently drop the local `local_gate_execution` evidence → Task 2 compares the requirement's evidence before/after.
- `windows-sys` adds packages to `Cargo.lock` → Task 2 re-runs the dependency scan on the new graph.
- A model-store override variable shared by both editions (`ABI_MODELS_DIR`) → Task 8 test sets it and asserts the personal edition refuses it.
- A workflow job that is skipped (guard variable off) being read as success → Task 11 tests.
- Ticking `compatible-security-pins` boxes from the lockfile alone while the plan's own steps (scans, focused tests) never ran on this revision → Task 9 runs them.

## File map

- Integrate: commit `5be3d25` (28 files incl. `src/daemon/windows.rs`, `src/daemon/server/{connection,windows}.rs`, `src/runtime/supervisor/windows.rs`, `Cargo.toml`, `Cargo.lock`, `install.ps1`, `.github/workflows/rust.yml`, `tools/ci/qualify-windows.ps1`, `docs/plans/2026-10-26-windows-foundation.md`, `docs/verification/2026-10-26-windows-foundation.md`)
- Modify: `src/edition.rs` (`SCOPED_ENV_SUFFIXES`), `src/daemon/runtime_v3/model_inventory.rs:30`, `tests/edition_isolation.rs` (model-store isolation, Branch A)
- Modify: `tools/tests/test_workflow_guards.py`, `tools/ci/require_executed_run.py`, `tools/tests/test_require_executed_run.py`, `.github/workflows/rust.yml`, `.github/workflows/desktop-release.yml`
- Modify: `docs/plans/2026-10-25-compatible-security-pins.md` (checkbox reconciliation, Donald-approved)
- Modify: `tasks/completion.json`, `tasks/todo.md`, `src/claims/registry.rs:619,762` (promotion tasks)
- Evidence: `docs/verification/YYYY-MM-DD-*.md`

---

### Task 1: Review the windows-foundation commit

REQUIRED SUB-SKILL for this task: superpowers:requesting-code-review (fresh reviewer, not the author).

- [ ] **Step 1:** `git show --stat 5be3d25`; confirm 1 unique commit vs `main` (`git rev-list --count main..abbey/windows-foundation` → 1) and its base `d34e258`.
- [ ] **Step 2:** Review checklist, recorded in `docs/verification/YYYY-MM-DD-windows-foundation-review.md`: (a) every `unsafe` has `#[allow(unsafe_code)]` + SAFETY comment; (b) all Windows code is `cfg(windows)` and the Unix server path is a pure extraction into `src/daemon/server/connection.rs` with unchanged framing/auth (diff `unix.rs` −343 vs `connection.rs` +350); (c) file sizes: `src/runtime/supervisor/windows.rs` (599), `src/daemon/windows.rs` (370) under 800; (d) `Cargo.lock` delta adds only `windows-sys` and its deps, no version churn; (e) installed Windows daemon requires `--read-only`, no fallback; (f) pipe DACL grants only the owner SID and rejects remote clients; (g) Job Object assigned before resume.
- [ ] **Step 3:** Each finding → fix commit on `abbey/windows-foundation` **only after its owner releases the worktree** (Donald step), or a follow-up task on the integration branch.

### Task 2: Integrate windows-foundation onto a task branch

- [ ] **Step 1:** **Donald step:** confirm the `abbey/windows-foundation` writer released commit `5be3d25` for integration. Evidence line in the review file.
- [ ] **Step 2:** `git worktree add .work/integrate-windows-foundation -b task/integrate-windows-foundation main` (manual task worktree per the readiness plan); `git cherry-pick 5be3d25`.
- [ ] **Step 3:** Expected conflicts: `tasks/completion.json` (both sides edit `windows-foundation`/`dependency-security-remediation` regions). Resolve by keeping **both** sides' evidence entries; then `python3 -I -c` dump of `windows-foundation` and `dependency-security-remediation` evidence must contain every key present on `main` plus the commit's additions. Never pick a side wholesale.
- [ ] **Step 4:** `./tools/cargo.sh test --locked -p abbey --lib daemon::` and `--lib runtime::supervisor` → PASS on macOS.
- [ ] **Step 5:** `sh tools/ci/cross-compile-guard.sh` — record exactly what was compiled or skipped (targets are not declared in `rust-toolchain.toml`; a skip is not a pass).
- [ ] **Step 6:** `sh tools/security/run-dep-scan.sh` on the new graph → record result.
- [ ] **Step 7:** `python3 -m unittest tools.tests.test_windows_ci_qualification tools.tests.test_workflow_guards` → OK; `./check.sh` then `./desktop/check.sh` → OK.
- [ ] **Step 8:** **Donald step:** approve merging `task/integrate-windows-foundation` into local `main` (readiness plan checklist items 6-8). Evidence: merge SHA in the review file. `windows-foundation` stays `in_progress`.

### Task 3: **Donald step** — native Windows 11 ARM execution (OB-9 + runner)

- [ ] **Step 1:** Provision an exclusive Windows 11 ARM64 runner with labels `self-hosted,Windows,ARM64,abbey` and enable `ABBEY_WINDOWS_ARM64_RUNNER` (and `ABBEY_WINDOWS_FOUNDATION_INSTALL=enabled` for the installer step) — `.github/workflows/rust.yml:286-298`.
- [ ] **Step 2:** Trigger the workflow on the exact head; then `python3 tools/ci/require_executed_run.py <run-id>` → `EVIDENCE: N job(s) executed and succeeded …` with the Windows job present.
- [ ] **Step 3:** Evidence `docs/verification/YYYY-MM-DD-windows-native-ci.md` (run id, head SHA, job log excerpts, installed safe/personal coexistence, wrong-bearer rejection).
- [ ] **Step 4:** Ledger: `windows-foundation.native_ci` → `passed` with refs. Remaining scope (private bearer files, private runtime stores, v2/v3/v4 authority on Windows, service registration) stays open; record it as acceptance notes. Registry `edition-windows-install-naming` (`src/claims/registry.rs:619`) Partial → Current only if this run executed `install.ps1` for both editions and confirmed coexistence (promotion rule).

### Task 4: **Donald step** — bring the macOS ARM64 runner back (OB-6) → `macos-ci`

The only registered runner `Abbey-Mac-arm64` has been offline since 2026-10-19 (`tasks/todo.md:199-210`).

- [ ] **Step 1:** Donald restarts/re-registers the runner (labels `self-hosted,macOS,ARM64,abbey`) and confirms `ABBEY_MACOS_ARM64_RUNNER=enabled`. Evidence: runner status `online` (GitHub API) with timestamp.
- [ ] **Step 2:** `workflow_dispatch` on exact head; `python3 tools/ci/require_executed_run.py <run-id>` → exit 0 with the macOS job.
- [ ] **Step 3:** Evidence `docs/verification/YYYY-MM-DD-macos-ci.md`; ledger `macos-ci.native_ci` passed; tick `tasks/todo.md:182` and `:199` items with the run id; status `complete`; `check_completion.py` → 0; commit.

### Task 5: **Donald step** — Linux ARM64 runner (OB-8) → `linux-ci` todos

- [ ] **Step 1:** Provision/register a Linux ARM64 runner `self-hosted,Linux,ARM64,abbey`; enable `ABBEY_LINUX_ARM64_RUNNER`.
- [ ] **Step 2:** Trigger on exact head; `require_executed_run.py <run-id>` → exit 0 with the Linux job.
- [ ] **Step 3:** Evidence `docs/verification/YYYY-MM-DD-linux-ci.md`; ledger `linux-ci.native_ci` passed; tick its two todos; status `complete`.

### Task 6: Promote `ci-self-hosted-linux-proof`

- [ ] **Step 1:** Registry `ci-self-hosted-linux-proof` (`src/claims/registry.rs:762`) requires "successful non-zero-job Linux ARM64 workflow run; successful Windows runtime job". After Tasks 3 and 5: Blocked → Current with both run ids (promotion rule); sync tool; `./check.sh`; fresh-reviewer review; commit.

### Task 7: `linux-service-foundation` — repo-side completion

Existing: `tools/linux_user_service.py` (`install/start/status/stop/restart/uninstall`, `:247-251`), `docs/linux-user-service.md`, `tools/tests/test_linux_user_service.py`.

- [ ] **Step 1:** Map each acceptance clause (edition-isolated service, desktop discovery/startup integration, owner-only credentials, exact Unix framing/correlation/cancellation parity) to an existing test; record gaps in `docs/verification/YYYY-MM-DD-linux-service-gaps.md`.
- [ ] **Step 2:** For each gap, TDD: failing test in `tools/tests/test_linux_user_service.py` (e.g. `test_personal_and_safe_units_never_share_a_name_or_state_dir`, `test_uninstall_retains_user_state`) or the desktop backend (`desktop_discovers_the_edition_matched_user_service`) → implement → PASS → commit per gap.
- [ ] **Step 3 (Donald, OB-8):** on Ubuntu ARM64 run `install/start/status/cancel/restart/uninstall` for both editions; evidence `docs/verification/YYYY-MM-DD-linux-service-native.md`; ledger `ubuntu_arm64` passed.

### Task 8: `model-store-isolation` — decision gate then fix

Today `src/daemon/runtime_v3/model_inventory.rs:30` calls `StorageRoot::resolve()`, which reads the shared `ABI_MODELS_DIR` then `$HOME/.abi/models` (`../abi/crates/abi-models/src/storage.rs:26-54`). `StorageRoot::at(path)` is public, and the lifecycle path already takes an absolute `storage_root` from its startup document (`src/daemon/runtime_v3/model_lifecycle.rs:95-110`).

- [ ] **Step 1 (spike):** List every `StorageRoot::resolve()` call site in Abbey (`grep -rn "StorageRoot::resolve" src`). If all are in Abbey code → **Branch A (Abbey-only)**. If any resolution happens inside ABI code Abbey cannot parameterise → **Branch S** (sibling request for an edition-qualified variable; stop after Step 2's test).
- [ ] **Step 2: Failing tests** (`tests/edition_isolation.rs`): `model_storage_root_is_edition_scoped_by_default` (safe → `<safe state>/models`, personal → `<personal state>/models`), `a_shared_abi_models_dir_override_is_refused_by_the_personal_edition`, `scoped_env_names_include_the_models_dir` (the exhaustive list at `src/edition.rs:213-222`).
- [ ] **Step 3 (Branch A):** add `"MODELS_DIR"` to `SCOPED_ENV_SUFFIXES`; resolve via `Edition::scoped_env("MODELS_DIR")` → default `<state root>/models`; replace `StorageRoot::resolve()` with `StorageRoot::at(..)`; when `ABI_MODELS_DIR` is set and differs from the edition root, refuse with a typed error (no silent adoption).
- [ ] **Step 4:** Tests PASS (default + personal cohorts); existing `model_inventory` tests (`:123-170`, which set `ABI_MODELS_DIR`) updated to the scoped variable.
- [ ] **Step 5:** `./check.sh` → OK. Commit `"Scope the model artifact store by edition"`. Ledger `model-store-isolation` `source`/`automated` passed; native host channels via the milestone.

### Task 9: Reconcile `2026-10-25-compatible-security-pins.md` (6 boxes)

Its spec: Tokio 1.54.0→1.54.1 (RUSTSEC-2026-0384) and ctutils 0.4.3→0.4.4 (RUSTSEC-2026-0385) in Abbey and three WDBX lockfiles; ZIP/GLib/proc-macro-error explicitly out of scope (`:5`). Abbey's `Cargo.lock` already has `tokio 1.54.1`, `ctutils 0.4.4`; WDBX `Cargo.lock` has `ctutils 0.4.4`.

- [ ] **Step 1:** Find the commit(s) that landed the pins: `git log -S'version = "1.54.1"' --oneline -- Cargo.lock`; for WDBX, `git -C ../wdbx log -S'0.4.4' --oneline -- Cargo.lock crates/abi-wdbx/fuzz/Cargo.lock specimen-studio/Cargo.lock` (read-only).
- [ ] **Step 2:** For each of the plan's six boxes, record evidence on the current revision: lock blocks; `sh tools/security/run-dep-scan.sh` output showing neither advisory; `./tools/cargo.sh metadata --locked --format-version 1 >/dev/null` exit 0; existing receipts in `docs/verification/2026-10-26-dependency-security-qualification.md` (`:44-50`). WDBX-side boxes cite WDBX receipts only (Abbey does not run WDBX gates).
- [ ] **Step 3:** **Donald step:** approve ticking the boxes that have evidence (that plan is under `docs/plans/`, not a ledger input). Then annotate each ticked box with its evidence path; commit `"Reconcile the compatible security pins packet with landed lockfiles"`.

### Task 10: `dependency-security-remediation`

Current evidence (`tasks/completion.json`): `dependency_audit` passed; `consumer_strict_dependency_audit` **failed** (exactly bincode RUSTSEC-2025-0141 and paste RUSTSEC-2024-0436 denied without ignores — `docs/verification/2026-10-26-dependency-security-qualification.md:72`); `local_gate_execution` passed; `license_review`, `secret_scan` and three native hosts required.

- [ ] **Step 1 (decision gate):** bincode arrives via `syntect` compiled syntax/theme data; paste via ABI's Candle/tokenizers graph (`tools/security/run-dep-scan.sh:9-10`). For each, record the options: (a) upstream replacement (syntect feature without bincode; ABI graph change = sibling request), (b) keep the scoped, reviewed exception and record the strict audit as "failed by design with reviewed exceptions". **Donald step:** choose per advisory. Evidence `docs/verification/YYYY-MM-DD-strict-audit-decision.md`.
- [ ] **Step 2 (if a replacement is chosen for bincode):** failing test `cargo tree -i bincode --locked` assertion in a new `tools/tests/test_dependency_graph.py::test_bincode_is_absent_from_the_root_graph` → change the syntect features → PASS → `./check.sh` + `./desktop/check.sh` + dep scan → commit.
- [ ] **Step 3 (GLib RUSTSEC-2024-0429):** keep the local two-line backport in `vendor/glib-0.18.5` verified by `python3 tools/security/check_gtk_vendor.py` (record exit); upstream path is a GTK4/Tauri graph change (sibling/upstream, not Abbey). Record as open with owner.
- [ ] **Step 4:** License inventory and secret scan on the current revision with the same tools used in `docs/verification/2026-10-22-completion-recovery/` (`security-candidate-license-inventory.json`, `frozen-2-secret-scan.json`); record; ledger channels `license_review`, `secret_scan`.
- [ ] **Step 5 (Donald, native hosts):** updater/install acceptance per edition on macOS ARM64, Ubuntu ARM64, Windows 11 ARM (desktop plan Task 13/15 evidence). Only then `complete`.

### Task 11: Workflow guards for `three-platform-artifact-ci`

**Files:** Modify `tools/tests/test_workflow_guards.py`, `tools/tests/test_require_executed_run.py`, `tools/ci/require_executed_run.py`, `.github/workflows/rust.yml`, `.github/workflows/desktop-release.yml`

- [ ] **Step 1: Failing tests:** `test_require_executed_run_rejects_a_run_whose_qualifying_job_was_skipped`, `test_require_executed_run_rejects_a_stale_head`, `test_require_executed_run_requires_all_three_platform_jobs_when_asked` (new flag `--require-platforms macos,linux,windows`), `test_release_jobs_verify_artifact_signature_and_provenance_per_edition`, `test_cancelled_jobs_never_count_as_success`.
- [ ] **Step 2:** `python3 -m unittest tools.tests.test_workflow_guards tools.tests.test_require_executed_run -v` → FAIL for the new ones.
- [ ] **Step 3:** Implement the flag and the workflow steps (artifact attestation/verification per host and edition; minimal permissions; exact immutable Abbey/ABI/WDBX inputs).
- [ ] **Step 4:** PASS; `./check.sh` → OK.
- [ ] **Step 5:** Commit `"Require executed, signed, provenance-checked artifacts on all three platforms"`.

### Task 12: **Donald step** — three-platform artifact run

- [ ] **Step 1:** With Tasks 3-5 runners online and OB-10 signing keys provisioned, trigger the release workflow on exact head.
- [ ] **Step 2:** `python3 tools/ci/require_executed_run.py <run-id> --require-platforms macos,linux,windows` → exit 0.
- [ ] **Step 3:** Evidence `docs/verification/YYYY-MM-DD-three-platform-artifact-ci.md`; ledger `three-platform-artifact-ci` channels.

### Task 13: Closure readiness inventory (Phase 11 dry run)

- [ ] **Step 1:** Run `python3 tools/check_completion.py`, `python3 tools/check_claims_sync.py`, `python3 tools/check_instructions.py`; produce a table of every requirement × required evidence channel × status into `docs/verification/YYYY-MM-DD-closure-readiness.md`.
- [ ] **Step 2:** For each non-`passed` channel, name the exact external prerequisite and the proof that closes it (Phase 11(d) "every remaining unchecked item names a real external prerequisite and the exact proof").
- [ ] **Step 3:** Commit. Repeat after each lane integrates.

### Task 14: Coordinated sibling gates on frozen inputs

- [ ] **Step 1:** Record a three-repo manifest (`tools/source_manifest.py` from the correctness plan) for the candidate.
- [ ] **Step 2:** **Donald step:** ABI and WDBX owners run their own gates on exactly those revisions (Abbey never runs them on their behalf). Evidence: their receipts referenced from `docs/verification/YYYY-MM-DD-sibling-gates.md`.
- [ ] **Step 3:** `tools/ci/verify-pinned-siblings.sh` against the same revisions → OK.

### Task 15: **Donald step** — independent final review (OB-19) and `final-closure`

- [ ] **Step 1:** Donald engages a reviewer with no write access to the candidate; reviewer receives the frozen manifest, closure-readiness table, all evidence, and the claims ledger. Evidence: reviewer report `docs/verification/YYYY-MM-DD-independent-final-review.md`.
- [ ] **Step 2:** Every reviewer finding is fixed (each with its own TDD task) or explicitly accepted by Donald; after the **last** repair the root and desktop gates rerun on the final bytes.
- [ ] **Step 3:** `python3 tools/check_claims_sync.py --write` regenerates every generated region from the registry (claims status reflects only independently reported evidence); check mode, `check_instructions.py`, `check_completion.py` → 0.
- [ ] **Step 4:** No secrets, weights, credentials or signing material in the repository (secret scan receipt).
- [ ] **Step 5:** Report local gates, hosted CI, runtime proof, signing, DAST, external review and multi-host as separate facts. `final-closure` → `complete` only if every included requirement is `complete`; canceled Zig and deliberate Out-of-scope rows stay excluded.
- [ ] **Step 6:** **Donald step:** push/release decision.
