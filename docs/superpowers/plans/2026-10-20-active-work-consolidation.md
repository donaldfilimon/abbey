# Abbey Active-Work Consolidation Implementation Plan

> Historical execution checklist. The active [2026-10-22 Completion Program](2026-10-22-abbey-completion.md) supersedes execution order and target-policy choices. These unchecked items are preserved, not asserted complete; distinct obligations are tracked by `retained-plan-reconciliation` in `tasks/completion.json`. Canonical workflow-ledger coverage does not claim to validate every historical plan checkbox.

> For agentic workers: use superpowers:executing-plans, task by task, with one independent final review. Track steps with checkboxes.

**Goal:** Establish reviewed, qualified consolidation on local main while preserving concurrent work.

**Architecture:** Attribute runtime/fixture integration, dependency resolution, and evidence separately. Qualification binds Abbey and its ABI/WDBX path dependencies to exact captured source.

**Tech Stack:** Rust 1.99.1, Cargo, SQLite/WDBX, ratatui, separate Tauri/React desktop workspace.

**Spec:** Design and implementation plan approved in this chat on 2026-10-20.

## Global Constraints

- User's subsequent coordination restrictions take precedence: no commits or pushes; do not edit or revert protected runtime, migration, test, lock, instruction, or rewrite-decision files. Cargo comment belongs to the Grok lane.
- Preserve canceled Zig work, historical evidence, unrelated changes, branches and worktrees.
- Retain Rust 1.99.1, existing approval/replay boundaries, edition isolation, defaults and capability statuses.
- One writer per shared file; producer changes require exact source attribution.
- Run candidate checks outside canonical source to avoid lockfile side effects.

## Review Focus

- Dirty sibling source: bind every consumer result to its exact producer inputs.
- Root/desktop resolution drift: verify each workspace's own lock and full gate.
- Replay audit failure: no actuator call or altered terminal authority.
- Cancellation readiness race: complete PID publication precedes consumption.
- Source drift during gates: compare hashes; retain failed or superseded attempts.

## Task 1: Establish the integration candidate

- [ ] Record all three repository heads, branches, index/working state, relevant untracked source, hashes and modes.
- [ ] Compare active changes with earlier receipts; inventory current manifests and locks.
- [ ] Record ownership and exclusions; do not infer ownership from modified files.
- [ ] Capture an isolated sibling-directory candidate; verify copied bytes against the inventory.

Expected: attributable input inventory and unchanged canonical source.

## Task 2: Review runtime integration

Interfaces: consume the existing ABI DurableLifecycle::refuse_replay contract; add no interface.

- [ ] Review governed runtime replay handling and existing regression assertions.
- [ ] Run focused replay persistence, refusal rollback, terminal-state and actuator boundary tests.
- [ ] Run TUI child cancellation/reaping fixture tests.
- [ ] Record demonstrated defects; any protected-file repair remains with its authorized owner.

Expected: durable fail-closed replay and bounded cancellation, with focused suites passing.

## Task 3: Review dependency candidate

- [ ] Summarize actual manifest/resolved dependency changes, additions and removals.
- [ ] Verify locked root and desktop resolution against exact captured sibling source.
- [ ] Keep any regeneration inside the isolated candidate; canonical locks remain protected.
- [ ] Record Rust 1.99.1 compatibility and any rejected candidate.

Expected: reproducible workspace locks and explained manifest changes.

## Task 4: Qualify combined source

- [ ] Freeze hashes and modes; record exact command environment.
- [ ] Run applicable producer checks and Abbey ./check.sh.
- [ ] Run separate ./desktop/check.sh and applicable isolated installer/accelerator acceptance.
- [ ] Compare fingerprints after checks; preserve failures, counts, skips and limitations.

Expected: passing unchanged source cohort; unavailable evidence stays explicitly unavailable.

## Task 5: Review and report integration

- [ ] Obtain independent final review; record findings and any necessary requalification.
- [ ] Verify what already exists on local main before proposing integration.
- [ ] Write an attributable receipt and source-equivalence result.
- [ ] Report commits already present, gate outcomes, preserved exclusions and blockers.

Integration commits are deferred under the user's explicit no-commit restriction. No push, publication or deployment.

## Execution ruling

At execution start Abbey was clean at 2ee9f7964428314c558bd43d70170eaecddb73c5. Another lane had already integrated the planned runtime, fixture, dependency and documentation changes. Do not recreate those commits. Fresh verification of the captured current composition remains separate from older receipts. ABI and WDBX still have dirty inputs with unverified ownership. Evidence and execution ledger live outside the repository under /Users/donald/.codex/verification/abbey-active-work-consolidation-20261020/.
