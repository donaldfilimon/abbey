# Abbey Authority, Tools and Editions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement durable v4 task/automation/action authority and event polling (`runtime-contracts`), MCP server conformance (`mcp-server-conformance`), the owned provider-neutral tool host (`owned-tool-host`), the personal edition's separate structured `shell.exec` path (`personal-edition`), the federation authority families one at a time (`daemon-abbey-v1-federation`), and surface-parity polish (`surface-parity-grok-codex-claude`).

**Architecture:** Every authority is store-first and deny-by-default: a durable store method with its own tests, then daemon dispatch, then the capability added to the served set, then a typed client method, then (desktop plan) UI. Nothing is advertised before it is served. The personal shell is a new feature-gated module beside — never inside — `src/os_control.rs`, whose allowlist and `--confirm` stay unchanged in both editions.

**Tech Stack:** Rust 1.99.1, rusqlite (`runtime.sqlite`, schema 11), protocol v3/v4 app-core contracts, `src/mcp_host/` (stdio + loopback HTTP), `contracts/abbey/` (federation corpus).

**Spec:** `tasks/completion.json` (`runtime-contracts`, `owned-tool-host`, `personal-edition`, `mcp-server-conformance`); `docs/superpowers/plans/2026-10-22-abbey-completion.md` §4 "Durable shared authority", §5 "Owned tools and execution policies", §8 "Federation"; `docs/agent-guide.md:17-35` (personal-edition ruling); `docs/completion-plan-2026-10-22.md` WS-3/5/6/7/11/17; `tasks/todo.md:1054-1090` (Phase 9 MCP remainder).

## Global Constraints

Inherit `docs/plans/2026-10-27-abbey-completion-index.md` § Global Constraints. Plan-specific, verbatim from `docs/agent-guide.md:24-33` and the invariants:
- "keep existing `os_control.rs` allowlist/confirmation behavior in both editions until the separate path ships." The safe edition's restrictions, OS authorization, signed privileged helpers, audit, bounded supervision, emergency stop, and exclusion from remote MCP and network workers remain required. No capability is promoted by this approval.
- `src/mcp_host/tools.rs:57-59` `EffectClass` keeps exactly one variant (`ReadOnly`); `shell.exec` never enters a served MCP registry in any edition.
- A new protocol type is not daemon authority; `SERVED` (`src/daemon/runtime_v4.rs:19-25`) widens only for a capability whose every command dispatches to a durable store.
- Store writes are evidence: a failed audit/event write fails the command.
- Restart recovery never replays an ambiguous effect; ambiguous → interrupted; retry creates a new attempt.

Precondition: main-sync plan Task 6 done.

## Review Focus

- A capability negotiated that has a refused command inside it (e.g. `ReadTasks` served while `ReadTaskEvents` still returns `invalid_command`) → Task 7 test `served_v4_capabilities_have_no_refused_commands`.
- Automation catch-up firing every missed occurrence after a long sleep → must fire only the latest (`src/automation_schedule.rs:157` `catch_up`), Task 4 test.
- A task list presented as a paged history while rows mutate between pages → fixed first-page watermark, Task 3 test.
- Personal `shell.exec` reachable through `abbey mcp serve` or a network worker → Task 21 tests (and Task 13 `served_mcp_never_lists_shell_exec_in_any_edition`).
- A federation family enabled in the catalog without its durable adapter → each family's "disabled before / enabled after" pair (Tasks 24-29).

## Decision: federation family order

Three documents list the families in different orders: the brief (Cognize, changes, episodes, manifests, consent), WS-5 (`docs/completion-plan-2026-10-22.md` WS-5: Authorize → Cognize → consent → episodes → manifests → changes), and the active execution plan §8 ("authorization, consent, cognition, episodes, manifests, and changes in that order"). The active plan supersedes earlier plans "for execution order and policy choices" (`docs/superpowers/plans/2026-10-22-abbey-completion.md:6-7`), and `Authorize` is already the prepared stub every authority-bearing family depends on (`src/daemon/federation.rs:180-183`). **This plan follows the active plan: Authorize → consent → Cognize → episodes → manifests → changes.** If Donald prefers the brief's order, reorder Tasks 25-29; each is independent once Task 24 (Authorize) lands.

## File map

- Delete: `src/app_core/v4_types.rs` (dead family; not declared in `src/app_core/mod.rs:61-66`)
- Create: `src/runtime/store/v4_tasks.rs`, `src/runtime/store/v4_automations.rs`, `src/runtime/store/v4_actions.rs`, `src/runtime/store/v4_events.rs` (+ `*_tests.rs` siblings); possibly `src/runtime/migration_12_v4_admin.sql` (decision gate, Task 2)
- Modify: `src/runtime/store/v4_admin.rs:53-91`, `src/runtime/store/v4.rs:71-77`, `src/daemon/runtime_v4.rs:19-25,141-160`, `src/daemon/client/v4.rs`, `src/automation_schedule.rs` (consumer only)
- Create: `tests/daemon_v4_admin.rs`
- Modify: `src/app_core/v3.rs:363` + `src/daemon/runtime_v3.rs` (PollEvents)
- Modify: `src/mcp_host/serve.rs:36-42`, `src/mcp_host/http/`, `src/mcp_host/tools.rs`; Create `tests/mcp_conformance.rs`
- Create: `src/tool_host/{mod,policy,mcp_client,acp_client,loop_driver}.rs`, `tests/tool_host.rs`, `tests/fixtures/tool_host/` (fixture servers)
- Create: `src/personal_shell/{mod,contract,exec,audit,stop}.rs` (feature `personal-edition`), `tests/personal_shell.rs`
- Modify: `src/edition.rs:290-307`, `tests/edition_isolation.rs`
- Split then modify: `src/daemon/federation.rs` → `src/daemon/federation/{mod,catalog,authorize,consent,cognize,episodes,manifests,changes}.rs`
- Modify: `src/claims/registry.rs:78-95,227-245,658-670,736-748` (promotion tasks only)

---

### Task 1: Remove the dead v4 contract family

**Files:** Delete `src/app_core/v4_types.rs`

- [ ] **Step 1:** `grep -rn "v4_types" src desktop tools tests` → no references (verified 2026-10-26). `grep -n "mod v4" src/app_core/mod.rs` → `v4_types` absent.
- [ ] **Step 2:** For each type in `v4_types.rs`, confirm its compiled counterpart exists under `src/app_core/v4/` (e.g. `V4Task` `task.rs:63`, `V4Automation` `automation.rs:37`, `V4Action` `action.rs:24`); list any type with no counterpart in the commit message. If one is needed by Tasks 3-5, port it first into the owning `v4/*.rs` file with a round-trip test.
- [ ] **Step 3:** `git rm src/app_core/v4_types.rs`.
- [ ] **Step 4:** `./tools/cargo.sh test --locked --lib v4_` → PASS; `cargo run --locked --quiet -p abbey-desktop-codegen -- --check` → no drift.
- [ ] **Step 5:** `./check.sh` → OK. Commit: `git commit -m "Remove the uncompiled duplicate v4 contract family"`.

### Task 2: Decision gate — do migration 10's tables carry the v4 admin contract?

Migration 10 already creates `assistant_tasks`, `assistant_automations`, `assistant_automation_runs`, `assistant_action_decisions` (`src/runtime/migration_10_assistant.sql:88,106,123,137`).

- [ ] **Step 1:** For each of `V4Task`/`V4TaskAttempt` (`src/app_core/v4/task.rs:39,63`), `V4Automation`/`V4AutomationOccurrence` (`automation.rs:37,125`), `V4Action`/`V4ActionDecision` (`action.rs:24,71`), map every field to a column. Record unmapped fields and missing uniqueness constraints (occurrence identity = automation revision + scheduled UTC instant; one task per occurrence; decision binds action id + decision id + kind).
- [ ] **Step 2 — Branch C (all mapped):** no migration; proceed.
- [ ] **Step 2 — Branch M (gaps):** write failing test `migration_12_adds_v4_admin_constraints_without_touching_applied_rows` in `src/runtime/migrations.rs` tests (opens a schema-11 database with rows, migrates, asserts rows intact + new constraints enforced); add `src/runtime/migration_12_v4_admin.sql` (additive only), constant, `MIGRATIONS` entry, `CURRENT_SCHEMA_VERSION = 12` (`src/runtime/migrations.rs:6`); test passes; commit `"Add migration 12 for v4 admin authority constraints"`.

### Task 3: Durable tasks store

**Files:** Create `src/runtime/store/v4_tasks.rs`, `src/runtime/store/v4_tasks_tests.rs`; Modify `src/runtime/store/v4_admin.rs:53-58` (delegate, remove stubs)

**Interfaces:**
```rust
impl RuntimeStore {
  pub(crate) fn v4_create_task(&self, t: &V4TaskCreate, now_ms: u64) -> Result<V4Task, StoreError>;      // snapshots route/profile/budget immutably
  pub(crate) fn v4_tasks(&self, q: &V4PageQuery) -> Result<V4TaskPage, StoreError>;                    // fixed first-page watermark
  pub(crate) fn v4_task(&self, id: &str) -> Result<V4Task, StoreError>;
  pub(crate) fn v4_mutate_task(&self, m: &V4TaskMutation, now_ms: u64) -> Result<V4Task, StoreError>;  // cancel/pause/resume
  pub(crate) fn v4_retry_task(&self, r: &V4TaskRetry, now_ms: u64) -> Result<V4Task, StoreError>;      // new attempt, never replay
  pub(crate) fn v4_recover_tasks(&self, now_ms: u64) -> Result<u32, StoreError>;                      // running → interrupted
}
```
(The `v4_tasks` query type changes from `V4ConversationQuery` to `V4PageQuery` only if `V4Command::ListTasks` carries a `V4PageQuery`; otherwise keep the existing parameter type.)

- [ ] **Step 1: Failing tests:** `a_created_task_snapshots_route_profile_and_budget_immutably`, `task_pages_keep_their_first_page_watermark_while_rows_change`, `retry_creates_a_new_attempt_and_never_reuses_the_run`, `restart_marks_running_attempts_interrupted_not_retried`, `a_failed_event_write_fails_the_task_command`. Each test owns a generated scratch state root.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --lib runtime::store::v4_tasks` → FAIL.
- [ ] **Step 3:** Implement; replace the two task stubs in `v4_admin.rs`.
- [ ] **Step 4:** Rerun → PASS; run the module serially (`-- --test-threads=1`) and in parallel → PASS both.
- [ ] **Step 5:** Commit: `"Persist v4 tasks with immutable snapshots and interrupted recovery"`.

### Task 4: Durable automations bound to the schedule calculator

**Files:** Create `src/runtime/store/v4_automations.rs` (+ tests); Modify `v4_admin.rs:59-71`

**Interfaces:** `v4_upsert_automation(&self, u: &V4AutomationUpsert, now_ms) -> Result<V4Automation>` (new revision on every change), `v4_automations(&self, q) -> Result<V4AutomationPage>`, `v4_set_automation_enabled(&self, t: &V4AutomationToggle, now_ms)`, `v4_reserve_due_occurrences(&self, now_ms) -> Result<Vec<V4AutomationOccurrence>>` (uses `CompiledSchedule::catch_up`, `src/automation_schedule.rs:157`; reserves one row per `(automation_id, revision, scheduled_utc_ms)`; creates exactly one task per reserved occurrence; no overlapping run).

- [ ] **Step 1: Failing tests:** `an_occurrence_is_reserved_once_per_revision_and_instant`, `catch_up_reserves_only_the_latest_missed_occurrence`, `a_running_occurrence_blocks_an_overlapping_one`, `disabling_revokes_unreserved_occurrences`, `dst_fold_reserves_the_first_occurrence` (reuse the instants from `src/automation_schedule/tests.rs` `dst_gap_fold_and_skipped_day`).
- [ ] **Step 2:** Run → FAIL. **Step 3:** implement. **Step 4:** Run → PASS (serial + parallel).
- [ ] **Step 5:** Commit: `"Reserve automation occurrences through the schedule calculator"`.

### Task 5: Action decisions and task events stores

**Files:** Create `src/runtime/store/v4_actions.rs`, `src/runtime/store/v4_events.rs` (+ tests); Modify `v4_admin.rs:72-85`

**Interfaces:** `v4_action(&self, q: &V4ActionQuery) -> Result<V4Action>`; `v4_decide_action(&self, d: &V4ActionDecision, now_ms) -> Result<V4ActionDecisionReceipt>` (binds action id + decision id + kind; idempotent for the identical decision id; conflicting second decision refused; revocation before effect); `v4_task_events(&self, q: &V4TaskEventQuery) -> Result<V4TaskEventPage>` (fixed watermark).

- [ ] **Step 1: Failing tests:** `a_decision_binds_action_decision_id_and_kind`, `a_conflicting_second_decision_is_refused`, `a_revoked_decision_cannot_reach_an_effect`, `task_event_pages_are_watermarked_and_cursor_bound`.
- [ ] **Step 2-4:** FAIL → implement → PASS. Delete `src/runtime/store/v4_unavailable_tests.rs` refusals only for the now-served methods (keep refusal tests for anything still unserved).
- [ ] **Step 5:** Commit: `"Persist v4 action decisions and task event pages"`.

### Task 6: Fix `v4_search_conversations`

`src/runtime/store/v4.rs:71-77` returns the unfiltered list while `SearchConversations` advertises search.

- [ ] **Step 1:** Failing test `search_conversations_filters_by_title_and_keeps_a_fixed_snapshot` in `src/runtime/store/v4_tests.rs`.
- [ ] **Step 2-4:** FAIL → filter by the query's search field (bounded `LIKE` with escaped wildcards) → PASS.
- [ ] **Step 5:** Commit: `"Filter v4 conversation search by its query"`.

### Task 7: Dispatch and serve v4 admin authority

**Files:** Modify `src/daemon/runtime_v4.rs:19-25,141-160`; Create `tests/daemon_v4_admin.rs`

- [ ] **Step 1: Failing tests:** unit `served_v4_capabilities_have_no_refused_commands` (for each capability in `SERVED`, every `V4Command` it permits dispatches without `invalid_command`); integration `tests/daemon_v4_admin.rs::real_daemon_creates_lists_cancels_and_retries_a_task`, `::real_daemon_reserves_one_task_per_automation_occurrence_across_restart`, `::an_unserved_admin_capability_is_never_negotiated`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Route each command in `:141-156` to Tasks 3-5; add `ReadTasks`, `WriteTasks`, `ReadAutomations`, `WriteAutomations`, `DecideActions`, `ReadPermissions`, `WritePermissions` to `SERVED` only when every command each one permits is routed. Keep the deny-by-default filter at `:44-55`.
- [ ] **Step 4:** PASS. `wc -l src/daemon/runtime_v4.rs` < 800 (split a `runtime_v4/admin.rs` if not).
- [ ] **Step 5:** `cargo run --locked --quiet -p abbey-desktop-codegen -- --check`; if drift, `(cd desktop && bun run codegen)` and include generated files.
- [ ] **Step 6:** `./check.sh` → OK. Commit: `"Serve durable v4 task, automation and action authority"`.

### Task 8: Typed v4 client methods

**Files:** Modify `src/daemon/client/v4.rs` (394 lines; add `src/daemon/client/v4/admin.rs` if it would exceed 800)

- [ ] **Step 1:** Failing tests in `src/daemon/client/v4/` tests: `admin_calls_echo_the_exact_grant_and_correlate_responses`, `admin_calls_stop_locally_when_the_grant_is_absent`.
- [ ] **Step 2-4:** FAIL → add `create_task`, `list_tasks`, `read_task`, `cancel_task`, `retry_task`, `pause_task`, `resume_task`, `task_events`, `list_automations`, `upsert_automation`, `set_automation_enabled`, `run_automation_now`, `read_action`, `decide_action` → PASS.
- [ ] **Step 5:** Commit: `"Add typed v4 client methods for tasks, automations and actions"`.

### Task 9: v3 `PollEvents` with a fixed watermark

**Files:** Modify `src/daemon/runtime_v3.rs`, `src/daemon/client/v3.rs`; tests `src/daemon/runtime_v3/tests.rs`

- [ ] **Step 1:** Failing tests `poll_events_first_page_watermark_is_bound_to_the_exact_query` and `poll_events_refuses_a_cursor_from_another_query`.
- [ ] **Step 2-4:** FAIL → serve `V3Command::PollEvents` (`src/app_core/v3.rs:363`) over a durable event cursor; grant `V3Capability::PollEvents` only from startup-owned authority → PASS. Update the module doc at `src/daemon/runtime_v3.rs:7` to state that polling is served.
- [ ] **Step 5:** Commit: `"Serve protocol-v3 event polling with query-bound watermarks"`.

### Task 10: Close `runtime-contracts`

- [ ] **Step 1:** Add registry rows (promotion rule): new `daemon-protocol-v4-task-automation-authority` Current with Tasks 3-8 test ids; `daemon-protocol-v3-event-polling` Current with Task 9 ids. `python3 tools/check_claims_sync.py --write`; check mode → 0; `python3 tools/check_instructions.py` → 0.
- [ ] **Step 2:** `tasks/completion.json` `runtime-contracts`: `source`/`automated` passed (refs: tests + gate log); tick its two todos only if their acceptance text is fully met; status `complete` (platform channels belong to the milestone, not the requirement).
- [ ] **Step 3:** `./check.sh` → OK; fresh-reviewer review. Commit: `"Record durable v4 authority and v3 polling as Current"`.

### Task 11: Decision gate — which MCP protocol revision is served

`src/mcp_host/serve.rs:36-42` serves `2025-06-18` (+ `2025-03-26`, `2024-11-05`); `tasks/todo.md:1085-1088` names a "2026-07-28 stateless lifecycle" not implemented. Acceptance: "Pin and validate the official served protocol revision".

- [ ] **Step 1:** **Donald step:** choose the pinned revision (keep `2025-06-18` as latest, or adopt the newer revision). Evidence: decision line in `docs/verification/YYYY-MM-DD-mcp-revision.md`, with the spec URL/version consulted.
- [ ] **Step 2:** Branch K (keep): Tasks 12-14 target `2025-06-18`. Branch N (new): add Task 12a — implement the new lifecycle behind version negotiation with `classic_clients_still_negotiate_2025_06_18` compatibility fixture before Tasks 12-14.

### Task 12: Cooperative cancellation and bounded streaming

**Files:** Modify `src/mcp_host/serve.rs`, `src/mcp_host/http/`; Create `tests/mcp_conformance.rs`

- [ ] **Step 1: Failing tests:** `notifications_cancelled_stops_an_in_flight_request`, `get_stream_resumes_from_last_event_id_within_bounds`, `a_slow_consumer_is_disconnected_at_the_buffer_ceiling`, `a_stale_or_cross_client_session_is_refused`.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --test mcp_conformance` → FAIL.
- [ ] **Step 3:** Implement; every new limit is a named constant in `src/mcp_host/limits.rs` with a threshold test (house rule in that file's table).
- [ ] **Step 4:** PASS; existing `tests/mcp_http.rs::the_transport_refuses_what_it_does_not_implement` still PASS (update only the parts now implemented, keep TLS/OAuth refusal).
- [ ] **Step 5:** Commit: `"Add cooperative cancellation and bounded resumable streaming to the MCP server"`.

### Task 13: `resources`/`prompts`/`tasks` for registered read-only handlers only

- [ ] **Step 1:** Failing tests `capabilities_list_resources_only_when_a_read_only_handler_is_registered`, `an_unregistered_prompt_is_never_advertised`, `served_mcp_never_lists_shell_exec_in_any_edition` (run in the personal-edition cohort too).
- [ ] **Step 2-4:** FAIL → implement → PASS; `src/mcp_host/tests.rs::no_advertised_tool_is_execution_capable` (`:575`) and `tests/mcp_server.rs::the_advertised_registry_contains_no_execution_capable_tool` (`:148`) still PASS.
- [ ] **Step 5:** Commit: `"Serve MCP resources, prompts and tasks only for registered read-only handlers"`.

### Task 14: Non-loopback stays unavailable; close `mcp-server-conformance`

- [ ] **Step 1:** Keep the hard error for non-loopback `--host`. TLS/OAuth 2.1/PKCE/resource indicators require a separately reviewed design — record in `docs/verification/YYYY-MM-DD-mcp-conformance.md` that the third acceptance bullet is satisfied by *remaining unavailable*.
- [ ] **Step 2:** Add `protocol_conformance` evidence (test ids + the pinned revision). `tasks/completion.json` → `complete` when all three channels pass; `check_completion.py` → 0; claims row for MCP updated via registry + sync tool.
- [ ] **Step 3:** `./check.sh` → OK. Commit.

### Task 15: Decision gate — tool-host runtime ownership (ABI Phase 5)

`owned-tool-host` todo "Phase 5 — Model and tool runtime ownership in ABI" (`tasks/todo.md:718`) places tool runtime ownership in ABI (`abi-agent-host`, `abi-agent-runtime` are path deps, `Cargo.toml:77-78`).

- [ ] **Step 1:** Read `../abi/crates/abi-agent-host` and `abi-agent-runtime` public APIs (read-only). Record whether they expose: structured tool-call parsing, schema validation, a loop driver, MCP/ACP client sessions.
- [ ] **Step 2 — Branch ABI-ready:** Tasks 16-18 wrap those APIs. **Branch ABI-missing:** file the missing pieces as index sibling request (g); Tasks 16-17 implement Abbey's policy/governance layer against a trait (`ToolRuntime`) with a fixture implementation; Task 18 waits for the sibling pin.

### Task 16: Tool-host policy layer

**Files:** Create `src/tool_host/mod.rs`, `src/tool_host/policy.rs`, `tests/tool_host.rs`

**Interfaces:** `pub(crate) trait ToolRuntime { fn call(&self, call: &StructuredToolCall, deadline: Instant, cancel: &CancellationToken) -> Result<ToolOutput, ToolError>; }`; `pub(crate) fn govern(call: &StructuredToolCall, policy: &PolicySnapshot, store: &RuntimeStore) -> Result<Authorized, Refusal>` (schema validation, current policy, durable authorization/intent written **before** the effect, via `src/runtime/store/governed.rs`).

- [ ] **Step 1: Failing tests:** `raw_model_prose_is_never_executed`, `a_call_failing_its_schema_is_refused_before_intent`, `intent_is_durable_before_any_effect`, `a_mutation_is_never_replayed_after_restart`, `hostile_output_over_the_bound_is_truncated_and_marked`.
- [ ] **Step 2-4:** FAIL → implement → PASS. **Step 5:** commit `"Add the governed tool-host policy layer"`.

### Task 17: External MCP/ACP client sessions (fixture servers)

**Files:** Create `src/tool_host/mcp_client.rs`, `src/tool_host/acp_client.rs`, `tests/fixtures/tool_host/*`

- [ ] **Step 1: Failing tests** in `tests/tool_host.rs`: `a_server_advertising_duplicate_tool_ids_is_refused`, `unexpected_capability_expansion_mid_session_is_refused`, `a_child_server_is_reaped_on_disconnect`, `disconnect_during_an_effect_is_recorded_ambiguous_not_retried`, `redirects_and_auth_failures_fail_closed`.
- [ ] **Step 2-4:** FAIL → implement stdio + authenticated-network sessions with deadlines and output bounds → PASS.
- [ ] **Step 5:** Commit `"Host external MCP and ACP servers as governed tool runtimes"`.

### Task 18: Multi-tool loop with a real model-produced call; close `owned-tool-host`

- [ ] **Step 1:** Failing test `tests/tool_host.rs::a_model_produced_structured_call_runs_through_policy_and_returns` using the existing fixture model path (`abi-bigram-v1` cannot produce tool calls → use a deterministic fixture provider that emits a structured call; label the evidence "fixture provider", not model quality).
- [ ] **Step 2-4:** FAIL → `src/tool_host/loop_driver.rs` → PASS.
- [ ] **Step 5:** Registry `runtime-provider-neutral-owned` (`src/claims/registry.rs:658`) Proposed → Partial (fixture provider) via promotion rule; Current requires a real provider run (record as a later Donald-authorized live check). Ledger update; `./check.sh`; commit.

### Task 19: Personal `shell.exec` contract (personal-edition only)

**Files:** Create `src/personal_shell/mod.rs`, `src/personal_shell/contract.rs`; Modify `src/lib.rs` (feature-gated `mod`)

**Interfaces:**
```rust
#[cfg(feature = "personal-edition")]
pub(crate) struct ShellExec { pub program: PathBuf, pub args: Vec<OsString>, pub cwd: PathBuf, pub env: BTreeMap<OsString, OsString>, pub timeout: Duration, pub capture_bytes: usize }
pub(crate) fn validate(call: &serde_json::Value) -> Result<ShellExec, ShellRefusal>; // structured only: no shell string, no `sh -c`
```

- [ ] **Step 1: Failing tests** (`tests/personal_shell.rs`, `#![cfg(feature = "personal-edition")]`): `a_shell_string_is_refused_and_only_argv_is_accepted`, `relative_program_paths_resolve_without_path_search_surprises`, `oversized_argv_or_env_is_refused_against_the_supervisor_budget` (reuse `src/argv_budget.rs` limits).
- [ ] **Step 2:** `./tools/cargo.sh test --locked -p abbey --features personal-edition --test personal_shell` → FAIL.
- [ ] **Step 3-4:** implement → PASS. Default cohort: `./tools/cargo.sh test --locked` → the module does not exist in the safe build (`safe_edition_has_no_personal_shell_symbol` in `tests/edition_isolation.rs`).
- [ ] **Step 5:** Commit `"Define the personal edition's structured shell.exec contract"`.

### Task 20: Personal shell execution, audit, teardown, emergency stop

**Files:** Create `src/personal_shell/exec.rs`, `audit.rs`, `stop.rs`

- [ ] **Step 1: Failing tests:** `every_exec_writes_an_audit_record_before_spawn`, `a_failed_audit_write_refuses_execution`, `timeout_tears_down_the_whole_process_group`, `the_emergency_stop_kills_all_running_execs`, `capture_is_bounded`. Execution goes through `crate::runtime::supervisor` (process groups on Unix; Job Object path from the platforms plan on Windows).
- [ ] **Step 2-4:** FAIL → implement → PASS (personal-edition cohort).
- [ ] **Step 5:** `./check.sh` (runs the personal cohort, `check.sh:66-70`) → OK. Commit `"Execute personal shell calls with audit, teardown and emergency stop"`.

### Task 21: Locality boundary and edition separation

- [ ] **Step 1: Failing tests:** `personal_shell_is_reachable_only_over_local_ipc` (daemon socket owner-only), `served_mcp_never_lists_shell_exec_in_any_edition` (from Task 13, now in the personal cohort), `network_worker_api_cannot_dispatch_shell_exec`, `os_control_allowlist_and_confirm_are_unchanged_in_both_editions` (asserts `src/os_control.rs:12-21` lists and `--confirm` requirement).
- [ ] **Step 2-4:** FAIL → wire the local-IPC-only dispatch → PASS.
- [ ] **Step 5:** Commit `"Keep personal shell execution local-only and separate from os_control"`.

### Task 22: **Donald step** — signed privileged helpers and the unrestricted flag (OB-10)

- [ ] **Step 1:** Privileged operations need signed platform helpers (Authorization Services/SMAppService on macOS, polkit on Linux, signed service/UAC on Windows). These require Donald's signing identities. Evidence: `docs/verification/YYYY-MM-DD-personal-helpers.md` listing which helpers were signed and on which host.
- [ ] **Step 2:** Only after Step 1 for at least macOS: change `Edition::unrestricted_runtime_implemented` (`src/edition.rs:306`) to return `true` **for `Edition::Personal` only**; update `tests/edition_isolation.rs` assertions (safe stays `false`). Registry `edition-personal-unrestricted` (`:736`) Proposed → Partial (macOS). Ledger `personal-edition` evidence. `./check.sh`; commit.

### Task 23: Split `federation.rs` before adding families

`src/daemon/federation.rs` is 636 lines; five adapters would cross the 1000-line hard limit.

- [ ] **Step 1:** Move without behaviour change into `src/daemon/federation/{mod.rs,catalog.rs,validate.rs,client.rs}`; tests stay green: `./tools/cargo.sh test --locked --lib daemon::federation` → PASS (same test names, incl. `read_only_catalog_works_and_authority_methods_are_disabled` at `:577`).
- [ ] **Step 2:** Commit `"Split the federation module before adding authority adapters"`.

### Task 24: `Authorize` family

**Files:** Create `src/daemon/federation/authorize.rs`; Modify catalog row `federation.authorization` (`src/daemon/federation.rs:353` before split)

- [ ] **Step 1: Failing tests:** `authorize_is_disabled_before_its_adapter_exists` (keep current behaviour as the "before" test, renamed from the `:590-594` assertions), `authorize_replays_an_identical_request_idempotently`, `authorize_refuses_a_digest_mismatch_before_any_state_change`. Contract corpus digests unchanged (`tests/abbey_contracts.rs::bundled_corpus_qualifies_exact_bytes_and_every_declared_fixture`).
- [ ] **Step 2-4:** FAIL → durable adapter over `runtime.sqlite` (append-only table; migration 12/13 per Task 2 numbering) → PASS; flip catalog row to `enabled:true`.
- [ ] **Step 5:** `./check.sh`; commit `"Serve federation Authorize through a durable adapter"`.

### Task 25: Consent epoch family

Methods `OpenConsentEpoch`/`AttestConsent`/`CloseConsentEpoch`/`ResumeConsentEpoch` (catalog row `federation.consent`, `src/daemon/federation.rs:358` before the split).

**Files:** Create `src/daemon/federation/consent.rs`; Modify `src/daemon/federation/catalog.rs` (row for this family).

- [ ] **Step 1: Write failing tests** in `src/daemon/federation/consent.rs` tests: `consent_methods_are_disabled_before_their_adapter_exists`, `a_closed_epoch_cannot_be_attested`, `resume_requires_the_exact_prior_epoch_digest`. The first one pins today's `CapabilityDisabled` refusal and must keep passing until Step 3 flips the row.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --lib daemon::federation::consent` → FAIL for every test except the `…_disabled_before_…` one.
- [ ] **Step 3:** Implement the durable adapter (append-only `runtime.sqlite` table in the next additive migration; intent recorded before any effect; ambiguous outcomes recorded, never replayed); then set the catalog row `enabled:true` and convert the disabled-test into `…_is_enabled_once_its_adapter_exists`.
- [ ] **Step 4:** Rerun → PASS; `./tools/cargo.sh test --locked --test abbey_contracts` → PASS (corpus digests unchanged).
- [ ] **Step 5:** `./check.sh` → OK. Commit `"Serve federation consent epochs through a durable adapter"`.

### Task 26: Cognize family

Method `Cognize` (catalog row `federation.cognition`, `:354`). Uses the existing configured provider route; adds no new provider authority.

**Files:** Create `src/daemon/federation/cognize.rs`; Modify `src/daemon/federation/catalog.rs` (row for this family).

- [ ] **Step 1: Write failing tests** in `src/daemon/federation/cognize.rs` tests: `cognize_is_disabled_before_its_adapter_exists`, `cognize_requires_an_open_consent_epoch`, `cognize_output_is_bounded_and_digest_bound`. The first one pins today's `CapabilityDisabled` refusal and must keep passing until Step 3 flips the row.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --lib daemon::federation::cognize` → FAIL for every test except the `…_disabled_before_…` one.
- [ ] **Step 3:** Implement the durable adapter (append-only `runtime.sqlite` table in the next additive migration; intent recorded before any effect; ambiguous outcomes recorded, never replayed); then set the catalog row `enabled:true` and convert the disabled-test into `…_is_enabled_once_its_adapter_exists`.
- [ ] **Step 4:** Rerun → PASS; `./tools/cargo.sh test --locked --test abbey_contracts` → PASS (corpus digests unchanged).
- [ ] **Step 5:** `./check.sh` → OK. Commit `"Serve federation Cognize through a durable adapter"`.

### Task 27: Episodes family

**Decision gate first:** the registry's next action says "add WDBX/episode/consent adapters only when durable paths exist" (`src/claims/registry.rs:227-245`). Read `../wdbx` at the pinned revision (read-only) for a durable episode commit API. If none exists → index sibling request; stop this task and record why.

Methods `RetrieveEpisodes`/`ProposeEpisodeWrite` (catalog row `federation.episodes`, `:356`).

**Files:** Create `src/daemon/federation/episodes.rs`; Modify `src/daemon/federation/catalog.rs` (row for this family).

- [ ] **Step 1: Write failing tests** in `src/daemon/federation/episodes.rs` tests: `episodes_are_disabled_before_their_adapter_exists`, `episode_write_is_a_proposal_until_committed`. The first one pins today's `CapabilityDisabled` refusal and must keep passing until Step 3 flips the row.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --lib daemon::federation::episodes` → FAIL for every test except the `…_disabled_before_…` one.
- [ ] **Step 3:** Implement the durable adapter (append-only `runtime.sqlite` table in the next additive migration; intent recorded before any effect; ambiguous outcomes recorded, never replayed); then set the catalog row `enabled:true` and convert the disabled-test into `…_is_enabled_once_its_adapter_exists`.
- [ ] **Step 4:** Rerun → PASS (also `./tools/cargo.sh test --locked -p abbey --features wdbx --lib daemon::federation::episodes`); `./tools/cargo.sh test --locked --test abbey_contracts` → PASS (corpus digests unchanged).
- [ ] **Step 5:** `./check.sh` → OK. Commit `"Serve federation episodes through a durable WDBX adapter"`.

### Task 28: Manifests family

Method `ApplyManifest` (catalog row `federation.manifests.apply`, `:357`).

**Files:** Create `src/daemon/federation/manifests.rs`; Modify `src/daemon/federation/catalog.rs` (row for this family).

- [ ] **Step 1: Write failing tests** in `src/daemon/federation/manifests.rs` tests: `apply_manifest_is_disabled_before_its_adapter_exists`, `apply_manifest_records_intent_before_local_effect`. The first one pins today's `CapabilityDisabled` refusal and must keep passing until Step 3 flips the row.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --lib daemon::federation::manifests` → FAIL for every test except the `…_disabled_before_…` one.
- [ ] **Step 3:** Implement the durable adapter (append-only `runtime.sqlite` table in the next additive migration; intent recorded before any effect; ambiguous outcomes recorded, never replayed); then set the catalog row `enabled:true` and convert the disabled-test into `…_is_enabled_once_its_adapter_exists`.
- [ ] **Step 4:** Rerun → PASS; `./tools/cargo.sh test --locked --test abbey_contracts` → PASS (corpus digests unchanged).
- [ ] **Step 5:** `./check.sh` → OK. Commit `"Serve federation ApplyManifest through a durable adapter"`.

### Task 29: Changes family

Methods `ProposeChange`/`ApproveChange`/`ExecuteChange`/`CompensateChange` (catalog row `federation.changes`, `:355`). `best_effort_executors_registered` stays `false` unless a registered executor exists.

**Files:** Create `src/daemon/federation/changes.rs`; Modify `src/daemon/federation/catalog.rs` (row for this family).

- [ ] **Step 1: Write failing tests** in `src/daemon/federation/changes.rs` tests: `changes_are_disabled_before_their_adapter_exists`, `execute_change_never_replays_an_ambiguous_effect`, `compensate_change_binds_the_exact_executed_change`. The first one pins today's `CapabilityDisabled` refusal and must keep passing until Step 3 flips the row.
- [ ] **Step 2:** `./tools/cargo.sh test --locked --lib daemon::federation::changes` → FAIL for every test except the `…_disabled_before_…` one.
- [ ] **Step 3:** Implement the durable adapter (append-only `runtime.sqlite` table in the next additive migration; intent recorded before any effect; ambiguous outcomes recorded, never replayed); then set the catalog row `enabled:true` and convert the disabled-test into `…_is_enabled_once_its_adapter_exists`.
- [ ] **Step 4:** Rerun → PASS; `./tools/cargo.sh test --locked --test abbey_contracts` → PASS (corpus digests unchanged).
- [ ] **Step 5:** `./check.sh` → OK. Commit `"Serve federation changes through a durable adapter"`.

### Task 29a: Federation claim status

- [ ] **Step 1:** If all six families (Tasks 24-29) are enabled with adapters: registry `daemon-abbey-v1-federation` (`src/claims/registry.rs:227`) Partial → Current via the promotion rule. Otherwise edit only its `local`/`next` text to list the families still disabled.
- [ ] **Step 2:** `python3 tools/check_claims_sync.py --write`; check mode → 0; `python3 tools/check_instructions.py` → 0.
- [ ] **Step 3:** `./check.sh` → OK; fresh-reviewer review; commit `"Update the federation claim to the families now served"`.

### Task 30: Surface-parity polish (WS-3)

- [ ] **Step 1:** Enumerate documented gaps: README compatibility section and `src/surfaces.rs` catalogue entries whose status is not parity; record each as `<surface>: <gap>` in `docs/verification/YYYY-MM-DD-surface-parity-gaps.md`.
- [ ] **Step 2:** For each gap, one TDD cycle: extend `tests/cli_surface.rs::compatibility_ux_acceptance_matrix` (`:702`) with a failing row → implement in `src/slash.rs`/`src/agent/argv.rs`/`src/tui/` → PASS → commit per gap. Per-backend argv-leak tests stay green.
- [ ] **Step 3:** When the gap list is empty, registry `surface-parity-grok-codex-claude` (`:78`) Partial → Current with the matrix test as evidence and the unchanged "not a reimplementation of vendor runtimes" note. Sync tool; `./check.sh`; commit.
