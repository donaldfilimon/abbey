# Abbey Models, Media and Federation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Carry `production-model`, `accelerator-runtime`, `production-adapters`, `neural-media`, `model-media-roadmap`, the two training claims, `compute-editions`, `consented-capture`, `live-guild-validation`, `guild-effects`, `worker-runtime`, `three-vm-proof` and `physical-federation-proof` as far as the repository can, and stop each one at its exact ABI/WDBX or owner gate with the evidence file that gate must produce.

**Architecture:** Abbey owns contracts, authority, governance and evidence; ABI owns model architectures, trainers and workers (`Cargo.toml:75-98` path deps). Every model-quality, licence, hardware, consent or VM fact is a Donald step. Repo-side work is: training/worker authority over durable ledgers, fail-closed refusal until a qualified artefact exists, and acceptance harnesses that are inert (ignored, env-gated) until the owner supplies the artefact.

**Tech Stack:** Rust 1.99.1, protocol v3 (`src/app_core/v3.rs:340-364`), `crates/abbey-learning` (native Rust LoRA pilot), `src/train/` (legacy consent-gated orchestration), `abi-models`/`abi-model-runtime`/`abi-worker`, `src/app_core/guild_intelligence/`.

**Spec:** `tasks/completion.json` (the thirteen requirements above); `docs/superpowers/plans/2026-10-22-abbey-completion.md` §7 "Production models, accelerators, adapters, and neural media" and §8 "Federation and shared compute"; `docs/completion-plan-2026-10-22.md` WS-7, WS-10, WS-12, WS-14, WS-15, WS-16, WS-18; `docs/model-family.md`; `docs/plans/2026-10-25-abbey-live-models.md` (12 open boxes, folded in by reference, Task 1-3).

## Global Constraints

Inherit `docs/plans/2026-10-27-abbey-completion-index.md` § Global Constraints. Plan-specific, verbatim or near-verbatim from the spec:
- **Synthetic training never qualifies a model, and this repository never trains a model as a side effect** of a gate, a test, or a plan step. Every real training run is a Donald step with a consent file (`abbey-train-consent: v1`, `src/train/consent.rs`) and, for private examples, the separate private-training line.
- "Validate roadmap Gemma 12B architecture/tokenizer/license and immutable artifacts before commitment, without substituting a different model." Gemma E4B adapters are not transferable to Gemma 12B; the training target (E4B) and inference target (12B) stay separate contracts (`docs/model-family.md:56-68`).
- Bars (active plan §7): 4K context + 64-token smoke; peak RSS ≤ 20 GB on the named 24 GB Mac; no sustained swap thrash; correct tokenizer fixtures; structured-call quality; documented CPU/Metal oracle tolerances; adapters ≥95 % valid held-out tool-call JSON, ≤2 % NLL regression, 100 % public-edition refusal success. "If the bar fails, keep the capability OFF with no silent substitution."
- Unavailable devices stay explicitly unavailable; kernel parity is not production model evidence; CoreML/Metal output-oracle success is not placement or residency evidence.
- "A three-VM demo on one Mac is multi-VM evidence, not physical multi-host evidence." Live Discord capture/effects stay separately consent-gated; prepare contracts and fixtures without live access; never reuse the synthetic fixture authority as live authorisation.
- `docs/plans/2026-10-25-abbey-live-models.md` has an **active writer** (worktree `abbey-live-models`, 52 dirty files on 2026-10-26). Do not edit that plan, its worktree, or `crates/abbey-learning` until the owner releases it (Task 1).

Precondition: main-sync plan Task 6 done; authority-tools plan Tasks 3-9 for anything that adds daemon authority.

## Review Focus

- A model-lifecycle acceptance test that silently passes when the licensed artefact is absent → must be `#[ignore]` with a reason, and the harness must fail (not skip) when the env var is set but the artefact is wrong.
- A `ManageTraining` grant that cannot mutate weights presented as training → the contract states it, and the desktop Training view stays unavailable until weight-changing runs exist.
- A worker inventory that advertises workers it cannot reach → `ReadWorkers` stays unnegotiated until a real startup adapter exists (`src/daemon/runtime_v3/tests.rs:365` `worker_authority_is_denied_until_a_real_startup_adapter_exists` must keep passing until Task 13).
- Media generation that reaches the network → offline proof runs with networking disabled.
- A guild adapter that enumerates members or reads message content → the read-only boundary checker `tools/check_p3_readonly.py` keeps passing.

## File map

- Create: `src/runtime/store/training_ops.rs` (+ tests), training migration (number = next after authority-tools Task 2), `tests/daemon_training.rs`
- Modify: `src/daemon/runtime_v3.rs`, `src/daemon/client/v3.rs`, `src/app_core/v3.rs` (contract notes only)
- Create: `tests/production_model_acceptance.rs` (ignored, env-gated), `tests/adapter_acceptance.rs` (ignored, env-gated), `tests/media_offline_acceptance.rs` (ignored, env-gated)
- Create: `src/media_local/{mod,provider}.rs` (provider interface, no weights)
- Create: `src/daemon/runtime_v3/workers.rs` (+ tests)
- Create: `tools/three_vm/` scenario harness + `tools/tests/test_three_vm_scenarios.py`
- Create: `docs/program-3-live-capture-contract.md` (draft for review)
- Evidence: `docs/verification/YYYY-MM-DD-*.md`

---

### Task 1: Fold in `2026-10-25-abbey-live-models.md` (12 boxes) after owner release

- [ ] **Step 1:** **Donald step:** confirm the `abbey-live-models` worktree writer has released (or is still active). Evidence line in `docs/verification/YYYY-MM-DD-live-models-reconciliation.md`. If still active, stop this task.
- [ ] **Step 2:** Map each open box to current evidence and record one of *met* (with test id), *superseded* (with the amendment that supersedes it), *open*:
  - Plan Task 1 boxes 1-4 (approved immediate memory) → candidates `tests/live_memory.rs` (`restart_correct_forget_and_project_scope` :41, `remote_ask_refused_before_any_store` :71, `foreign_project_and_unapproved_data_are_excluded` :83, `exact_wdbx_restart_and_deletion` :130, `unavailable_backend_never_falls_back` :159) and `src/knowledge/live.rs`.
  - Plan Task 2 boxes 1-5 (Python MLX tooling) → *superseded* by that plan's own "Approved scope amendment: entire runtime in Rust" (`docs/plans/2026-10-25-abbey-live-models.md:53-55`); the Rust equivalents are `crates/abbey-learning/tests/native_pipeline.rs` (`native_private_pipeline_keeps_separate_consent_and_refuses_bad_promotion` :291, `native_public_pipeline_needs_only_public_consent` :295). `tools/abbey_model_family.py` does not exist.
  - Plan Task 3 boxes 1-3 (review, gates, hardware baseline) → Tasks 3, 6 and 7 below.
- [ ] **Step 3:** Run every cited test on the merged revision (`./tools/cargo.sh test --locked --test live_memory`, `./tools/cargo.sh test --locked -p abbey-learning --test native_pipeline`, and the `--features wdbx` cohort for `exact_wdbx_restart_and_deletion`); record results.
- [ ] **Step 4:** **Donald step:** approve ticking *met* boxes and annotating *superseded* ones in the 2026-10-25 plan (an approved document). Then make that edit (no box deleted), commit `"Reconcile the live-models plan against landed memory and native learning work"`.

### Task 2: Training-operation ledger (store)

WS-7: `ManageTraining` operates over a durable ledger, not over weights.

**Files:** Create `src/runtime/store/training_ops.rs`, `src/runtime/store/training_ops_tests.rs`, migration `src/runtime/migration_<N>_training_ops.sql`

**Interfaces:** `create_training_operation(&self, start: &V3TrainingStart, now_ms) -> Result<TrainingOperation>`; `training_operation(&self, id) -> Result<TrainingOperation>`; `transition_training_operation(&self, id, to: TrainingState, now_ms) -> Result<TrainingOperation>` (states: `prepared → running → evaluated → promoted | refused | cancelled | interrupted`; monotonic; single-use operation ids); `recover_training_operations(&self, now_ms) -> Result<u32>` (running → interrupted, never retried). Record ids are opaque, domain-separated — never raw memory record ids.

- [ ] **Step 1: Failing tests:** `training_operations_are_single_use_and_monotonic`, `restart_marks_running_training_interrupted_not_retried`, `training_records_never_expose_raw_memory_ids`, `migration_adds_training_ops_without_touching_applied_rows`.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --lib runtime::store::training_ops` → FAIL.
- [ ] **Step 3:** Implement (additive migration; bump `CURRENT_SCHEMA_VERSION`).
- [ ] **Step 4:** PASS (serial and parallel).
- [ ] **Step 5:** Commit `"Add a durable training-operation ledger"`.

### Task 3: Serve `ReadTraining` / `ManageTraining` over the ledger

**Files:** Modify `src/daemon/runtime_v3.rs`, `src/daemon/client/v3.rs`; Create `tests/daemon_training.rs`

- [ ] **Step 1: Failing tests:** `tests/daemon_training.rs::real_daemon_reports_training_dataset_status_from_train_candidates`, `::start_training_without_a_consent_digest_is_refused`, `::manage_training_contract_states_it_does_not_mutate_weights_without_a_trainer`, `::cancel_training_signals_the_running_operation`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Dispatch `TrainingDatasetStatus`, `StartTraining`, `TrainingStatus`, `CancelTraining`, `TrainingMetrics`, `PromoteAdapter`, `RollbackAdapter` (`src/app_core/v3.rs:351-357`). `StartTraining` calls the native pipeline only through the existing consent contract; with no configured trainer it records `refused` with a typed reason. Grant from startup-owned authority only.
- [ ] **Step 4:** PASS; `./check.sh` → OK.
- [ ] **Step 5:** New registry row `daemon-protocol-v3-training-authority` Current (scope: ledger + consent-gated dispatch; explicitly not model quality) via the promotion rule; commit.

### Task 4: Decision gate — which trainer produces E4B adapters

`production-adapters` needs "Actual E4B adapter weight update". Two candidates: Abbey's native `crates/abbey-learning` LoRA (pilot; `training-lora-pipeline` Partial at `src/claims/registry.rs:684`) or an ABI trainer contract (OB-15).

- [ ] **Step 1:** Spike (read-only + `--help`): `./tools/cargo.sh run --locked --bin abbey -- train family --help` and `docs/model-family.md` "supported architectures"; record whether Gemma 4 E4B (architecture + tokenizer) is supported natively.
- [ ] **Step 2 — Branch Native:** Tasks 5-7 use `abbey train family`. **Branch ABI:** file/confirm index sibling request (e) and stop Tasks 5-7 at the pinned trainer revision.
- [ ] **Step 3:** Record the decision in `docs/verification/YYYY-MM-DD-adapter-trainer-decision.md`; **Donald step:** approve it.

### Task 5: Adapter acceptance harness (inert until artefacts exist)

**Files:** Create `tests/adapter_acceptance.rs`

**Interfaces:** env `ABBEY_ADAPTER_ACCEPTANCE_DIR` → directory with `base.manifest.json` (hashes), `adapter/`, `heldout/` fixtures, `consent.txt`. Absent → test is `#[ignore = "needs licensed E4B base + consented corpus (OB-13, Donald)"]`.

- [ ] **Step 1: Write tests:** `adapter_produces_at_least_95_percent_valid_tool_call_json_on_held_out_fixtures`, `adapter_nll_regression_is_at_most_two_percent_versus_base`, `adapter_passes_every_public_edition_refusal_fixture`, `adapter_rollback_restores_the_previous_artifact_bytes`, `a_tampered_base_hash_refuses_promotion`.
- [ ] **Step 2:** With a deliberately wrong fixture dir (synthetic tiny model), run `ABBEY_ADAPTER_ACCEPTANCE_DIR=<tmp> ./tools/cargo.sh test --locked --test adapter_acceptance -- --ignored` → FAIL on the bars (proves the bars bite). Record that a synthetic run failing the bar is expected and is not qualification.
- [ ] **Step 3:** Default `./check.sh` → OK (tests ignored).
- [ ] **Step 4:** Commit `"Add the held-out adapter acceptance harness"`.

### Task 6: **Donald steps** — licensed base, consented corpus, real training run

- [ ] **Step 1 (OB-13):** Donald accepts the Gemma 4 E4B licence and records it in the edition-scoped acceptance ledger; supplies the artefact hashes. Evidence: `docs/verification/YYYY-MM-DD-e4b-licence.md` (no weights committed).
- [ ] **Step 2 (consent):** Donald writes the consent file(s) for the approved corpus (and the private line if private examples are included). Evidence: consent digest only.
- [ ] **Step 3:** Donald (or a session he explicitly authorises for this one run) executes the training run with the chosen trainer and runs Task 5 with `--ignored`. Evidence: `docs/verification/YYYY-MM-DD-adapter-qualification.md` with corpus digest, consent digest, base/adapter hashes, seed, split, metrics, rollback proof, hardware.
- [ ] **Step 4:** Only if every bar passes: `production-adapters` channels `consent`, `license`, `held_out_quality`, `automated`, `source` passed; registry `training-lora-pipeline` Partial → Current for the qualified E4B reference only (promotion rule); commit. If any bar fails: record and keep Partial.

### Task 7: `training-local-consent-pipeline` live cycle (Donald step)

Registry `:697` requires "a live promotion-and-rollback cycle on a corpus of realistic prose text" with a delegated trainer whose context window can continue prose.

- [ ] **Step 1:** Confirm a delegated trainer with a sufficient context window exists at the pinned ABI revision (sibling request (e)); if not, stop.
- [ ] **Step 2:** **Donald step:** run `abbey train prepare/run/evaluate/rollback --consent <p>` on an approved realistic-prose corpus; evidence `docs/verification/YYYY-MM-DD-train-live-cycle.md` with sealed corpus digest, consent digest, artifact digest, score, promotion, read-only re-score, rollback, ledger rows.
- [ ] **Step 3:** Registry `training-local-consent-pipeline` Partial → Current only with that evidence; commit.

### Task 8: Production model acceptance harness (inert until OB-12/13/14)

**Files:** Create `tests/production_model_acceptance.rs`

**Interfaces:** env `ABBEY_PRODUCTION_MODEL_DIR` (signed registry + artefacts for `google/gemma-4-12B-it-qat-w4a16-ct`), `ABBEY_PRODUCTION_MODEL_HOST=<named host>`. Absent → ignored with reason.

- [ ] **Step 1: Write tests** (drive a real scratch `abbeyd` like `tests/daemon_model_lifecycle.rs`): `production_model_loads_and_answers_a_4k_context_64_token_smoke`, `production_model_peak_rss_stays_within_20_gb` (sample RSS of the daemon process during the smoke), `production_model_tokenizer_matches_its_fixtures`, `production_model_emits_valid_structured_tool_calls`, `cpu_and_metal_outputs_agree_within_the_documented_tolerance`, `a_wrong_revision_is_refused_without_substitution`.
- [ ] **Step 2:** With the fixture `abi-bigram-v1` artefacts pointed at by the env var → FAIL (architecture mismatch) — proves the harness refuses substitution.
- [ ] **Step 3:** Default gate OK (ignored). Commit `"Add the production model acceptance harness"`.

### Task 9: **Donald/sibling steps** — production model (OB-12, OB-13, OB-14)

- [ ] **Step 1 (OB-12, sibling):** ABI owner lands the Gemma 4 architecture + compressed-tensor loader (or a pinned reviewed Candle fork); Abbey pins it via `tools/ci/verify-pinned-siblings.sh`. Evidence: pinned revision + ABI receipt.
- [ ] **Step 2 (OB-13):** Donald accepts the 12B licence; artefact hashes recorded.
- [ ] **Step 3 (OB-14):** On the named 24 GB Apple Silicon host, run Task 8 with `--ignored`. Evidence: `docs/verification/YYYY-MM-DD-production-model.md` (host, RSS trace, swap stats, outputs, tolerances).
- [ ] **Step 4:** All pass → `production-model` channels; registry `models-production-local-weights` (`:671`) Proposed → Current (scope: that model on that host); else keep OFF and record.

### Task 10: Accelerator runtime — Apple whole-model path

**Files:** Modify `src/daemon/runtime_v3/model_lifecycle/model_inference.rs`; tests in `tests/daemon_model_lifecycle.rs`

- [ ] **Step 1: Failing tests** (accel cohort): `metal_inference_reports_requested_and_executed_device_for_every_layer_run`, `metal_allocation_failure_is_refused_not_downgraded_to_cpu`, `metal_inference_cancellation_stops_within_the_client_deadline`, `an_unsupported_kernel_is_refused_explicitly`. Existing `real_daemon_runs_the_exact_fixture_on_metal_without_falling_back` stays green.
- [ ] **Step 2:** `./tools/cargo.sh test --locked -p abbey --features accel --test daemon_model_lifecycle` → FAIL.
- [ ] **Step 3-4:** implement against the ABI Metal adapter (requires the pinned ABI APIs; if missing → sibling request) → PASS.
- [ ] **Step 5:** `./check.sh` (accel cohort) → OK; commit.
- [ ] **Step 6 (Donald, OB-17):** non-Apple (CUDA/Vulkan/TPU) needs hardware; record "unavailable" explicitly. Registry `accelerator-execution-runtime` (`:710`) Proposed → Partial (Apple) only after Task 9 provides a real whole model on Metal; no performance figure without separate timing evidence.

### Task 11: Neural media — provider interface and offline proof harness

**Files:** Create `src/media_local/mod.rs`, `src/media_local/provider.rs`, `tests/media_offline_acceptance.rs`

**Interfaces:** `pub(crate) trait LocalMediaProvider { fn kind(&self) -> MediaKind; fn generate(&self, req: &MediaRequest, cancel: &CancellationToken) -> Result<MediaArtifact, MediaError>; }`; `MediaArtifact` records device, runtime, revision, duration, output digest, seed. macOS system speech stays a separate provider; cloud speech stays out of scope.

- [ ] **Step 1: Failing tests:** unit `a_provider_without_a_licensed_artifact_is_unavailable`, `generation_records_device_revision_seed_and_output_digest`, `cancellation_leaves_no_partial_artifact`; env-gated ignored `whisper_large_v3_turbo_transcribes_fixtures_offline`, `parler_tts_mini_synthesises_fixtures_offline` (assert network disabled via a deny-all proxy env and no socket connects).
- [ ] **Step 2-4:** FAIL → implement interface + registry/artefact reuse from the model lifecycle → unit PASS; ignored by default.
- [ ] **Step 5:** Commit `"Add the local media provider interface and offline acceptance harness"`.
- [ ] **Step 6 (Donald, OB-16):** licences + artefacts for Whisper large-v3-turbo and Parler-TTS Mini v1.1 (speech first; SDXL, LTX-Video later); run ignored tests offline; evidence `docs/verification/YYYY-MM-DD-neural-media-speech.md`; registry `media-local-neural-models` (`:723`) Proposed → Partial (speech).

### Task 12: `model-media-roadmap` aggregate

- [ ] **Step 1:** When `production-model`, `accelerator-runtime`, `production-adapters`, `neural-media` are each `complete`, set `model-media-roadmap` evidence to their refs and status `complete`; `check_completion.py` → 0; commit. Until then, no change.

### Task 13: Worker inventory and jobs (`worker-runtime`, repo side)

**Files:** Create `src/daemon/runtime_v3/workers.rs` (+ tests); Modify `src/daemon/runtime_v3.rs`, `src/daemon/client/v3.rs:368,442`

- [ ] **Step 1:** Decision gate: read `../abi/crates/abi-worker` public API at the pinned revision; record whether it provides signed job envelopes, leases/deadlines, cancellation, result hashes and an mTLS transport. Missing pieces → sibling request (f); proceed only with local-IPC workers.
- [ ] **Step 2: Failing tests:** `local_ipc_worker_inventory_is_truthful_and_negotiated`, `a_job_lease_expires_and_is_reported_not_retried_silently`, `cancel_job_reaches_the_worker_and_is_acknowledged`, `quota_exhaustion_refuses_new_jobs`. Replace `worker_authority_is_denied_until_a_real_startup_adapter_exists` (`src/daemon/runtime_v3/tests.rs:365`) only in the same commit that adds the real startup adapter, keeping a renamed test for the no-adapter case (`worker_authority_is_denied_without_a_startup_adapter`).
- [ ] **Step 3-4:** FAIL → implement → PASS; `./check.sh` → OK.
- [ ] **Step 5:** Commit `"Serve local IPC worker inventory and jobs"`.

### Task 14: Three-VM scenario harness (inert until OB-11)

**Files:** Create `tools/three_vm/run_scenarios.py`, `tools/tests/test_three_vm_scenarios.py`

**Interfaces:** `python3 tools/three_vm/run_scenarios.py --inventory <vms.json> --out <dir>`; scenarios: worker/VM termination, leader failure, network partition, stale leader, duplicate delivery, conflicting WDBX values, read repair, reconnect, rolling restart, certificate rotation, clean teardown. Inventory requires three distinct IPs, identities and state dirs; refuses if any two are equal.

- [ ] **Step 1: Failing tests:** `test_inventory_with_duplicate_ips_is_refused`, `test_every_scenario_records_pass_fail_and_evidence_path`, `test_a_same_host_inventory_is_labelled_not_multi_vm`.
- [ ] **Step 2-4:** FAIL → implement (SSH command runner injected; tests never touch a network) → PASS.
- [ ] **Step 5:** Commit `"Add the three-VM shared-compute scenario harness"`.
- [ ] **Step 6 (Donald, OB-11):** provision three Ubuntu ARM64 VMs with distinct IPs, independent state, private CA + worker certs, and an independent membership/control store; run the harness. Evidence `docs/verification/YYYY-MM-DD-three-vm-proof.md`. Registry `mesh-three-vm-and-production` (`:749`) Proposed → Partial (three-VM on one Mac only).

### Task 15: `compute-editions` aggregate

- [ ] **Step 1:** Complete when `worker-runtime` + `three-vm-proof` (shared compute) and `personal-edition` (authority-tools plan Tasks 19-22) are each `complete`; record refs; commit.

### Task 16: `physical-federation-proof` (owner-blocked, no OB row)

- [ ] **Step 1:** Blocked on index Task 3 (Donald decides OB row vs deferral). No repo work beyond reusing Task 14's harness with a physical-host inventory.
- [ ] **Step 2 (Donald):** physical distinct hosts, geographic deployment, hardware; evidence `docs/verification/YYYY-MM-DD-physical-federation.md`. The production half of `mesh-three-vm-and-production` stays Proposed until then.

### Task 17: Program 3 live capture contract (draft for review)

**Files:** Create `docs/program-3-live-capture-contract.md`

- [ ] **Step 1:** Draft the separately reviewed read-only Discord metadata adapter contract: manager authorisation, participant consent rules, least-privilege scopes, bounded freshness, contradiction handling, redaction, retention, revocation; explicitly no message content, no member enumeration, no persistence beyond the contract, no effects. Cite `tasks/todo.md:330-342`.
- [ ] **Step 2:** Add a test to `tools/tests/test_check_p3_readonly.py`: `test_live_capture_modules_cannot_import_effect_paths` (the read-only checker covers any new live-capture module path) → FAIL → extend `tools/check_p3_readonly.py` → PASS.
- [ ] **Step 3:** Commit `"Draft the Program 3 live capture contract for review"`.
- [ ] **Step 4 (Donald, OB-18):** independent review of the contract + guild manager authorisation + participant consent records. Evidence `docs/verification/YYYY-MM-DD-p3-capture-authorisation.md`. Only then may an adapter be implemented (new TDD task: `live_capture_records_only_authorised_metadata`).

### Task 18: `live-guild-validation` and `guild-effects`

- [ ] **Step 1:** `live-guild-validation` follows Task 17 Step 4: separate authorised owner/admin read-only recordings compared against a closed replay with deterministic findings and revocation evidence (`guild_intelligence_replay` style test `live_recording_replays_to_the_same_findings_as_its_closed_replay`). Donald step for the recording.
- [ ] **Step 2:** `guild-effects` is a separate effect program: approvals, optimistic concurrency, exact before-state checks, bounded compensation, audit, rollback. **Donald step:** approve opening that program; until then no code. `program-3-guild-intelligence-synthetic` stays Current and is not widened.
