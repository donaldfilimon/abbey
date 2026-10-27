# Abbey Correctness Milestone Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close or honestly bound the `correctness` milestone requirements (`identity-recovery-flake`, `portable-ollama-probe`, `corpus-patterns`, `corpus-command`, `model-request-validation`), the baseline requirement `retained-plan-reconciliation`, de-duplicate `tasks/todo.md`, and reconcile the ten `implemented` requirements that still carry open todos.

**Architecture:** Local, test-first changes in Abbey; sibling-dependent rows (`corpus-*`, the ABI elapsed-duration half of `model-request-validation`) stop at a written request and a consuming test that is red until the reviewed sibling revision is pinned. The flake work is a measurement harness plus a decision branch; it never attributes an unobserved cause.

**Tech Stack:** Rust 1.99.1, `./tools/cargo.sh`, Python 3 (`tools/*.py`, `unittest`), `tasks/completion.json`, `tasks/todo.md`.

**Spec:** `tasks/completion.json` (acceptance text per requirement), `tasks/todo.md:602-652` (flake history), `tasks/todo.md:1499-1680` (review-thread triage), `docs/superpowers/plans/2026-10-22-abbey-completion.md` §3 "Existing correctness and contract defects".

## Global Constraints

Inherit `docs/plans/2026-10-27-abbey-completion-index.md` § Global Constraints (Rust 1.99.1, `./tools/cargo.sh`, `--locked`, sibling path deps never edited from here, claims via registry + sync tool, append-only migrations, file-size ceilings, scratch edition-scoped state, commits only when authorized, correctness ≠ performance ≠ live ≠ production). Precondition: `docs/plans/2026-10-27-abbey-main-sync-plan.md` Task 6 is done.

## Review Focus

- A repeat harness that reports "0 failures" because the filter matched no test → `tools/flake_repeat.py` must fail when cargo reports `0 passed` (Task 2 test `test_a_filter_that_matches_nothing_is_an_error`).
- A portable probe that waits forever on a child that never exits → the deadline variant must stop it (Task 6 test).
- A probe timeout read as "tag not listed" → it must stay indeterminate (`None`), as on Unix (`src/agent/backend.rs:403-431`).
- Removing a duplicate todo whose first-line text is the only copy bound in `tasks/completion.json` → `check_completion.py` reports `stale todo`; Task 12 removes the binding in the same commit.
- Marking an `implemented` row `complete` whose acceptance no longer holds on the merged tree → Task 13 re-runs the cited tests on the current revision before any status change.

## File map

- Create: `tools/flake_repeat.py`, `tools/tests/test_flake_repeat.py` (repeat/load harness)
- Create: `tools/source_manifest.py`, `tools/tests/test_source_manifest.py` (byte hash + mode + command environment manifest)
- Modify: `src/agent/bounded_capture.rs` (deadline-bounded variant), `src/agent/backend.rs:395-432` (portable probe branch)
- Modify: `tests/abbey_contracts.rs` (anchoring regression), `src/abbey_contracts.rs:23-25`, `contracts/abbey/**` (re-vendor, only after sibling pin)
- Modify: `tasks/todo.md:1499-1561` (dedup), `tasks/todo.md:602-652` (flake disposition), `tasks/completion.json`
- Create: `docs/verification/YYYY-MM-DD-identity-recovery-flake/`, `docs/verification/YYYY-MM-DD-retained-plan-reconciliation.md`, `docs/verification/YYYY-MM-DD-implemented-todo-reconciliation.md`

---

### Task 1: Source manifest tool (feeds every gate receipt)

`retained-plan-reconciliation` acceptance: "Capture source file modes and exact command environment in addition to byte hashes". Existing receipts carry `mode`/`sha256` (e.g. `docs/verification/2026-10-22-completion-recovery/frozen-2-source-manifest.json`) but no repository tool produces them.

**Files:** Create `tools/source_manifest.py`, `tools/tests/test_source_manifest.py`

**Interfaces:** Produces `python3 tools/source_manifest.py --repo . --repo ../abi --repo ../wdbx --out <file.json>` and `--compare <a.json> <b.json>` (exit 1 and print differing paths when they differ). JSON shape: `{"<repo-name>": {"head": sha, "files": {"<path>": {"mode": int, "sha256": hex}}}, "environment": {"rustc": str, "cargo": str, "python": str, "env": {<allowlisted names>: value}}}`. Files = `git ls-files -co --exclude-standard`. Environment allowlist: `ABBEY_*` names that are not credentials (exclude any name containing `TOKEN`, `KEY`, `SECRET`, `BEARER`, `PASSWORD`), `CARGO_TARGET_DIR`, `RUSTUP_TOOLCHAIN`.

- [ ] **Step 1: Write failing tests** in `tools/tests/test_source_manifest.py`: `test_manifest_records_mode_and_hash_for_tracked_and_untracked_files` (temp git repo with an executable file → `mode` 0o755 recorded), `test_credential_like_environment_names_are_never_recorded` (set `ABBEY_DAEMON_BEARER_TOKEN=x` → absent), `test_compare_reports_a_mode_only_change`.
- [ ] **Step 2:** `python3 -m unittest tools.tests.test_source_manifest -v` → FAIL (module missing).
- [ ] **Step 3:** Implement with the standard library only (`subprocess`, `hashlib`, `os.stat`).
- [ ] **Step 4:** Rerun → PASS. Then `python3 -m unittest discover -s tools/tests -p 'test_*.py'` → OK.
- [ ] **Step 5:** `./check.sh` → `check.sh: OK` (the gate runs Python tool tests).
- [ ] **Step 6:** Commit: `git commit -m "Add a source manifest tool that records modes and command environment"`.

**Evidence:** `docs/verification/YYYY-MM-DD-retained-plan-reconciliation.md` § manifest tool.

### Task 2: Flake repeat/load harness

**Files:** Create `tools/flake_repeat.py`, `tools/tests/test_flake_repeat.py`

**Interfaces:** `python3 tools/flake_repeat.py --test conversation_identity_cutover --runs N --mode {serial,parallel,load} [--burners K] [--filter NAME] --out DIR`. Each run executes `./tools/cargo.sh test --locked --test <test> [<filter>] -- [--test-threads=1]` (serial adds `--test-threads=1`), writes `DIR/run-<i>.log`, and appends `{"run": i, "mode": m, "exit": code, "seconds": s, "passed": n, "failed": n}` to `DIR/runs.jsonl`. `load` starts K busy-loop processes (`multiprocessing`) for the duration of each run and always terminates them in `finally`. A run whose cargo summary shows `0 passed` is an error, not a pass. Exit status: 0 only if every run passed.

- [ ] **Step 1: Write failing tests** (command runner injected so tests never invoke cargo): `test_records_every_run_exit_code`, `test_a_filter_that_matches_nothing_is_an_error`, `test_load_burners_are_terminated_even_when_a_run_raises`, `test_serial_mode_passes_test_threads_one`.
- [ ] **Step 2:** `python3 -m unittest tools.tests.test_flake_repeat -v` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Rerun → PASS; full Python tool suite → OK.
- [ ] **Step 5:** Commit: `git commit -m "Add a repeat and load harness for intermittent test diagnosis"`.

### Task 3: Measure `identity-recovery-flake`

Background (`tasks/todo.md:602-652`): one 2026-08-09 failure of `tests/conversation_identity_cutover.rs::real_cli_recovers_committed_conversation_mirrors_after_failpoint` (`:172`), never reproduced; two observer defects fixed 2026-10-21 (`BARRIER_TIMEOUT` 15 s at `:25`; observer rule pinned by `the_pending_marker_observer_uses_the_recovery_paths_rule` at `:289`); the discriminating red control `shared_state_control_reproduces_the_missing_pending_marker_symptom` (`:263`) exists.

**Files:** Create `docs/verification/YYYY-MM-DD-identity-recovery-flake/` (`runs.jsonl`, logs, `manifest-before.json`, `manifest-after.json`, `README.md`)

- [ ] **Step 1:** `python3 tools/source_manifest.py --repo . --repo ../abi --repo ../wdbx --out <dir>/manifest-before.json`.
- [ ] **Step 2:** `python3 tools/flake_repeat.py --test conversation_identity_cutover --runs 50 --mode serial --out <dir>/serial`.
- [ ] **Step 3:** `... --runs 50 --mode parallel --out <dir>/parallel`.
- [ ] **Step 4:** `... --runs 50 --mode load --burners <logical CPUs> --out <dir>/load`.
- [ ] **Step 5:** Repeat Steps 2-4 for the `wdbx` and `personal-edition` cohorts by running the harness with `ABBEY_FLAKE_CARGO_ARGS="-p abbey --features wdbx"` / `"-p abbey --features personal-edition"` (add this pass-through to Task 2's tool if not present — test `test_extra_cargo_args_are_forwarded`, same TDD steps).
- [ ] **Step 6:** Manifest after; `python3 tools/source_manifest.py --compare` → identical, else the measurement is void.
- [ ] **Step 7:** Write `README.md`: counts per mode/cohort, machine, load level, and the sentence "this measures recurrence, not cause".

**Decision gate (from Step 2-5 results):**
- **Branch R — reproduced (any non-control failure):** REQUIRED SUB-SKILL: superpowers:systematic-debugging. Continue with Task 4R.
- **Branch N — not reproduced:** continue with Task 4N.

### Task 4R: Diagnose a reproduced failure (only in Branch R)

- [ ] **Step 1:** Preserve the failing run's log and scratch path (the fixture's `Scratch` drops its dir; rerun the failing seed with `ABBEY_KEEP_SCRATCH=1` only if the fixture supports it — if not, add that support first with test `scratch_is_retained_when_keep_is_requested`).
- [ ] **Step 2:** Follow systematic-debugging phases 1-4: reproduce at will, form one hypothesis, write the narrowest failing test that encodes it, fix the defect (production or fixture), rerun Task 3 Steps 2-4 at 50 runs each with zero failures.
- [ ] **Step 3:** Commit fix + test: `git commit -m "Fix <diagnosed cause> in conversation identity recovery"`.

### Task 4N: Retain the unattributed occurrence (only in Branch N)

**Files:** Modify `tasks/todo.md:602-652`, `tasks/completion.json` (`identity-recovery-flake`)

- [ ] **Step 1:** Append to the `tasks/todo.md:602` item a dated line: runs/modes/cohorts measured, zero recurrences, evidence path, "original cause unattributed; retained as evidence". Mark the checkbox `[x]` (the acceptance is "reproduce **or retain**").
- [ ] **Step 2:** Set `identity-recovery-flake` evidence `source` and `automated` to `passed` with refs to the evidence directory and the two existing tests (`:263`, `:289`); status `complete`.
- [ ] **Step 3:** `python3 tools/check_completion.py` → 0; `python3 tools/check_claims_sync.py --write` then check mode → 0 (todo counts change).
- [ ] **Step 4:** Commit: `git commit -m "Retain the unreproduced 2026-08-09 recovery failure with recurrence evidence"`.

### Task 5: Decision gate — portable probe path

The probe calls `run_with_checkpoint` (`src/agent/backend.rs:406-432`), which off Unix returns `SupervisorError::Unsupported` (`src/runtime/supervisor.rs:206-207,249-252`). The unreviewed commit `5be3d25` (`abbey/windows-foundation`) modifies `src/runtime/supervisor.rs` (+Job Object). Which lands first decides the design.

- [ ] **Step 1:** Check whether `5be3d25` has been integrated (platforms plan Task 1-3) and whether `run_with_checkpoint` then succeeds on `cfg(windows)`.
- [ ] **Step 2 — Branch W (supervisor supports Windows):** the probe needs no new path; skip Task 6 Steps 3-4, keep Task 6 test 2 (indeterminate on timeout) against the supervisor, and record the decision.
- [ ] **Step 2 — Branch P (supervisor still Unsupported off Unix):** implement Task 6 in full.

### Task 6: Deadline-bounded portable capture and probe (Branch P)

**Files:** Modify `src/agent/bounded_capture.rs`, `src/agent/backend.rs:395-432`

**Interfaces:**
```rust
// src/agent/bounded_capture.rs
pub(crate) struct CaptureLimits { pub deadline: Duration, pub stdout_bytes: usize, pub stderr_bytes: usize }
pub(crate) enum CaptureOutcome { Exited { status: ExitStatus, stdout: Vec<u8> }, TimedOut, StdoutLimit, StderrLimit }
pub(crate) fn bounded_capture_with_deadline(program: &Path, args: &[OsString], limits: CaptureLimits) -> std::io::Result<CaptureOutcome>;
// src/agent/backend.rs — compiled on every host so it is tested everywhere
fn ollama_lists_model_portable(path: &Path, model: &str) -> Option<bool>;
```
`ollama_lists_model_once` dispatches to `ollama_lists_model_portable` when `cfg(not(unix))`; the portable function maps outcomes exactly as the supervisor branch does (`Exited` → parse; limits → `Some(false)`; `TimedOut`/spawn error → `None`). Existing `bounded_capture_output` keeps its signature and behaviour.

- [ ] **Step 1: Write failing tests** in `src/agent/bounded_capture.rs` tests: `a_child_that_outlives_its_deadline_is_stopped_and_reported_timed_out` (`#[cfg(unix)]` `sleep 30` stub, 200 ms deadline, asserts `TimedOut` within 5 s). In `src/agent/backend.rs` tests: `portable_ollama_probe_checks_the_exact_requested_tag` (mirror of `ollama_probe_checks_the_exact_requested_tag` at `:624-670`, calling `ollama_lists_model_portable`) and `portable_ollama_probe_treats_a_timeout_as_indeterminate`.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --lib agent::` → FAIL (symbols missing).
- [ ] **Step 3:** Implement (reuse `read_limited`/overflow channel pattern from `bounded_capture.rs:30-140`; kill the child on deadline and reap it).
- [ ] **Step 4:** Rerun → PASS; `./tools/cargo.sh test --locked --lib agent::backend` → PASS.
- [ ] **Step 5:** `sh tools/ci/cross-compile-guard.sh` (soft; record skips) and `./check.sh` → OK. Confirm `src/agent/backend.rs` stays under 800 lines (`wc -l`); if not, move the probe into `src/agent/ollama_probe.rs` first.
- [ ] **Step 6:** Commit: `git commit -m "Probe Ollama readiness through bounded portable capture off Unix"`.

### Task 7: **Donald step** — native Windows acceptance for the probe (OB-9)

- [ ] **Step 1:** On a Windows 11 ARM host, run `./tools/cargo.sh test --locked --lib agent::backend::tests::portable_ollama_probe` with a `.cmd` stub listing/not listing the tag, plus a real `ollama list` if installed. Evidence: `docs/verification/YYYY-MM-DD-portable-ollama-probe-windows.md` (host, toolchain, output).
- [ ] **Step 2:** Only then: set `portable-ollama-probe` `automated` + `source` `passed`, tick `tasks/todo.md` item "The Ollama readiness probe is unavailable off Unix" (the copy kept after Task 12), status `complete`; `check_completion.py` → 0; commit.

### Task 8: `model-request-validation` — close the local half, gate the sibling half

All three Abbey fixes exist: frame-sized prompts and multiline whitespace (`src/app_core/v3_model_tests.rs:101-112`, validation at `src/app_core/v3_model.rs:71-79`), typed budget errors (`src/daemon/runtime_v3/model_lifecycle/model_inference.rs:132-135`, test `:217`). Missing: `automated` evidence and the ABI `RunContext::finish` elapsed-duration repair.

**Files:** Modify `tasks/completion.json`; Create `docs/verification/YYYY-MM-DD-model-request-validation.md`

- [ ] **Step 1:** `./tools/cargo.sh test --locked --lib v3_model` and `./tools/cargo.sh test --locked --lib model_inference` → PASS; record.
- [ ] **Step 2:** Reference the merged-tree root gate log from main-sync Task 6 as the `automated` receipt; set `automated: passed`.
- [ ] **Step 3:** Keep status `in_progress` with a note naming sibling request (c) from the index Task 2.
- [ ] **Step 4 (after the ABI owner lands and pins the fix):** write failing test `inference_refuses_a_run_that_exceeds_its_duration_ceiling` in `src/daemon/runtime_v3/model_lifecycle/model_inference.rs` tests using a provider that sleeps past a 50 ms ceiling; expect `budget_exceeded`. It must FAIL on the old pin and PASS on the new pin (`tools/ci/verify-pinned-siblings.sh`). Then status `complete`, tick `tasks/todo.md:1648` item, commit `"Refuse model runs that exceed their duration ceiling"`.

### Task 9: `corpus-command` — verify before repairing

`contracts/abbey/corpus/README.md:32-42` documents `python3 tools/abbey_contracts.py verify contracts/abbey`. That tool exists in the sibling (`../abi/tools/abbey_contracts.py`, argparse at `:739-750`) but not in Abbey.

- [ ] **Step 1:** From `../abi` (read-only), run `python3 -I tools/abbey_contracts.py verify contracts/abbey`; record the exit code and the ABI revision.
- [ ] **Step 2 — if it exits 0:** the command exists and works in its own repository; the defect is that the vendored README does not say the path is ABI-relative. Request (index Task 2 b): the upstream README states "run from the ABI repository root". Abbey change only after re-vendoring (Task 11).
- [ ] **Step 3 — if it fails:** record the failure in the sibling request; Abbey does not patch vendored bytes.
- [ ] **Step 4:** Record the result in `docs/verification/YYYY-MM-DD-corpus-command.md`; commit.

### Task 10: `corpus-patterns` — consuming regression (red until re-vendor)

**Files:** Modify `tests/abbey_contracts.rs`

- [ ] **Step 1: Write the test** `bundled_corpus_rejects_unanchored_digest_prefixes_and_suffixes`: for every `"pattern"` in `contracts/abbey/corpus/**/*.schema.json`, assert it starts with `^` and ends with `$`; and validate `junksha256:<64 hex>suffix` against `common/definitions.schema.json`'s digest definition (`contracts/abbey/corpus/v1/schemas/common/definitions.schema.json:15`) through `ContractCorpus` — expect rejection.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --test abbey_contracts bundled_corpus_rejects_unanchored` → FAIL (patterns unanchored today, e.g. `contracts/abbey/corpus/v1/schemas/cognition/request.schema.json:19`).
- [ ] **Step 3:** Mark the test `#[ignore = "corpus-patterns: awaits anchored upstream corpus (index Task 2 a)"]`; `./check.sh` → OK. Commit `"Add the anchored-pattern regression for the vendored corpus"`.

### Task 11: Re-vendor the qualified corpus (after the sibling pin)

**Files:** Modify `contracts/abbey/**`, `contracts/abbey/abbey-contracts.lock.json`, `src/abbey_contracts.rs:23-25`

- [ ] **Step 1:** Precondition: the ABI owner merged anchored patterns (+ README fix) and the CI sibling pin points at that revision.
- [ ] **Step 2:** Run the upstream vendoring tool from ABI (`../abi/tools/vendor_abbey_contracts.py`, per its own usage) into this checkout; never hand-edit corpus bytes.
- [ ] **Step 3:** Update `SOURCE_REVISION` and `AGGREGATE_DIGEST` (`src/abbey_contracts.rs:24-25`) to the lock file's values.
- [ ] **Step 4:** Remove the `#[ignore]` from Task 10's test; `./tools/cargo.sh test --locked --test abbey_contracts` → PASS (including `bundled_corpus_qualifies_exact_bytes_and_every_declared_fixture`).
- [ ] **Step 5:** `./check.sh` → OK; `tools/ci/verify-pinned-siblings.sh` → OK.
- [ ] **Step 6:** Ledger: `corpus-patterns` and `corpus-command` `source`/`automated` passed; tick `tasks/todo.md` items at `:1615` and `:1622`; status `complete`; `check_completion.py` → 0; `check_claims_sync.py --write`.
- [ ] **Step 7:** Commit: `git commit -m "Re-vendor the anchored contract corpus at <abi-rev>"`.

### Task 12: De-duplicate `tasks/todo.md`

`tasks/todo.md:1499-1561` ("2026-10-22 — review-thread triage") and `:1563-1613` ("the rest of the review-thread triage") carry seven checkboxes with the same first lines (`:1508/1571`, `:1519/1581`, `:1527/1587`, `:1532/1591`, `:1541/1597`, `:1549/1604`, `:1555/1610`). Keep the later section (it is the superset: it adds #80 to the wire-schema item and continues into the corpus/guild/accel/model items). `check_completion.py` binds todos by first-line text (`tools/check_completion.py:183-207`).

**Files:** Modify `tasks/todo.md:1499-1561`, `tasks/completion.json` (`wire-vocabularies.todos`)

- [ ] **Step 1:** In the earlier section, replace each of the seven `- [ ] **…**` blocks with one non-checkbox line: `- Tracked once below under "the rest of the review-thread triage": <first-line title>.` Keep the section heading and its introductory paragraph (history).
- [ ] **Step 2:** The earlier first item's title ("The protocol v1 and v3 schemas do not cover the claim lifecycle") exists nowhere else; remove that exact string from `wire-vocabularies.todos` in `tasks/completion.json` (its substance — #94/#98 — is in the kept "The wire schemas do not cover every vocabulary they now carry" item).
- [ ] **Step 3:** `python3 tools/check_completion.py` → 0 (no `stale todo`, no `unmapped todo`).
- [ ] **Step 4:** `python3 -m unittest tools.tests.test_completion_matrix` → OK; `python3 tools/check_claims_sync.py --write`, then check mode → 0 (open-todo count drops by 7).
- [ ] **Step 5:** Commit: `git commit -m "Track each 2026-10-22 review finding once in the todo ledger"`.

### Task 13: Re-verify the nine `implemented` rows with satisfied channels

Rows: `wire-vocabularies`, `evidence-channels`, `provider-model-switch`, `prompt-budget`, `deferred-readiness`, `voice-claim-boundary`, `corpus-golden-vectors`, `synthetic-guild-correctness`, `accelerator-install-isolation`. Each has `source` and `automated` `passed` today.

**Files:** Create `docs/verification/YYYY-MM-DD-implemented-todo-reconciliation.md`

- [ ] **Step 1:** For each row, list the test identifiers named in its `evidence` refs (e.g. `src/daemon/client/tests.rs::completion_legacy_v1_claim_schema_is_rejected`, `src/claims/tests.rs::the_portable_host_claim_states_the_voice_dispatch_boundary`, `src/abbey_contracts/golden_tests.rs`, `src/claims/tests/evidence_channels.rs`, `tools/tests/test_install_accel_editions.py`) and, for log-only refs, the test names inside the focused logs (`docs/verification/2026-10-22-completion-recovery/{ollama,guild}-green.log`).
- [ ] **Step 2:** Run each on the current merged revision (`./tools/cargo.sh test --locked <name>` / `python3 -m unittest …`); record pass/fail per row.
- [ ] **Step 3:** For each row, compare its `acceptance` text with what the tests assert; record "met" or the gap.
- [ ] **Step 4:** Commit the evidence file.

### Task 14: Close the reconciled rows

**Files:** Modify `tasks/todo.md` (the kept copies), `tasks/completion.json`

- [ ] **Step 1:** For each row recorded "met" with all tests passing: tick its todo(s) `[x]` and append `(closed YYYY-MM-DD; evidence: docs/verification/YYYY-MM-DD-implemented-todo-reconciliation.md)` on a continuation line; set status `complete`.
- [ ] **Step 2:** For any row with a gap: leave status `implemented`, todo open, and record the gap as a new acceptance bullet only after Donald confirms (it changes an approved requirement).
- [ ] **Step 3:** `linux-ci`: no change — its `native_ci` channel is `blocked` (OB-8); its todos stay open (platforms plan Task 5-6).
- [ ] **Step 4:** `python3 tools/check_completion.py` → 0; `python3 tools/check_claims_sync.py --write` + check → 0; `./check.sh` → OK.
- [ ] **Step 5:** Fresh-reviewer review of the ledger diff (promotion rule 6). Commit: `git commit -m "Close implemented correctness findings whose acceptance is re-verified"`.

### Task 15: `retained-plan-reconciliation` — audit the four acceptance items

**Files:** Create `docs/verification/YYYY-MM-DD-retained-plan-reconciliation.md`; Modify `tasks/completion.json`

- [ ] **Step 1 (item 1):** `python3 tools/check_completion.py` → 0 proves all 346 retained open occurrences (`tasks/completion-historical.json`, `open_occurrences`) are mapped. Additionally list every `docs/superpowers/plans/*.md` with open boxes and confirm each source is in `sources`; record any plan added since 2026-10-22.
- [ ] **Step 2 (item 2):** Produce a manifest with Task 1's tool for Abbey/ABI/WDBX; for prior "consolidation qualified" claims, run `--compare` against an isolated sibling-directory candidate (`tools/ci/verify-pinned-siblings.sh` layout). Record equal/different.
- [ ] **Step 3 (item 3):** Locate the stale-but-usable negative controls for feature-matched gate artifact selection (`check.sh:39-43` builds `--bins` per cohort). If no test pins "a prior cohort's binary is never exercised", write `tools/tests/test_gate_scope.py::test_every_cohort_builds_its_own_bins_before_testing` asserting each `cargo test` line in `check.sh` is preceded by a matching `cargo build … --bins` with the same `-p`/`--features`; TDD as in Task 1.
- [ ] **Step 4 (item 4):** For each bound — fixed-snapshot title search, cross-run cursor refusal, serialized response ceilings, 4 MiB / 65,536-event (`src/stream/tap.rs:9`) / 16 KiB preview bounds, replies completed between polls — grep the test that pins it and record `file::test`. Any bound without a test gets a test in the owning module (TDD), one commit each.
- [ ] **Step 5:** When all four are recorded, set `source`/`automated` `passed`, status `complete`; `check_completion.py` → 0; commit `"Reconcile retained plans against the completion matrix"`.
