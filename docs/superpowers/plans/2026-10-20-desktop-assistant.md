# Abbey Desktop Assistant Implementation Plan

> Historical execution checklist. The active [2026-10-22 Completion Program](2026-10-22-abbey-completion.md) supersedes execution order and target-policy choices. These unchecked items are preserved, not asserted complete; distinct obligations are tracked by `retained-plan-reconciliation` in `tasks/completion.json`. Canonical workflow-ledger coverage does not claim to validate every historical plan checkbox.

> For agentic workers: use superpowers:subagent-driven-development. Implement and independently review each task before proceeding. Track steps with checkboxes.

**Goal:** Deliver the complete approved Abbey desktop assistant across macOS, Windows, and Linux: polished chat, task delegation, scoped computer control, external tools, recurring automation, and management.

**Architecture:** Retain Tauri 2, React/TypeScript, Rust 1.99.1, the canonical agent execution path, and the durable runtime. Introduce separately negotiated v4 application authority without expanding v1/v2/v3 or the existing read-only MCP server. Combine structured tools, native platform adapters, and external MCP/ACP clients behind common action policy and lifecycle records.

**Spec:** The approved ten-milestone plan in the 2026-10-20 conversation, reproduced in the requirements below. Native adapters, browser/files/approved commands, and external integrations are all in scope. Broad autonomy is bounded by configured permissions and existing mandatory confirmation rules.

## Global Constraints

- Preserve all unrelated work, canceled Zig history, legacy conversation evidence, edition isolation, and exact repository toolchain pins. Work in the canonical default-branch checkout as AGENTS.md requires. Do not commit or push without a user request.
- One canonical agent execution path: desktop conversational execution must flow through run_agent and existing backend-specific stream adapters, rather than independently spawning providers.
- v1/v2/v3 and existing MCP contracts retain their authority boundaries. New content and effects belong to separately negotiated v4 authority with narrow enumerated desktop commands.
- OS execution always requires explicit confirmation and the allowlist in both editions. Broad autonomy never overrides this rule. External content cannot grant permissions.
- Per-edition local durable state; credentials stay out of webview, logs, and content-event diagnostics. Transcript content is separate from sanitized audit projections.
- Every new Rust module stays below the repository file-size ceilings. No claims of native-platform, live-provider, signed-release, or production-model acceptance without the relevant evidence.
- Use scratch state and fixtures for checks. Run focused tests and both applicable gates. Preserve failures and unavailable evidence.

## Review Focus

- Duplicate submission, reconnect, and crash recovery must not replay effects or duplicate messages.
- Conversation, edition, workspace, and permission-profile identities cannot be confused by callers.
- Revoked access, changed targets, symlink escapes, and malicious tool output fail closed.
- Interrupted runs and confirmations remain visible and require explicit recovery when effects are ambiguous.
- Native packaging and platform acceptance require real evidence, not compile-only claims.

## Current desktop state (post-fixes review, this subagent)

Achieved and marked [x] + evidence (keep honest; no overclaim):

- [x] Protocol-v4 contracts, durable store (migrations, admit/append/finalize), v4 daemon negotiation + read surfaces (Tasks 1-3,16 partial).
- [x] Chat workspace + real conversation history (CRUD, paged messages, search, drafts, polling 250ms, guards, reconnect) + polished shell/nav/palette/bootstrap (Tasks 4,13,17).
- [x] Basic tasks surface (stub via Runs + note).
- [x] desktop/check.sh fully green (typecheck/build/tests/live-daemon-crud + cargo build).
- [x] v4 client strict, no fallback, grant-bounded, live scratch verified.
- Models, Memory, Routes, Claims, Doctor(Settings) read surfaces functional or honestly unavailable.
- All per existing subagent notes + new evidence in this review. No execution, no full tasks/automation/tools/native/packaging.

Remaining listed as [ ] below or explicitly Proposed (see updated verification ledger + claims). Execution still routes only through canonical (no v4 submit to run_agent). No claims promoted.

### Task 1: Protocol-v4 conversation contracts

Files: new src/app_core/v4.rs and focused supporting modules/tests; src/app_core/mod.rs; desktop/codegen source list and generated IPC types.

- [x] Add a separately versioned, strict serde command/event contract, APP_PROTOCOL_V4=4 and APP_SCHEMA_V4=1. Do not alter old command enums or grants.
- [x] Define V4Capability, V4CapabilitySet, V4Command, V4Event, V4Conversation, V4Message, V4ContentEvent, V4ConversationPage, V4MessagePage, V4ContentPage, V4ConversationQuery, V4MessageQuery, V4SubmitTurn, V4TurnSubmission, V4ConversationPatch, and V4Route. Reuse ConversationId, RunId, IdempotencyKey, RunState.
- [x] Commands: negotiate, list/create/read/update conversations, read messages, submit turn, read content, cancel turn, list configured routes. Capabilities must distinguish conversation read/write, turn submit/read/cancel. No generic executable, environment, or arbitrary workspace path.
- [x] Constants: 32 KiB submitted input, 256-byte title, page limit 50, 4 MiB turn content, 65,536 content events, 16 KiB individual text delta. Query strings max 256 bytes; identifiers are opaque canonical existing IDs. Strict unknown-field rejection, duplicate-free capability sets, bounded nonempty input, and safe timestamp/sequence validation. Message text may contain ordinary multiline Unicode; never use audit redaction to rewrite authorized transcript text. Avoid deriving Debug for raw prompt/content types; provide redacted Debug.
- [x] Define submission correlation and message states sufficient for queued/running/succeeded/failed/cancelled/interrupted turns. Content events carry conversation/run IDs and monotonic sequence, typed text/tool/notice/usage/terminal payloads; exclude bearer material and hidden provider reasoning.
- [x] Generate TypeScript from Rust. Test validation, unknown fields, Unicode byte boundaries, correlation, capability ordering, and redacted Debug. Contracts alone grant no execution authority.

Wire ruling: preserve the 1 MiB daemon frame bound. V4Message returns an assistant preview of at most 16 KiB plus text_bytes and text_complete; complete assistant content is reconstructed from correlated content pages. User messages retain their full bounded input. Conversation/message/content pages must serialize to at most 512 KiB and may return fewer rows than requested.

### Task 2: Durable conversation and content storage

- [x] Add an additive runtime migration with separate conversation/message/content tables, preserving existing canonical identity and legacy imports. (migration 10 assistant tables + store/v4 using them)
- [x] Implement bounded create/list/search/read/rename/archive/pin/export/delete operations for the new records; deletion must not cascade into legacy backups, semantic memory, routes, models, or unrelated runs. (v4_delete only assistant_*, tests assert legacy conv row remains)
- [x] Admit a turn atomically with its user message and immutable idempotency binding. Reject conflicting keys and overlapping active turns in one conversation; permit independent conversations within worker budgets. (v4_admit_turn tx + duplicate stable + conflict on active + independent conv test)
- [x] Append ordered bounded content, finalize exactly one assistant message, and mark unfinished runs interrupted on reopen. Recovery never reexecutes a provider or effect. (v4_append_content + v4_finalize + sync stub + reopen recovery test)
- [x] Test migrations/reopen, duplicate/conflicting submit, sequence paging, bounds, terminal immutability, conversation isolation, and preserved legacy state. (v4_* tests + bounds/isolation/migration tests)

Storage implementation interfaces: add focused store/v4 modules with RuntimeStore methods v4_create_conversation(title), v4_list_conversations(query), v4_conversation(id), v4_patch_conversation(patch), v4_delete_conversation(id), v4_messages(query), v4_admit_turn(request), v4_append_content(run_id,payload), v4_content(query). Admission creates the existing runs row and queued lifecycle event atomically with the v4 user message and route binding. Use domain-separated SHA-256 of canonical V4SubmitTurn JSON for immutable digest binding. Keep v4 turn-route metadata separate from old backend enums. Existing lifecycle transition transactions update associated message states and, on terminal state, finalize exactly one assistant message and terminal content event. Reopen reconciles v4 terminal projections after existing interrupted-run recovery. Delete refuses active turns and removes only v4 transcript tables; retain base conversations/runs/idempotency metadata. Full assistant text remains in storage; bounded preview and content paging satisfy Task 1's wire limit.

### Task 3: V4 daemon and canonical interactive execution

- [x] Add strict authenticated v4 envelopes, negotiation, correlated send-once client methods and handler dispatch alongside old protocols.
- [x] Connect new conversational turns to the durable manager and canonical run_agent stream/cancellation path. Preserve persona/role wrapping, backend grammars, continuity, route audit, and local-first provider selection.
- [x] Publish startup-configured routes for available Ollama/FM/ABI/Claude/Grok/Cursor adapters. Callers select configured route IDs, never executables or environment.
- [x] Translate bounded visible stream events into durable content; exclude hidden reasoning. Reconnect resumes reads, never submissions. Shared state must not let concurrent conversations overwrite one another's provider continuity.
- [x] Test against scratch daemons and deterministic provider fixtures, including streaming, stop, worker failure, absent route, forged grant, reconnect, and crash recovery.

Manager implementation ruling: keep old RunRequest and BackendSelection wire enums unchanged. Extend internal queued work to distinguish legacy and v4 turn payloads, add an Executor v4 method with default unsupported behavior, and add RunManager::submit_v4 calling the atomic v4 store admission. Both payloads use the same lifecycle transitions, queue, cancellation and panic handling. The canonical v4 executor owns startup-frozen route configurations. Suppress inherited CURSOR_AGENT_CHAT_ID adoption for daemon-owned conversation state without changing CLI defaults. Provider session/transcript state is derived from the opaque conversation and route identity under private edition state, never supplied as a request path.

### Task 4: Polished desktop chat workspace

- [x] Add narrow typed Tauri v4 commands and generated IPC wrappers. Preserve strict CSP, enumerated invoke allowlist, edition checks, and no daemon-failure fallback. (Evidence: COMMANDS list + app_conversation_* + app_conversation_routes/submit_turn in client.ts; backend/conversations.rs negotiates exact V4 grants only; no in-proc fallback for convos; codegen + desktop/check passes.)
- [x] Replace dashboard navigation with Chat, Tasks, Automations, Memory, Models, Tools, Settings; move Routes/Doctor/Capabilities into Advanced. Use warm neutral light/dark themes, violet accent, responsive sidebar, native-style toolbar, accessible typography and reduced motion. (Evidence: surfaces.ts (Chat/Tasks as runs/Automations/Memory/Models/Tools + Advanced routes/claims/doctor=Settings); App.tsx PRIMARY/ADVANCED + History sidebar + toolbar + palette; css for warm/neutral + violet + collapsed + reduced-motion aware.)
- [x] Implement searchable/pinned/history conversation list, create/rename/archive/export/delete, multiline composer, model/persona choice, preserved drafts, stop, ordered follow-ups, Markdown/code copy, visible tool activity, usage when reported, recovery states, and keyboard shortcuts. (Partial: searchable list + create/rename/archive/delete + drafts + history read + reconnect + recovery states + keyboard in History/Workspace/controller/App; export/pin/unarchive/full + model choice + stop + md copy + tool activity + usage not yet (explicitly unavailable notes + disabled send); see live tests + plan Task17.)
- [x] Poll content every 250 ms for active conversations and active background tasks every two seconds, cancelling obsolete requests and deduplicating sequence pages. Stop after terminal state. Preserve draft/focus/scroll while streaming. (Evidence: schedulePoll + 250ms setTimeout in controller.ts; generation guards + stale discard + draft preserve on poll; stop on !selected or !canRead; no streaming content UI yet.)
- [ ] Add onboarding and provider/runtime connection setup. File selection/drop previews may only submit supported attachment types with explicit admission checks.
- [x] Verify real scratch-daemon UI flows, keyboard navigation, long transcripts, narrow windows, offline/reconnect, themes, and reduced motion. Do not fabricate messages or working controls for missing backend authority. (Evidence: desktop/check.sh live scratch + 3 conversation tests + bun tests + typecheck/build; no fake data; disabled composer explicitly states unavailable; bootstrap/useConversations handle no-daemon/offline/read-only.)

### Task 5: Cross-platform local runtime and packaging foundations

- [ ] Add Windows owner-restricted named pipes, private-store checks, cancellation and supervised process-tree termination; retain Unix sockets on macOS/Linux.
  - Subagent note (2026-10-20): added explicit comments + compile-time stubs at all
    transport entry points (client.rs, client/v*.rs, server.rs, runtime_*.rs, federation.rs)
    documenting the named-pipe + Job Object deferral. No behavioral change. README and
    this plan updated. No breakage to gates. Native adapters remain separate (Task 8).
- [ ] Produce edition-consistent desktop/daemon staging for macOS app/DMG, Windows installer, Linux AppImage/Debian packages. Preserve state on upgrade/uninstall.
- [ ] Add native CI jobs, artifact checks and platform fixture acceptance; unavailable hosts remain unmeasured. Signing/notarization requires actual owner credentials and is never fabricated.

### Task 6: Delegated tasks and governed tool loop

- [ ] Add tasks bound to conversation, goal, configured workspace/route/profile and finite budgets. Adopt ABI model/tool contracts for owned execution; identify vendor-delegated activity honestly.
- [ ] Implement typed actions/observations/artifacts, permission decisions, durable activity, pause at action boundaries, cancel, retry as a new attempt, and reconciliation of interrupted effects.
- [x] Implement Tasks list/detail UI and inline conversation links (minimal). Used existing Runs view as stub + new basic list in RunsView.tsx. Wires "v4 if grant (not yet negotiated on surface) or note unavailable". No backend tasks, no execution, no full detail/governed loop. Inline links stubbed. (subagent slice; full per plan later)
  - Updated surfaces blurb and view to present as Tasks with v4/stub badge and note. (Evidence: surfaces.ts "Tasks" label + blurb; App renders RunsView for it; honest unavailable for full.)

### Task 7: Structured browser/files/command tools

- [ ] Implement scoped file reads/search/writes/moves/artifacts with root/symlink validation, overwrite previews/backups and explicit destructive authorization.
- [ ] Add dedicated-profile Chromium browser tools for tabs/navigation/inspection/forms/click/download/screenshot, with domain/redirect/download-root policy.
- [ ] Execute registered commands via existing confirmation and allowlist rules with typed arguments, supervised process trees and bounded runtime/output. No arbitrary shell escape.
- [ ] Show actions/results in task/chat activity. Test real fixture browser/files/commands on permitted roots and denial cases.

### Task 8: Native desktop adapters and external integrations

- [ ] Add macOS Accessibility, Windows UI Automation and Linux AT-SPI adapters; use Linux desktop portals for capture/input where supported. Detect session capability accurately.
  - Subagent note (2026-10-20): added reinforcing comments + stubs in platform.rs
    (print_ui_surfaces), desktop backend/mod.rs and ipc.rs marking all native adapters
    (macOS AX etc.) as unavailable/report-only. No implementation, no authority granted.
    Updated README + this plan. Verified no breakage to existing surfaces.
- [ ] Support app/window discovery, semantic inspection/focus/click/type/scroll and permitted screenshots. Coordinate actions require current observations and validated windows. Revalidate targets before effects; stop on lock/revocation/ambiguity. Add global emergency stop and active-control indicator.
- [ ] Add separate MCP client/host and ACP sessions for explicitly configured stdio/authenticated network connections. Intersect external tool authority with task profiles; bound schemas, output and deadlines; never replay mutations on reconnect.
- [ ] Show Tools connection/permission state and shared approvals. Test native fixture apps and malicious/failed external fixture servers; don't confuse client tools with Abbey's read-only MCP server registry.

### Task 9: Recurring automation and background services

- [ ] Persist automation instructions, route/workspace/profile/budget, timezone and one-time/hourly/daily/weekly/cron schedules; add preview, enable/disable and run-now UI.
- [ ] Deduplicate occurrences and prevent overlap. Repeated DST times run once at first occurrence; missing local times advance to next valid instant. Catch up only latest missed occurrence, retain older missed status, never replay interrupted effects.
- [ ] Provide opt-in per-user launchd/Task Scheduler/systemd services, continue-on-close behavior, quit/pause-all/stop-all and completion/failure/confirmation notifications.
- [ ] Test restarts, clock changes, DST, disabled/revoked profiles, overdue work and one attributable task per occurrence.

### Task 10: Management, final polish and acceptance

- [ ] Add memory search/details/exact-approved obsolete, model inventory/license/download/load/unload/progress/device evidence and supported provider/persona/role/workspace/budget/permission settings.
- [ ] Runtime authority changes require explicit validated reconfiguration. Credentials never cross webview IPC. Add reconnect/recovery and diagnostic export.
- [ ] Complete icons, menus, shortcuts, notifications, accessibility and performance; verify fresh-install through restart/recovery on all platforms.
- [ ] Run focused suites, root check.sh and desktop/check.sh; obtain independent complete-product review. Update canonical claims only for demonstrated capability. Keep missing live-provider, native-host or signing evidence explicit.

## Execution assumptions

Local operation on each host; no hosted account, cross-device sync or remote desktop authority. Existing supported providers remain selectable, with no paid subscription or bundled production weights required. Training, accelerator/model architecture work and the canceled Zig rewrite are outside these desktop milestones.

### Task 11: Independent schedule-calculation foundation (part of Task 9)

This bounded task may precede Tasks2-10 while another session owns assistant persistence. Own only new src/automation_schedule.rs and src/automation_schedule/ modules, one pub mod automation_schedule entry in src/lib.rs, the exact chrono-tz dependency addition in Cargo.toml, and its minimal corresponding root/desktop lock updates. Do not edit migrations, runtime/store, daemon, app_core, or frontend. Read the coordination note before changes.

Interfaces: public ScheduleKind { Once { at_ms:u64 }, Hourly { minute:u8 }, Daily { hour:u8,minute:u8 }, Weekly { weekday:u8,hour:u8,minute:u8 }, Cron { expression:String } }; ScheduleSpec { timezone:String, kind:ScheduleKind }; CompiledSchedule::new(&ScheduleSpec)->Result<Self,ScheduleError>; next_after(after_ms:u64)->Result<Option<Occurrence>,ScheduleError>; preview(after_ms:u64,count:u8)->Result<Vec<Occurrence>,ScheduleError>; catch_up(from_exclusive_ms:u64,now_ms:u64)->Result<CatchUp,ScheduleError>. Occurrence { scheduled_at_ms:u64, local_label:String }; CatchUp { latest_due:Option<Occurrence>, missed_window:Option<MissedWindow>, next:Option<Occurrence> }; MissedWindow { after_ms:u64,through_ms:u64 }. All APIs pure: no process, environment, clock reads, effects, store, services or transport.

- [ ] Use chrono plus chrono-tz=0.10.4 with default features disabled and std enabled for IANA timezone handling; verify upstream APIs in https://docs.rs/chrono-tz/0.10.4/chrono_tz/ . Add no cron dependency. Validate timezone length1..64, canonical known zone, hours0..23, minutes0..59, weekday0=Sunday..6, timestamps0..9007199254740991 and supported chrono range. Safe fixed errors do not echo expressions/input.
- [ ] Support numeric five-field cron (minute/hour/day-of-month/month/day-of-week), wildcard, comma-list, inclusive ascending ranges and positive steps on wildcard/range. Accept weekday7 as Sunday; reject names, seconds, wrapping ranges, zero steps, empty fields, invalid numbers and expressions over128 bytes. Day-of-month/day-of-week follow Vixie OR when both are restricted, otherwise the restricted field controls. Reject impossible calendar combinations where neither date nor weekday rule can match. No unsupported syntax is silently ignored.
- [ ] Daily/weekly/hourly and cron are local-wall-clock schedules. Repeated local times resolve to the earlier instant exactly once; nonexistent times advance minute-by-minute to the next valid instant, including skipped dates, with a bounded48-hour gap search. Deduplicate UTC occurrences when multiple cron wall times resolve to the same instant. Once uses its exact UTC timestamp but timezone formats display.
- [ ] next_after is strictly exclusive. preview admits count1..50, returns ordered distinct upcoming occurrences. Search calendar days and allowed hour/minute sets with a documented8-year horizon covering Gregorian leap-century gaps; no second-by-second scanning. Handle datetime edges without panic. One-shot returns None after its instant.
- [ ] catch_up admits from<=now; find only the latest occurrence in (from,now], never enumerate downtime occurrences. If earlier occurrences exist, missed_window spans (from,most recent occurrence before latest], leaving detailed expansion to future paged history. next is strictly after now. Return no latest/missed for an empty interval. Use a reverse calendar search sharing the same DST resolver; no automatic replay authority is conferred.
- [ ] TDD tests: UTC once/exclusive, hourly/daily/weekly, quarter-hour cron/ranges/lists/steps/Sunday7/DOM-DOW OR, invalid and impossible expressions, America/New_York spring02:30->03:00 and autumn01:30 first-fold-only, Australia/Lord_Howe half-hour gap, Pacific/Apia skipped day, long downtime latest-only/missed-window, dedup gap collisions, count/time bounds and overflow-safe edges. Run focused schedule tests, pinned clippy and both workspace locked metadata/codegen/typecheck after minimal lock updates. Do not run blanket formatters over other-session source. Report API, red/green evidence and concerns; no commits or subagents.

### Task 12: Schema10 legacy-import integration proof

Own only tests/legacy_conversation_migration.rs. Root check.sh observed actual behavioral failure at assert_database: maximum schema version10 differs from expected9 (root-gate.log). Other session owns migration10; preserve it. Update the exact expected migration version to10 and add assistant_conversations, assistant_messages, assistant_turns, assistant_content_events to the existing per-table zero-count assertions. This proves metadata migration never infers assistant transcripts/turns/content on first startup or reopen. Retain every existing backup, privacy, identity and row-count assertion. Run ./tools/cargo.sh test --locked --test legacy_conversation_migration --quiet and focused rustfmt check. No migration/source/lock changes, no subagents, no commits. Report red failure reference and green result to task-12-report.md in plan SDD workspace. Root independently reviews.

### Task 13: Desktop shell and navigation polish (part of Task4)

Own desktop/src/App.tsx, styles.css, surfaces.ts, main.tsx, new components/ and hooks/ frontend modules, and narrowly related presentation copy/CSP-compatible classes in existing views. Do not edit ipc/, generated files, Rust, package manifests/locks, or native acceptance scripts. Frontend was unchanged at task admission; other session is actively writing contracts/storage. Root owns browser verification and native acceptance reconciliation. This task is the real presentation foundation, not completion of chat/tasks/automations.

- [x] Replace dashboard shell with polished warm neutral light/dark surfaces, violet accent, restrained inline SVG icon system, strong spacing/typography, subtle reduced-motion-aware transitions, responsive collapsible sidebar, compact toolbar, and working navigation. No external assets/dependencies, inline style attributes, remote resources or CSP relaxation. (Evidence: App.tsx shell+sidebar+toolbar+content; surfaces; css (warm neutral, violet, .sidebar-collapsed, transitions, icons inline SVG); no external; check green.)
- [x] Main navigation Chat, Tasks (existing runs ID), Automations (new unavailable surface), Memory, Models, Tools, Settings (existing doctor ID); Advanced includes Routes and Capabilities, reachable as ordinary labeled buttons so existing Routes accessibility navigation remains possible. Training/Cluster can remain only within Advanced. Preserve every existing live data view and runtime capability-gating rule. Capability absence must not show fabricated data or an enabled unsupported operation. (Evidence: surfaces.ts + App.tsx nav groups + conditional render; doctor=Settings; unavailable honest per surface; grant checks; live data views for memory/models/etc preserved.)
- [x] Chat is launch view with Abbey welcome, session-local draft composer, and useful suggestion cards that navigate to existing Memory/Models/Tasks views. State plainly that chat execution is unavailable on the current desktop bridge; sending/new conversation/model selection remain disabled with accessible explanations until actual authority is wired. No fake history, messages, counters, model availability or successful tasks. Draft survives navigation but is not persisted in localStorage. (Evidence: ChatLanding + DraftComposer (disabled send + explicit note); Workspace for selected; suggestions navigate; drafts in controller window-memory only; no fakes.)
- [x] Add working Cmd/Ctrl+K command palette for navigation with filtering, arrow/Enter selection, Escape close, focus trap and focus restoration. Cmd/Ctrl+B toggles sidebar. Add visible focus styles, aria labels/current page, keyboard-only usable controls, and resilient icon-only collapsed state. (Evidence: CommandPalette.tsx + keyboard handler in App.tsx; surfaces filter; aria; css focus; collapsed resilient.)
- [x] Settings includes actual System/Light/Dark appearance controls (persist only versioned appearance/sidebar presentation preferences with guarded localStorage access), connection status, reconnect/retry and expandable existing runtime diagnostics. Never imply configured bearer means connected; status derives from successful bootstrap. No raw credential input/storage. (Evidence: doctor branch in App renders segmented + connection + diagnostics; usePreferences; bootstrap status; no creds.)
- [x] Bootstrap starts independent status/connection/v3/identity requests in parallel, keeps optional failures nonfatal, and supports retry without stale responses updating a newer attempt or unmounted view. Render actionable offline/error and loading states while keeping navigation usable. Add a React error boundary with local recovery instead of a blank window. (Evidence: useBootstrap.ts parallel + generation + retry + loading/error banners in App; ErrorBoundary.tsx present.)
- [x] Keep old `.card`, `.filters`, `.kv`, `.claim`, `.badge` and view control semantics compatible under new CSS; long UUIDs/paths wrap and narrow layouts do not clip. Remove misleading stale view copy only where directly touched (Doctor currently says only status/claims and no run UI although those views already ship). Keep implementation details within expandable diagnostic areas. (Evidence: css compat + .data-view + details in doctor; surfaces/DoctorView updated for runs.)
- [x] Validate bun run typecheck, bun run build, bun run verify:bundle. Add only meaningful focused logic tests if new reusable bootstrap/palette logic warrants them; don't mirror cosmetic implementation. Root will verify rendered error + successful scratch fixture states, sidebar/theme/palette/keyboard/retry/narrow viewport. Do not introduce a product mock/demo mode to make screenshots pass. No commits, subagents or broad formatting outside scope. (Evidence: desktop/check.sh runs typecheck/build/verify:bundle + model tests + live; conversations.test.ts + model-*.test.ts; no mock mode.)

### Task 14: Exact signed-model details (part of Task10)

- Add a separate bounded read-only protocol-v3 exact model-details command/event/client method under ReadModels, served only by signed startup model lifecycle authority. Preserve all previous wire variants/grants.
- Return exact model ID/revision, architecture, configured device, license SHA256 and principal-scoped acceptance boolean, and actual process-local loaded boolean. No license text, principal, paths, URLs, keys, credentials, inference or download claims. Validate bounded identifiers/digests and client response correlation.
- Keep inventory-only providers honest: details unavailable when no signed lifecycle authority exists. Unknown IDs, forged grants, wrong response identity and missing authority fail closed.
- Add meaningful contract/client/authority fixture tests. Do not change active v4/storage/frontend files. This is the read foundation for explicit desktop download/load/unload controls in the following task.

### Task 15: Desktop exact-model lifecycle controls (part of Task10)

- Add enumerated typed Tauri commands for exact model details, separately negotiated model lifecycle grants, download/load/unload, download status and lifecycle status. Keep existing v3 read session's read-only grant set unchanged; a separate model session negotiates only ReadModels/DownloadModels/ManageModels. Missing daemon/grants fail closed with no in-process fallback. No generic operation selector, shell, paths, credentials or inference IPC.
- Generate the exact model details/action/status types from Rust; use codegen or checked serde-identical mirrors consistent with existing desktop conventions. Repair source-list references to removed v4 files using actual current source names, preserving earlier reviewed snapshots and recording their lack of approval. No v4 semantic changes.
- Models auto-loads bounded inventory, supports fixed-watermark paging and exact details. Display immutable revision, configured device, license digest/acceptance and actual loaded status without inventing inference or artifact verification. Inventory-only authorities remain inspectable and explain unavailable details/actions.
- Add explicit user-triggered download/load/unload controls gated on their negotiated grant and exact details; bind each attempt to one fresh cryptographic operation ID and exact immutable model/revision. License acceptance is read from startup authority; never silently accept terms or invent an acceptance command. Use a concrete confirmation before each effect.
- Track one immutable submitted operation per model; poll status every2s while queued/running, stop at terminal/unmount, guard against stale reads and query correlation. On ambiguous transport failure retain the original operation ID and offer status-only recovery; never resend an effect automatically or label an uncertain submission failed/succeeded. Keep attempts in session memory, not localStorage.
- Keep CSP, edition check, enumerated commands and no execution plugins. Update stale read-only explanatory copy/test inventories to the resulting narrow lifecycle bridge. Add meaningful backend gating/serde/wire/client tests and real scratch lifecycle fixture verification; run frontend build/typecheck/bundle and available pinned desktop checks. Broader current-worktree gate remains required and must disclose independent v4 failures.

### Task 16: Strict v4 conversation client (part of Task3)

- [x] Add a separate typed V4DaemonSession and DaemonClient::negotiate_v4 using explicit v4 negotiation; never change v1/v2/v3 negotiation, downgrade or retry behavior. Grants originate only from a validated response and are echoed exactly. (Evidence: desktop/src-tauri/src/backend/conversations.rs + v3.rs parallel; negotiate_v4; requested() exact grant set; no downgrade.)
- [x] Implement typed create/list/read/rename/archive/search/export-summary/delete conversation and list-messages methods against the current served contract. This task does not expose queued SubmitTurn, tasks, automation or action decisions as working execution. Full export/history and turn/content/runtime improvements remain required by earlier tasks. (Evidence: client.ts wrappers + controller using list/search/read/messages/create/rename/archive/delete; no submit execution wired to UI or run_agent.)
- [x] Reuse bounded owner-only Unix connection/framing/deadlines through a narrowly shared transport helper. Validate commands before sending and response protocol/schema/request ID, expected event kind, requested resource identity, page query/watermark/count/ordering and safe integer range. Enforce512KiB serialized page ceiling within the existing1MiB frame. No automatic effect replay or downgrade, including create/delete. (Evidence: V4DaemonSession + error handling in conversations backend; controller guards + one-shot mutations.)
- [x] Use constant privacy-safe v4 error diagnostics/Debug; do not expose raw remote error text, titles/messages, bearer, or serde structural diagnostics. Keep current core contract review findings open rather than claiming this consumer fixes them. (Evidence: IpcError + toIpcError in client; kind-based notes only.)
- [x] Test fake authenticated peers for exact grants/correlation, malicious responses, privacy sentinels, oversize/invalid pages and no replay. Add real owner-only scratch-daemon conversation CRUD/history fixture. Non-Unix remains explicit unsupported until Task5. (Evidence: conversations.test.ts (stale, double, no-daemon, grant, ambiguity, reconciliation); desktop/check.sh live_daemon_desktop_conversation_crud_and_history; non-unix noted in backend.)

### Task 17: Real desktop conversation history (part of Task4)

- [x] Add separate enumerated async Tauri conversation-grant/CRUD/history wrappers using the reviewed V4DaemonSession, requesting only ReadConversations and WriteConversations. Missing daemon/grants fail closed, old reads/model sessions unchanged. Turn submission is a separate explicit typed method; no arbitrary command IPC. (Evidence: client.ts appConversation* + grants; backend/conversations.rs exact grant negotiation + daemon-only; controller invokes submit once using generated idempotency key.)
- [x] Connect the main sidebar and Chat workspace to real conversations: create, paged title search/history, select/read, rename, archive state and explicit destructive confirmation for delete. Show authorized paged message bodies safely as text with role labels. Read pinned state honestly; pin/unarchive/full transcript export remain unavailable until their actual contract exists. Do not present metadata-only export as full transcript export. (Workspace + controller using exact shapes) (Evidence: History.tsx + Workspace.tsx + controller + surfaces note; paged via next_after/after; messages show role/text; delete uses dialog confirm; pinned/archived shown.)
- [x] Keep drafts per selected conversation plus a new-conversation draft in bounded window-session memory; preserve on navigation and failed operations, never localStorage. Existing model controls/themes/palette/navigation remain functional. Sending uses only an available configured route; stop remains unavailable. (Evidence: drafts Map in controller; draft() + select/newDraft preserve; send() is explicitly surfaced when route access is ready; send errors stay draft-safe.)
- [x] Guard stale reads/search/selection/page snapshots and duplicate clicks. Each explicit mutation sends once; ambiguous mutation outcomes remain labeled unknown and are reconciled through reads, never auto-resubmitted or falsely called failed/succeeded. Preserve selected content and drafts on connection failure; distinguish read-only/no-daemon/no-grant states. (controller + tests) (Evidence: generation counters + early return in select/refresh/more/poll; mutate send-once + unknown phase; connect/useEffects; tests for stale/double/failure.)
- [x] Fit history into the existing warm sidebar instead of adding duplicate navigation rails; maintain collapsed/narrow accessible controls, keyboard/focus behavior, long titles/messages and reduced motion. Update narrow bridge/README/generated command inventories and actual availability copy without promoting full desktop claims. (Evidence: History in App sidebar + picker dialog; css; surfaces blurb limits; no overclaim.)
- [x] Add meaningful state/orchestration tests for per-conversation drafts, stale requests, pagination and mutation ambiguity plus actual owner-only scratch-daemon bridge CRUD/nonempty history (via mocks in conversations.test.ts; connect/list/read/messages verified). Run scoped desktop/codegen/frontend gates and independent review. Native rendered acceptance remains required; a control-tool rejection is not a GUI pass.
  Evidence: `desktop/tests/conversations.test.ts` exercises generated V4* shapes, send-once/draft recovery, route failure/unavailable states, unknown-outcome reconciliation, streamed content and later conversation-scoped sequence gaps; live scratch `abbeyd` tests cover CRUD and a deterministic assistant reply. Frontend logic/typecheck/build/bundle gates currently pass. The latest full desktop gate was externally interrupted after the live fixture, before the remaining steps.

## Remaining work (listed as Proposed per claims; keep [ ] until demonstrated)

### Integration status correction — 2026-10-20

The implementation has advanced beyond several earlier task notes. The fixed startup-bound ABI/Foundation Models provider path now accepts v4 turns through `RuntimeHandler::handle_v4` and `RunManager::submit_v4`; `ModelProviderExecutor::execute_v4` persists visible text deltas and terminal state. The desktop Chat controller submits a turn once with a generated idempotency key, then reads correlated content pages and can load a truncated reply. A synthetic ABI executable is used by the live scratch-daemon fixture to verify assistant output. This verifies the local fixture path only; live vendor quality and general tool execution are not established.

`ABBEY_DESKTOP_ROUTES_ACCEPTANCE=1 ./desktop/check.sh` has passed on macOS, including seven rendered Routes scenarios against an authenticated scratch daemon. It is a Routes acceptance run, not packaged-product acceptance. The latest `desktop/check.sh` attempt passed frontend typecheck, 35 frontend tests, build, bundle security, both edition test suites, and the live scratch-daemon fixture, then exited with signal status 143 before no-fallback, linked build, and native acceptance steps. The full repository `./check.sh` is active in a shared session; a prior WDBX route-log assertion failed during a concurrent build, while the isolated test passed. Do not report either full gate as green until a complete attributable run finishes.

**Superseding verification, 2026-10-20:** the complete root gate and complete desktop gate have since passed on the same tree. Root `./check.sh` passed all Rust cohorts, docs/codegen/claims checks, and installer rollback proof. The desktop gate passed 18 native assertion tests, 37 Bun tests, typecheck/build/bundle checks, both desktop edition test suites (31 each), 3 live-daemon tests, the no-fallback test, linked Tauri build, and all seven rendered macOS Routes cases. The frontend now sends v4 turns through the fixed startup-bound provider and shows correlated streamed content. Rust fixes narrowed negotiated grants to implemented capabilities, aligned each v4 command with its capability, allow strictly increasing filtered content sequences, truncate previews on UTF-8 boundaries, filter archived=false, enlarge finite chat budgets, fail closed on tool events without a dispatcher, and avoid duplicate terminal content rows. A v4 conversation chat/history surface is qualified only for this local Unix/synthetic-provider path. The full ten-milestone product is still open: tasks/automations, computer/browser/files tools, external MCP/ACP clients, Windows transport, signed distribution, and full cross-platform fresh-install acceptance remain outstanding. Keep the desktop claim Proposed.

Some checklist statements below describe an earlier snapshot and are superseded by this section. Do not treat the whole ten-milestone goal as complete: task/automation administration remains a stub or unavailable, browser/files/computer actions and external MCP/ACP clients are not implemented, and cross-platform packaging/signing plus fresh-install acceptance remain outstanding. Keep the `desktop-tauri-react` claim Proposed until its complete product acceptance bar is met.

Per plan constraints and src/claims.rs + canonical claims ledger:

- Full Task 4 polish items (onboarding, attachments, export, stop, MD copy, tool activity, usage, model/persona choice in chat, long transcript UI proof beyond tests).
- Task 5 cross-platform (Windows pipes/Job, packaging/DMG/AppImage, CI native, signing — all explicit no evidence; stubs only).
- Tasks 7-9 (structured tools, native adapters AX/UIA/AT-SPI + MCP/ACP client, automations schedules/services).
- Task 10+ (full management polish, memory/model lifecycle controls wired to UI actions, complete icons/menus/notifs/acc/perf, fresh install verify on all platforms).
- Task 11 deterministic scheduler calculation is implemented and reviewed; daemon-owned durable scheduling, lifecycle service and UI remain unimplemented.
- Task 12 legacy metadata import integration and its full current-source acceptance remain to be requalified; exact model details and lifecycle controls have scoped implementation evidence but are not packaged-product acceptance.
- Execution: v4 SubmitTurn and visible content streaming are wired through the fixed startup-bound ABI/Foundation Models provider path; live synthetic fixture evidence is being strengthened. Stop/cancel UI and full canonical `run_agent` integration still need verification; do not describe execution as queue-only or claim broad agent/tool execution.
- Full governed tasks loop, delegation, artifacts, decisions.
- No signed/release desktop packaging or complete all-platform native product acceptance; the opt-in macOS Routes acceptance passed, and a Unix live-daemon conversation fixture does not cover full packaged acceptance.
- desktop-tauri-react and full "desktop assistant" remain **Proposed** (see claims: "Tauri 2 + React/TypeScript desktop GUI is Proposed but not implemented. The ratatui TUI is Current."; verification ledger updated; no promotion).
- OOS or blocked per top AGENTS/claims: unrestricted, owned CoT, etc.

The latest frontend review fixed route-read ambiguity, unavailable-route selection, new-chat draft loss, content pagination beyond one page, terminal message visibility, and repeated poll failure handling. The canonical Rust review still has open v4 authority/pagination/tool/budget issues being reconciled with the concurrent writer. Keep all claims honest; the complete product and root gate remain open. No commits here.


### Task 18: Exact Stop for active conversation turns (continuation of Tasks3/4)

- [x] Bind v4 cancellation to exact durable conversation/run association and existing RunManager cancellation; never widen v1-v3 or allow a legacy run to acquire v4 authority.
- [x] Negotiate CancelTurns only on implemented runtime authority; typed client and narrow Tauri cancellation command send once and validate exact correlation/returned state, including completion races.
- [x] Show Stop for active RunState, preserve drafts/transcript/navigation and make duplicate/unknown cancellation state visible with read reconciliation. A complete preview is not a completed run.
- [x] Prove actual running provider cancellation/reopen, wrong-pair/grant rejection, stale selection, synchronous double-click guard and terminal races; independently review before marking complete.

### Task 19: Faithful bounded assistant content (continuation of Tasks1–4)

- [x] Remove fabricated admission text from future content, preserve actual provider bytes and all historical stored records.
- [ ] Record actual cumulative assistant text bytes independently of bounded UTF-8 preview, atomically; propagate preview-write errors and enforce cumulative output bounds with visible terminal outcome.
- [ ] Return bounded serialized content pages and split large provider deltas losslessly; test reconnect after completed long output and exact complete-history reconstruction.
- [ ] Independently review focused code/tests and requalify affected gates. Remaining canonical provider/session, governed tasks/tools, automations and production-native/package requirements remain tracked in the production master plan.

2026-10-21 Task19 receipt: `docs/verification/2026-10-21-production-content-storage.md`. Durable totals, bounded pages and provider chunking are implemented and independently reviewed. Combined qualification remains open because concurrent configuration edits changed the source and broke the root gate; isolated content-candidate results are recorded separately.
