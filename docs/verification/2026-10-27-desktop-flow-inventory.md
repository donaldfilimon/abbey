# Desktop chat/product flow inventory — 2026-10-27

Status: Step 1 of desktop-plan Task 11
(`docs/plans/2026-10-27-abbey-desktop-plan.md`). This is an inventory. It
changes no code, runs no gate, and promotes no claim. It records where each
flow named in §6 "Complete desktop product" of
`docs/superpowers/plans/2026-10-22-abbey-completion.md:110-119` stands on
`main` at `0ad2fef`.

## Method

- **Sources.** `desktop/README.md:413-451` ("Views: real vs placeholder"), the
  22 bun test files under `desktop/tests/`, the Rust tests in
  `desktop/src-tauri/src/`, the Tauri invoke list at
  `desktop/src-tauri/src/main.rs:209-247`, and the served v4 grant set at
  `src/daemon/runtime_v4.rs:19-25`.
- **Splitting.** Several §6 items name more than one flow, for example
  "attachments/search/pin/unarchive/export". Each §6 item is a heading, and
  each flow inside it has its own row with exactly one status.
- **Statuses.**
  - `shipped`: the flow exists and a named test proves it.
  - `missing`: the flow does not exist, or exists only in part, and it needs
    no daemon or runtime authority that is unavailable today. A
    Donald-gated qualification step, such as an Accessibility-enabled host
    or an OB step, is not authority. A flow that waits only on such a step
    is `missing`, with a pointer to the plan step.
  - `blocked-on-authority`: the flow needs daemon or runtime authority that
    the desktop cannot negotiate today. The row names the plan task that
    serves the authority. If no 2026-10-27 plan task serves it, the row says
    so instead of inventing one.
- **Test ids.** Bun tests are written as `desktop/tests/<file>::<test name>`.
  Rust desktop tests are written as `desktop/src-tauri/src/<file>::<fn>`.
  The task spec asks for `desktop/tests/*.test.ts(x)` names. Some flows,
  however, are proven only in the Rust desktop crate: edition binding,
  bearer non-exposure, the native menu, and window configuration. For those
  rows, the Rust test is the test id. No row is marked `shipped` without at
  least one test id.

## Inventory

### 1. Edition-matched onboarding/daemon startup

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Edition-matched daemon connection | shipped | `desktop/src-tauri/src/backend/connection_tests.rs::peer_edition_must_match_active_identity`; `::scratch_authenticated_discovered_clients_bind_edition_and_fail_closed`; `desktop/src-tauri/src/main_tests.rs::the_two_editions_never_share_a_bundle_identity` | — |
| First-run onboarding flow | missing | No onboarding or first-run module exists in `desktop/src`. The only first-contact surfaces are the empty states in `desktop/tests/app-shell.test.tsx::the empty state asks for a connection when there is none` and `::the empty state names the grant a connected runtime withheld`, which explain the problem but do not set anything up. | No 2026-10-27 plan task serves this. |
| Daemon startup from the desktop | missing | Deliberately absent. `desktop/tests/system-integration.test.tsx::the shipped client cannot register a login item, install a service, or spawn a process` pins that the client never starts `abbeyd`. | Needs a product decision, because the boundary is pinned by a test. Packaging and services are covered by desktop-plan Task 14 and platforms-security-closure-plan Task 7. |

### 2. Credentials outside WebView IPC

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Bearer never crosses IPC, and only its kind and a boolean are reported | shipped | `desktop/src-tauri/src/backend/mod.rs::connection_detail_never_contains_bearer_material`; `desktop/src-tauri/src/backend/connection_tests.rs::configuration_errors_and_debug_do_not_expose_credentials`; `::discovered_connection_metadata_does_not_connect_or_expose_a_secret`. The WebView renders only `bearer_configured` (`desktop/src/views/DoctorView.tsx:173-174`), and `desktop/tests/doctor-effective-model.test.tsx` fixtures carry no secret. | — |

### 3. Provider/workspace/profile configuration

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Choosing a configured daemon route for a turn | shipped | `desktop/tests/conversations.test.ts::route read failure stays distinct from an empty inventory and retry selects only an available route`; `::successfully empty route inventory is not misreported as a route read failure` | — |
| Reading the effective model and route | shipped | `desktop/tests/doctor-effective-model.test.tsx::the doctor names the selected route as the authority for the answer` | — |
| Editing provider, workspace or profile configuration | blocked-on-authority | No invoke command writes configuration (`desktop/src-tauri/src/main.rs:209-247`), and no v4 capability for configuration writes exists (`src/app_core/v4.rs`). | No 2026-10-27 plan task serves configuration-write authority. |

### 4. Durable multi-turn streamed chat

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Durable multi-turn chat with incremental assistant content | shipped | `desktop/tests/conversations.test.ts::active assistant content is read by exact run cursor and replaces the bounded preview`; `::manual loading continues paging an older truncated reply until its terminal event`; `::later conversation-scoped run accepts its first nonzero event sequence without replay`; `::poll refreshes tail of selected conversation without clobbering draft or selection on transient error`; `desktop/src-tauri/src/backend/conversations.rs::live_daemon_desktop_conversation_crud_and_history`. | The served grants are `SubmitTurns` and `ReadTurns` (`src/daemon/runtime_v4.rs:19-25`). |

"Streamed" here means cursor-paged polling of durable content. There is no
push stream to the WebView.

### 5. Attachments/search/pin/unarchive/export

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Attachments | blocked-on-authority | v4 content is text-only: `V4ContentPayload` has only `TextDelta` (`src/app_core/v4_content.rs:14-15`), and the README calls messages "text-only" (`desktop/README.md:417`). | Needs non-text v4 content authority. No 2026-10-27 plan task serves it. |
| Search | shipped | `desktop/tests/conversations.test.ts::slow search and selection cannot replace newer results or message bodies`; `::a failed replacement search cannot page the previous query or mix its rows` | The desktop side is shipped. Caveat: the daemon's `v4_search_conversations` returns the unfiltered list, which authority-tools-plan Task 6 fixes. |
| Pin and unpin | shipped | `desktop/tests/conversation-metadata.test.ts::pin/unpin and unarchive use exact target patch and preserve draft`; `::pending metadata action is single flight and late acknowledgement preserves navigation`; `::unknown patch reconciles immutable target by reads without replay after navigation` | Goes through the existing `app_conversation_update` (`desktop/src-tauri/src/main.rs:244`) with a `V4ConversationPatch` under `WriteConversations`, which is served (`src/daemon/runtime_v4.rs:21`). No new daemon authority is needed. |
| Unarchive, with the active/archived filter | shipped | `desktop/tests/conversation-metadata.test.ts::pin/unarchive controls and active/archived filter render without a title dialog`; `::archive filter resets cursor and discards stale active results`; `::missing update API refuses metadata mutations` | Same path as pin: `app_conversation_update` and `WriteConversations`. |
| Full transcript export | missing | The workspace says "Full transcript export is unavailable on this bridge" (`desktop/src/conversations/Workspace.tsx:50`). No export code exists. | Needs only reads that are already served: `app_conversation_messages` and `app_conversation_content` (`desktop/src-tauri/src/main.rs:238-239`). It is eligible for Task 11 Step 2. |

`desktop/README.md:417` says "Pin, unarchive, and full transcript export
remain unavailable". That is stale for pin and unarchive, which the tests
above prove. This inventory records the discrepancy and does not edit the
README.

### 6. Explicit retry

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Explicit retry of a terminal attempt | shipped | `desktop/tests/conversation-retry.test.ts::retry uses exact original input and fresh key once, preserving draft and recording honest relationship`; `::user-only terminal attempts render exactly one retry action`; `::unknown retry blocks only affected conversation and exact-key lookup reconciles without replay`; `desktop/tests/conversation-windows.test.ts::terminal assistant split from its original input remains retryable after latest scan` | — |

### 7. Exact Stop recovery

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Stop the exact active turn and recover by reads | shipped | `desktop/tests/conversation-stop.test.ts::double Stop sends exact pair once synchronously and preserves draft/transcript while stopping`; `::ambiguous Stop reconciles only through reads, including navigation away and back`; `::a captured Stop run cannot be retargeted to a later turn in the same conversation`; `::completion racing Stop is reported as succeeded and removes active Stop immediately` | `CancelTurns` is authorised by RuntimeHandler (`src/daemon/runtime_v4.rs:31`). |

### 8. Memory curation

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Memory search and metadata (read) | shipped | `desktop/tests/memory-surface.test.tsx::the memory view reads exactly one summary space`; `::a record carries an opaque id, a bounded label, and a state` | — |
| Curation, such as marking obsolete or deleting | blocked-on-authority | `desktop/tests/memory-surface.test.tsx::the memory view offers no mutation control` and `::the memory view says the mutation is not on this surface`. Obsolete is not registered on the desktop bridge (`desktop/src/views/MemoryView.tsx:5`). | Needs a v3 memory-mutation grant on the desktop bridge. No 2026-10-27 plan task serves it. |

### 9. Models/roles/permissions

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Models: inventory, details and download/load/unload | shipped | `desktop/tests/models-view.test.tsx::the button matrix needs the grant, the licence, and an unloaded model`; `::the confirmation gate names the exact action it would send`; `desktop/tests/model-runner.test.ts::interrupted effect is sent once; recovery only reads the retained operation`; `desktop/tests/model-operations.test.ts::ambiguous submission retains exact identity and blocks a fresh effect` | — |
| Roles: viewing or selecting persona and role bindings | blocked-on-authority | The only role surface is the read-only route audit (`desktop/src/surfaces.ts:169`, `desktop/tests/routes-surface.test.tsx::a route row shows the decision the application core published`). Role bindings are Current in Abbey, but no invoke command exposes them (`desktop/README.md:439-441`). | No 2026-10-27 plan task serves role authority on the desktop bridge. |
| Permissions | blocked-on-authority | `ReadPermissions` and `WritePermissions` exist (`src/app_core/v4.rs:81-82`) but are not in `SERVED` (`src/daemon/runtime_v4.rs:19-25`). | Served by authority-tools-plan Task 7, with transport decided in desktop-plan Task 5 and the Approvals surface in desktop-plan Task 8. |

### 10. Diagnostics

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Doctor: runtime, effective model, route, grants and host facts | shipped | `desktop/tests/doctor-effective-model.test.tsx::the doctor panel shows the effective model the runtime published`; `::an unreadable effective model states why instead of guessing one`; `desktop/tests/system-integration.test.tsx::the system integration section states all four host facts`; `desktop/tests/ledger-surfaces.test.tsx::the error card shows a remedy when the daemon offered one` | — |

### 11. Backup/restore/retention

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Backup of conversations and state | blocked-on-authority | No backup command exists in the invoke list (`desktop/src-tauri/src/main.rs:209-247`) or in the v4 capabilities (`src/app_core/v4.rs`). | No 2026-10-27 plan task serves it. |
| Restore | blocked-on-authority | Only presentation defaults can be restored (`desktop/src/components/PresentationSettings.tsx:37-38`). Restoring data needs a write path that does not exist. | No 2026-10-27 plan task serves it. |
| Retention policy | blocked-on-authority | Absent. Only window views are bounded (row 15). Durable retention would need daemon-side deletion authority beyond single-conversation delete. | No 2026-10-27 plan task serves it. |

### 12. Services/notifications

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Service install and management from the desktop | missing | Deliberately absent: `desktop/tests/system-integration.test.tsx::the shipped client cannot register a login item, install a service, or spawn a process`; `desktop/src-tauri/src/main_tests.rs::no_execution_capable_plugin_is_declared`. | This is host packaging, not daemon authority. It is covered by platforms-security-closure-plan Task 7 (`linux-service-foundation`) and desktop-plan Task 14. Moving the pinned boundary needs a product decision. |
| Notifications | missing | No notification code exists in `desktop/src` or `desktop/src-tauri/src`. | No 2026-10-27 plan task serves it. |

### 13. Native menus/window state

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Native application menu | shipped | `desktop/src-tauri/src/main_tests.rs::the_menu_keeps_apple_roles_and_one_user_initiated_handler`; `::the_app_menu_offers_a_bounded_update_check`. The packaged proof is macOS-only (`desktop/README.md` "Unmet bars"). | — |
| Persisted sidebar state | shipped | `desktop/tests/presentation-preferences.test.ts::all preference combinations round-trip through version 1 JSON`; `::invalid enums and non-boolean collapsed are defaulted independently` | — |
| Persisted window size and position | missing | No window-state code exists in `desktop/src` or `desktop/src-tauri/src`. Window configuration is static (`desktop/src-tauri/src/main_tests.rs::the_two_editions_agree_on_the_window_except_the_title`). | No authority needed. No 2026-10-27 plan task serves it. |

### 14. Accessibility

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Reduced-transparency, increased-contrast and forced-colors degradation | shipped | `desktop/tests/presentation-preferences.test.ts::material resolution covers every platform and accessibility combination`; `desktop/src-tauri/src/appearance.rs::reduced_transparency_degrades_to_opaque`; `::increased_contrast_degrades_to_opaque`; `::forced_colors_degrade_to_opaque` | — |
| Labelled navigation | shipped | `desktop/tests/app-shell.test.tsx::the whole shell renders the navigation, the banner, and the footer` (asserts `aria-label` for each surface) | — |
| Keyboard, screen reader, 200 % zoom and reduced motion | missing | No proof yet. The Accessibility drivers (`desktop/scripts/drive-surfaces-macos.py`, `drive-controls-macos.py`) are not in the client gate. | This is qualification gated by Donald, not authority. It is covered by desktop-plan Task 2, Task 3 (OB-5) and Task 12. |

### 15. Bounded history

| Flow | Status | Evidence | Authority / plan |
|---|---|---|---|
| Bounded conversation and message windows, and bounded drafts | shipped | `desktop/tests/conversation-windows.test.ts::10k sparse short pages browse both directions with bounded windows including evicted cursors`; `::workspace renders one bounded window and usable navigation controls`; `desktop/tests/conversations.test.ts::draft inventory refuses to evict existing drafts at the bounded limit`; `desktop/tests/message-content.test.tsx::4MiB newline or inline-heavy text falls back losslessly to bounded plain text` | — |

## Summary

- **Shipped (18 flows):** edition-matched connection; credentials kept out
  of IPC; route choice; effective-model read; durable chat; search
  (desktop side); pin and unpin; unarchive; retry; Stop; memory read;
  models; diagnostics; native menu; sidebar state; accessibility material
  degradation; labelled navigation; bounded history.
- **Missing, with no authority needed (7 flows):** first-run onboarding;
  daemon startup (pinned boundary); full transcript export; service
  management (pinned boundary); notifications; window size and position;
  keyboard, screen-reader, zoom and reduced-motion proof.
- **Blocked on authority (8 flows):** provider, workspace and profile
  writes; attachments; memory curation; roles; permissions, which
  authority-tools-plan Task 7 serves; backup; restore; retention.
- **Candidates for Task 11 Step 2:** full transcript export is the only
  missing chat flow that needs no new daemon authority and no product
  decision. Persisting window size and position also needs no authority,
  but no plan task covers it.
- **Gaps in the plans:** no 2026-10-27 plan task serves configuration
  writes, attachments, memory curation on the desktop, role bindings on the
  desktop, backup, restore, retention, notifications, onboarding, or
  persisting window size and position.
- **Stale documentation:** `desktop/README.md:417` still lists pin and
  unarchive as unavailable.
