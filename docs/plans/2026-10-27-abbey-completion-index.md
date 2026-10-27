# Abbey Completion Index (2026-10-27 re-baseline)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-baseline the 2026-10-22 workstreams WS-1…WS-20 against the 32 requirements that `tasks/completion.json` still records as not `implemented`, assign each one a class, an owning plan file and an order, and fix the rule by which any claim may be promoted.

**Architecture:** This file is the spine. It owns no source change. Six sibling plans (all `docs/plans/2026-10-27-*.md`) own the work; this index owns the mapping, the lanes, the promotion rule, and a short set of bookkeeping tasks that keep the mapping true as the other plans land.

**Tech Stack:** Rust 1.99.1 (`./tools/cargo.sh`), Python 3 ledger tools (`tools/check_completion.py`, `tools/check_claims_sync.py`), `tasks/completion.json` (schema 1).

**Spec:** `docs/superpowers/plans/2026-10-22-abbey-completion.md` (active execution plan, Donald-approved 2026-10-22) and `docs/completion-plan-2026-10-22.md` (WS-1…WS-20, OB-1…OB-19). `tasks/completion.json` is the requirement matrix; `src/claims/registry.rs` is the capability authority.

## Context (verified 2026-10-26/27 against the checkout)

- Checkout: `main` at `8fdb4ed`, ahead 1 / behind 2 of `origin/main` (`6901d11`, PR #108). Dirty tree described in `2026-10-27-abbey-main-sync-plan.md`. **No source work in the canonical checkout may start until that plan's Task 6 (gate on the merged tree) is done.**
- `tasks/completion.json`: 45 requirements — 13 `implemented`, 5 `in_progress`, 27 `open` → **32 not implemented**. Milestones: `baseline` in_progress; `platforms`, `correctness`, `authority`, `tools-editions`, `desktop`, `models-media`, `federation-compute`, `qualification` open.
- Claims (`docs/claims.md:5`): 42 Current · 5 Partial · 7 Proposed · 1 Blocked · 5 Out of scope.
  - Partial: `surface-parity-grok-codex-claude` (`src/claims/registry.rs:78`), `daemon-abbey-v1-federation` (`:227`), `edition-windows-install-naming` (`:619`), `training-lora-pipeline` (`:684`), `training-local-consent-pipeline` (`:697`).
  - Proposed: `desktop-tauri-react` (`:645`), `runtime-provider-neutral-owned` (`:658`), `models-production-local-weights` (`:671`), `accelerator-execution-runtime` (`:710`), `media-local-neural-models` (`:723`), `edition-personal-unrestricted` (`:736`), `mesh-three-vm-and-production` (`:749`).
  - Blocked: `ci-self-hosted-linux-proof` (`:762`).
- **New plan files are not ledger inputs.** `tools/check_completion.py:121` scans only `docs/superpowers/plans/*.md` and `docs/*plan*.md` (non-recursive) for unmapped checkboxes. Files under `docs/plans/` are therefore not gated by `tasks/completion-historical.json`, and these seven plans do not turn `./check.sh` red. Do **not** move them into `docs/superpowers/plans/` without registering every checkbox in `tasks/completion-historical.json` first.

## Global Constraints (copied from `AGENTS.md` and `docs/agent-guide.md`; every task in every sibling plan inherits these)

- Rust 1.99.1 and `./tools/cargo.sh`; `./check.sh` is the authoritative merge gate. Every gate cargo call is `--locked` (`cargo fmt` excepted). `tools/cargo.sh` never adds `--locked` for you.
- `../abi` and `../wdbx` are mandatory sibling path dependencies. Abbey tasks never edit them; a sibling change is a request to that repository's owner and is consumed only through a reviewed, pinned revision (`tools/ci/verify-pinned-siblings.sh`).
- The Zig rewrite is canceled. Preserve retained Rust code, dirty work, history, and unpushed commits. One writer per repository; task branches/worktrees only per `docs/superpowers/plans/2026-10-25-local-main-readiness.md`.
- Shipped OS-execution safeguards (`src/os_control.rs` allowlist + `--confirm`, both editions), consent, audit, edition separation and claims/evidence boundaries remain in force. Persona choice grants no authority.
- Capability status changes only in `src/claims/registry.rs`, then `python3 tools/check_claims_sync.py --write`. Never hand-edit a generated region. Goal/todo counts are recounted by that tool.
- Migrations are append-only; `CURRENT_SCHEMA_VERSION` is 11 today (`src/runtime/migrations.rs:6`). Never rewrite an applied migration.
- `src/**/*.rs` files warn over 800 lines and fail over 1000; `main.rs` fails over 200. `unsafe_code` is denied by manifest lint.
- Tests use isolated, edition-scoped scratch state (`abbey::edition::ACTIVE.state_dir_env()`); never a user's state, socket, or bearer.
- Generated desktop IPC (`desktop/src/ipc/generated*.ts`) is regenerated with `(cd desktop && bun run codegen)`, never hand-edited.
- Commits are made only when the executing session holds Donald's authorization to commit; pushes, merges into `main`, releases and deployments are always **Donald steps**.
- Correctness, performance, live, and production qualification are separate facts. A passing test never proves the others.
- Synthetic training never qualifies a model; Abbey never trains a model as a side effect of any gate or test.
- Default acceptance and evidence for every task in every sibling plan, unless the task states otherwise: **acceptance** = the task's named tests pass with the shown command, plus `./check.sh` → `check.sh: OK` (and `./desktop/check.sh` → `desktop/check.sh: OK` when the task touches `desktop/`); **evidence** = the task's commit plus `docs/verification/YYYY-MM-DD-<plan-slug>-task<N>.md` recording the exact commands, revision (`git rev-parse HEAD` + sibling heads) and results, including skips and failures.

## Review Focus

- A requirement marked `implemented` with a still-open todo: `check_completion.py` only refuses `complete` with open todos (`tools/check_completion.py:209-210`), so an `implemented` row can silently hide unfinished work. The correctness plan Task 12-14 reconciles the ten such rows.
- A todo text bound twice: duplicate checkbox *text* binds once (`tools/check_completion.py:203-207`), so removing one copy of a duplicated todo is safe only if the other copy keeps the exact first-line text.
- A Partial/Proposed claim promoted because tests passed: forbidden by the promotion rule below.
- A sibling (`../abi`, `../wdbx`) change consumed from a dirty local checkout instead of a pinned revision.
- A plan step that substitutes for a Donald step (runner, key, licence, consent, push).

## WS-1…WS-20 re-baseline

| WS | 2026-10-22 goal | State on 2026-10-26 (verified) | Remaining work → plan |
|---|---|---|---|
| WS-1 | Qualify unified workspace | Done: `baseline-record` implemented; `tools/tests/test_workspace_topology.py` exists | `retained-plan-reconciliation` → correctness plan |
| WS-2 | Desktop harness/render gaps | Partly done: `desktop/tests/models-view.test.tsx`, `update-check.test.tsx` exist; `prove_packaged_macos.py` already selects by identifier (`desktop/scripts/prove_packaged_macos.py:16-25,78-100`) | CommandPalette/ErrorBoundary/usePreferences tests; wire `drive-surfaces/controls` → desktop plan |
| WS-3 | Surface-parity polish | Open (Partial claim) | → authority-tools plan |
| WS-4 | One v4 contract family | Port done (`src/app_core/v4/{task,automation,action,admin}.rs`; `v4.rs` 233 lines); `src/app_core/v4_types.rs` (763 lines) still present, not declared in `src/app_core/mod.rs:61-66` | Delete dead family → authority-tools plan |
| WS-5 | Federation families | Open; `Authorize` stub at `src/daemon/federation.rs:180-183`, catalog `:350-358` all authority rows `enabled:false` | → authority-tools plan |
| WS-6 | Durable v4 tasks/automations/actions | Open; store stubs `src/runtime/store/v4_admin.rs:53-82`, refusals `src/daemon/runtime_v4.rs:141-160`; schema now 11 so this is migration **12** | → authority-tools plan (`runtime-contracts`) |
| WS-7 | v3 training/worker/polling | Open; capabilities `src/app_core/v3.rs:40-45`, `PollEvents` `:363` | Polling → authority-tools; training → models plan; workers → models plan |
| WS-8 | Desktop governed surfaces | Open; README "Unavailable execution surfaces" (`desktop/README.md:436`) | → desktop plan |
| WS-9 | Notarized release | Repo-side authored; OB-1…OB-4 | → desktop plan (Donald steps) |
| WS-10 | Adapter training | Native Rust pilot landed (`crates/abbey-learning`, `training-lora-pipeline` Partial) | → models plan |
| WS-11 | Tool host + MCP | Open | → authority-tools plan |
| WS-12 | Production model | Open; OB-12/13/14 | → models plan |
| WS-13 | Windows | 1 unreviewed commit `5be3d25` on `abbey/windows-foundation` | → platforms plan |
| WS-14 | Program 3 live | Open; OB-18 | → models-media-federation plan |
| WS-15 | Accelerator | Kernel parity Current; runtime Proposed | → models plan |
| WS-16 | Neural media | OB-16 | → models plan |
| WS-17 | Personal edition | Open | → authority-tools plan |
| WS-18 | Workers + 3 VMs | Open; OB-11 | → models-media-federation plan |
| WS-19 | Linux/Windows CI | Jobs authored: `gate-macos` (`.github/workflows/rust.yml:39-48`), `gate-linux` (`:161-170`), `qualify-windows-arm64` (`:290-298`); all runners unavailable | → platforms plan (Donald/runner steps) |
| WS-20 | Final closure | Open; OB-19 | → platforms-security-closure plan |

## Requirement map (32 not implemented)

Class key: **A** = agent-doable now in this repo; **S** = needs a reviewed `../abi` or `../wdbx` change first; **OB-n** = owner-blocked (completion-plan §3). A row can carry several classes: the plan implements the A part and stops at the S/OB gate.

| # | Requirement | Milestone | Status | Class | Plan file | Order |
|---|---|---|---|---|---|---|
| 1 | retained-plan-reconciliation | baseline | open | A | correctness | 1 |
| 2 | macos-ci | platforms | open | OB-6 | platforms-security-closure | 3 |
| 3 | windows-foundation | platforms | in_progress | A (review/integrate `5be3d25`) + OB-9 | platforms-security-closure | 2 |
| 4 | linux-service-foundation | platforms | in_progress | A + OB-8 | platforms-security-closure | 3 |
| 5 | model-store-isolation | platforms | open | A or S (decision gate) | platforms-security-closure | 2 |
| 6 | identity-recovery-flake | correctness | open | A | correctness | 1 |
| 7 | portable-ollama-probe | correctness | open | A + OB-9 (native Windows) | correctness | 2 |
| 8 | corpus-patterns | correctness | open | S (`../abi/contracts/abbey`) | correctness | 3 |
| 9 | corpus-command | correctness | open | S (verify first; may already be resolved upstream) | correctness | 3 |
| 10 | model-request-validation | correctness | in_progress | A (gate evidence) + S (ABI elapsed-duration) | correctness | 2 |
| 11 | runtime-contracts | authority | in_progress | A | authority-tools | 2 |
| 12 | owned-tool-host | tools-editions | open | A + S (ABI tool runtime, Phase 5) | authority-tools | 3 |
| 13 | personal-edition | tools-editions | open | A + OB-10 (signed helpers) | authority-tools | 3 |
| 14 | mcp-server-conformance | tools-editions | open | A | authority-tools | 2 |
| 15 | native-controls-proof | desktop | open | A (wiring) + OB-5 | desktop | 2 |
| 16 | desktop-product | desktop | open | A (after #11) + OB-1…4, OB-9 | desktop | 4 |
| 17 | portable-package-services | desktop | open | OB-8, OB-9, OB-10 | desktop | 5 |
| 18 | production-model | models-media | open | S (OB-12) + OB-13, OB-14 | models-media-federation | 4 |
| 19 | accelerator-runtime | models-media | open | A (Apple) + S + OB-17 | models-media-federation | 4 |
| 20 | production-adapters | models-media | open | S (OB-15) + OB-13 + consent | models-media-federation | 4 |
| 21 | neural-media | models-media | open | OB-16 | models-media-federation | 5 |
| 22 | model-media-roadmap | models-media | open | aggregate of #18-21 | models-media-federation | 6 |
| 23 | compute-editions | federation-compute | open | aggregate of #13, #27 | models-media-federation | 6 |
| 24 | consented-capture | federation-compute | open | OB-18 | models-media-federation | 5 |
| 25 | live-guild-validation | federation-compute | open | OB-18 (after #24) | models-media-federation | 6 |
| 26 | guild-effects | federation-compute | open | OB-18 + separate approval | models-media-federation | 7 |
| 27 | worker-runtime | federation-compute | open | A + S (`abi-worker`) | models-media-federation | 4 |
| 28 | three-vm-proof | federation-compute | open | OB-11 | models-media-federation | 6 |
| 29 | physical-federation-proof | federation-compute | open | owner-blocked; **no OB row exists** for physical/geographic hosts (OB-11 covers VMs only) | models-media-federation | 8 |
| 30 | final-closure | qualification | open | OB-19 | platforms-security-closure | 9 |
| 31 | three-platform-artifact-ci | qualification | open | OB-6, OB-8, Windows runner, OB-10 | platforms-security-closure | 8 |
| 32 | dependency-security-remediation | qualification | in_progress | A + S (GLib upstream) + native hosts | platforms-security-closure | 3 |

Order 0 for everything: `2026-10-27-abbey-main-sync-plan.md`.

Implemented requirements that still carry open todos (reconciled in the correctness plan): `linux-ci`, `wire-vocabularies`, `evidence-channels`, `provider-model-switch`, `prompt-budget`, `deferred-readiness`, `voice-claim-boundary`, `corpus-golden-vectors`, `synthetic-guild-correctness`, `accelerator-install-isolation`.

Retained unfinished plans folded in by reference: `docs/plans/2026-10-25-abbey-live-models.md` (12 boxes) → models-media-federation plan Task 1-3; `docs/plans/2026-10-25-compatible-security-pins.md` (6 boxes) → platforms-security-closure plan Task 13.

## Lanes

| Lane | Plan | Can start | Writer | Runs parallel with |
|---|---|---|---|---|
| 0 | main-sync | now | canonical checkout (sole writer) | Donald steps only |
| 1 | correctness | after lane 0 Task 6 | task worktree `task/correctness-*` | 2, 3, 6 |
| 2 | authority-tools | after lane 0 Task 6 | task worktree `task/authority-*` | 1, 3, 6 |
| 3 | platforms-security-closure Tasks 1-14 | after lane 0 Task 6 | task worktree `task/platforms-*` | 1, 2, 6 |
| 4 | desktop | after lane 2 `runtime-contracts` lands for Tasks 6+; Tasks 1-5 after lane 0 | task worktree `task/desktop-*` | 1, 3 |
| 5 | models-media-federation | Tasks 1-6 after lane 0; rest after sibling/owner gates | task worktree `task/models-*` | 1, 3 |
| 6 | Sibling requests (`../abi`, `../wdbx`) | now (as written requests) | sibling owners | all |
| 7 | Donald steps (runners, keys, licences, consent, VMs) | now | Donald | all |

Lanes 1-5 may run simultaneously **only** in separate task worktrees with separate `CARGO_TARGET_DIR`s, and integrate into local `main` one at a time per the local-main-readiness checklist. Two lanes touching the same file (`src/claims/registry.rs`, `tasks/completion.json`, `tasks/todo.md`, `check.sh`) serialize their ledger commits: the later lane rebases its ledger hunk.

## Claim promotion rule (binding on every sibling plan)

1. A claim's status changes only by editing its row in `src/claims/registry.rs` (types in `src/claims/registry/types.rs`).
2. The edit names concrete evidence: source paths, test identifiers that exist and pass, and — for anything beyond local behaviour — the native/host/CI/licence/consent receipt under `docs/verification/`.
3. `python3 tools/check_claims_sync.py --write` regenerates every claims region (`AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/claims.md`, `docs/*.md`, `tasks/*.md`); then `python3 tools/check_claims_sync.py` (check mode) and `python3 tools/check_instructions.py` pass; then `./check.sh` passes on the exact bytes.
4. `tasks/completion.json` requirement `status` and `evidence` channels are updated in the same commit; `python3 tools/check_completion.py` passes. A requirement becomes `complete` only when every `required_evidence` channel is `passed` with refs.
5. Never: promote on a green gate alone; promote Proposed→Current skipping Partial when a platform/host channel is unmet; remove a fail-closed refusal path while promoting (`src/claims/tests.rs` refusal tests stay); widen a fixture-bounded row (e.g. `daemon-protocol-v3-exact-model-inference`) implicitly.
6. A promotion commit is reviewed by someone other than its implementer (fresh reviewer subagent minimum; OB-19 for final closure).

---

### Task 1: Freeze the re-baseline as evidence

**Files:**
- Create: `docs/verification/YYYY-MM-DD-completion-rebaseline.md` (execution date)
- Create: `docs/verification/YYYY-MM-DD-completion-rebaseline/requirements.json`

- [ ] **Step 1:** After main-sync Task 6, run `python3 -I -c "import json;d=json.load(open('tasks/completion.json'));print(sum(r['status']!='implemented' for r in d['requirements']))"`. Expected: `32`. If different, stop and update the requirement map above before any other lane starts.
- [ ] **Step 2:** Dump `id, milestone, status, todos, evidence keys` for all 45 requirements to `requirements.json` with the same one-liner pattern (write to the evidence directory).
- [ ] **Step 3:** Run `python3 tools/check_completion.py` and `python3 tools/check_claims_sync.py`; record exit codes and the summary line from `docs/claims.md:5` in the evidence markdown.
- [ ] **Step 4:** Commit (authorized session only): `git add docs/verification/YYYY-MM-DD-completion-rebaseline*` ; `git commit -m "Record the 2026-10-27 completion re-baseline"`.

**Acceptance:** evidence records 32 open requirements, both checkers' exit codes, and the claims summary line.

### Task 2: File the sibling requests (lane 6)

**Files:**
- Create: `docs/verification/YYYY-MM-DD-sibling-requests.md`

- [ ] **Step 1:** Write one section per sibling request with: requester requirement id, exact defect, the Abbey-side test that will consume it, and the pinned-revision update it requires. Requests: (a) anchor corpus schema `pattern`s (`corpus-patterns`); (b) confirm/repair the corpus README command (`corpus-command`); (c) `RunContext::finish` consults elapsed duration (`model-request-validation`); (d) Gemma 4 architecture + compressed-tensor loader (OB-12, `production-model`); (e) bounded trainer + held-out evaluation contract (OB-15, `production-adapters`); (f) `abi-worker` authenticated job protocol (`worker-runtime`); (g) ABI tool-runtime ownership for `owned-tool-host`; (h) GLib advisory upstream path for `dependency-security-remediation`.
- [ ] **Step 2:** **Donald step:** send/approve each request to the `../abi` / `../wdbx` owners. Evidence: the request links or dated acknowledgements appended to the same file.
- [ ] **Step 3:** Commit: `git commit -m "Record sibling change requests for completion lanes"`.

**Acceptance:** eight requests recorded; none of them edits a sibling checkout from Abbey.

### Task 3: Add the missing owner-block row for physical federation

**Files:**
- Create: `docs/verification/YYYY-MM-DD-owner-block-gap.md`

- [ ] **Step 1:** Record that `physical-federation-proof` (`tasks/completion.json`) requires physical distinct hosts, geographic deployment and hardware, and that completion-plan §3 has no OB row for it (OB-11 is VMs only, `docs/completion-plan-2026-10-22.md:663`).
- [ ] **Step 2:** **Donald step:** decide whether to add an OB row (proposed "OB-20: physical multi-host + geographic deployment") to the completion plan or to mark the requirement deferred. Evidence: Donald's decision recorded in this file. Do not edit `docs/completion-plan-2026-10-22.md` without that decision.
- [ ] **Step 3:** Commit the evidence file.

### Task 4: Keep the index true after each lane lands

**Files:**
- Modify: `docs/plans/2026-10-27-abbey-completion-index.md` (this file — the only edit any later task makes to it)

- [ ] **Step 1:** After every integration into local `main`, rerun Task 1 Step 1; update the Status column for any requirement that moved.
- [ ] **Step 2:** Run `python3 tools/check_completion.py`; expected exit 0.
- [ ] **Step 3:** Commit with the integrating lane's ledger commit.

**Acceptance:** the table's status column equals `tasks/completion.json` at every integrated `main` revision.
