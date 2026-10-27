# Abbey authority plan, Tasks 1 and 2: verification (2026-10-27)

Plan: `docs/plans/2026-10-27-abbey-authority-tools-plan.md`, Task 1 (remove the
dead v4 contract family) and Task 2 (decision gate for the v4 admin schema).
Work happened in task worktree `abbey-authority-20261027` on branch
`task/abbey-authority-20261027`. It was fast-forwarded to local `main` `0ad2fef`
before any change. Nothing was merged into `main` or pushed, and no claim was
promoted.

## Commits

| Commit | Subject |
| --- | --- |
| `c6a53db` | Remove the uncompiled duplicate v4 contract family |
| `2126d39` | Add migration 12 for v4 admin authority constraints |

Code verified at `git rev-parse HEAD` = `2126d39142e780b8b8334bf8c7fdbf5a87b8f4ea`.
This document is committed on top of that revision.

## Task 1

`grep -rn "v4_types" src desktop tools tests` returned no references, and
`src/app_core/mod.rs` doesn't declare `v4_types`. Every type in the deleted file
has a compiled counterpart. Four of them have different names:
`V4TurnReceipt` → `V4TurnSubmission`, `V4ContentKind` → `V4ContentPayload`,
`V4ContentEventPage` → `V4ContentPage` and `V4EventQuery` → `V4ContentQuery`.
A grep found no reference to them from the task, automation or action stores.

## Task 2 decision: Branch M (gaps found)

Migration 10 doesn't carry the v4 contract.

| v4 type | Migration 10 gap | Migration 12 table |
| --- | --- | --- |
| `V4Task` | Missing `revision`, `sequence`, `workspace_id`, `profile_revision`, `execution_kind` and `current_attempt`. Its states (`completed`, no `starting`/`paused`/`cancel_requested`) differ from `V4TaskState`. | `v4_tasks`: all 15 fields, with `current_attempt` as `current_attempt_id` |
| `V4TaskAttempt` | No table | `v4_task_attempts`: all 6 fields, `RunState` vocabulary, terminal state ⇔ `finished_at_ms` |
| `V4TaskEvent` | No table | `v4_task_events` (`kind` as JSON) |
| `V4Automation` | Missing `revision`, `sequence`, `workspace_id`, `profile_revision` and timestamps. The schedule shape isn't the tagged `V4ScheduleKind`. | `v4_automations`: `spec` as `spec_json`, with `automation_id` and `enabled` checked against their columns |
| `V4AutomationOccurrence` | No `revision`, manual source, `attempt_id` or `run_id`. Its states differ from `V4OccurrenceState`. `task_id` is nullable and not unique. Uniqueness is (automation, instant) without revision. | `v4_automation_occurrences`: all 8 fields |
| `V4Action` | No table | `v4_actions`: all 13 fields |
| `V4ActionDecision` / `Receipt` | Has no `task_id`, `attempt_id`, `expected_revision`, `preview_digest` or `applied_revision`. | `v4_action_decisions` |

### Corrections made to the draft SQL

- **Occurrences.** Added `attempt_id`, `run_id` and `state`, using the 7-value
  `V4OccurrenceState` vocabulary. The unconditional append-only update trigger
  would have blocked every state change. It is now an identity-immutable
  trigger: state and the current attempt/run may change, and nothing can be
  deleted.
- **Attempts.** Added the CHECK that a terminal state and `finished_at_ms` appear
  together.
- **Binding.** Events, actions and occurrences now use a composite foreign key
  `(task_id, attempt_id, run_id)` into attempts. Decisions use a composite
  foreign key `(action_id, task_id, attempt_id, preview_digest)` into actions.
  Previously the decision's `task_id` and `attempt_id` had no bound or link.
- **Immutability.** The decision trigger now also freezes `task_id`,
  `attempt_id` and `created_at_ms`. The action trigger also freezes
  `resource_label`. Actions and automations gained no-delete triggers.
- **Digests.** Every digest column now requires lowercase hex
  (`NOT GLOB '*[^0-9a-f]*'`), matching `digest()`.

Uniqueness required by the plan:

- One occurrence per automation revision and scheduled UTC instant:
  `UNIQUE (automation_id, revision, scheduled_at_ms)`.
- Manual occurrences are unique per automation:
  `UNIQUE (automation_id, manual_operation_id)`.
- One task per occurrence: `task_id UNIQUE`.
- One decision per action: `action_id UNIQUE`, `decision_id` primary key, and
  `kind` immutable.

### Constraints left to the store

- `v4_tasks.current_attempt_id` has no foreign key, because attempts reference
  their task (a circular dependency).
- The cross-table invariants in `V4Task::validate` are left to the store: task
  state against attempt state, and admission time within the task's created and
  updated times.
- Automation profile existence and schedule validity are enforced by the store
  (Task 4) and by the `v4_tasks` foreign keys when an occurrence admits its task.
- SQLite `length()` counts characters, while Rust counts bytes. The text bounds
  are therefore no stricter than the contract. Timestamps have no
  `MAX_SAFE_INTEGER` upper bound in SQL.
- Production enforces foreign keys: `src/runtime/store.rs` sets
  `PRAGMA foreign_keys=ON`. The test sets the pragma explicitly.

## Test-driven sequence

1. `v12_tests.rs` was wired into `src/runtime/migrations.rs` before the
   migration. Running `./tools/cargo.sh test --locked -p abbey --lib migration_12`
   failed as expected (`left: 11, right: 12`).
2. Added `MIGRATION_12`, the `MIGRATIONS` entry and
   `CURRENT_SCHEMA_VERSION = 12`. The same command passed (1 passed).

The test asserts that:

- applied rows survive the migration;
- the task snapshot is immutable and state changes with a new revision;
- a terminal attempt without `finished_at_ms` is refused;
- `spec_json` and its columns must agree;
- a duplicate (revision, instant) occurrence is refused;
- a task can't serve two occurrences;
- an occurrence can't point at another task's attempt;
- an occurrence's state may change but its identity can't;
- a decision with the wrong preview digest is refused;
- a second decision for the same action is refused;
- a decision's `kind` is immutable;
- an action can't be deleted.

Each refusal is matched by its error text.

`grep -rnE "schema[^0-9]{0,24}\b1[12]\b" src tests desktop/src tools` (excluding migrations) found no other hard-coded schema version. Other schema-version uses go through the constant (`migrations.rs` tests,
`src/runtime/store/tests.rs`), so they needed no change.
`docs/agent-guide.md` now names `migration_12_*.sql`.

## Commands and results

All commands ran with `CARGO_TARGET_DIR=$HOME/.cache/cd23-abbey-authority-target`,
the pinned 1.99.1 toolchain via `./tools/cargo.sh`, and `--locked`.

| Command | Result |
| --- | --- |
| `./tools/cargo.sh test --locked -p abbey --lib migration_12` | Before wiring: FAIL, as expected. After: 1 passed |
| `./tools/cargo.sh test --locked -p abbey --lib runtime::` | 201 passed, 0 failed |
| `./tools/cargo.sh test --locked -p abbey --lib` | 1323 passed, 0 failed, 0 ignored |
| `./tools/cargo.sh test --locked -p abbey --test app_core_v4_contract --test daemon_runtime` | 4 passed; 1 passed |
| `./tools/cargo.sh clippy --locked --all-targets -- -D warnings` | Exit 0, no warnings |
| `./tools/cargo.sh fmt --package abbey -- --check` | OK. The included `v12_tests.rs` was also checked with rustfmt 1.99.1, edition 2024. |
| `./tools/cargo.sh run --locked --quiet -p abbey-desktop-codegen -- --check` | 5 generated files up to date |
| `python3 tools/check_instructions.py` | OK |
| `python3 tools/check_claims_sync.py` | OK (60 claims, digest unchanged) |

These builds used the worktree's `../abi` and `../wdbx` symlinks, which point to
the canonical sibling checkouts:

- ABI `dbb2b38c18585ecf25685046c9aca463671aa6d4` (clean)
- WDBX `8e00bbb7cd544d35b800054dca8e79f29ab11819` (2 dirty paths from another
  session)

## Not run, or deferred

- **`tools/ci/verify-pinned-siblings.sh`** was not run.
  `--print-pins` reports `ABI_REVISION=7b9519b6cfb25cc9bc9db2982e0a901318ba039b`
  and `WDBX_REVISION=5182bc3a60300e1dd152611025e774c4b906638b`. Those pins
  already fail on `main` `0ad2fef`, independent of this work: ABI `7b9519b` lacks
  `abi_agent_host::local_approval`, which `src/runtime/store/governed/local.rs`
  imports (E0432). The pins are being replaced on a separate branch. The full
  pinned gate is deferred to the integration run after that fix lands.
- **`./check.sh`** was not run, because live `../wdbx` is dirty from another
  session.
- **Desktop gate, feature cohorts and integration suites.** None of these was
  run: the desktop gate, the `wdbx`, `personal-edition` and `accel` cohorts, and
  the integration test files other than the two listed above.
- **CLAUDE.md.** Its module map still says "schema v10" and "all ten", which was
  already stale at `main`. It wasn't edited because the canonical checkout has
  uncommitted `CLAUDE.md` edits that a branch change would collide with.

No migration was applied to a live `runtime.sqlite`. These are local schema
tests, not runtime, daemon or platform qualification.
