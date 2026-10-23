# Abbey Refactor and Full Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Delegation requires explicit authorization; this plan does not authorize spawning agents.

**Goal:** Refactor the retained Rust product into clear shared authorities and finish every included requirement in the approved Abbey Completion Program with independently verified release evidence.

**Architecture:** Preserve the single Rust workspace and canonical app-core contracts. Keep transport, durable state, authorization, execution and frontend presentation separate; refactor a boundary immediately before extending it, keeping serialization and recovery behavior stable. CLI, TUI and desktop consume the same daemon authorities rather than implement their own lifecycle logic.

**Tech Stack:** Rust 1.99.1 stable, SQLite/rusqlite, optional WDBX, ABI providers, Tauri/React/TypeScript, Python gate tooling, native Unix/Windows process and IPC adapters.

**Spec:** `docs/superpowers/plans/2026-10-22-abbey-completion.md`, `tasks/completion.json`, `AGENTS.md`, and `REWRITE-DECISION-2026-10-19.md`. This document decomposes the approved program; it does not replace its policies or promote capability claims.

## Global Constraints

- Retain Rust 1.99.1, the single Cargo workspace/lockfile, exact qualified dependency pins, existing user state, and canceled Zig history.
- Native acceptance is required on macOS ARM64, Ubuntu ARM64, and Windows 11 ARM at every milestone. External prerequisites block their own evidence channels, not unrelated implementation.
- Work in the canonical checkout; preserve in-flight edits, untracked files, historical receipts and finalized backups. Do not commit, push, publish, acquire paid artifacts, accept licenses or contact live participants merely because a plan step exists.
- `./check.sh` is the merge gate; `./desktop/check.sh` adds desktop acceptance. Run them serially on unchanged, fully manifested inputs. Use `./tools/cargo.sh` for focused Rust tests and never `cargo fmt --all`.
- Current runtime migration version is 11 and active v4 schema is 2 at inspection. Allocate the next unused migration/schema version at execution, never overwrite historical migrations.
- Existing `os_control` remains allowlisted/confirmed in both editions until a separate local personal structured shell path ships. Safe restrictions, OS authorization, signed privileged helpers, audit, bounds, emergency stop and exclusion from remote MCP/network workers remain required.
- One requirement has one canonical owner in `tasks/completion.json`; the tasks below are dependency-ordered work packages, not duplicate requirements. Preserve implemented evidence but revalidate before closure.
- Keep functions/types at current public boundaries during refactors. New authority-bearing APIs require a focused subsystem design with exact Rust signatures, wire samples, limits and failure states before implementation; broad program work packages do not invent unverified interfaces.

## Review Focus

1. Concurrent source changes invalidate exact-tree evidence: freeze manifests and detect drift before publishing a result (Tasks 1, 16).
2. Crash after an effect but before its result must never replay the effect during reconnect/restart (Tasks 6–9, 14–15).
3. Edition, principal, profile or grant changes must not reuse earlier authority or leak credentials/content (Tasks 5–11).
4. Pagination and streaming under concurrent writes, slow readers or disconnects must remain bounded and correlated (Tasks 3, 6, 9–10).
5. Missing hardware, licenses, keys, hosts or consent must remain explicit unavailable evidence rather than simulated production success (Tasks 11–16).

## Execution order and decision gates

Tasks 1–4 establish the baseline and refactoring seams. Task 5 supplies native foundations. Tasks 6–9 build authority before Task 10 exposes it. Tasks 11–15 qualify distribution and the remaining model/media/federation capabilities. Task 16 closes the program only after every requirement and all mandatory channels pass.

Refactor only the owner of the next deliverable. Do not perform a repository-wide rename/reorganization, rewrite the product, introduce a second runtime, or replace functioning behavior without a discriminating regression. Prefer focused files; preserve the existing hard 1,000-line source guard and 800-line warning. Large later subsystems require their own detailed plans before code, with the acceptance requirements below carried forward unchanged.

For each task: write a focused regression, observe its failure, implement the smallest complete slice, observe success, then inspect the diff and record evidence. A behavior-preserving extraction uses characterization tests that pass before and after. Commit only when requested. Full milestone acceptance adds both serial gates and all three native-platform receipts; a focused command alone never closes a milestone.

## Task 1: Freeze and reconcile the integration baseline

**Requirements:** `baseline-record`, `requirement-coverage`, `retained-plan-reconciliation`

**Files and ownership:** tasks/completion.json; tasks/completion-historical.json; docs/verification/2026-10-23-refactor-baseline.md. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Retain byte hashes, file modes, dirty/index patches, untracked files, exact branch/worktree inventories and compiler/ABI/WDBX revisions. Reconcile every retained obligation. The preceding merge attempt failed rustfmt in src/daemon/config/bearer_tests.rs; recheck current source before attributing or changing it. Integrate the one missing plugin commit only after a green merge bar; preserve dirty work and verify the merged tree.

**Regression acceptance:** Coverage rejects an unmapped checkbox and stale location; baseline records actual command exits, and retained dirty files match their captured hashes.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `python3 tools/check_completion.py` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 2: Recover reproducible gates and dependency qualification

**Requirements:** `dependency-security-remediation`, `completion-review-boundaries`, `identity-recovery-flake`

**Files and ownership:** src/runtime/identity.rs; src/runtime/store/identity.rs; src/train/; src/tui/run_loop.rs; Cargo.toml; Cargo.lock; desktop/Cargo.toml; tools/security/run-dep-scan.sh; .github/workflows/rust.yml. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Revalidate inherited fixes before editing. Reproduce identity recovery with serial, parallel and loaded-process fixtures; preserve an unreproduced failure as unresolved evidence. Qualify dependency candidates against pinned sibling revisions and the complete generated graph, including Tokio, updater ZIP, GLib and proc-macro-error. Promote pins only after candidate gates and native updater acceptance; never silently expand advisory exemptions.

**Regression acceptance:** Crash recovery cannot resurrect cleared identity; inherited pipes cannot escape trainer deadlines; failure leaves installed artifacts intact; strict dependency audit distinguishes unresolved findings from passing channels.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./check.sh` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 3: Refactor contract ownership without changing wire behavior

**Requirements:** `runtime-contracts`

**Files and ownership:** src/app_core/v4.rs; src/app_core/v4_types.rs; src/app_core/v4_protocol.rs; src/app_core/mod.rs; src/daemon/server.rs; src/runtime/store/v4.rs; desktop/codegen/src/. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Keep the compiled v4 command/event family canonical. Extract validation, conversation contracts, task contracts and automation contracts into focused modules with explicit reexports. Port necessary prototype types before deleting the disconnected v4_types.rs family. Split daemon framing/authentication/dispatch along existing seams. Preserve public names and serialized bytes; version intentional incompatible changes. Avoid unrelated mechanical splitting of doctor.rs, learn.rs or tui/app.rs until their feature tasks touch them.

**Regression acceptance:** Golden serialized requests/events and negative schema cases remain identical across extraction; codegen --check passes; no newly advertised capability lacks a dispatch implementation.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked --test app_core_contract` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 4: Close existing correctness and corpus obligations

**Requirements:** `wire-vocabularies`, `evidence-channels`, `provider-model-switch`, `prompt-budget`, `portable-ollama-probe`, `deferred-readiness`, `voice-claim-boundary`, `corpus-patterns`, `corpus-command`, `corpus-golden-vectors`, `synthetic-guild-correctness`, `model-request-validation`

**Files and ownership:** src/models.rs; src/models/selection_tests.rs; src/capture.rs; src/voice.rs; src/claims/registry.rs; src/app_core/v3_model.rs; src/abbey_contracts/; tests/ollama_no_pull.rs; tests/guild_intelligence_regressions.rs. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Retest implemented rows rather than repeat implementation. Repair portable probe bounds, evidence-channel isolation and remaining corpus defects. Fix corpus definitions and elapsed-deadline handling in ABI first, then qualify immutable upstream bytes before re-vendoring or pin promotion. Keep 32 KiB per-argument/64 KiB aggregate admission, normal multiline prompts, typed budget exhaustion and no model auto-pull.

**Regression acceptance:** Cross-provider changes persist a valid model; same-provider changes preserve explicit choice; local verbs spawn no probe; junk prefix/suffix IDs fail; independently computed golden values fail when tampered; channel failure preserves unrelated valid evidence.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked --test abbey_contracts` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 5: Finish native transports, supervision and installed services

**Requirements:** `windows-foundation`, `linux-service-foundation`, `macos-ci`, `linux-ci`, `accelerator-install-isolation`, `model-store-isolation`

**Files and ownership:** src/daemon/server.rs; src/daemon/client/; src/daemon/installed_service.rs; src/runtime/supervisor.rs; src/runtime/supervisor/unix.rs; tools/linux_user_service.py; install.sh; install.ps1; tools/ci/qualify-windows.ps1. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Create a Windows transport adapter and Job Object supervisor behind cfg boundaries, sharing frame/correlation/deadline rules with Unix. Finish Linux owner-only credential discovery and service lifecycle using the in-flight installed_service work. Isolate model stores and accelerator publication by edition. Native runner operators supply actual trusted executed jobs; a configured or skipped job is insufficient.

**Regression acceptance:** Wrong owner/peer and malformed/oversized frames fail closed; timeout/cancel kills descendants; safe/personal services and artifact roots cannot collide; restart and uninstall retain intended user state on each real host.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `python3 -m unittest discover -s tools/tests -p "test_*.py"` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 6: Implement durable tasks and immutable attempts

**Requirements:** `runtime-contracts`

**Files and ownership:** src/app_core/v4.rs; src/daemon/runtime_v4.rs; src/runtime/store/; src/runtime/migrations.rs; src/runtime/migration_12_tasks.sql (new if 12 remains next); tests/daemon_runtime.rs. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Add task/attempt storage through the next unused numbered migration. Bind goal, conversation, selected route, workspace, profile revision, budget, attempt and run IDs. Keep provider execution outside SQLite locks. Publish ReadTasks/WriteTasks only after real dispatch and client validation exist. Retry creates a fresh attempt; startup turns ambiguous admitted/running effects into interrupted outcomes.

**Regression acceptance:** Task admission is atomic, duplicate identity cannot execute twice, crash windows remain interrupted, cancelled runs do not restart, changed profiles cannot alter an existing attempt, and pages retain fixed watermarks across concurrent writes.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked --test daemon_runtime` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 7: Connect schedules, profiles and durable automation occurrences

**Requirements:** `runtime-contracts`

**Files and ownership:** src/automation_schedule.rs; src/automation_schedule/tests.rs; src/runtime/store/; src/app_core/v4.rs; src/daemon/runtime_v4.rs; tests/daemon_runtime.rs. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Connect the pure schedule calculator to persisted automation revisions and a unique occurrence reservation keyed by automation ID, revision and UTC instant. Preserve existing DST rules, latest-only catch-up and non-overlap. Revision changes, disablement and revocation invalidate future admissions. Use typed profile versions and the task admission interface from Task 6.

**Regression acceptance:** DST gap/fold, backward clock movement, restart at admission, duplicated ticks, disabled schedules and stale revisions produce at most one permitted task per occurrence; overlapping work is refused.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked automation_schedule` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 8: Build the owned structured tool loop and separate edition policies

**Requirements:** `owned-tool-host`, `personal-edition`

**Files and ownership:** src/agent/; src/os_control.rs; src/runtime/store/tool_approval.rs; src/runtime/store/tool_execution.rs; src/daemon/runtime_v3/tool_catalog.rs; tests/daemon_tool_execution.rs; src/tools/ (new). Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Route model-produced tool calls through registered schemas, effect-scoped policy, exact durable decisions, prepared intent and a bounded executor. Add scoped file actions, registered commands, dedicated-profile browser and native desktop adapters in separate modules. Introduce local personal shell.exec as a separate structured path; preserve existing os_control behavior in both editions. Enforce process-tree cancellation, capture limits, audit, emergency stop and OS permissions.

**Regression acceptance:** Prose never becomes shell syntax; approval cannot cross input/call/profile revisions; symlink escapes, stale targets and hostile output fail; crash after intent/effect never replays automatically; personal shell authority is absent from safe builds, served MCP and remote workers.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked --test daemon_tool_execution` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 9: Complete external protocol clients and served read-only MCP

**Requirements:** `owned-tool-host`, `mcp-server-conformance`

**Files and ownership:** src/mcp_host/; src/protocols/mcp.rs; src/protocols/; tests/mcp_server.rs; tests/mcp_http.rs; plugins/abbey/. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Keep external MCP/ACP client hosting separate from served read-only registry authority. At execution time consult and pin official protocol revisions before selecting version-specific lifecycle, cancellation and streaming contracts. Implement bounded correlation/session lifecycle, slow-consumer handling, redaction and explicit reconnect behavior. Resources/prompts/tasks appear only with functioning read-only handlers. Keep non-loopback serving unavailable unless its separate authentication acceptance is met.

**Regression acceptance:** Wrong-client/stale sessions, malformed depth, response loss, disconnect mid-mutation and duplicate IDs never grant or replay effects; classic compatible clients still pass; personal execution never enters server inventory.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked --test mcp_server --test mcp_http` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 10: Finish desktop product flows on shared authority

**Requirements:** `desktop-product`, `native-controls-proof`

**Files and ownership:** desktop/src/views/; desktop/src-tauri/src/backend/; desktop/src-tauri/src/commands.rs; desktop/src-tauri/src/ipc.rs; desktop/codegen/src/; desktop/scripts/; src/daemon/client/v4.rs. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Complete onboarding, selected-route/provider/workspace/profile configuration and credential-safe startup. Finish durable chat, exact Stop, explicit retry, attachments, search, pin/unarchive/export and bounded history. Connect Tasks/Tools/Approvals/Automations/Training/Cluster after their runtime tasks; unavailable authority remains visibly unavailable. Add memory curation, roles/permissions, diagnostic export, backup/restore/retention, notifications, menus and restored window state.

**Regression acceptance:** Reconnect preserves exact run identity without resubmission; changed grants revoke UI actions; raw credentials never enter WebView IPC; 10,000 messages remain bounded; keyboard, screen reader, 200% zoom and reduced motion pass actual packaged host journeys.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./desktop/check.sh` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 11: Ship edition-isolated native packaging and recovery

**Requirements:** `portable-package-services`, `three-platform-artifact-ci`

**Files and ownership:** desktop/src-tauri/tauri.conf.json; .github/workflows/desktop-release.yml; .github/workflows/desktop.yml; install.sh; install.ps1; tools/linux_user_service.py; tools/ci/require_executed_run.py. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Build macOS app/DMG, Linux native packages and Windows installers for both editions with exact runtime resources and service identities. Add signed update, failed-update rollback, retained-state uninstall and clean-user installed journeys. Detect Linux X11/Wayland and portal capabilities. Bind each CI receipt to immutable inputs, executed run, artifact digest and signature/provenance.

**Regression acceptance:** Partial install/update failures restore old working bytes; safe/personal coexist; stale-ref and zero-job workflows fail qualification; missing signing keys or native hosts stay explicit blockers.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `python3 -m unittest discover -s tools/tests -p "test_*release*.py"` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 12: Qualify the exact production model and accelerator runtime

**Requirements:** `production-model`, `accelerator-runtime`, `model-media-roadmap`

**Files and ownership:** src/daemon/runtime_v3/model_lifecycle/; src/app_core/v3_model.rs; src/accel/; tests/daemon_model_lifecycle.rs; docs/verification/; upstream ABI model/runtime modules. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Create an exact model acquisition/qualification subplan before implementation: immutable Gemma 12B architecture, tokenizer fixtures, licensed artifacts, device policy and no substitute model. Require 4K context plus 64-token smoke and RSS <=20 GB on the named 24 GB Mac without sustained swap thrash. Implement complete inference/training workloads through ABI; retain CPU oracle tolerances and requested/executed-device evidence. Qualify each claimed hardware backend separately.

**Regression acceptance:** Corrupt/wrong-revision artifacts, absent acceptance, unsupported architecture/device and deadline exhaustion fail explicitly; bigram fixtures and numerical kernels never stand in for production inference; capture actual memory/device/quality measurements.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked --test daemon_model_lifecycle` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 13: Deliver real adapters and offline neural media

**Requirements:** `production-adapters`, `neural-media`

**Files and ownership:** src/train/; src/voice.rs; src/agent/; src/claims/registry.rs; tests/learn_distill.rs; upstream ABI training and media providers. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Write provider-specific subplans against accepted artifacts before selecting new interfaces. Extend consent-gated training to real E4B adapters with immutable base identity, redaction/deduplication and deterministic 80/10/10 splits. Require actual changed weights, >=95% held-out valid tool-call JSON, <=2% NLL regression and 100% safe/public-edition refusal success. Deliver Whisper/Parler speech before SDXL/LTX images/video with artifact provenance and rollback.

**Regression acceptance:** Missing consent or license prevents training/download; corrupt artifacts, cancellation and resource exhaustion clean up deterministically; held-out leakage fails qualification; quality regressions preserve prior installed adapter; media outputs identify exact provider/artifacts.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked --test learn_distill` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 14: Implement federated authorization and worker execution

**Requirements:** `worker-runtime`, `compute-editions`, `three-vm-proof`, `physical-federation-proof`

**Files and ownership:** src/abbey_contracts/; src/daemon/; src/runtime/; src/mesh.rs; tests/abbey_contracts.rs; upstream ABI/WDBX federation modules. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Enable authorization, consent, cognition, episodes, manifests and changes in that order, one durable adapter at a time. Implement worker inventory, signed jobs/results, quotas, leases/deadlines, mTLS, key rotation and cancellation. Isolate membership/control state from WDBX placement. First prove three Ubuntu ARM64 VMs with independent IPs/identities/state; separately qualify physical hosts and geographic recovery. Remote workers never inherit personal shell authority.

**Regression acceptance:** Partitions, stale leaders, duplicate results, expired leases, certificate revocation and worker death preserve authority and prevent duplicate effects; read repair, restore, rolling upgrade and teardown pass on the actual claimed deployment class.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked --test abbey_contracts` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 15: Qualify consented guild observations and separately approved effects

**Requirements:** `consented-capture`, `live-guild-validation`, `guild-effects`

**Files and ownership:** src/abbey_contracts/; src/app_core/guild_intelligence.rs; src/app_core/guild_intelligence/; tests/guild_intelligence_replay.rs; tests/guild_intelligence_permissions.rs; docs/verification/. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Prepare recording/retention/redaction and revocation contracts with synthetic fixtures first. Obtain explicit participant consent and service authorization before non-synthetic recording. Compare independent owner/admin observations deterministically. Add effects only through exact observed preconditions, approved plan digest, durable intent, outcome and rollback previews from Task 8.

**Regression acceptance:** Missing/revoked consent blocks capture; permission differences remain visible; stale observations and changed targets invalidate approval; partial failure reports actual outcomes and never invents successful rollback.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./tools/cargo.sh test --locked --test guild_intelligence_replay --test guild_intelligence_permissions` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Task 16: Freeze, review and close the full product

**Requirements:** `final-closure`

**Files and ownership:** tasks/completion.json; tasks/goals.md; tasks/todo.md; src/claims/registry.rs; docs/verification/; docs/production.md; .github/workflows/. Resolve existing module locations before edits; create focused siblings only where the owner does not already exist.

**Interfaces:** Consumes the approved program and preceding task contracts; preserves existing public APIs during extraction. Produces the capability/acceptance contract below. For new APIs, record exact signatures and serialization fixtures in the subsystem plan before implementing.

**Implementation:** Freeze exact Abbey/ABI/WDBX inputs and run serial root/desktop plus coordinated dependency gates, supported feature cohorts, strict dependency/license/SBOM/secret checks and required external security review. Verify installed signed artifacts, recovery/support runbooks and all platform/device/consent channels. Measure cold start <=5s, warm interaction p95 <=2s, navigation feedback <=100ms, idle CPU <=2% of one core and bounded 24-hour soak resources on named hardware. Regenerate claims only after independent evidence review.

**Regression acceptance:** Every included requirement has current source, automated and explicitly required external/native evidence; skipped or unavailable checks remain incomplete. All active branches are integrated after green gates, user work is retained, and release artifacts match tested inputs.

- [ ] Capture current owner modules, exact inputs and a focused task design; verify dependency tasks and external prerequisites.
- [ ] Add or select discriminating regressions for the acceptance above; record the expected failure, or passing pre-refactor characterization.
- [ ] Implement the bounded slice described above without changing unrelated authority or user state.
- [ ] Run `./check.sh` and the task-specific regression selection; require exit 0 and the asserted behavior, then run serial milestone gates when the slice is ready.
- [ ] Review the diff, record source/test/native channels separately, reconcile ledgers and historical checkbox bindings; commit only if requested.

## Complete requirement acceptance matrix

This is the scope contract, copied from the live requirement ledger at planning time. Execution must re-read the ledger for subsequent authorized changes. `implemented` rows still require evidence review. No requirement is silently excluded.

### `baseline-record` — Task 1

Owner: Abbey maintainer. Ledger status at planning: `open`. Required channels: source, automated.

- Record exact revisions, clean/dirty manifests, locks, compiler, root/desktop exit status and limitations

### `requirement-coverage` — Task 1

Owner: Abbey maintainer. Ledger status at planning: `implemented`. Required channels: source, automated.

- Coverage checker rejects new unmapped work, conflicting owners, stale bindings and unsupported completion claims

### `retained-plan-reconciliation` — Task 1

Owner: Abbey maintainer. Ledger status at planning: `open`. Required channels: source, automated.

- Audit retained completion/consolidation/desktop plans for obligations absent from the canonical workflow ledger; record each inclusion or explicit supersession without deleting historical checkboxes
- Capture source file modes and exact command environment in addition to byte hashes; verify isolated sibling-directory candidate equivalence before treating prior consolidation work as qualified
- Retain deliberate stale-but-usable executable negative controls proving feature-matched gate artifact selection
- Preserve fixed-snapshot title search and cross-run cursor refusal, serialized response ceilings, 4 MiB/65,536-event/16 KiB preview bounds and replies completed between polls

### `native-controls-proof` — Task 10

Owner: Desktop maintainer. Ledger status at planning: `open`. Required channels: source, automated.

- Native Accessibility/control driver assertion and fixture journeys run on real packaged hosts; source parsing alone does not close

### `macos-ci` — Task 5

Owner: Runner administrator. Ledger status at planning: `open`. Required channels: source, automated, native_ci.

- Bring the registered macOS ARM64 runner online and retain an exact-head successful executed job

### `linux-ci` — Task 5

Owner: Runner administrator. Ledger status at planning: `implemented`. Required channels: source, automated, native_ci.

- Register Ubuntu ARM64 runner, enable guarded job and satisfy require_executed_run.py against an actual successful Linux run

### `windows-foundation` — Task 5

Owner: Windows maintainer and runner administrator. Ledger status at planning: `in_progress`. Required channels: source, automated, native_ci.

- Implement native named pipes, owner ACLs, peer restriction, bounded framing/deadlines, Job Object teardown and native Windows ARM CI/install proofs

### `runtime-contracts` — Task 3, 6, 7

Owner: Abbey runtime maintainer. Ledger status at planning: `open`. Required channels: source, automated.

- Canonical compiled types, append-only migrations, real task/automation/profile/action stores and dispatch pass grant/recovery/contract fixtures

### `owned-tool-host` — Task 8, 9

Owner: Abbey and ABI maintainers. Ledger status at planning: `open`. Required channels: source, automated.

- Real model-produced structured multi-tool loop and external MCP/ACP sessions pass policy, schema, timeout, correlation, hostile-output and mutation non-replay tests; served MCP remains separately scoped

### `desktop-product` — Task 10

Owner: Desktop maintainer and release owner. Ledger status at planning: `open`. Required channels: source, automated.

- All product and management flows use actual grants/daemon data and pass packaged native acceptance for both editions

### `production-model` — Task 12

Owner: ABI model maintainer and license holder. Ledger status at planning: `open`. Required channels: source, automated, license, native_quality.

- Qualify exact Gemma 12B artifacts/tokenizer/architecture/license and actual inference against the model milestone budgets

### `accelerator-runtime` — Task 12

Owner: ABI accelerator maintainer and hardware operator. Ledger status at planning: `open`. Required channels: source, automated, native_hardware.

- Real complete model/training workloads pass CPU oracle, no-fallback, requested/executed-device and allocation/cancel tests on each claimed device

### `production-adapters` — Task 13

Owner: ABI training maintainer and dataset consent owner. Ledger status at planning: `open`. Required channels: source, automated, consent, license, held_out_quality.

- Actual E4B adapter weight update, held-out quality bars, provenance, compatibility and deterministic rollback pass; demo training fidelity is insufficient

### `neural-media` — Task 13

Owner: Media maintainer and artifact license holder. Ledger status at planning: `open`. Required channels: source, automated, license, native_quality.

- Offline licensed speech/image/video providers pass quality, resource, corruption, cancellation and artifact-provenance fixtures

### `compute-editions` — Task 14

Owner: ABI/Abbey maintainers and VM operator. Ledger status at planning: `open`. Required channels: source, automated.

- Shared compute and the independently isolated personal edition each meet their own evidence bars

### `consented-capture` — Task 15

Owner: Recording consent owners. Ledger status at planning: `open`. Required channels: source, automated, authorization, consent.

- Reviewed retention/recording contract plus explicit participant consent precede any non-synthetic capture

### `live-guild-validation` — Task 15

Owner: Live service authorization and participant consent owners. Ledger status at planning: `open`. Required channels: source, automated, authorization, consent.

- Separate authorized owner/admin read-only recordings have deterministic findings and revocation evidence

### `guild-effects` — Task 15

Owner: Live service authorization and participant consent owners. Ledger status at planning: `open`. Required channels: source, automated, authorization, consent.

- Separately approved effects bind observed preconditions, exact plan, rollback, durable intent and outcome; no deployment follows from fixture success

### `identity-recovery-flake` — Task 2

Owner: Abbey runtime maintainer. Ledger status at planning: `open`. Required channels: source, automated.

- Reproduce or retain the exact historical recovery failure; serial/parallel and load fixtures discriminate journal ownership without attributing an unobserved cause

### `worker-runtime` — Task 14

Owner: ABI worker maintainer and VM operator. Ledger status at planning: `open`. Required channels: source, automated.

- Actual workers have truthful negotiated inventory, lifecycle, local IPC/mTLS jobs, quotas, leases, cancellation and recovery

### `personal-edition` — Task 8

Owner: Abbey runtime maintainer and platform signing owners. Ledger status at planning: `open`. Required channels: source, automated.

- Revise conflicting invariants explicitly, keep safe os_control behavior, implement local personal structured shell without per-command confirmation and verify isolation, bounds, audit and OS authorization

### `model-media-roadmap` — Task 12

Owner: ABI model/media/training maintainers. Ledger status at planning: `open`. Required channels: source, automated.

- Every inference, accelerator, training and media acceptance in milestone models-media passes independently

### `three-vm-proof` — Task 14

Owner: VM infrastructure operator. Ledger status at planning: `open`. Required channels: source, automated, three_vm.

- Three independent Ubuntu ARM64 VM identities/state/IPs pass authenticated fault/recovery and upgrade scenarios with an independent membership control store

### `final-closure` — Task 16

Owner: Release owner and independent reviewers. Ledger status at planning: `open`. Required channels: source, automated.

- Every included requirement and platform evidence is complete before ledger closure; canceled Zig and deliberate OOS remain excluded
- Run coordinated ABI/WDBX gates, dependency/license/secret and external security reviews, native installed journeys and sustained-resource acceptance against exact frozen inputs; regenerate claims only from their independently reported evidence

### `wire-vocabularies` — Task 4

Owner: Abbey protocol maintainer. Ledger status at planning: `implemented`. Required channels: source, automated.

- Bump affected v1/v3 schemas, reject old clients before changed claim/model vocabularies, update canonical claim boundaries and generated wire samples

### `evidence-channels` — Task 4

Owner: Abbey claims maintainer. Ledger status at planning: `open`. Required channels: source, automated.

- Channel-specific failed/expired evidence preserves independently verified local channels; registry schema and sync tests pin the representation

### `provider-model-switch` — Task 4

Owner: Abbey TUI maintainer. Ledger status at planning: `implemented`. Required channels: source, automated.

- Every cross-provider transition selects/persists a valid default, same-provider selection preserves explicit model, and failed persistence leaves identity unchanged

### `prompt-budget` — Task 4

Owner: Abbey process maintainer. Ledger status at planning: `implemented`. Required channels: source, automated.

- Prompt/Doctor ceiling matches 32 KiB argument and 64 KiB aggregate bounds including fixed arguments; Unicode and multi-argument fixtures cross real process validation

### `portable-ollama-probe` — Task 4

Owner: Abbey process maintainer and Windows acceptance owner. Ledger status at planning: `open`. Required channels: source, automated.

- Readiness uses bounded portable capture off Unix and passes native success/missing tag/timeout/overflow fixtures on Windows

### `deferred-readiness` — Task 4

Owner: Abbey provider maintainer. Ledger status at planning: `implemented`. Required channels: source, automated.

- Local-only commands never spawn a readiness probe; generation auto-selection probes the effective requested tag and never downloads

### `voice-claim-boundary` — Task 4

Owner: Abbey claims maintainer. Ledger status at planning: `open`. Required channels: source, automated.

- Revalidate corrected registry and generated docs against dispatch: portable adapters are non-macOS only; macOS failure has no portable fallback

### `corpus-patterns` — Task 4

Owner: ABI corpus maintainer. Ledger status at planning: `open`. Required channels: source, automated.

- Anchor upstream identifier/digest schemas, re-vendor exact qualified bytes and digest; junk prefixes/suffixes fail fixtures

### `corpus-command` — Task 4

Owner: ABI corpus maintainer. Ledger status at planning: `open`. Required channels: source, automated.

- Upstream digest-covered README names an executable verified command, then qualified corpus is re-vendored

### `corpus-golden-vectors` — Task 4

Owner: Abbey and ABI corpus maintainers. Ledger status at planning: `in_progress`. Required channels: source, automated.

- Compute and independently compare canonical bytes and expected corpus digests; altered expected values cannot return Valid

### `synthetic-guild-correctness` — Task 4

Owner: Abbey synthetic-engine maintainer. Ledger status at planning: `implemented`. Required channels: source, automated.

- No empty advertised alternatives, overwrite absence preserved in rollback, visibility digest binds bot authority, JSON is bounded before allocation, cyclic parent graphs refused

### `accelerator-install-isolation` — Task 5

Owner: Abbey installer maintainer. Ledger status at planning: `implemented`. Required channels: source, automated.

- Safe/personal installed accelerator layouts cannot replace each other; staged/published verification and rollback fixtures preserve prior bytes

### `model-request-validation` — Task 4

Owner: Abbey and ABI model-runtime maintainers. Ledger status at planning: `in_progress`. Required channels: source, automated.

- Serialized frame bounds and ordinary multiline whitespace are tested; budget exhaustion stays typed and elapsed-duration handling is repaired upstream before a qualified pin change

### `model-store-isolation` — Task 5

Owner: Abbey and ABI model-store maintainers. Ledger status at planning: `open`. Required channels: source, automated.

- Edition-qualified model storage selection refuses shared/cross-edition overrides and passes real artifact-store isolation fixtures

### `mcp-server-conformance` — Task 9

Owner: Abbey MCP protocol maintainer. Ledger status at planning: `open`. Required channels: source, automated, protocol_conformance.

- Pin and validate the official served protocol revision; implement version-appropriate lifecycle, cooperative cancellation and bounded streaming/resumability with classic-client compatibility fixtures
- Advertise resources/prompts/tasks only for exact registered read-only handlers; test frame/depth limits, stale/cross-client sessions, slow consumers, cancellation and secret redaction
- Keep non-loopback unavailable unless separately reviewed TLS/resource-server authentication, PKCE/resource indicators/audience binding, revocation and redirect/Host/Origin protection pass; personal execution never enters served MCP

### `portable-package-services` — Task 11

Owner: macOS/Linux/Windows platform maintainers and release owners. Ledger status at planning: `open`. Required channels: source, automated, macos_arm64, ubuntu_arm64, windows11_arm, signing.

- Build edition-isolated macOS app/DMG, Windows installers and Linux native packages with correct dependencies, runtime resources, service, updater and retained-state uninstall behavior
- Exercise real PowerShell coexistence, native WebView chat/tasks/tools/automations, service restart and clean-user package journeys on Windows 11 ARM and Ubuntu ARM64
- Detect X11/Wayland and portal capabilities explicitly; unavailable capture/control remains unavailable

### `three-platform-artifact-ci` — Task 11

Owner: Runner administrators and release owners. Ledger status at planning: `open`. Required channels: source, automated, macos_arm64, ubuntu_arm64, windows11_arm, native_ci, signing, artifact_provenance.

- Run nonzero trusted jobs on macOS ARM64, Ubuntu ARM64 and Windows 11 ARM with exact immutable Abbey/ABI/WDBX inputs and minimal permissions
- Require root/desktop gates, release package builds, installed journeys, security scans and artifact signature/provenance verification per host and edition
- Verify failure propagation, cancellation, stale-ref rejection, exact run/ref/artifact correlation and cleanup; zero-job or skipped workflows never satisfy proof

### `physical-federation-proof` — Task 14

Owner: Physical-host infrastructure and deployment operators. Ledger status at planning: `open`. Required channels: source, automated, physical_hosts, geographic_deployment, hardware.

- Separately qualify authenticated physical multi-host and geographic-HA/multi-device operation using real distinct identities, devices, addresses and persisted state
- Exercise physical-host loss, network partitions, stale leaders, certificate rotation, recovery, rolling upgrades and teardown; same-host processes and three VMs cannot substitute

### `linux-service-foundation` — Task 5

Owner: Linux platform maintainer and Ubuntu ARM64 runner operator. Ledger status at planning: `in_progress`. Required channels: source, automated, ubuntu_arm64, native_ci.

- Provide edition-isolated Linux daemon/service and desktop discovery/startup integration with owner-only credentials and exact Unix framing/correlation/cancellation parity
- Run native minimal install/start/status/cancel/restart/uninstall-retention fixtures on Ubuntu ARM64 before foundation closure; completed Tasks/Tools/Automations journeys are tracked separately under portable-package-services

### `dependency-security-remediation` — Task 2

Owner: Dependency maintainers, Windows update acceptance owner and release reviewer. Ledger status at planning: `in_progress`. Required channels: source, automated, dependency_audit, license_review, secret_scan, macos_arm64, ubuntu_arm64, windows11_arm.

- Repair Tokio and Tauri updater ZIP dependency advisories using an exact reviewed candidate and qualify the full generated dependency graph; retain existing pins until candidate promotion is justified
- Resolve or explicitly review GLib VariantStrIter unsoundness and proc-macro-error maintenance findings without silently broadening the existing advisory exceptions
- Require strict dependency/license/secret evidence plus root/desktop gates and native updater/install acceptance for all supported editions/hosts before release qualification

### `completion-review-boundaries` — Task 2

Owner: Abbey training and TUI maintainers. Ledger status at planning: `implemented`. Required channels: source, automated.

- Trainer deadlines include inherited pipes and preserve process-group ownership; invocation errors remove partial artifacts
- Rollback validates and records consent; failed baseline scoring never replaces current state
- UTF-8 cannot bypass PEM redaction; terminal entry errors reach event-loop cleanup
- Focused discriminating regressions plus serial unchanged-source root/desktop gates; required native evidence remains separate
- CLI fixtures isolate HOME and both editions even when a stale executable is launched; qualification builds and metadata checks never overlap

## Completion and handoff

The deliverable is a qualified retained Rust application covering the full approved scope, with a reconciled ledger, current claims, tested installed artifacts and recovery documentation. No estimate or calendar promise is made before the baseline, hardware/artifact availability and runner access are known. Implementation begins with Task 1; planning itself performs no merge, product refactor, dependency promotion or release.
