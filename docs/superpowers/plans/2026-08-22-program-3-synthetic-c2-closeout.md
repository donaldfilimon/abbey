# Program 3 Synthetic C2 Closeout Implementation Plan

> **Historical and superseded progress record (2026-08-27).** This document
> preserves the original pre-execution SDD checklist and its unchecked boxes;
> it is not the current backlog and must not be replayed to infer unfinished
> source work. The closed synthetic C2 slice is represented by the current
> `program-3-guild-intelligence-synthetic` claim and the canonical
> `tasks/goals.md` / `tasks/todo.md` ledgers. Authorized non-synthetic capture,
> live Discord validation, persistence, approval, deployment, and effects
> remain separate acceptance layers. Historical steps below are intentionally
> not rewritten as though they were a live progress tracker.

> **Original execution instruction, retained for provenance:** "For agentic
> workers: REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan
> task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking."

**Goal:** Close every synthetic Program 3 C1/C2 acceptance gap named by the approved focused specification while preserving a pure, non-executable, metadata-only boundary.

**Architecture:** Keep `RecordingGuildSource` as the only public entry point and exercise it through real closed JSON recordings. Split plan construction into a focused child module so preconditions and postconditions can join the public data contract without pushing `guild_intelligence.rs` toward Abbey's file-size ceiling. Promote only the closed synthetic replay evidence from C1 to C2; authorized Discord capture, live validation, persistence, approval, and effects remain separate unfinished programs.

**Tech Stack:** Rust nightly-2026-08-19, edition 2024, Serde, SHA-256, existing Abbey app-core integration tests, Python static boundary guard.

**Spec:** `../abi/docs/superpowers/specs/2026-08-22-spec-discord-guild-intelligence-read-only.md`

## Global Constraints

- Accept only recordings with `synthetic: true`; owner or administrator is a fixture assertion, never live authorization.
- Preserve `SCHEMA_VERSION = 1`, `MAX_OBJECTS = 2_048`, and `MAX_REF_LEN = 96` unless the approved spec is revised first.
- Unknown JSON fields, dangling references, duplicate targets, unrecognized overwrite kinds, and incomplete required coverage fail closed.
- Permission calculation remains deterministic: owner and `ADMINISTRATOR` override; otherwise guild base union, everyone overwrite, aggregate role overwrites, then member overwrite.
- Plans remain data only. No Serenity, Discord I/O, network, process, filesystem, WDBX, durable state, commands, tools, approval, execution, or write behavior may enter the Program 3 module.
- Every substantive alternative requires explicit selection and carries desired state, exact observed preconditions, expected postconditions, and rollback preview. `do-nothing` carries empty vectors.
- Plan digests bind the observation digest, option id, preconditions, desired states, postconditions, and rollback preview.
- The redacted status may promote only to `C2ClosedSyntheticReplay`; it must continue to report `read_only: true` and `fresh: false` and reveal no opaque reference.
- Use the command-local PATH prefix `/opt/homebrew/opt/rustup/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/opt/homebrew/sbin` for Rust gates so Swiftly's unselected `cc` shim cannot intercept linking.
- Do not commit, push, merge, deploy, or open a Discord session in this execution. Abbey's project instruction requires a separate explicit request before commits.

---

### Task 1: Close fixture authorization and hard-limit evidence

**Files:**
- Modify: `tests/guild_intelligence_replay.rs`

**Interfaces:**
- Consumes: `RecordingGuildSource::from_json(&str) -> Result<RecordingGuildSource, GuildIntelligenceError>` and `RecordingGuildSource::replay(Option<&str>) -> Result<ReplayArtifact, GuildIntelligenceError>`.
- Produces: acceptance evidence for administrator authority, exact reference/object boundaries, and rejection immediately beyond each boundary.

- [ ] **Step 1: Add an explicit administrator acceptance test**

Add `synthetic_administrator_authority_is_accepted_without_claiming_ownership`. Parse `FIXTURE` into `serde_json::Value`, change `operator_authority` to `"administrator"`, change `operator_ref` to `"synthetic-admin-a"`, and keep `owner_ref` unchanged. Assert replay succeeds, `status.authorization_basis == "synthetic_fixture_claim"`, `status.read_only`, and `!status.fresh`. This test catches accidentally treating administrator as lower authority or conflating administrator with owner identity.

- [ ] **Step 2: Add exact opaque-reference boundary vectors**

Add `opaque_reference_limits_accept_the_boundary_and_reject_the_next_byte`. Replace only `guild_ref` with `"g".repeat(96)` and assert `from_json` succeeds; then use `"g".repeat(97)` and assert `InvalidRecording`. Add a third vector containing `"guild\ncontrol"` and assert `InvalidRecording`. Expected values are literal consequences of the approved 96-byte bound, not values read back from production constants.

- [ ] **Step 3: Add exact object and overwrite boundary vectors**

Add `collection_limits_accept_2048_and_reject_2049`. Build a recording with 2,048 uniquely referenced active threads sharing the existing valid parent and assert `from_json` succeeds; append the 2,049th and assert `InvalidRecording`. Separately replace the role set with exactly 2,048 roles: one position-zero everyone role and 2,047 non-everyone roles. Put 2,047 unique role overwrites plus the one recorded-bot member overwrite on a single channel and assert success. Append an everyone overwrite as the 2,049th target and assert `InvalidRecording`; this isolates the overwrite-total boundary without also exceeding the role bound. Keep every generated reference under 96 bytes.

- [ ] **Step 4: Run focused evidence tests**

Run:

```sh
env PATH=/opt/homebrew/opt/rustup/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/opt/homebrew/sbin \
  cargo test --locked --test guild_intelligence_replay
```

Expected: all replay tests pass. These are characterization/acceptance vectors over already implemented validation, so a passing first run is permitted; each test names the concrete production mutation it catches.

- [ ] **Step 5: Inspect task scope without committing**

Run `git diff --check` and `git diff -- tests/guild_intelligence_replay.rs`. Confirm no production file changed and do not commit.

---

### Task 2: Add exact plan preconditions and postconditions test-first

**Files:**
- Create: `src/app_core/guild_intelligence/plan.rs`
- Modify: `src/app_core/guild_intelligence.rs`
- Modify: `src/app_core/mod.rs`
- Modify: `tests/guild_intelligence_replay.rs`

**Interfaces:**
- Consumes: normalized `GuildRecording`, `Alternative`, `DesiredPermissionState`, and the existing `digest<T: Serialize>` helper.
- Produces: public `PermissionCondition` and an extended `DesiredStatePlan { preconditions, desired_states, postconditions, rollback_preview }`; field names and vector order are stable serialized contract surface.

- [ ] **Step 1: Write the failing public-contract assertions**

In `synthetic_replay_is_closed_deterministic_and_non_executable`, assert the least-privilege plan has one precondition and one postcondition and that both use literal values from the fixture:

```rust
assert_eq!(plan.preconditions.len(), 1);
assert_eq!(plan.preconditions[0].scope_ref, "synthetic-guild-a");
assert_eq!(plan.preconditions[0].subject_ref, "role-everyone");
assert_eq!(plan.preconditions[0].allow, 3_072);
assert_eq!(plan.preconditions[0].deny, 0);
assert_eq!(plan.postconditions.len(), 1);
assert_eq!(plan.postconditions[0].allow, 1_024);
assert_eq!(plan.postconditions[0].deny, 0);
assert_eq!(plan.postconditions[0].source_observation_digest, plan.source_observation_digest);
```

For `focused-overwrite`, assert both condition vectors have two elements. In normalized channel order, `channel-public` has an observed precondition of allow `0`, deny `0`, while `channel-staff` has allow `0`, deny `1_024`; both postconditions deny `2_048` in addition to the respective prior deny. For `do-nothing`, assert preconditions, desired states, postconditions, and rollback preview are all empty. These assertions catch omitted before-state binding, a postcondition derived from rollback rather than desired state, and silent executable semantics on do-nothing.

- [ ] **Step 2: Run the test and verify RED**

Run the focused test command from Task 1. Expected: compile failure because `DesiredStatePlan` has no `preconditions` or `postconditions` fields. A different failure must be corrected before production code changes.

- [ ] **Step 3: Create the focused plan module**

Create `src/app_core/guild_intelligence/plan.rs` with this public condition type:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionCondition {
    pub source_observation_digest: String,
    pub scope_ref: String,
    pub subject_ref: String,
    pub allow: u64,
    pub deny: u64,
}
```

Move `DesiredPermissionState`, `RollbackPermissionState`, `DesiredStatePlan`, and `make_plan` from the parent file into the child module. Extend `DesiredStatePlan` with `preconditions: Vec<PermissionCondition>` immediately before `desired_states` and `postconditions: Vec<PermissionCondition>` immediately after it. Re-export the four public types from the parent module so existing `abbey::app_core::*` paths stay source-compatible.

- [ ] **Step 4: Construct conditions from the observed and desired states**

For least privilege, create one precondition from the observed everyone role, one desired state with `SEND_MESSAGES` removed, one postcondition identical to that desired state, and one rollback entry identical to the observed state. For focused overwrite, create one precondition per normalized channel from its observed everyone overwrite or zero/zero when absent; create the postcondition from the matching desired state. Every condition carries the exact `source_observation_digest`. `do-nothing` returns four empty vectors.

- [ ] **Step 5: Bind conditions into the plan digest**

Compute `plan_digest` from this exact tuple order:

```rust
(
    observation_digest,
    id,
    &preconditions,
    &desired_states,
    &postconditions,
    &rollback_preview,
)
```

This catches any later change that approves or compares a digest omitting the before/after contract.

- [ ] **Step 6: Verify GREEN and refactor**

Run the focused integration test. Expected: pass. Then run `cargo fmt --all`, rerun the focused test, and check `wc -l src/app_core/guild_intelligence.rs src/app_core/guild_intelligence/plan.rs`; the parent must remain below 800 lines and the child below 400.

- [ ] **Step 7: Inspect task scope without committing**

Run `git diff --check` and inspect the four-file diff. Confirm there is no method named `execute`, `apply`, `approve`, `dispatch`, `write`, or `persist`, and do not commit.

---

### Task 3: Complete permission and unknown-target vectors

**Files:**
- Create: `tests/guild_intelligence_permissions.rs`

**Interfaces:**
- Consumes: the public `RecordingGuildSource` replay boundary and `tests/fixtures/guild_intelligence/community-risk.json`.
- Produces: independent literal permission vectors for owner/admin override, base-role union, overwrite precedence, and unknown-target refusal.

- [ ] **Step 1: Add owner and administrator override vectors**

Use real JSON recordings, not mocks. In one case make `bot_self.ref_id` equal `owner_ref`; in another add the `ADMINISTRATOR` bit (`8`) to an assigned bot role. Assert every effective channel permission equals `u64::MAX`. These tests catch applying overwrites after an override or requiring both owner and administrator.

- [ ] **Step 2: Add base-union and overwrite-order vectors**

Construct a channel where everyone grants `VIEW_CHANNEL` (`1_024`), two assigned roles contribute `SEND_MESSAGES` (`2_048`) and another literal bit (`4_096`), the everyone overwrite denies `VIEW_CHANNEL`, aggregate role overwrites deny `SEND_MESSAGES` and allow `VIEW_CHANNEL`, and the member overwrite finally denies `VIEW_CHANNEL`. Assert the final literal permission mask retains `4_096` and loses both `1_024` and `2_048`. This test catches each realistic reordering of Discord's evaluation sequence.

- [ ] **Step 3: Add unknown-target refusal vectors**

Create separate cases for a missing role target, a member target other than the recorded bot, an `unrecognized` target kind, and two semantically duplicate everyone targets. Each must return `InvalidRecording` before replay. The semantic-duplicate test is the RED case: current validation keys `Everyone { everyone_ref }` and `Role { everyone_ref }` separately. After observing that expected failure, add the minimal validation rule that maps both representations to one canonical everyone target before duplicate detection.

- [ ] **Step 4: Run the permission suite**

Run:

```sh
env PATH=/opt/homebrew/opt/rustup/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/opt/homebrew/sbin \
  cargo test --locked --test guild_intelligence_permissions
```

Expected: all permission vectors pass with no warning or ignored test.

- [ ] **Step 5: Run both Program 3 suites and inspect without committing**

Run both `guild_intelligence_replay` and `guild_intelligence_permissions`, then `git diff --check`. Confirm expectations are literal rather than calculated with production helpers and do not commit.

---

### Task 4: Promote only the closed synthetic replay evidence to C2

**Files:**
- Modify: `src/app_core/guild_intelligence.rs`
- Modify: `tests/guild_intelligence_replay.rs`
- Modify: `src/claims/registry.rs`
- Modify: `tasks/goals.md`
- Modify: `tasks/todo.md`
- Regenerate: `docs/claims.md`
- Modify after Abbey gates, in a separate ABI worktree: `docs/superpowers/specs/2026-08-22-spec-discord-guild-intelligence-read-only.md`

**Interfaces:**
- Consumes: completed Tasks 1–3 and the existing deterministic `ReplayArtifact::canonical_json` boundary.
- Produces: `EvidenceLevel::C2ClosedSyntheticReplay`, complete local replay/plan evidence, and claim-honest ledgers that retain every live/deployment exclusion.

- [ ] **Step 1: Write the failing evidence-level assertion**

Change the replay test's expected evidence level to `EvidenceLevel::C2ClosedSyntheticReplay` and add a two-recording matrix: the owner fixture and the accepted administrator variant. Replay each twice with `least-privilege` and assert byte-identical `canonical_json`; assert owner and administrator artifacts differ from each other. Run the replay test and verify RED because the enum variant does not exist.

- [ ] **Step 2: Add the closed C2 evidence variant**

Replace `C1LocalSyntheticContract` with `C2ClosedSyntheticReplay` and emit it from `analyze`. Do not add a live, production, Discord, or authorization evidence variant.

- [ ] **Step 3: Update the Abbey capability claim and goal ledger**

Change only `program-3-guild-intelligence-synthetic`: describe C2 closed synthetic replay, name the administrator/limit/permission/unknown-target/plan-condition tests, and retain `EvidenceState::NotRequired` for external evidence because the claim excludes live behavior. In `tasks/goals.md`, append dated evidence rather than rewriting the historical C1 paragraph. In `tasks/todo.md`, mark the synthetic C2 closeout complete but leave authorized non-synthetic capture, live read-only validation, and desired-state execution unchecked.

- [ ] **Step 4: Regenerate claims and run focused checks**

Run:

```sh
python3 tools/check_claims_sync.py --write
python3 tools/check_p3_readonly.py
python3 -m unittest tools.tests.test_check_p3_readonly
env PATH=/opt/homebrew/opt/rustup/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/opt/homebrew/sbin \
  cargo test --locked --test guild_intelligence_replay --test guild_intelligence_permissions
```

Expected: all checks pass; generated claim counts and digest update consistently.

- [ ] **Step 5: Update the approved ABI spec after Abbey evidence exists**

In a fresh ABI worktree from current `origin/main`, update only the focused Program 3 spec status/evidence paragraph and acceptance table notes. State the exact Abbey candidate branch and fresh gate results. Remove the four now-closed synthetic gaps. Preserve authorized non-synthetic capture, live Discord validation, deployment, persistence, approval, and effects as incomplete and separately authorized.

- [ ] **Step 6: Inspect both repository diffs without committing**

Run `git diff --check` in Abbey and ABI. Confirm no status text converts synthetic fixture authority into P2/live authorization and do not commit.

---

### Task 5: Run strict gates and perform the completion audit

**Files:**
- No new files beyond Tasks 1–4.

**Interfaces:**
- Consumes: the complete candidate diffs in the isolated Abbey and ABI worktrees.
- Produces: fresh local source evidence and an explicit residual list; no deployment or live claim.

- [ ] **Step 1: Run the complete Abbey gate**

Run:

```sh
env PATH=/opt/homebrew/opt/rustup/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/opt/homebrew/sbin ./check.sh
```

Capture the command's own exit status and full output. Do not pipe through `tail` and do not infer success from a wrapper.

- [ ] **Step 2: Run the complete ABI documentation/spec gate**

From the isolated ABI worktree, run `./tools/check.sh` and capture the command's own exit status and full output.

- [ ] **Step 3: Audit every approved synthetic requirement**

Re-read the focused Program 3 spec and map each C1/C2 requirement to current evidence: closed schema and exact limits; owner/admin fixture authority; owner mismatch and lower-authority rejection; permission vectors and unknown targets; deterministic two-recording replay; alternatives and explicit selection; preconditions, desired state, postconditions, and rollback preview; redacted status; static exclusion guard; Abbey strict gate; ABI strict gate. Any missing or indirect item remains incomplete.

- [ ] **Step 4: Audit the prohibited surfaces**

Run the static guard, inspect `git diff --stat`, and search changed Program 3 production files for forbidden transport, state, command, tool, approval, execution, and write vocabulary. Confirm the candidate adds no dependency, env var, credential, connector, deployment artifact, or Discord operation.

- [ ] **Step 5: Report the exact boundary and stop before external side effects**

Report fresh commands, counts, exit statuses, changed files, and every residual: authorized non-synthetic capture, live read-only Discord validation, Program 5 desired-state execution, deployment, and participant-consented voice acceptance. Do not commit, push, merge, deploy, or open a Discord session without Donald's next explicit instruction.
