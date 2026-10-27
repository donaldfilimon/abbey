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

- `v4_tasks.current_attempt_id` originally had no foreign key. The review
  fix below added a deferred one.
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

## Review findings (adversarial review, fixed in place)

An independent review checked migration 12 in `sqlite3 :memory:` with
`foreign_keys=ON` and found defects. Migration 12 had never been applied
anywhere, so it was fixed in place in a follow-up commit. Each regression test
was added to `v12_tests.rs` first. The three defect tests failed against the
unfixed schema ("statement must be refused"), then passed after the fix.

### Fixed in the schema

- **Profile revisions were blocking.** `v4_profile_revisions` had no triggers,
  so its revision and revocation epoch could go backwards and rows could be
  deleted. A regressed epoch would re-validate approvals that a revocation had
  voided. It now has an advance-only update trigger (same `profile_id`, a higher
  `revision`, and `revocation_epoch` never lower) and a no-delete trigger.
  Covered by `migration_12_profile_revisions_only_advance_and_are_never_deleted`.
- **Automation spec changes were blocking.** The automation trigger only
  refused a lower revision, so `spec_json` and `enabled` could change silently
  under the same revision. Now they change only with a new revision, while
  `cursor_ms` and `next_run_at_ms` may still advance without one. Covered by
  `migration_12_automation_spec_changes_need_a_new_revision`. The original test
  now bumps the revision so that it still exercises the spec/column CHECK.
- **The task's current attempt was unbound.** This could only be added at
  CREATE time. `v4_task_attempts` gained `UNIQUE (task_id, attempt_id)`, and
  `v4_tasks` gained a composite foreign key from
  `(task_id, current_attempt_id)` to it, `DEFERRABLE INITIALLY DEFERRED`. A
  nonexistent attempt or another task's attempt is refused at COMMIT. The test
  helper now admits each task and its attempt in one `BEGIN`/`COMMIT`. Covered
  by `migration_12_task_current_attempt_belongs_to_the_task`.

### New coverage that already passed

- **Attempt lifecycle.** Queued → starting → running → failed with
  `finished_at_ms` succeeds, and a finished attempt is frozen. A retry
  succeeds: a new attempt with ordinal 2, the task back to queued, and
  `current_attempt_id` switched in one transaction. Covered by
  `migration_12_attempt_lifecycle_allows_finish_and_retry_but_freezes_finished_attempts`.
- **Append-only records and manual occurrences.** `v4_admin_operations` is
  append-only. `v4_task_events` has unique sequences, its attempt must belong to
  its task, and it is append-only. A duplicate manual operation id for the same
  automation is refused. Covered by
  `migration_12_append_only_records_and_manual_occurrence_identity`.

### Left store-owned (no schema change)

- **Cross-row state consistency.** The schema doesn't check task state against
  attempt state, occurrence state against task state, or action state against
  attempt state.
- **Terminal regression.** Before an attempt is finished, its state may move
  between non-terminal values in any order. Once `finished_at_ms` is set, the
  row is frozen. Tasks, occurrences and actions have no transition graph in the
  schema, so a terminal task could be moved back to a live state with a new
  revision. The store must refuse that, except for an explicit retry.
- **Value types.** There are no `typeof()` CHECKs, so SQLite's type affinity
  applies. The store binds typed Rust values.
- **`run_id` has no foreign key to `runs(id)`.** This is deliberate. Whether a
  v4 attempt creates a migration-1 `runs` row is decided by the task store
  (Task 3). Until then, `run_id` is bound only to its attempt (unique, 36
  characters) and, through the composite keys, to its events, actions and
  occurrences.
- **One decision per action.** `action_id UNIQUE` on decisions means that
  re-approving an expired or denied action needs a new action id.

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
| Review fix, before: `... --lib migration_12` | 3 passed, 3 failed (the three defect tests, as expected) |
| Review fix, after: `... --lib migration_12` | 6 passed, 0 failed |
| Review fix: `./tools/cargo.sh test --locked -p abbey --lib` | 1328 passed, 0 failed, 0 ignored |
| Review fix: `clippy --locked --all-targets -- -D warnings` | Exit 0, no warnings |
| Review fix: `fmt --package abbey -- --check`, plus rustfmt on `v12_tests.rs` | OK |
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
