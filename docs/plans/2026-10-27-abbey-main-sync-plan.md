# Abbey Main Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring local `main` (`8fdb4ed`) level with `origin/main` (`6901d11`, PR #108) without losing the real local work in the dirty tree, gate the result, classify the 18 worktrees, and leave the push to Donald.

**Architecture:** Snapshot first, then remove only the dirty hunks that are byte-identical to PR #108 (Donald-authorized), merge `origin/main` (not a fast-forward: `main` is ahead by one commit), re-apply the RerankBounds and evidence edits from the snapshot, gate, then inventory worktrees. Nothing is deleted, retired, or pushed without a Donald step.

**Tech Stack:** git, `./check.sh`, `desktop/check.sh`, `tools/check_completion.py`, `tools/check_claims_sync.py`.

**Spec:** `docs/superpowers/plans/2026-10-25-local-main-readiness.md` (integration checklist, ownership rules); `AGENTS.md` "Local main readiness (2026-10-25)".

## Context (verified 2026-10-26)

- `git rev-list --left-right --count main...origin/main` → `1 2`. Local-only commit: `8fdb4ed docs(verification): record 2026-10-26 dependency-security qualification packet` (touches `docs/verification/2026-10-26-dependency-security-qualification.md`, `.../checks-summary.json`, `tasks/completion.json`). Origin-only: `4699a0f` + merge `6901d11` (touch `CLAUDE.md`, `src/ralph.rs`, `src/ralph/{mcp,run,tests}.rs`, `tools/tests/smoke_tui_slash_pty.py`). The two file sets are disjoint, so the merge is expected to be conflict-free.
- Dirty tree (`git status --short`):
  - Byte-identical to `origin/main` (checked with `git show origin/main:<f> | cmp - <f>`): `CLAUDE.md`, `src/ralph.rs`, untracked `src/ralph/run.rs`, `src/ralph/mcp.rs`, `src/ralph/tests.rs`, `tools/tests/smoke_tui_slash_pty.py`.
  - Real local work: `src/tui/predict.rs` (adds `RerankBounds`, `LLM_TERMINATE_GRACE`, threads `bounds` through `llm_hint_owned`), `src/tui/prediction_owner.rs`, `src/tui/tests/prediction_producer.rs`, `tasks/completion.json` (adds `local_gate_execution` evidence to `dependency-security-remediation`), `docs/verification/2026-10-26-dependency-security-qualification.md`, `docs/verification/2026-10-26-dependency-security-qualification/checks-summary.json`.
- The untracked `src/ralph/*.rs` files will make `git merge` refuse ("untracked working tree files would be overwritten") until they are removed — hence the Donald step.
- Shared-learning note (verified lesson, 2026-10-26): a root gate can exit 0 while another session edits a test mid-run; record before/after source inventories around every gate.

## Global Constraints

Inherit the block in `docs/plans/2026-10-27-abbey-completion-index.md` § Global Constraints. Additionally:
- Never `git reset --hard`, `git checkout -- <file>`, `git clean`, `git stash drop`, `git worktree remove/prune`, `git branch -D`, or `git push` without the named Donald step.
- Snapshots live under `.work/main-sync-YYYYMMDD/` (ignored) and are never deleted by this plan.

## Review Focus

- A re-applied hunk silently dropped (e.g. `RerankBounds` lands but `prediction_owner.rs` caller change does not) → the build or the prediction tests fail; Task 5 runs them.
- `tasks/completion.json` re-applied on a base where origin changed the same region → none today (origin does not touch it), but Task 4 re-checks with `git diff`.
- A worktree classified "retire" while it holds dirty or unique work → Task 8 requires 0 dirty and 0 unique-vs-origin before even proposing retirement.
- A gate run whose source changed mid-run reported as a pass → Task 6 records before/after manifests.
- `integrate-20261026-gate` is a detached HEAD holding a unique commit; detached commits are lost on prune → never prune before Donald releases it.

## File map

- Create: `.work/main-sync-YYYYMMDD/` (`dirty.patch`, `untracked.tar`, `status-before.txt`, `manifest-before.json`, `rerank.patch`, `evidence.patch`)
- Create: `docs/verification/YYYY-MM-DD-main-sync.md`
- Create: `docs/verification/YYYY-MM-DD-worktree-inventory.md`
- Modify (merge result only): every file in the origin and local sets listed above.
- Modify (follow-up task): `docs/superpowers/plans/2026-10-25-local-main-readiness.md` § Observed Git state — **only via Task 9, after Donald approves editing that plan.**

---

### Task 1: Snapshot the dirty tree

**Files:** Create `.work/main-sync-YYYYMMDD/*`

- [ ] **Step 1:** `mkdir -p .work/main-sync-YYYYMMDD && git status --porcelain=v1 --untracked-files=all > .work/main-sync-YYYYMMDD/status-before.txt`. Expected content: 9 ` M` lines and 3 `??` lines exactly as listed in Context. If it differs, stop and re-derive the Context lists.
- [ ] **Step 2:** `git diff --binary > .work/main-sync-YYYYMMDD/dirty.patch` and `tar -cf .work/main-sync-YYYYMMDD/untracked.tar src/ralph/run.rs src/ralph/mcp.rs src/ralph/tests.rs`.
- [ ] **Step 3:** Split the real work: `git diff --binary -- src/tui/predict.rs src/tui/prediction_owner.rs src/tui/tests/prediction_producer.rs > .work/main-sync-YYYYMMDD/rerank.patch`; `git diff --binary -- tasks/completion.json docs/verification/2026-10-26-dependency-security-qualification.md docs/verification/2026-10-26-dependency-security-qualification/checks-summary.json > .work/main-sync-YYYYMMDD/evidence.patch`.
- [ ] **Step 4:** Record `shasum -a 256` of every snapshot file and of each dirty file's current bytes into `manifest-before.json`.
- [ ] **Step 5:** Verify recovery: in a scratch clone (`git clone --shared . .work/main-sync-YYYYMMDD/verify && cd` there), `git checkout 8fdb4ed && git apply --check ../dirty.patch` → exits 0. Delete nothing.

**Acceptance:** `dirty.patch` applies cleanly to `8fdb4ed` in the scratch clone; hashes recorded.

### Task 2: Prove the PR #108 hunks are byte-identical (evidence for the Donald step)

**Files:** Create `docs/verification/YYYY-MM-DD-main-sync.md`

- [ ] **Step 1:** For each of `CLAUDE.md src/ralph.rs src/ralph/run.rs src/ralph/mcp.rs src/ralph/tests.rs tools/tests/smoke_tui_slash_pty.py`, run `git show origin/main:$f | cmp - "$f" && echo IDENTICAL $f`. Expected: six `IDENTICAL` lines.
- [ ] **Step 2:** For `src/tui/predict.rs tasks/completion.json`, the same command must report a difference (these are not origin copies).
- [ ] **Step 3:** Write both outputs and the `origin/main` hash into the evidence file.

### Task 3: **Donald step** — authorise discarding the six identical files

- [ ] **Step 1:** Present Task 2's evidence and ask Donald to authorise discarding the working-tree copies of the six files (they are recoverable from `origin/main` and from `.work/main-sync-YYYYMMDD/`). Evidence: Donald's approval, dated, appended to `docs/verification/YYYY-MM-DD-main-sync.md`.
- [ ] **Step 2:** On approval only: `git checkout -- CLAUDE.md src/ralph.rs tools/tests/smoke_tui_slash_pty.py` and `rm src/ralph/run.rs src/ralph/mcp.rs src/ralph/tests.rs`.
- [ ] **Step 3:** `git status --short` now lists only the six real-work files.

### Task 4: Merge `origin/main`

- [ ] **Step 1:** Park the real work: `git stash push -m main-sync-real-work -- src/tui/predict.rs src/tui/prediction_owner.rs src/tui/tests/prediction_producer.rs tasks/completion.json docs/verification/2026-10-26-dependency-security-qualification.md docs/verification/2026-10-26-dependency-security-qualification/checks-summary.json`. (The `.work` patches remain the authoritative copy; the stash is a convenience and is not dropped by this plan.)
- [ ] **Step 2:** `git fetch origin` (read-only) and confirm `git rev-parse origin/main` is still `6901d11…`; if it moved, re-run Task 2 against the new tip.
- [ ] **Step 3:** `git merge --no-ff origin/main -m "Merge origin/main (PR #108) into local main"` (add the attribution trailer). Expected: no conflicts. If conflicts appear, stop and report — do not resolve by choosing a side.
- [ ] **Step 4:** `git log --oneline -3` shows the merge on top of `8fdb4ed`; `git rev-list --left-right --count main...origin/main` → `2 0`.

### Task 5: Re-apply RerankBounds and the evidence edits

**Files:** Modify `src/tui/predict.rs`, `src/tui/prediction_owner.rs`, `src/tui/tests/prediction_producer.rs`, `tasks/completion.json`, `docs/verification/2026-10-26-dependency-security-qualification.md`, `.../checks-summary.json`

- [ ] **Step 1:** `git apply --3way .work/main-sync-YYYYMMDD/rerank.patch` then `git apply --3way .work/main-sync-YYYYMMDD/evidence.patch`. Expected: both apply cleanly.
- [ ] **Step 2:** Byte-compare each re-applied file against `manifest-before.json`; every hash must match.
- [ ] **Step 3:** `./tools/cargo.sh test --locked --lib tui::tests::prediction_producer` → PASS (the focused suite the 2026-10-26 receipt ran).
- [ ] **Step 4:** `python3 tools/check_completion.py` → exit 0.
- [ ] **Step 5:** Commit RerankBounds separately from the evidence: `git add src/tui/predict.rs src/tui/prediction_owner.rs src/tui/tests/prediction_producer.rs && git commit -m "Pass rerank bounds per call instead of holding them globally"`; then `git add tasks/completion.json docs/verification/2026-10-26-dependency-security-qualification* && git commit -m "Record local gate execution for dependency-security remediation"`. (Authorized session only.)

### Task 6: Gate the merged tree

**Files:** Append to `docs/verification/YYYY-MM-DD-main-sync.md`

- [ ] **Step 1:** Check free disk space (`df -h .`) — a full disk fails the Tauri link with an error that reads like a code fault.
- [ ] **Step 2:** Record a source manifest (`git rev-parse HEAD`, `git status --porcelain`, `git -C ../abi rev-parse HEAD`, `git -C ../wdbx rev-parse HEAD`).
- [ ] **Step 3:** `./check.sh 2>&1 | tee .work/main-sync-YYYYMMDD/root-gate.log`. Expected last line `check.sh: OK`.
- [ ] **Step 4:** `./desktop/check.sh 2>&1 | tee .work/main-sync-YYYYMMDD/desktop-gate.log` (serially; both take the per-checkout gate lock). Expected `desktop/check.sh: OK`.
- [ ] **Step 5:** Re-record the manifest; it must equal Step 2's. If not, the run is not a frozen-source pass — say so in the evidence.
- [ ] **Step 6:** `python3 tools/check_claims_sync.py` → exit 0 (counts unchanged by this sync).
- [ ] **Step 7:** Commit the evidence file.

**Acceptance:** both gates OK on an unchanged manifest; or each failure recorded with its log and the plan stops here.

### Task 7: Inventory the worktrees

**Files:** Create `docs/verification/YYYY-MM-DD-worktree-inventory.md`

- [ ] **Step 1:** For each entry of `git worktree list --porcelain`, record: path (note the `/Users/donald/dev/...` vs `/Volumes/Extreme SSD/...` alias — same SSD), branch or detached SHA, `git -C <wt> status --porcelain | wc -l`, `git rev-list --count origin/main..<ref>`, `git cherry origin/main <ref>`.
- [ ] **Step 2:** Classify each as one of: **reachable** (0 unique, 0 dirty), **owner-held** (dirty > 0 or an active session), **unique-pending-review**, **already-landed-by-other-means**. Observed 2026-10-26 (re-verify):

| Worktree | Ref | Dirty | Unique vs main | Proposed class |
|---|---|---|---|---|
| `.claude/worktrees/mystifying-hopper-3cf11b` | detached `ea2ae07` | 2 | 0 | owner-held (app-managed) |
| `.claude/worktrees/new-claude-code-plugin-6c8c33` | `abbey/new-claude-code-plugin` | 4 | 0 | owner-held (app-managed) |
| `abi-c07-20261025/abbey` | `task/finish-abi-c07-20261025` | 0 | 0 | reachable |
| `agent-main-plans-20261025` | `task/agent-main-plans-20261025` | 0 | 0 | reachable |
| `cohorts/000-baseline/abbey` | `task/modernization-baseline-20261025` | 2 | 0 | owner-held |
| `cohorts/001-reviewed/abbey` | `task/modernization-reviewed-cohort-20261025` | 0 | 0 | reachable |
| `tasks/004-worker-lock/abbey` | `task/modernization-worker-lock-20261025` | 2 | 0 | owner-held |
| `tasks/005-consumer-dependencies/abbey` | `task/modernization-consumer-dependencies-20261025` | 2 | 0 | owner-held |
| `tasks/007-updater-zip9/abbey` | `task/modernization-updater-zip9-20261025` | 2 | 0 | owner-held |
| `tasks/009-abbey-gtk/abbey` | `task/modernization-abbey-gtk-20261025` | 0 | 0 | reachable |
| `tasks/013-abbey-schema3/abbey` | `task/modernization-abbey-schema3-20261025` | 42 | 0 | owner-held (active writer) |
| `tasks/014-bot-source-import/abbey` | `task/014-bot-source-import` | 0 | 0 | reachable |
| `rust-vendor-harness-20261025/abbey` | `task/rust-vendor-harness-20261025` | 0 | 0 | reachable |
| `.work/windows-foundation/abbey` | `abbey/windows-foundation` | 0 | 1 (`5be3d25`) | unique-pending-review → platforms plan Task 1 |
| `abbey-live-models` | `task/abbey-live-models` | 52 | 0 | owner-held (active writer) |
| `integrate-20261026-gate` | detached `97d29e3` | 0 | 1 | already-landed-by-other-means candidate: its change (drop `lookup("lora")` from `approved_expansion_is_proposed`) is already absent on main (`src/claims/tests.rs:419-423`) but `git cherry` reports it as not patch-equivalent |
| `integrate-all-20261025` | `task/integrate-all-20261025` | 0 | 0 | reachable |

- [ ] **Step 3:** For `97d29e3`: `git diff 97d29e3^ 97d29e3 -- src/claims/tests.rs` vs `git log -S'lookup("lora")' --oneline -- src/claims/tests.rs` on main; record which main commit removed it.
- [ ] **Step 4:** Commit the inventory.

### Task 8: Retire only after owner release

- [ ] **Step 1:** **Donald step:** for each worktree classed **reachable** or **already-landed-by-other-means**, Donald confirms its writer has released it and that no ignored assets need custody. Evidence: per-worktree release line in the inventory file.
- [ ] **Step 2:** For released worktrees only: `git worktree remove <path>` (never `--force`), then `git branch -d <branch>` (lowercase `-d`; refuses unmerged branches). For `97d29e3` create a recovery ref first: `git tag recovery/integrate-20261026-gate 97d29e3`.
- [ ] **Step 3:** App-managed `.claude/worktrees/*` are never removed by this plan (lifecycle owned by the app).
- [ ] **Step 4:** Re-run Task 7 Step 1 and append the post-state.

### Task 9: Refresh the readiness-plan inventory (follow-up task)

**Files:** Modify `docs/superpowers/plans/2026-10-25-local-main-readiness.md` § Observed Git state

- [ ] **Step 1:** **Donald step:** approve editing the 2026-10-25 readiness plan (it is an approved plan; editing it changes a Donald-owned document).
- [ ] **Step 2:** Replace its observed-state table with Task 7/8's post-state, keeping the prior table under a dated "Superseded observation" heading (do not delete history).
- [ ] **Step 3:** That file is under `docs/superpowers/plans/`, which `tools/check_completion.py:121` scans: if the edit adds any `- [ ]`, register it in `tasks/completion-historical.json` in the same commit. Run `python3 tools/check_completion.py` → exit 0.
- [ ] **Step 4:** Commit: `git commit -m "Refresh local main readiness inventory after PR #108 sync"`.

### Task 10: **Donald step** — push

- [ ] **Step 1:** Present: `git log --oneline origin/main..main`, both gate logs, the worktree inventory. Donald decides whether and when to `git push origin main`. Evidence: the pushed SHA and the resulting CI run id (or "no runner available" — see platforms plan) appended to `docs/verification/YYYY-MM-DD-main-sync.md`.
- [ ] **Step 2:** If CI cannot run (runner offline since 2026-10-19), record that explicitly; a push is not CI evidence.
