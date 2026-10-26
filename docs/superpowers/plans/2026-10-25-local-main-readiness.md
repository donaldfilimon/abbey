# abbey: local main integration readiness

## Goal and authority

User-approved documentation task, 2026-10-25. Prepare owned work for local
`main` only after ownership release, preservation and the repository gates.
Status: **pending owner confirmation and fresh checks**, not merge-ready.
Existing product plans and qualification contracts remain authoritative.

## Observed Git state

Inventory recorded on 2026-10-25; re-read Git and writer state before acting.
Canonical branch: `main`; HEAD: `ec324972a6f194546824298dcc38c38e268622ba`.
Remote comparisons below use existing local tracking refs; no fetch was run.

| Local branch | Pinned tip | Commits only on main / only on branch | Cached upstream state |
| --- | --- | --- | --- |
| `abbey/mystifying-hopper-3cf11b` | `ea2ae07c9020` | 12 / 0 | none  |
| `abbey/new-claude-code-plugin` | `bd00ca3cc343` | 11 / 0 | none  |
| `abbey/new-claude-code-plugin-6c8c33` | `ea2ae07c9020` | 12 / 0 | none  |
| `main` | `ec324972a6f1` | 0 / 0 | origin/main [ahead 2] |
| `task/abbey-live-models` | `ec324972a6f1` | 0 / 0 | none  |
| `task/modernization-abbey-gtk-20261025` | `80187be34582` | 0 / 13 | none  |
| `task/modernization-baseline-20261025` | `dc29a9117d16` | 0 / 1 | none  |
| `task/modernization-consumer-dependencies-20261025` | `8e266b747b47` | 0 / 4 | none  |
| `task/modernization-updater-zip9-20261025` | `6fee7cf24827` | 0 / 9 | none  |
| `task/modernization-worker-lock-20261025` | `dc29a9117d16` | 0 / 1 | none  |

| Existing worktree (relative to DEV_HOME) | Branch | Pinned HEAD | Tracked dirty / untracked files |
| --- | --- | --- | --- |
| `projects/company/legacy/abbey` | `main` | `ec324972a6f1` | 29 / 45 |
| `.worktrees/abbey-live-models/abbey` | `task/abbey-live-models` | `ec324972a6f1` | 29 / 56 |
| `repos/local/the-donald-company/company/legacy/abbey/.claude/worktrees/mystifying-hopper-3cf11b` | `detached` | `ea2ae07c9020` | 2 / 0 |
| `repos/local/the-donald-company/company/legacy/abbey/.claude/worktrees/new-claude-code-plugin-6c8c33` | `abbey/new-claude-code-plugin` | `bd00ca3cc343` | 4 / 0 |
| `worktrees/local/the-donald-company/products-modernization-20261025/cohorts/000-baseline/abbey` | `task/modernization-baseline-20261025` | `dc29a9117d16` | 2 / 0 |
| `worktrees/local/the-donald-company/products-modernization-20261025/tasks/004-worker-lock/abbey` | `task/modernization-worker-lock-20261025` | `dc29a9117d16` | 2 / 0 |
| `worktrees/local/the-donald-company/products-modernization-20261025/tasks/005-consumer-dependencies/abbey` | `task/modernization-consumer-dependencies-20261025` | `8e266b747b47` | 2 / 0 |
| `worktrees/local/the-donald-company/products-modernization-20261025/tasks/007-updater-zip9/abbey` | `task/modernization-updater-zip9-20261025` | `6fee7cf24827` | 2 / 0 |
| `worktrees/local/the-donald-company/products-modernization-20261025/tasks/009-abbey-gtk/abbey` | `task/modernization-abbey-gtk-20261025` | `80187be34582` | 0 / 0 |

Worktree locations describe custody, not runtime dependencies. Clean status does
not prove a writer has released a worktree. Existing ignored assets remain in
their original custody; this task neither inventories their contents nor removes
them. App-managed worktrees retain their application-managed lifecycle.

Canonical dirty-file state (grouped to keep this plan readable):

| State | File or subtree | Files |
| --- | --- | --- |
| tracked dirty | `AGENTS.md` | 1 |
| tracked dirty | `CLAUDE.md` | 1 |
| tracked dirty | `Cargo.lock` | 1 |
| tracked dirty | `README.md` | 1 |
| tracked dirty | `desktop/codegen` | 1 |
| tracked dirty | `desktop/src` | 1 |
| tracked dirty | `docs/architecture.md` | 1 |
| tracked dirty | `docs/claims.md` | 1 |
| tracked dirty | `docs/identity.md` | 1 |
| tracked dirty | `docs/production.md` | 1 |
| tracked dirty | `src/app_core` | 1 |
| tracked dirty | `src/claims` | 3 |
| tracked dirty | `src/claims.rs` | 1 |
| tracked dirty | `src/cli.rs` | 1 |
| tracked dirty | `src/commands.rs` | 1 |
| tracked dirty | `src/config.rs` | 1 |
| tracked dirty | `src/config_snapshot.rs` | 1 |
| tracked dirty | `src/entry.rs` | 1 |
| tracked dirty | `src/lib.rs` | 1 |
| tracked dirty | `tasks/completion.json` | 1 |
| tracked dirty | `tasks/goals.md` | 1 |
| tracked dirty | `tasks/lessons.md` | 1 |
| tracked dirty | `tasks/todo.md` | 1 |
| tracked dirty | `tools/check_claims_sync.py` | 1 |
| tracked dirty | `tools/check_instructions.py` | 1 |
| tracked dirty | `tools/tests` | 1 |
| tracked dirty | `zig/AGENTS.md` | 1 |
| untracked | `docs/agent-guide.md` | 1 |
| untracked | `docs/knowledge.md` | 1 |
| untracked | `docs/training-qualification.md` | 1 |
| untracked | `docs/verification` | 33 |
| untracked | `src/app_core` | 2 |
| untracked | `src/claims` | 1 |
| untracked | `src/knowledge` | 1 |
| untracked | `src/knowledge.rs` | 1 |
| untracked | `tests/app_core_v4_contract.rs` | 1 |
| untracked | `tests/knowledge_cli.rs` | 1 |
| untracked | `tools/check_training_acceptance.py` | 1 |
| untracked | `tools/tests` | 1 |

Counts cover every path from the captured short-status inventory. The separate
local execution inventory retains the complete path list. Re-enumerate paths
with `git status --short --untracked-files=all` before any integration.

This documentation task uses `task/agent-main-plans-20261025`, based on
`ec324972a6f194546824298dcc38c38e268622ba`. Its worktree is separate from the
observed checkouts above. Existing uncommitted guidance was not copied into this
branch or claimed as this task's work; reconcile it with its owner at integration.

## Ownership and repository blockers

Use tools/cargo.sh and declared adjacent ABI/WDBX sources; coordinate their pinned cohorts before integration. Qualify desktop changes with desktop/check.sh and the documented opt-in acceptance only when authorized. Preserve generated claims summaries and retained Rust; do not resume canceled Zig work.

Each existing worktree remains with its current writer until release is
confirmed. The inventory establishes branch custody, not writer identity or
authorization to merge. An unpublished commit is retained local work, even when
its branch is already reachable from main. Do not discard it or its branch label
as cleanup.

## Integration checklist

1. Re-read AGENTS.md, this plan, active product plans, refs and all worktree
  statuses. Confirm each selected writer has released an exact commit/file set.
2. Preserve all refs and dirty/untracked work before integration; preserve
  required ignored assets separately and verify recovery. Keep app worktrees.
3. Classify every branch as already reachable, selected, owner-held or deferred
  with a reason. Review main-only/branch-only commits and file differences;
  do not merge all branches merely because they exist.
4. Use one manual integration branch/worktree for the selected changes and
  an isolated writable build/test environment. Reuse only documented sibling
  layouts. Commit released dirty work on its owning branch after preservation.
5. Integrate selected commits in dependency order, resolve conflicts against
  existing contracts and retain failed checks and provenance pins.
6. Run the documented gates below against the exact candidate revision;
  record command, revision, result and unmet platform/external prerequisites.
7. Recheck main HEAD, canonical dirty state and writer ownership immediately
  before local integration. Advance main only if those guards still hold.
8. Record resulting commit and remaining work. Preserve recovery refs and
  worktrees until their owners release them and ignored-asset custody is verified.

## Required checks and completion evidence

Run from this repository's root, respecting its wrapper/toolchain prerequisites:

```sh
./check.sh
git diff --check
```

For this documentation task, use the repository's scoped documentation/instruction
checks and record actual results separately from the full integration gate.
No product gate is claimed by writing this plan. Readiness requires ownership
release, preserved work and passing applicable checks on the candidate; failures,
skips or unavailable prerequisites remain explicit blockers. Platform, hardware,
model and production qualification retain their existing independent criteria.
Publishing, remote merges, releases and deployment are outside this task.
