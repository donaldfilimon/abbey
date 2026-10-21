# Abbey Production Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax. Donald has already selected subagent-driven implementation and repeatedly authorized continuation. No commits or pushes without a separate request.

**Goal:** Complete the approved Abbey desktop assistant and the remaining Abbey production roadmap, with reproducible, independently reviewed source and installed-artifact evidence.

**Architecture:** Retain the Rust application and shared application core; CLI, TUI, daemon and Tauri consume the same session/runtime authorities. Extend narrow protocol-v4 capabilities for desktop content/tasks/actions/automations while preserving v1-v3 and read-only MCP. Production model/compute/training/media programs consume ABI/WDBX contracts through coordinated immutable handoffs.

**Tech Stack:** Rust 1.99.1, Tauri 2, React/TypeScript, Bun, SQLite/WDBX, ABI provider/tool/model/worker contracts, native OS adapters.

**Spec:** `docs/superpowers/specs/2026-10-20-production-completion.md`; the earlier detailed desktop subplan remains `docs/superpowers/plans/2026-10-20-desktop-assistant.md`. This master orders and extends that plan; it does not cancel unfinished items or revive Zig.

## Global Constraints

- Preserve retained Rust, exact qualified toolchain pins, legacy/user data, unrelated dirty work and edition isolation. The Zig rewrite/cutover is canceled.
- Canonical default-branch checkout; explicit per-task file ownership, one implementation writer per shared interface, independent review. Do not commit/push/publish or edit sibling-owner work without the corresponding authorization/handoff.
- Shared canonical provider/session execution; no second desktop engine. Legacy fixed startup authority remains intact while the new v4 path is implemented.
- New effects use separately negotiated narrow v4 commands; v1/v2/v3 and read-only MCP retain their established boundaries. No stub capability is advertised.
- OS command effects retain mandatory confirmation and allowlist enforcement in both editions. External content never supplies permission.
- Store prepared intent before acting; immutable exact approvals; ambiguous effects never automatically replay. Keys, passwords and raw diagnostics never cross WebView IPC.
- Rust main stays below 200 lines; other modules below 1,000 (split before the 800-line warning becomes a structural problem).
- Verify current exact source/artifact inputs. Tests, live providers, native hosts, CI, signing and distributed deployments are separate evidence classes.

## Current-state evidence and open gaps

Inspected on 2026-10-20; current worktree is authoritative. This is a moving shared checkout, not a frozen release candidate.

| Area | Existing evidence | Remaining requirement |
|---|---|---|
| Retained Rust | Rust 1.99.1 root gate passed in prior implementation turn | Freeze final Abbey/ABI/WDBX inputs; rerun after final repair wave |
| Desktop shell/history | Chat/history, themes/palette, read/model views; 37 Bun tests, both edition bridge suites, live daemon fixture, 7 macOS Routes scenarios | Full chat/native rendered journeys and remaining controls |
| Chat runtime | V4 admits/executes fixed ABI/FM routes and persists content | Canonical backend/session path, local-first Ollama and all supported provider selections, multi-turn context, Stop and exact recovery |
| V4 administration | Task/automation/permission/action declarations and tables exist | `v4_admin.rs` methods are stubs; do not count schema/type names as functionality |
| Tools/runtime | Canonical read-only MCP and exact-approved memory effect; recording-only governance substrate | Owned tool loop, scoped adapters, external client-host, approvals UI |
| Scheduling | `CompiledSchedule` implements deterministic IANA/DST/cron calculation | Durable occurrences, task admission, service, UI and notifications |
| Native packaging | Tauri metadata/icons/entitlements and macOS release script exist in dirty work | Audit script/artifact selection, daemon embedding, actual signed/notarized clean-install proof |
| Platforms/CI | Unix transport; macOS local gate and limited adjunct CI design | Windows pipes/Job Objects, actual Windows/Linux GUI/service/install/CI execution |
| Model/compute/media | Signed lifecycle + fixture inference, Metal numerical kernels, curated data, platform/delegated voice/media | Production weights/architecture, accelerator inference/training, reviewed adapters and offline neural media |
| Federation/mesh | Strict local read-only federation and same-host multiprocess proof | Durable authority integration, three-VM proof, then separate production multi-host proof |
| Claims | 42 Current / 3 Partial / 8 Proposed / 1 Blocked / 5 Out of scope | Close each exact boundary with its own evidence; preserve deliberate refusals |

### Release and project gates

**Core desktop acceptance checkpoint:** Tasks 1–22, with no missing D1–D8 requirement from the spec. This includes actual tasks/automations/tools and supported-host packages; a passing Routes test alone cannot close it. The existing Completion Program also requires functional Training/adapters and Cluster/workers desktop views, so full desktop-product acceptance additionally depends on the corresponding Tasks 23–28 and the final Task30 review. The core checkpoint is an intermediate delivery, not permission to close those remaining surfaces.

**Full Abbey completion:** Desktop gate plus Tasks 23–30 and every approved open roadmap requirement. Production local models, training/media and distributed proof remain visible even if a desktop release happens first. External prerequisites can block their own gate; they cannot disappear from the project definition.

**Execution order:** 1 → 2 → 3/4 → 5 → 6 → 7 → 8/9/10 → 11/12 → 13 → 14 → 15 → 16/17 → 18 → 19/20 → 21/22. Then complete 23 → 24 → 25/26 → 27/28/29 → 30. Independent read-only audits may overlap; mutations to common contracts/codegen and broad Cargo gates are serialized.

## Review Focus

1. Lost responses, process death and retries must not duplicate an effect or mislabel an unknown outcome (Tasks 2, 4, 7, 13, 22).
2. Conversation, workspace, edition, route, revision and target identity must not be substituted between preview/approval/dispatch (Tasks 2, 3, 6–12, 15, 27).
3. Malicious model/tool/MCP/web/filename text must remain data, never permission or executable syntax (Tasks 7–12, 19).
4. Revoked profiles, expired grants, locked screens, changed windows and symlink/redirect changes must stop the exact operation (Tasks 8–14, 19).
5. Cached binaries, stale signatures, zero-job CI, skipped hosts and mutable sibling sources must not create false release evidence (Tasks 1, 16, 17, 20–22, 30).

---

## Workstream A — Correct and complete the shared assistant

### Task 1: Establish source ownership and reproducible qualification inputs

**Files:** `.superpowers/coordination/desktop-assistant.md`, plan ledger, `.github/workflows/rust.yml`, `Cargo.lock`, `desktop/Cargo.lock`, `docs/verification/`.
**Interface:** produce an exact source manifest for Abbey/ABI/WDBX (HEAD plus dirty content hashes), toolchain, lockfiles, generated IPC and package inputs; do not label a mutable checkout as an immutable release.

- [ ] Inventory every dirty/untracked file and active writer; reserve files and preserve snapshots before edits. Reconcile stale task/claim narratives against current source without deleting historical receipts.
- [ ] Audit feature-matched executable selection and Cargo target/profile configuration. Add regression controls that deliberately leave a usable stale executable and prove the gate chooses the current compiled artifact.
- [ ] Record known-green baseline and exact failures; publish owner-ready sibling handoffs only after their own gates. Final CI pins must name public immutable qualified revisions.

### Task 2: Finish v4 contract, storage and recovery correctness

**Files:** `src/app_core/v4*.rs`, `src/runtime/store/v4*.rs`, `src/runtime/migrations.rs`, `desktop/codegen/`, `src/daemon/client/v4.rs`; new focused `tests/daemon_conversations.rs`.
**Interface:** existing `V4SubmitTurn`, conversation/message/content queries and pages; one authoritative definition per type. `V4ContentPage` carries exactly correlated increasing conversation-global sequences filtered by run.

- [ ] Add failing cases for full multi-turn replay/reopen, paged title search cursors, new writes during fixed snapshots, cross-run cursors, corrupted selected records, terminal immutability and maximum serialized response.
- [ ] Enforce cumulative 4 MiB/65,536-event turn limits and 16 KiB UTF-8 preview while recording actual total text bytes. Remove synthetic `[admitted]` text from assistant content; use typed lifecycle/notice metadata. Ensure a long reply completed between polls is still reconstructed in full.
- [ ] Preserve atomic admission/idempotency and legacy retention; test crashes around transaction boundaries and delete/archive with active work. Run `./tools/cargo.sh test --lib v4_`, the new process test, codegen `--check`, and independent review.

### Task 3: Use the canonical session/provider path for desktop chat

**Files:** `src/runtime/provider.rs`, `src/runtime/provider/process.rs`, `src/agent/{mod,streaming,argv,backend}.rs`, `src/runtime/manager.rs`, `src/daemon/runtime_{config,handler}.rs`; new focused `src/runtime/conversation_executor.rs` if needed.
**Interface:** `Executor::execute_v4` delegates to the shared canonical conversation adapter. Startup-owned route descriptors select a known provider/model/session policy; request data cannot select binaries/argv/environment.

- [ ] Add fixture tests proving two-turn context, independent conversation identity, exact vendor resume, fresh-session semantics, interruption, no accidental cloud route and no cross-backend flags.
- [ ] Integrate local-first Ollama, Foundation Models and supported ABI/Claude/Grok/Cursor routes using existing backend-specific execution/capture/stream adapters. Expose availability and explicit route/model/persona choice; unsupported configuration remains unavailable.
- [ ] Prove identical canonical behavior from CLI/TUI/daemon/desktop fixture entry points. Keep existing v2 fixed-route authority and old wire contracts unchanged; verify missing provider, cancellation, budgets and transcript continuity before live optional-provider tests.

### Task 4: Deliver exact Stop/cancel and explicit retry

**Files:** Task 18 in the desktop subplan/brief; `src/daemon/runtime_handler.rs`, `src/daemon/client/v4.rs`, `src/runtime/store/v4_turn.rs`, desktop conversation bridge/controller/workspace/tests.
**Interface:** `CancelTurn(conversation_id, run_id)` checks the exact durable v4 association before `RunManager::cancel`; `TurnCancelled` preserves honest completion races. A retry creates a new explicit attempt only after original state is known.

- [ ] Implement and independently review Task 18 (currently assigned): negotiated runtime-only cancellation, send-once typed client and narrow IPC, Stop UI, draft/navigation preservation, unknown-result read reconciliation.
- [ ] Add explicit failed/interrupted-turn retry using a fresh idempotency key and displayed attempt relationship; never retry an unknown accepted send or unresolved effect.
- [ ] Verify actual slow-fixture cancellation and descendant teardown, queued/running/completed races, wrong conversation/legacy-run denial, double-click and lost-response behavior.

### Task 5: Finish the chat product experience

**Files:** `desktop/src/conversations/`, `desktop/src/components/ChatLanding.tsx`, `desktop/src/styles.css`, `desktop/src/ipc/client.ts`, `desktop/src-tauri/src/commands.rs`; new focused attachment admission/export modules in `src/app_core/` and `src/runtime/`.
**Interface:** typed bounded attachment references and full transcript export (distinct from metadata export), exact conversation patch fields, available route descriptors.

- [ ] Add pin/unpin, archive/unarchive, complete transcript export, safe markdown/code/copy, text selection, authoritative usage labels and long-history virtualization. Preserve full Unicode text and show incomplete/failed output honestly.
- [ ] Add bounded file attachment admission with explicit root/size/type validation, escaped names, no arbitrary asset protocol, and clear removal/rejection states. Bind accepted content to the submitted turn.
- [ ] Test rendered multi-turn chat, first-run/empty/offline/error states, draft recovery, 10,000 messages, concurrent navigation, streaming scroll follow and 200% zoom. Record real WebView journeys, beyond controller mocks.

## Workstream B — Tasks, actions and automation

### Task 6: Replace task/profile placeholders with durable contracts

**Files:** new `src/app_core/v4_task.rs`, `v4_permission.rs`; replace stub definitions in `v4.rs`/`v4_types.rs`; new `src/runtime/store/tasks.rs`, `permissions.rs`; additive migration after current schema (never rewrite an applied migration).
**Interface:** `TaskSpec {conversation_id, goal, route_id, workspace_id, profile_id, budget}`, `TaskSnapshot {task_id, attempt, state, run_id, timestamps}`, opaque profile/workspace identities, bounded paged queries. Generate exact Rust/TS shapes once.

- [ ] Define strict serde/state-transition/byte-bound contracts and redacted debug; add unknown-field, invalid-ID, profile-revocation, wrong-workspace and edition tests first.
- [ ] Persist create/read/list/update/cancel/retry-attempt relationships and immutable route/profile/budget snapshots; never infer successful tasks from runs or return fabricated records from `v4_admin.rs`.
- [ ] Serve only implemented dedicated grants, typed client/Tauri operations and actual Tasks list/detail with conversation navigation. Test restart, paging, missing permissions and mutation ambiguity.

### Task 7: Connect the governed agent/tool loop

**Files:** new `src/runtime/agent_loop.rs`, `tasks.rs`; reuse `src/runtime/store/governed.rs`, tool approval/execution stores and ABI agent-host/runtime contracts; new action contracts in `src/app_core/v4_action.rs`.
**Interface:** typed action preview/observation/artifact/outcome bound to task attempt, canonical input digest, profile version and target observation; ABI lifecycle prepares before dispatch.

- [ ] Add model-produced structured calls through registered schema validation, current policy, exact approval, durable prepared intent, bounded execution and terminal result. Raw model text never executes.
- [ ] Implement pause-at-action-boundary, approve/deny/cancel, fresh-attempt retry, emergency stop and interrupted-effect reconciliation. Observe revocation before each action; no reconnect replay.
- [ ] Kill fixture processes after prepare, during effect and before result commit. Assert at most one admitted effect, honest interrupted state, preserved result digests and UI activity/approval/artifact trace.

### Task 8: Scoped file tools and artifacts

**Files:** new `src/tools/files/{mod,policy,operations}.rs`, `src/tools/artifacts.rs`; register through Task 7; focused fixture tests under `tests/tools_files.rs`.
**Interface:** root-handle-relative typed read/search/write/move/delete/export operations; exact before-state/overwrite preview, bounded content and opaque artifact IDs.

- [ ] Prove refusal of traversal, symlink swaps, hardlink/alias ambiguity, special files, root replacement, oversized files and changed overwrite targets.
- [ ] Implement authorized writes with atomic publication/backups and separately confirmed destructive operations; transcript deletion cannot imply filesystem deletion.
- [ ] Exercise real fixture directories, process interruption and recovery; display exact path/root and planned overwrite before consent without leaking unrelated paths to audit.

### Task 9: Dedicated-profile browser tools

**Files:** new `src/tools/browser/{mod,session,policy}.rs`, `desktop/src/tools/`; fixture web server/tests in `tests/tools_browser.rs`.
**Interface:** bounded browser observation and typed tab/navigation/inspect/form/click/download/screenshot operations; session/profile IDs, current observation version and domain/download-root policy.

- [ ] Establish an explicitly owned dedicated browser profile and session; do not adopt personal logged-in profiles or expose browser credentials to model/IPC.
- [ ] Enforce domain/redirect/popup/download rules at dispatch and completion; re-observe targets before effects and stop on ambiguity or navigation changes.
- [ ] Use a real local fixture site to prove allowed forms/downloads and denied redirects, stale targets, prompt injection, oversized output and malicious filenames. Surface permission/connection state and action outcomes.

### Task 10: Registered commands with supervised execution

**Files:** `src/os_control.rs`, `src/runtime/supervisor*`, new `src/tools/commands.rs`, action policy/UI fixtures.
**Interface:** registered command ID plus typed arguments; existing allowlist and explicit confirmation remain the authority. No free shell string.

- [ ] Add schema/argument/environment/workspace validation and an exact approval preview for each command recipe.
- [ ] Reuse bounded capture, finite timeout, process-tree teardown and durable effect records on Unix and Windows; refuse unregistered launchers/paths and injection.
- [ ] Run success/denial/cancel/timeout/descendant fixtures with both editions. Prove emergency stop reaps owned processes and leaves unknown external effects labeled unknown.

### Task 11: Native desktop inspection and control

**Files:** new `src/tools/desktop/{mod,observation,macos,windows,linux}.rs`, native helper projects where required, desktop Tools/permission/active-control UI; platform fixtures.
**Interface:** stable opaque app/window/element observations with generation/target binding; typed focus/click/type/scroll/capture actions and global stop token.

- [ ] Implement macOS Accessibility/screen capture, Windows UI Automation and Linux AT-SPI/desktop portal adapters under one bounded contract; report permission/session capability precisely.
- [ ] Revalidate process/window/element identity and lock/revocation immediately before effects; stop on stale observations, secure input, inaccessible targets or changed foreground state. Persist visible active-control indication.
- [ ] Exercise native fixture apps on each host, including disappearing windows, permission revocation, lock screen, scale/multimonitor, cancellation and screenshot policy. Platform source compilation alone cannot close this task.

### Task 12: External MCP and ACP client-host sessions

**Files:** new `src/tool_host/{mod,mcp,acp,policy}.rs`, configured connection contracts, desktop Tools views; extend inventory presentation without changing safe MCP server authority.
**Interface:** startup/user-approved connection ID, negotiated schemas/capabilities, request correlation, bounded output/deadlines; authority is the intersection of external descriptor and local task profile.

- [ ] Implement configured stdio and authenticated-network sessions with explicit trust/credential handling, transport limits, schema validation and connection lifecycle.
- [ ] Feed external calls into Task 7's approval/intent/outcome pipeline; reconnect performs reads/renegotiation only and never replays a mutation.
- [ ] Test malicious schemas/outputs, duplicate IDs, child leaks, disconnect during an effect, auth failure, redirects and unexpected capability expansion against real fixture servers. Preserve Abbey's existing read-only MCP server as a separate role.

### Task 12B: Finish separately scoped MCP server conformance

**Files:** existing `src/mcp_host/` stdio/HTTP protocol, transport/limits/session modules and fixture conformance tests; reuse only the safe read-only registry.
**Interface:** explicitly versioned protocol negotiation; any future non-loopback resource server is a separate authenticated/TLS authority, not a wider binding flag on current unauthenticated loopback.

- [ ] Implement version-appropriate lifecycle/conformance, cooperative cancellation, bounded streaming/resumability and resources/prompts/tasks only through separately declared exact capabilities and registered safe handlers; preserve classic-client compatibility fixtures.
- [ ] Before any non-loopback support, implement reviewed TLS/resource-server authentication, PKCE/resource-indicator/audience binding, no token passthrough, revocation and redirect/Host/Origin protection. Validate against pinned official protocol corpus at implementation time; no made-up auth claims.
- [ ] Run conformance, malformed/oversized frames, stale/cross-client sessions, slow consumers, cancellation, auth/audience and secret-redaction tests. Shell/personal tool authority stays absent from remotely discoverable MCP.

### Task 13: Durable recurring automations

**Files:** new `src/app_core/v4_automation.rs`, `src/runtime/store/automations.rs`, `src/runtime/automations.rs`, desktop automation view/controller; reuse `src/automation_schedule.rs` unchanged where correct.
**Interface:** `AutomationSpec {instruction, schedule: ScheduleSpec, route_id, workspace_id, profile_id, budget, enabled}`, occurrence identity bound to automation revision and scheduled UTC instant, attributable task attempt.

- [ ] Persist validated once/hourly/daily/weekly/cron schedules; expose preview/create/edit/enable/disable/run-now through typed v4 commands and real UI.
- [ ] Atomically reserve occurrence and create one task; prevent overlapping runs, use first DST fold occurrence, advance gaps, catch up only latest missed occurrence and retain missed/interrupted evidence.
- [ ] Test fake-clock forward/backward jumps, restart at every commit boundary, revocation, edits while due, disabled profiles, concurrent ticks and exact one-task-per-occurrence attribution.

### Task 14: Background services and notifications

**Files:** new `src/service/`, edition-scoped launchd/systemd/Task Scheduler templates, desktop lifecycle/tray/notifications, installer integration.
**Interface:** explicit install/start/stop/status/pause-all/stop-all operations and edition-specific service identity; window close and application quit have distinct documented semantics.

- [ ] Implement opt-in per-user service registration and safe daemon startup/reconnect; credentials are owner-only and never passed through WebView or service command-line logs.
- [ ] Add continue-on-close, explicit quit/pause/stop, actual completion/failure/approval notifications and recoverable service errors. Never auto-enable background execution by installing the UI.
- [ ] Verify logout/login, service crash/update, close/quit, disabled startup, simultaneous editions, missing providers and offline operation on all supported hosts.

## Workstream C — Management and desktop release

### Task 15: Complete provider, memory, model and permission management

**Files:** desktop settings/memory/model views, `src/config.rs`, edition credential helpers, existing v3 exact model/memory authority, new v4 reconfiguration/profile contracts.
**Interface:** versioned validated configuration update with explicit apply/restart outcome; exact-approved memory record ID and immutable model ID/revision operations.

- [ ] Add real onboarding and available-provider/persona/role/model/workspace/budget settings, credential status without values, profile creation/revocation and current route explanation.
- [ ] Wire exact memory obsoletion preview/approval/result and model license acceptance/download/load/unload/status/recovery; credentials, registry paths and license identity remain startup-owned.
- [ ] Test revoked grants, conflicting configuration versions, ambiguous model effects, missing license/artifact, interrupted download/load, offline operation, edition separation and backup/restore. Do not silently install loaded model as another route.

### Task 16: Qualify packaged macOS application and release script

**Files:** `desktop/scripts/release-macos.sh`, Tauri configs/entitlements/icons, new package manifest/acceptance driver, installer assets.
**Interface:** artifact discovery from the successful bundler output for exact edition/target/profile; manifest lists files with individual hashes or a deterministic archive hash (never hash a directory as a file).

- [ ] Add script tests for default/custom Cargo target directories, stale alternate-edition bundles, partial signing credentials, interrupted signing, missing artifact and failed codesign/notary/staple checks.
- [ ] Bundle required daemon/runtime resources, use repository toolchain helper, correct product descriptions/platform floor, minimal entitlements and explicit staging versus Developer ID/notarized modes. Never treat ad-hoc signature as public distribution acceptance.
- [ ] Build isolated `.app`/`.dmg`, verify contained resource identities/signatures and actually launch fresh account/install/upgrade/restart/uninstall flows. Prepare exact signing artifacts before any request for owner credentials/publishing.

### Task 17: Complete Windows and Linux transport and packages

**Files:** new `src/daemon/{server,client}/windows.rs`, Windows supervisor Job Object module, `install.ps1`, Tauri platform configs and native package/service scripts; Linux packaging and portal integration.
**Interface:** authenticated owner-restricted named pipe with the same framing/correlation/limits as Unix; structured worker cancellation through owned process tree. Linux uses exact edition-scoped Unix authority.

- [ ] Implement Windows pipe ACL/peer checks and Job Object lifetime/capture/timeout/cancel semantics; preserve no-bearer/no-fallback behavior and safe/personal namespaces.
- [ ] Build signed Windows installer and Linux native packages with exact dependencies/resources, updater/uninstaller and service semantics. Handle X11/Wayland capability differences explicitly.
- [ ] Execute actual PowerShell install/coexistence, native WebView chat/tasks/tools/automations, service restart and clean-user package acceptance on Windows/Linux. Cross compilation is additional evidence only.

### Task 18: Complete native desktop polish, accessibility and performance

**Files:** desktop shell/components/styles/preferences/menu/notifications, native window modules, new rendered accessibility and performance acceptance harness.
**Interface:** persistent safe window/preferences state, keyboard-accessible actions, semantic labels/status/focus, measurable launch/render/poll/resource telemetry without transcript capture.

- [ ] Finish menu/shortcuts/icons/tray/window restoration, markdown/code interactions, undo where supported, loading/empty/error/reconnect states, safe links and actual source availability. Eliminate enabled dead controls and fabricated task records.
- [ ] Run complete keyboard/screen-reader/high-contrast/reduced-motion/200%-zoom journeys, IME/paste/multiline Unicode, narrow windows, long titles and 10,000-message history.
- [ ] Measure spec startup/interaction/idle budgets and a 24-hour soak on named hardware. Fix unbounded history rendering, polling, leaked subscriptions/processes and heap growth; record measured results rather than timing assumptions.

### Task 19: Security, privacy and recovery qualification

**Files:** action/transport/credential modules, `tools/security/`, bundle CSP/capabilities, diagnostic export and backup/restore helpers; release threat model/receipts.
**Interface:** explicit trust-boundary matrix, redacted diagnostic schema, backup compatibility version and artifact provenance; no secrets or uncontrolled file/network/process authority in WebView.

- [ ] Review real current diff and attack paths across model/tool/web/MCP/file/native/IPC inputs; fuzz parsers and state transitions; prove controls on frame/depth/collection/output/deadline limits and ID replay/substitution.
- [ ] Exercise full SQLite/WDBX backup/restore, corruption refusal, migration interruption, disk-full/read-only state, lost credentials, revoked profile, malicious artifacts and cross-edition paths. Preserve committed provenance and never recurse-delete user state.
- [ ] Run dependency/license/secret scans and bundle CSP checks. Resolve every Critical/High security issue and Important correctness/authority finding; document any external audit/DAST prerequisite and actual result separately.

### Task 20: Reproducible CI and artifact pipeline

**Files:** `.github/workflows/rust.yml`, new desktop/release workflow jobs, `tools/ci/{require_executed_run,cross-compile-guard,checkout-public-revision}*`, release receipts/SBOM tooling.
**Interface:** immutable public Abbey/ABI/WDBX inputs, feature/host/edition matrix, executed nonzero job IDs, artifact hashes/signatures/attestations; no secrets on untrusted forks.

- [ ] Provision/validate Linux ARM64 and Windows runners, retain trusted self-hosted event restrictions and separate safe untrusted static checks. No zero-job workflow may satisfy the CI claim.
- [ ] Gate root and desktop, locked dependencies, release package build, installed fixture journeys, security scans, artifacts and signature validation on actual macOS/Windows/Linux runners.
- [ ] Prove failure propagation, cancelled/stale runs and exact run/ref/artifact correlation. Close `ci-self-hosted-linux-proof` and Windows naming Partial only with the specific executed-host evidence.

### Task 21: Full installed-product journeys and release operations

**Files:** new `desktop/scripts/prove-product-{macos,windows,linux}` harnesses, release checklist/runbook, diagnostics and update/rollback modules.
**Interface:** actual produced installed package + scratch user home/state; typed acceptance receipt with host/edition/build/source/signature/provider/scenario identities.

- [ ] Exercise install → onboarding → two-turn chat → Stop/retry → attachment → task approval/tool/artifact → recurring automation → memory/model management → close/background → restart/recover → update/rollback → uninstall retention.
- [ ] Repeat both editions, optional-provider missing/offline paths, primary locales/IME/accessibility and native permission denial/revocation. Include slow/failed provider and power-loss fixtures.
- [ ] Document support, log/diagnostic export, crash reporting opt-in, upgrade compatibility and rollback instructions; run staged pilot with real consented providers before release promotion.

### Task 22: Core desktop qualification and release-candidate review

**Files:** desktop plan/verification ledger, `src/claims/registry.rs`, generated claims/docs, package manifests and independent review reports.
**Interface:** complete D1–D8 traceability matrix; every requirement points to source, tests and relevant installed/native evidence at exact hashes.

- [ ] Freeze inputs after the final repair wave, run `./check.sh` and complete desktop gates, feature-combination coverage and all three installed-product harnesses. Skipped required hosts leave the gate open.
- [ ] Obtain independent complete-product/code/authority/UX review; fix and reverify findings against the same release candidate. Verify public signatures/notarization and updater trust separately from local staging.
- [ ] Record core-desktop qualification and publish only after explicit authorization. Keep the complete desktop-product claim open while required Training/adapters or Cluster/workers views remain unproved; Tasks 23–30 stay open. A staged candidate is not full project completion.

## Workstream D — Complete the broader approved Abbey project

### Task 23: Production-capable local model runtime

**Files:** existing Abbey model lifecycle/inference authority and desktop model views; coordinated ABI `abi-models`/`abi-model-runtime` architecture/tokenizer/loader modules; dedicated model acceptance subplan and fixture suite.
**Interface:** signed immutable model/revision/artifact/license manifest; exact device request, bounded context/output/memory, actual executed-device evidence and reproducible quality corpus.

- [ ] Qualify roadmap target `google/gemma-4-12B-it-qat-w4a16-ct` and its tokenizer through a reviewed architecture implementation. Keep current Ollama default/aliases independent; no silent artifact/model substitution.
- [ ] Prove 4K context + 64-token smoke, peak RSS <=20 GB on the named 24 GB Mac, no sustained swap thrash, correct tokenizer fixtures and valid structured calls; record CPU/Metal oracle tolerance and actual backend execution.
- [ ] Execute real licensed-artifact download/load/infer/unload/reopen/device-failure and quality/security evaluation through actual Models/Chat controls. Fixture bigram inference or artifact presence cannot promote production-weight claims.

### Task 24: Accelerator inference/training integration

**Files:** Abbey `src/accel*`, coordinated ABI compute/GPU/model runtime adapters, device capability/selection contracts and hardware acceptance receipts.
**Interface:** exact requested/executed device, no-fallback/mixed evidence, bounded allocations and numerical oracle; model and training workloads use the same negotiated compute contract.

- [ ] Connect supported CPU/Metal inference/training first; separately qualify each additional CUDA/Vulkan/NPU/TPU target only on real hardware and reviewed runtime adapters.
- [ ] Test allocation failure, unsupported kernels, fallback refusal, mixed execution, cancellation and output parity for complete model workloads; report residency only if directly observable.
- [ ] Benchmark reproducibly against CPU on named hardware, including warmup/variance/memory/thermal limits. Kernel correctness alone cannot establish speedup or whole-model device placement.

### Task 25: Curated adapter training and rollback

**Files:** `src/distill.rs`, `src/learn*`, new bounded training orchestration/evaluation module; coordinated ABI trainer interface; training acceptance subplan/artifact manifests.
**Interface:** curated consent/provenance dataset manifest, deterministic splits/config/seed, immutable base identity, adapter output hash and evaluation/rollback record.

- [ ] Add explicit dataset review/redaction/consent/license checks, local deduplication and deterministic 80/10/10 train/validation/test splits; no upload or remote training. Do not imply all teacher outputs permit training.
- [ ] Train adapters for the separate Gemma 4 E4B instruction base; never label them 12B-compatible. Verify actual weight changes and compatibility, finite budgets/cancellation and held-out quality/safety regression bars.
- [ ] Require >=95% valid tool-call JSON on held-out fixtures, NLL regression <=2% against base, 100% public-edition refusal-fixture success and deterministic reopen/rollback. Register signed base hashes/dataset digest/seed/metrics/rollback provenance; reject incompatible/corrupt artifacts. Complete actual desktop Training/adapters review/start/progress/evaluate/activate/rollback flows before promotion.

### Task 26: Local neural speech and media

**Files:** `src/voice*`, new local media provider contracts/runtime/desktop UI; model registry/artifact reuse; offline speech/image/video acceptance suites.
**Interface:** bounded licensed model input/output, selected device, cancellation/progress and opaque artifacts; distinguish platform I/O, delegated generation and local neural execution.

- [ ] Implement explicit opt-in offline Whisper large-v3-turbo STT and Parler-TTS Mini v1.1 TTS first with consented microphone/voice use, device selection, timing/quality corpus, interruption and audio-device failure behavior. Validate actual license/revision/artifact availability before selection.
- [ ] Add explicit opt-in SDXL image then LTX-Video adapters with hash-verified licensed weights, explicit disk/VRAM/runtime budgets, desktop preview/cancel and artifact provenance. Record revision, device, runtime, duration, seed and output digest. Keep system speech/delegated media separate; do not bundle cloud SaaS as local.
- [ ] Verify network-disabled generation, corrupted/missing weights, cancelled/oversized output, real quality metrics and actual device evidence before media claim promotion.

### Task 27: Complete edition-specific runtime authority and helpers

**Files:** `src/edition.rs`, action policy/tool registries, credential/audit namespaces, native signed helper services, both desktop bundle configs and installation tests.
**Interface:** edition-specific capability/consent model, structured shell.exec proposal, signed helper request and immutable audit record. Existing safe-edition allowlist/confirmation is never weakened.

- [ ] Produce an explicitly reviewed personal-edition authority/consent specification before enabling new shell/helper effects; resolve any conflict with mandatory current confirmation rules explicitly. Preserve all safe-edition refusals.
- [ ] Implement only validated structured operations with bounded process lifetime/capture/timeouts, local-only transport, OS authorization and emergency stop; never accept model text as shell syntax or store an OS password.
- [ ] Prove installed safe/personal coexistence and noninterference of binary/bundle/state/credentials/audit/tools/daemon/service/update paths. Personal authority must not leak into remote MCP or network workers.

### Task 28: Federation authority and authenticated shared compute

**Files:** `src/daemon/federation.rs`, `src/app_core/` federation contracts, `src/mesh.rs`; coordinated ABI worker/gateway/WDBX substrate, VM/deployment fixtures.
**Interface:** exact corpus/capability manifest, durable consent/authorization adapters; private-CA mTLS workers, signed job envelopes, leases/deadlines, result hashes and quotas.

- [ ] Integrate declared federation authority only after its consent/durable/execution adapter exists; add backward-compatibility/tamper/correlation/limits tests. Keep unsupported methods disabled.
- [ ] Prove three Ubuntu ARM64 VMs on one Mac with distinct IPs, independent state/cert identities and namespaces: membership, leader/worker death, duplicate delivery, partition/stale leader, lease expiry, conflicting state/read repair, cancellation, backup/restore, rolling upgrades and clean teardown. The consensus/membership control store must be independent of WDBX placement; no circular authority dependency. Complete actual desktop Cluster/workers discovery/status/quotas/jobs/cancel/failure flows.
- [ ] Separately prove physical-host/geographic-HA/multi-device production operation with real deployment evidence. A same-host process or three-VM demo cannot close production mesh claims.

### Task 29: Explicitly consented guild-intelligence expansion

**Files:** existing guild-intelligence app-core engine, contracts/corpus, new separately authorized recorder/transport/effect adapters and consent/evidence receipts.
**Interface:** participant-consented recording identity and revocable scope; observed/desired/rollback plan digests and exact effect approvals.

- [ ] Keep the existing synthetic engine pure and read-only; prepare a reviewed recording/retention/consent design and fixture acceptance without contacting live Discord.
- [ ] After explicit authorization/participant consent exists, execute separate owner/admin live read-only validation and compare deterministic findings/plans; report permission/unknown-target failures.
- [ ] Treat desired-state execution/deployment as a distinct approved effect program with precondition checks, rollback, durable intent and audit. Missing consent/authorization remains a named external prerequisite.

### Task 30: Final project completion audit and sustained quality

**Files:** `tasks/{goals,todo,lessons}.md`, `src/claims/registry.rs`, generated docs, all subproject specs/plans, final signed release/CI/hardware/deployment evidence manifest.
**Interface:** requirement-by-requirement completion matrix across D1–D8 and P1–P8, including every approved open checkbox/claim and its exact proof.

- [ ] Reconcile stale historical descriptions without deleting history; each locally actionable item has implementation/tests/docs/review and required runtime proof. Remaining external prerequisites name owner, exact artifact/action and missing acceptance; the full project remains open while those are unproved.
- [ ] Run frozen full Abbey and required coordinated ABI/WDBX gates, all supported feature/edition/platform packages, privacy/security/accessibility/performance/soak checks, real model/training/media/hardware and distributed proofs. Report every unavailable check honestly.
- [ ] Obtain independent final requirements/code/release review after the last repair, validate signatures/update/rollback/support runbooks, and regenerate claims/docs. Only then mark the full objective complete. Canceled Zig and the five deliberate Out-of-scope refusals remain excluded by explicit policy, not relabeled unfinished features.

## Verification commands and receipt rules

Existing commands: `./tools/cargo.sh --abbey-qualify-toolchain`, `./check.sh`, `ABBEY_DESKTOP_ROUTES_ACCEPTANCE=1 ./desktop/check.sh`, `python3 tools/check_claims_sync.py`, `python3 tools/check_instructions.py`, `cd desktop && bun test tests && bun run typecheck && bun run build && bun run verify:bundle`, `./desktop/scripts/prove-daemon-read.sh`. Confirm exact script names before use; new harness commands in tasks become required only after their executable fixtures exist.

Each task records RED/GREEN where appropriate, command exit/result, exact touched inputs, independent review and remaining evidence. Read-only/documentation tasks use semantic/link/schema validation rather than imitation tests. Broad gates run once per coherent repair wave, serially, after focused tests pass. Source edits after qualification invalidate affected receipts until the relevant checks rerun.

## Traceability and prerequisites

| Spec requirements / original scope | Owning tasks |
|---|---|
| D1 onboarding + local-first provider independence | 3, 14, 15, 21 |
| D2 chat + original desktop Tasks1–4, Stop continuation18 | 2–5, 18, 21 |
| D3 delegated tasks + original Task6 | 6–7, 21 |
| D4 tools/native/external + original Tasks7–8 | 7–12, 19, 21 |
| D5 automation/service + original Task9 + pure schedule11 | 13–14, 21 |
| D6 management + original Tasks10/14/15 | 15, 19, 21 |
| D7 polish + original Tasks4/10/13/17 | 5, 18, 21 |
| D8 platforms/packages + original Tasks5/10 | 1, 16–17, 19–22 |
| P1 shared runtime/federation/provider parity | 2–3, 7, 12, 28 |
| P2 production local models | 23 |
| P3 accelerator execution | 24 |
| P4 curated adapter training | 25 |
| P5 local neural media | 26 |
| P6 editions/mesh | 27–28 |
| P7 CI + blocked Linux/Windows proof + Windows install naming | 17, 20–22 |
| P8 synthetic/live guild expansion boundaries | 29 |
| All claims, final evidence/retention/supply-chain invariants | 1, 19–22, 30 |

External owners: repository/runner administrator (Linux/Windows CI), release owner (Apple/Windows signing and publication), model/artifact license holder and hardware operator (23–26), VM/cluster operator (28), live service/participant consent owners (29). Prepare reviewable code and artifacts first; request external actions only when necessary. No absent credential, hardware or permission stops unrelated local work.

## Immediate execution checkpoint

- [x] Reinspect current tree and identify task/automation stubs, fixed-provider path, packaging script and CI limitations.
- [x] Save production specification and ordered full-scope plan; preserve previous desktop plan and all unfinished milestones.
- [ ] Complete/review desktop Task18 Stop (assigned to fresh implementer; report under `.superpowers/sdd/2026-10-20-desktop-assistant/`).
- [ ] Audit Task2 content accounting/placeholder/history continuity, then implement Task3 canonical provider/session path before task/tool execution.

Self-review: D1–D8 and P1–P8 each map above; eight Proposed, three Partial, CI Blocked and deliberate OOS/canceled boundaries remain accounted for. This is an ordered production master plan. Risky subsystem implementation must use its task-level exact contracts/fixtures and independent review; a workstream label never grants runtime authority.
