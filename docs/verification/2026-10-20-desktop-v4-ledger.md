# Desktop assistant execution ledger — 2026-10-20 (updated post-fixes)

Protocol-v4 conversation submission + desktop chat/history surfaces are implemented and gated on this macOS checkout for the fixed local provider fixture. This file records evidence only. It is not a capability-ledger update (see claims and plan). The broader desktop assistant plan remains incomplete.

## Proven on this host (core v4 + desktop chat/history)

- Protocol v4 uses its own envelope. `SUPPORTED_PROTOCOL_VERSIONS` remains `[1, 2, 3]`.
- A Unix frame with version 4 stays a v4 envelope. A bad bearer returns `Unauthorized`. A handler that has not opted in returns `UnsupportedVersion`.
- `RuntimeHandler` opts in. It persists conversation summaries, authorized message bodies, text-free content events (plus tasks/automations stubs in store).
- A repeated turn submission with the same request id and payload returns the original receipt. A different payload for that id conflicts. A second active turn in the same conversation conflicts.
- Title search does not match message bodies. The turn receipt serialization does not contain the prompt.
- Export returns the conversation summary. Complete message text is read with `ListMessages` (via messages query).
- Store-level `v4_admit_turn` returns `Queued`; the authenticated `RuntimeHandler` submits through `RunManager`, which executes against its fixed startup-bound ABI/Foundation Models route. The desktop consumes correlated text events. The live scratch daemon uses a deterministic synthetic provider; live vendor quality is not claimed.
- Message pages are trimmed to 512 KiB. Assistant previews stop at 16 KiB. User text is not truncated.
- `ListRoutes` returns the fixed startup-bound route list. `SubmitTurn` requires `SubmitTurns`; content reads require `ReadTurns`. `CancelTurn` and task, automation, permission, and action commands remain unserved; their capabilities are no longer negotiated.
- A v1 status request on the same socket still returns the legacy envelope.
- Desktop v4 client (strict negotiation, send-once, grant-checked) implemented: `V4DaemonSession`, typed CRUD + messages + routes in `desktop/src-tauri/src/backend/conversations.rs` + `src/ipc/client.ts`.
- Generated IPC + codegen drift check passes (5 files).
- Real owner-only scratch-daemon conversation CRUD + paged history verified (live_daemon_desktop_conversation_crud_and_history + prove script).
- Chat workspace + history: searchable/pinned/history list (History.tsx + picker), create/rename/archive/delete, multiline draft composer (per-conversation + new), preserved drafts in window session memory, paged messages, more/refresh, 250ms polling for tail, stale-read guards, mutation ambiguity handling, reconnect preserves state. See conversations/{controller.ts,useConversations.ts,Workspace.tsx,History.tsx}.
- Polished shell (App.tsx): Chat launch view (welcome + suggestions + route-aware composer), responsive collapsible sidebar, History nav, Cmd/Ctrl+K command palette (filter/select), Ctrl+B toggle, appearance (system/light/dark via usePreferences + data attr), bootstrap parallel status/v3/identity with retry, error states, and no fabricated execution data. Keyboard nav, aria, focus.
- Basic tasks: RunsView repurposed as "Tasks" with honest stub note ("minimal... v4 grant if present, else stub... no backend tasks, no execution").
- Models/Memory/Claims/Routes/Doctor(Settings) surfaces live against grants or unavailable honest.
- `ABBEY_DESKTOP_ROUTES_ACCEPTANCE=1 ./desktop/check.sh` passed end-to-end: 18 Python acceptance assertions, 37 Bun tests, typecheck/build/bundle verification, 31 desktop Rust tests for each edition, 3 live scratch-daemon tests, bearer no-fallback, linked Tauri binary, and seven rendered macOS Routes scenarios.
-  `bun run typecheck`, `bun run build`, `bun run verify:bundle` pass.
- Conversation tests (conversations.test.ts) cover drafts, stale, double-mutation, reconciliation, pagination, no-daemon vs no-grant, etc.
- Chat submits once with an idempotency key over an available exact route, reads correlated assistant content pages, and guards ambiguous outcomes against duplicate sends. Drafts remain window-only; there is no send retry, stop control, or general multi-tool loop.
- Windows named pipes + packaging + native adapters + full governed loop + automations + MCP client etc. remain unimplemented.

## Dispatch / store / runtime notes (retained + current)

- The provider sink does not attach tools without a dispatcher and fails closed on unexpected tool events. Content sequence validation permits strictly increasing gaps in per-run filtered pages; UTF-8 previews truncate at a character boundary; terminal state produces one terminal content event.
- `./tools/cargo.sh test --lib -- v4_ ...` and focused pass (numbers vary with additions).
- Full `./check.sh` and opt-in `ABBEY_DESKTOP_ROUTES_ACCEPTANCE=1 ./desktop/check.sh` both passed on the qualified tree. Root gate explicitly builds each feature's executable before that cohort's process-level tests, preventing stale cross-feature binaries.
- Subagent fixes: TS undefined[0] via .at(0)??, supplied missing routes/submit to ConversationApi, removed unused RunState from non-test import (no more warnings in desktop gate).

## Not accepted / remaining Proposed (honest)

- The desktop does not yet expose cancellation, governed tasks/actions, or a general assistant tool loop.
- Desktop full assistant (send/turn execution, tasks governed loop, browser/files/cmd tools, native adapters, automations, packaging, signed releases) not shipped.
- No claim promotion in src/claims.rs for "desktop" or "tauri-react" beyond existing Proposed (see "Tauri 2 + React/TypeScript desktop GUI is Proposed but not implemented. The ratatui TUI is Current.").
- Per claims table: `desktop-tauri-react` remains Proposed (safe-edition macOS Routes-only window acceptance prior; packaged product and full interactive UI beyond ratatui TUI unproven).
- No live vendor-provider quality evidence, native OS-control adapters, signing/notarization evidence, or cross-platform package acceptance.
- Compilation, unit, live-read tests, and green desktop/check.sh are not macOS/Windows/Linux full product acceptance for the complete plan.

Evidence boundaries kept strict per AGENTS.md and plan constraints. `desktop-tauri-react` remains Proposed until the full product and packaging acceptance bar is met.
