# Abbey desktop (Tauri 2 + React + TypeScript)

A desktop **client of Abbey's shared application core**. It is not a second
Abbey: it owns no agent or memory. The current shell exposes read-only `Status`, `Claims`, `ReadRoutes`, protocol-v2
`GetRun`/`RunEvents`, protocol-v3 memory summaries/model inventory/exact claims,
and connection/bundle identity. A separate model session exposes exact signed
model details and confirmed download/load/unload with operation-status recovery.
A separate v4 conversation session exposes metadata CRUD/title search, authorized paged text history, configured routes, one-shot turn submission and correlated assistant content (create/rename/archive/delete + window-session drafts + polling). Turns use the daemon's fixed startup-bound provider route; this is not a generic tool-execution or inference bridge. Cancel, computer control, external tools and automation remain unavailable.

Ledger status: `desktop-tauri-react` is **Proposed**. Nothing here changes that.
The client compiles, typechecks, and reads the linked application core when no
bearer is set. With a bearer, process-level reads go through a real scratch
`abbeyd` (`desktop/scripts/prove-daemon-read.sh`). It now packages and signs on
macOS — an ad-hoc signature with the hardened runtime, **not** notarized, so
first launch on another machine is still unproven. The safe macOS Routes view
now has native window acceptance against a scratch daemon; see
[Unmet bars](#unmet-bars). Chat/history surfaces + basic nav are additional
demonstrated read surfaces (full assistant Proposed).

`desktop/check.sh` is the local gate; it includes live-daemon history/turn fixtures and an opt-in macOS Routes window acceptance run.

## Layout

```
desktop/
  package.json  tsconfig.json  vite.config.ts  index.html   bun + Vite + React 19
  scripts/verify-bundle.mjs                                  artifact security checks
  scripts/release-macos.sh                                   package → sign → verify (both editions)
  scripts/assemble-update-feed.sh                             per-edition updater feed assembly
  scripts/render-icon.swift                                  the 1024 icon master renderer
  scripts/prove-packaged-macos.sh                            packaged-app Routes acceptance
  scripts/prove-menu-icon-macos.sh                           packaged menu/icon acceptance
  codegen/                                                    Rust → TypeScript generator
    src/main.rs        driver, --check drift mode
    src/tsgen.rs       syn projection of serde contracts
    src/samples.rs     real serde output as `as const satisfies` fixtures
    src/editions.rs    bundle identity → tauri configs + TS
  src/
    main.tsx  App.tsx  styles.css  surfaces.ts
    components/UpdateCheck.tsx       the single updater trigger surface
    ipc/client.ts                  typed wrappers over the 37 enumerated commands
    ipc/generated.ts               @generated — IPC types
    ipc/generated.samples.ts       @generated — serde cross-check fixtures
    ipc/generated.editions.ts      @generated — edition identity
    views/DoctorView.tsx  ClaimsView.tsx  RoutesView.tsx  RunsView.tsx
                               MemoryView.tsx  ModelsView.tsx  UnavailableView.tsx  ErrorCard.tsx
  src-tauri/
    Cargo.toml  build.rs  tauri.conf.json  tauri.personal.conf.json
    capabilities/default.json      core:default only
    icons/                         1024 master + regenerated macOS/Windows chain
    src/main.rs                    generate_handler! — the whole IPC surface
    src/commands.rs                the 37 enumerated commands
    src/backend/                   daemon-first v1/v2 reads + separate v3 read/model and v4 conversation sessions
    src/ipc.rs                     desktop-only transport types
    src/v3_ipc.rs                  codegen-only v3 memory/grant/model wire shapes
```

`desktop/` is a **member of the repository's cargo workspace** (`abbey`,
`abbey-desktop`, `abbey-desktop-codegen`), not a second one. The ordinary
`cargo build`, `cargo test`, and `cargo clippy` at the repository root
therefore build and test this client too, against one lockfile: a change to
`src/app_core/` that breaks the client fails the root gate instead of waiting
for someone to run the desktop gate. The root gate also runs the locked IPC
codegen drift check; the frontend, bundle, and packaging checks belong to the
desktop gate below, because they need bun and a WebView toolchain.

## Commands

```bash
cd desktop
bun install
./check.sh             # the desktop gate — run this before committing here
bun run build          # tsc --noEmit && vite build
bun run typecheck
bun run codegen        # regenerate the IPC types
bun run codegen:check  # fail if any generated file is stale
bun run verify:bundle  # security checks against dist/ and the crate manifest
cargo test -p abbey-desktop
cargo build -p abbey-desktop
bun run tauri dev      # requires the Rust toolchain and a WebView
```

`desktop/check.sh` is deliberately **not** called by the repository root
`./check.sh`, which needs neither bun nor a macOS desktop. The root gate
compiles this client, so it needs the Tauri system dependencies: on Linux the
`webkit2gtk-4.1` and `libsoup3` development packages, on macOS the Xcode
WebView framework the toolchain already provides. The root gate checks
generated IPC types using the desktop codegen crate with `--locked`; a stale
desktop lockfile makes that check explicitly UNMEASURED. Frontend type
checking, the bundle scan, desktop tests and native linking run only in
`desktop/check.sh`.

Two verification notes:

- `cargo check` does **not** link. `desktop/check.sh` runs `cargo build` as
  well, because a Tauri binary that type-checks can still fail to link against
  the platform WebView frameworks.
- `backend::tests::connection_detail_never_contains_bearer_material` and
  `a_configured_daemon_never_falls_back_to_the_in_process_core` are
  _conditional_ on a bearer being exported, and no-op in the default run. Run
  the suite a second time to exercise them
  (`ABBEYD_BEARER_TOKEN=$(python3 -c "print('a'*48)") cargo test -p abbey-desktop`);
  setting the variable from inside a test is not possible because
  `std::env::set_var` is `unsafe` in edition 2024 and this crate denies
  `unsafe_code`.

## Native macOS Routes acceptance

Run `bun run smoke:routes:macos` from this directory, or
`ABBEY_DESKTOP_ROUTES_ACCEPTANCE=1 ./desktop/check.sh` from the repository root.
The opt-in gate requires the smoke to pass; it never substitutes a backend test
or silently skips a missing GUI prerequisite. The root `./check.sh` remains a
separate required gate. The assertion self-tests run in every desktop gate.

This uses a Swift Accessibility driver against the ordinary bundled Tauri
frontend and real scratch `abbeyd`. It requires a logged-in macOS GUI session,
the selected Xcode toolchain, Bun dependencies, pinned Rust, and Accessibility
permission for the runner. The driver checks existing permission without
requesting or changing it. Missing permission, an inaccessible window, traversal
limits, and timeouts fail with fixed diagnostic IDs. The driver is built at
`target/routes-acceptance/routes-ax`; grant permission through macOS settings
if the runner reports `accessibility_or_interactive_gui_required`.

The runner owns all launched PIDs and uses fresh owner-only scratch state,
configuration, bearer, and socket paths. Separate daemon and desktop route logs
make a local fallback observable. It checks the rendered 25/10/50-row tails,
opaque workspace digests, redacted paths, authentication rejection, daemon loss
without stale rows, and an empty log. Seeded route logs must remain byte-identical.
Only read operations cross the existing desktop bridge; fixture setup and daemon
startup can write within scratch state. Cleanup terminates only owned processes.

Every collected Accessibility snapshot is checked for workspace/path canaries,
the generated bearer values, and the forbidden local fallback marker. The driver
scans string attributes, including labels, values, descriptions, help, and titles.
WebKit can invalidate AX elements during updates: the driver retries the entire
tree at most three times for that specific error, retaining already-read strings
from discarded attempts in the secrecy scan. It never accepts a partial tree.
This is bounded Accessibility evidence for these scenarios, not a hidden-DOM
inspection or a guarantee about all possible secret strings. No mock IPC, browser
substitute, test-only application command, or remote-debugging endpoint is used.

`target/routes-acceptance/receipt.json` records the most recent attempt, source
revision and dirty state, available artifact hashes, completed scenarios, and
success or a fixed failure ID. Gate fields remain `not_run` until the actual
separate gate results are recorded. Build logs are owner-only in that same
ignored directory; raw AX snapshots and runtime logs are not retained. A failed
attempt replaces any previous success receipt. Running this script does not
update claims, todos, or release status.

The **packaged** prover (`scripts/prove-packaged-macos.sh`) drives the binary
inside a real `.app` bundle. It selects that bundle by the bundle's own
identifier, never by its name or by its position in a directory listing: both
editions stage into `target/release/bundle/macos`, and "the last `.app` found"
is not a proof of which edition was proved. `--expect-edition` states which
edition must be proved — `safe` by default, `personal` only when that bundle
was actually staged — and the run fails loudly (`bundle_edition_mismatch`,
`no_staged_bundle_for_expected_edition`) rather than proving some other
edition's app. The receipt records the identifier and edition it proved.

## Generated IPC types

`src/ipc/generated*.ts` are produced by `desktop/codegen` and carry an
`@generated … DO NOT EDIT` banner. The inputs are listed in
`codegen/src/main.rs::SOURCES`:

- `src/app_core/ids.rs`
- `src/app_core/run.rs`
- `src/app_core/contracts.rs`
- `desktop/src-tauri/src/ipc.rs`

`ts-rs`, `schemars`, and `specta` were all rejected: each needs a derive macro
added to the types in `src/app_core/`, and a client does not get to reshape the
crate it is a client of. The generator parses the Rust source with `syn`
instead, honours the serde attributes it finds, and **errors rather than
emitting `any`** for a type it does not understand.

A source projection can be confidently wrong, and `--check` only proves a file
is unchanged. So the generator also emits `generated.samples.ts`: the real
`serde_json` output of real `abbey::app_core` values, written as
`… as const satisfies <Type>`. TypeScript checks each literal against the
generated declaration with excess-property checking on, so a wrong field name,
a missed `rename_all`, or a mangled tag fails `tsc --noEmit`.

Verified negatively: changing `CapabilitySet` to `Array<AppCapability>` and the
`approval_requested` tag to `approvalRequested` in `generated.ts` produced five
`tsc` errors (three of them inside `generated.samples.ts`) and a `codegen
--check` failure. `CapabilitySet` is the live canary — its only field is
_private_, and the serde wire shape is an object, not a bare array.

**Task17 verification note:** the generated inventory below is current, not a
retained snapshot. `generate_handler!` enumerates 37 commands and
`src/ipc/client.ts` carries the 37 matching `invoke<…>("app_…")` wrappers, and
the codegen drift check is clean (`codegen: 5 generated file(s) up to date`).
Native acceptance is recorded for the canonical seven Routes scenarios against
the **packaged** bundle (`scripts/prove-packaged-macos.sh`), and for the
packaged menu bar, icon chain, and per-edition window title
(`scripts/prove-menu-icon-macos.sh`, 10 checks per edition) — see
[2026-10-20-desktop-macos-staging.md](../docs/verification/2026-10-20-desktop-macos-staging.md).
That is acceptance of the Routes, menu, and icon surfaces on macOS ARM64
against an ad-hoc signed bundle: other views, the personal-edition GUI, and
other platforms remain unproven, and the bundle is not notarized.

<!-- BEGIN abbey-generated:desktop-capability-summary -->
<!-- Generated by tools/check_claims_sync.py; do not edit. -->

Desktop source inventory: **37 enumerated commands · 10 read capabilities · 8 available views · 3 unavailable views**.

- Invoke IDs: `app_status`, `app_effective_model`, `app_claims`, `app_routes`, `app_run_status`, `app_run_events`, `app_v3_grants`, `app_memory_search`, `app_memory_metadata`, `app_models_list`, `app_model_grants`, `app_model_details`, `app_model_download`, `app_model_load`, `app_model_unload`, `app_model_download_status`, `app_model_lifecycle_status`, `app_claim_by_id`, `app_connection`, `app_bundle_identity`, `app_conversation_grants`, `app_conversations_list`, `app_conversations_search`, `app_conversation_read`, `app_conversation_create`, `app_conversation_rename`, `app_conversation_archive`, `app_conversation_delete`, `app_conversation_messages`, `app_conversation_content`, `app_conversation_routes`, `app_conversation_submit_turn`, `app_conversation_cancel_turn`, `app_conversation_lookup_turn`, `app_conversation_update`, `app_tools_list`, `app_system_integration`
- Read capability IDs: `read_status`, `read_claims`, `read_routes`, `read_run`, `read_run_events`, `read_memory`, `read_models`, `read_claims_by_id`, `list_tools`, `read_conversations`
- Available view IDs: `doctor` → `read_status`, `claims` → `read_claims`, `chat` → `read_conversations`, `runs` → `read_run`, `tools` → `read_status`, `memory` → `read_memory`, `models` → `read_models`, `routes` → `read_routes`
- Unavailable view IDs: `automations`, `training`, `cluster`

<!-- END abbey-generated:desktop-capability-summary -->

## The complete Tauri command surface

37 commands are enumerated in `generate_handler!`: the app-core reads, the exact
signed-model lifecycle, the v4 conversation methods, the read-only tool inventory,
and this build's host-integration facts. Existing read/model sessions stay
unchanged; the separate conversation session requests exactly `ReadConversations`
and `WriteConversations`. No conversation operation falls back to an in-process
store.

| Command                                                | Returns                    | Backed by                                                                                                                                 |
| ------------------------------------------------------ | -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `app_status`                                           | `RuntimeStatus`            | `AppCommand::Status` (`ReadStatus`)                                                                                                       |
| `app_claims(query: ClaimsQuery)`                       | `ClaimsSnapshot`           | `AppCommand::Claims` (`ReadClaims`)                                                                                                       |
| `app_routes(query: RouteAuditQuery)`                   | `RouteAuditPage`           | `AppCommand::ReadRoutes` (`ReadRoutes`)                                                                                                   |
| `app_run_status(query: RunQuery)`                      | `RunSnapshot`              | `AppCommand::GetRun` (`ReadRun`)                                                                                                          |
| `app_run_events(query: RunEventsQuery)`                | `RunEventPage`             | `AppCommand::RunEvents` (`ReadRunEvents`)                                                                                                 |
| `app_v3_grants`                                        | `V3CapabilitySet`          | negotiate `ReadMemory` + `ReadModels` + `ReadClaimsById`                                                                                  |
| `app_memory_search(request: V3SearchRequest)`          | `V3EntityPage`             | v3 `SearchMemory`                                                                                                                         |
| `app_memory_metadata(query: V3ResourceQuery)`          | `V3EntityRecord`           | v3 `ReadMemoryMetadata`                                                                                                                   |
| `app_models_list(query: V3PageQuery)`                  | `V3EntityPage`             | v3 `ListModels` (`ReadModels`)                                                                                                            |
| `app_model_grants`                                     | `V3CapabilitySet`          | separate model-session negotiation                                                                                                        |
| `app_model_details(query: V3ResourceQuery)`            | `V3ModelDetails`           | v3 exact `ModelDetails` (`ReadModels`)                                                                                                    |
| `app_model_download(action: V3ModelAction)`            | `V3OperationStatus`        | v3 `DownloadModel` (`DownloadModels`)                                                                                                     |
| `app_model_load(action: V3ModelAction)`                | `V3OperationStatus`        | v3 `LoadModel` (`ManageModels`)                                                                                                           |
| `app_model_unload(action: V3ModelAction)`              | `V3OperationStatus`        | v3 `UnloadModel` (`ManageModels`)                                                                                                         |
| `app_model_download_status(query: V3ResourceQuery)`    | `V3OperationStatus`        | v3 exact-operation `ModelDownloadStatus` (`ReadModels`)                                                                                   |
| `app_model_lifecycle_status(query: V3ResourceQuery)`   | `V3OperationStatus`        | v3 exact-operation status (`ReadModels`); no inference command                                                                            |
| `app_claim_by_id(query: V3ResourceQuery)`              | `V3StableClaim`            | v3 `ClaimById` (`ReadClaimsById`)                                                                                                         |
| `app_connection`                                       | `ConnectionInfo`           | local; describes the route, reads no Abbey state                                                                                          |
| `app_bundle_identity`                                  | `BundleIdentity`           | `abbey::edition` + the running Tauri config                                                                                               |
| `app_conversation_grants`                              | `V4CapabilitySet`          | separate v4 read/write conversation negotiation                                                                                           |
| `app_conversations_list(query: V4PageQuery)`           | `V4ConversationPage`       | v4 fixed-watermark history summaries                                                                                                      |
| `app_conversations_search(query: V4SearchQuery)`       | `V4ConversationPage`       | v4 title-only search                                                                                                                      |
| `app_conversation_read(query: V4ResourceQuery)`        | `V4Conversation`           | v4 exact metadata read                                                                                                                    |
| `app_conversation_create(query: V4ConversationCreate)` | `V4Conversation`           | v4 create metadata once                                                                                                                   |
| `app_conversation_rename(query: V4ConversationRename)` | `V4Conversation`           | v4 rename once                                                                                                                            |
| `app_conversation_archive(query: V4ResourceQuery)`     | `V4Conversation`           | v4 archive once                                                                                                                           |
| `app_conversation_delete(query: V4ResourceQuery)`      | `()`                       | v4 delete once, after destructive UI confirmation                                                                                         |
| `app_conversation_messages(query: V4MessageQuery)`     | `V4MessagePage`            | v4 bounded authorized text bodies                                                                                                         |
| `app_conversation_content(query: V4ContentQuery)`      | `V4ContentPage`            | v4 bounded authorized content continuation                                                                                                |
| `app_conversation_routes`                              | `Vec<V4Route>`             | v4 `ListRoutes` (`ReadRoutes`) over the conversation session                                                                              |
| `app_conversation_submit_turn(request: V4SubmitTurn)`  | `V4TurnSubmission`         | v4 `SubmitTurn` (`SubmitTurns`) on the selected route                                                                                     |
| `app_conversation_lookup_turn(conversationId, runId)`  | `Option<V4TurnSubmission>` | v4 exact admitted-turn read (`ReadTurns`)                                                                                                 |
| `app_conversation_update(query: V4ConversationPatch)`  | `V4Conversation`           | v4 conversation patch, once                                                                                                               |
| `app_conversation_cancel_turn(conversationId, runId)`  | `RunState`                 | exact durable v4 pair; one cancellation request, then history/content reads                                                               |
| `app_tools_list(query: V3PageQuery)`                   | `V3ToolPage`               | separate tool-session negotiation (`ListTools` only — enumeration, never invocation)                                                      |
| `app_effective_model(routeId, backend, model)`         | `EffectiveModel`           | the model a turn would use right now, resolved with Abbey's shared spawn-path rules from the runtime's own published route (`ReadRoutes`) |
| `app_system_integration`                               | `SystemIntegration`        | local; the compiled-in capability manifest and the edition table                                                                          |

There is no `exec`, no generic run submit/cancel, no tool invoke, no obsolete, no
command-name parameter that resolves to a subprocess, and no plugin that
would provide one. Model effects are limited to exact signed download/load/unload; there is no inference command. In-process `GetRun`/`RunEvents`/memory/model authority is rejected.

### The tool inventory is a list, not a lever

`app_tools_list` opens its own protocol-v3 session that negotiates exactly one
capability: `ListTools`. It never requests `InvokeTools`,
`DecideToolApprovals`, or `CancelTools`, so this client cannot run a tool, answer
an approval, or cancel one — the desktop has no such command at all, and
`main_tests.rs` asserts the handler contains no `invoke`, `approve`, or
`obsolete` entry.

The Tools view renders what the daemon actually advertises: the safe edition's
`abbeyd` lists the three canonical read-only tools plus the one mutating
`abbey_memory_mark_obsolete`, and the personal edition's `abbeyd` lists only the
three read-only tools. Each row shows the daemon's own description and effect
class. The mutating tool is shown as _approval required_ — the daemon requires an
exact-call approval before it runs — and there is no control on that page that
could supply one. When the daemon is unreachable, refuses the grant, or has not
negotiated `ListTools`, the view says which of those happened; it never shows an
empty list or a list of its own.

### System integration is stated, not offered

`app_system_integration` reports four read-only facts about the shipped build,
derived from the compiled-in `capabilities/default.json` and the edition table:
login startup is **not registered** by this app, no background service is
installed or started by it (the daemon is a separate owner-only process), the
declared OS permissions are exactly `core:default` and `updater:default` — with
no filesystem, shell, process, or opener permission among them — and closing the
window does not cancel daemon-owned work. Nothing in the desktop registers a
login item, installs a service, requests an OS permission, or spawns a helper
process; `tests/system-integration.test.tsx` re-checks that against the shipped
manifest and sources.

**Routing.** If a daemon bearer is configured for the active edition, every app-core read
goes over `abbeyd`'s authenticated owner-only Unix socket and a failure is
_reported_. It is never retried in-process: `AGENTS.md` requires that client
failures never fall back to in-process claims. In-process is used only when no
bearer is configured at all, and `app_connection().source` always says which
answered.

## Security properties, and how each is checked

| Property                                        | Where                                                         | Checked by                                                                                                                                                                              |
| ----------------------------------------------- | ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Strict CSP                                      | `tauri.conf.json` `app.security.csp`                          | `main.rs::tests::the_declared_csp_is_strict`, `verify-bundle.mjs`                                                                                                                       |
| No remote JS                                    | Vite emits only local assets                                  | `verify-bundle.mjs` scans `dist/` for remote `src`/`href` and remote dynamic `import()`                                                                                                 |
| No `eval`                                       | —                                                             | `verify-bundle.mjs` scans `dist/` for `eval(` and `new Function(`                                                                                                                       |
| No execute-shell invoke                         | no shell/fs/http/process/opener/dialog plugin in `Cargo.toml` | `main.rs::tests::no_execution_capable_plugin_is_declared`, `verify-bundle.mjs`                                                                                                          |
| Narrow enumerated commands                      | `generate_handler!`                                           | `verify-bundle.mjs` rejects any `invoke` outside `src/ipc/client.ts` and any command name off the list                                                                                  |
| Least-privilege capability                      | `capabilities/default.json` → `core:default`                  | asserted in `main.rs` tests                                                                                                                                                             |
| Secrets never echoed                            | `ipc.rs` has no field that can hold a bearer                  | `backend.rs::tests::connection_detail_never_contains_bearer_material`                                                                                                                   |
| Updater artifacts verify against the pinned key | `plugins.updater.pubkey` + `tests/updater_signature.rs`       | `cargo test -p abbey-desktop --test updater_signature` — the same `minisign-verify` path the plugin uses; the release workflow re-runs it against each built artifact before publishing |

The shipped CSP is:

```
default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:;
font-src 'self'; connect-src 'self' ipc: http://ipc.localhost; object-src 'none';
base-uri 'none'; form-action 'none'; frame-src 'none'; frame-ancestors 'none';
worker-src 'none'
```

`devCsp` additionally allows `'unsafe-inline'` styles and the Vite HMR socket.
It is a dev-server-only value and never ships; `verify-bundle.mjs` checks the
shipped `csp` and deliberately ignores `devCsp`.

Secret hygiene is a type-level property: no field on any type in `ipc.rs` can
hold a bearer token. The daemon bearer is reported as a boolean plus a _source
kind_ (`inline_env` / `token_file` / `conflicting`) — never a value, never the
token file's contents, and `DaemonConfig` is never formatted into an IPC type.

## Native menu, icon, and the update trigger

Four surfaces shipped on 2026-10-20 and are proven against the **packaged**
app, not only in source.

**Menu bar.** `src/main.rs` builds the menu with `.menu(...)`: an app menu
(About carrying the same version the Doctor surface prints, then hide/quit),
an Edit menu (undo/redo/cut/copy/paste/select-all — so the composer gets the
standard macOS shortcuts), and a Window menu (fullscreen/minimize/zoom/
bring-all-to-front/close). Every entry is an OS-provided role; there are no
custom handlers, so no menu item can reach webview state, the daemon, or the
filesystem. The first submenu becomes the macOS app menu, whose name the OS
takes from the bundle rather than from the submenu title.

**Icon.** The tracked icon used to be a flat placeholder rectangle.
`scripts/render-icon.swift` renders a 1024 master from vector geometry
(gradient squircle, white Romanesque abbey-arch doorway) and `bun run tauri
icon` regenerates the whole chain from it; the master is tracked, so the chain
is reproducible. `public/favicon.png` gives the webview the same mark.

**Window defaults.** `tauri.conf.json` centers the window on first launch, and
the title is set at startup from the edition's product name — the personal
build never presents itself as "Abbey".

Settings also offers automatic/glass/solid webview material, comfortable/compact
density, and a presentation-only restore-defaults action. Automatic material is
macOS-only, with opaque fallback elsewhere and when reduced transparency,
increased contrast, or forced colors apply. This is glass-inspired CSS over an
opaque native window, **not native Apple Liquid Glass**. It adds no startup,
background-service, model-route, or tool permissions. See
[`2026-10-21-desktop-presentation.md`](../docs/verification/2026-10-21-desktop-presentation.md)
for evidence and remaining qualification work.

**Update trigger.** The updater plugin is registered with the pinned Ed25519
public key and `requireSignedVersion: true`, so the version announced in a
feed must equal the version inside that artifact's signature. There is one
endpoint per edition: the safe edition reads `latest.json`, the personal
edition's overlay reads `latest-personal.json`. The only trigger is the
Doctor surface's Updates card (`src/components/UpdateCheck.tsx`): checks are
user-initiated, the plugin verifies the artifact against the pinned key before
installing it, and there is no in-app relaunch (no process plugin is
registered — the capability set stays exactly `core:default` +
`updater:default`), so the surface says to quit and reopen. Feeds are
assembled by `scripts/assemble-update-feed.sh` and published by
`.github/workflows/desktop-release.yml`; that workflow is **authored, not
executed** — it needs the owner's Apple credentials and production updater
key, and it creates draft releases only.

## Edition identity

`src/edition.rs` is the single identity table. The desktop derives from it
rather than keeping a second copy:

- `codegen/src/editions.rs` **writes** `identifier` and `productName` into
  `tauri.conf.json` (safe) and `tauri.personal.conf.json` (personal override)
  from `Edition::{Safe,Personal}.identity()`. Editing them by hand fails
  `bun run codegen:check`.
- `main.rs` tests assert both configs match the edition table and differ from
  each other.
- At startup, `main.rs` refuses to run if the packaged identifier does not equal
  the compiled edition's `bundle_id`, so a personal build can never present
  itself under `com.donaldfilimon.abbey`.

Build the personal edition with:

```bash
cargo build -p abbey-desktop --features personal-edition
bun run tauri build --config src-tauri/tauri.personal.conf.json
```

## Views: real vs placeholder

**Real** (backed by live capabilities the desktop bridge consumes):

- **Chat** — sidebar title search/history with paged summaries and text-only messages. Create/rename/archive/delete metadata require v4 write grants; delete has an explicit confirmation. The composer submits one turn to an available configured daemon route and reads correlated assistant content. Drafts use window-session memory only: at most 32 nonempty drafts, each capped at 8,192 UTF-16 code units; the controller refuses to evict existing drafts. History is bounded to 256 summaries/messages per window view; search or refresh starts a new page snapshot. Unknown submission outcomes are not replayed; history reconciliation checks for the exact sent user message. Pin, unarchive, and full transcript export remain unavailable. Stop targets the active selected conversation turn through its exact durable run identity and reconciles cancellation through reads. This is not a task-delegation or tool-execution loop.

- **Doctor** — runtime state, protocol/schema versions, build stamp, edition,
  granted capabilities, connection route, bundle identity (`ReadStatus`).
- **Capabilities** — the canonical ledger with the core-validated filter (`ReadClaims`).
- **Routes** — a bounded sanitized route-audit tail with opaque workspace digests (`ReadRoutes`).
- **Runs & trace** — one durable run snapshot and a fixed-watermark lifecycle
  page (`ReadRun` / `ReadRunEvents`). In-process `CapabilitySet::standard()`
  does not grant those; a protocol-v2 daemon does. Submit and cancel stay off
  the invoke surface. Task _delegation_ is not on this invoke surface either:
  the page says so through the same mechanism the unavailable pages use — the
  contract-level reason, plus the live ledger rows for it — so the refusal has
  one voice everywhere in this client.
- **Memory** — sanitized `memory-v1-summary` search and metadata (`ReadMemory`
  v3 grant). No payload, provenance, path, or raw record id. Obsolete is not
  registered. In-process never opens the store.

- **Models** — bounded fixed-watermark inventory plus exact signed-model details when startup authority exists. Details expose revision, architecture, configured device, license digest/acceptance, and actual loaded state. Download/load/unload require their distinct grants and concrete confirmation; license terms are never accepted by the desktop. Each operation is sent once and tracked by immutable ID in window-session memory. Ambiguous responses retain their ID for status-only recovery. Inventory-only authorities remain readable without lifecycle controls. Loaded status does not demonstrate inference.

**Unavailable execution surfaces** — governed tasks, Tools & approvals, Automations, Training, Cluster, browser/files/computer control.
These views render no fabricated operational data. Each states the contract-level
reason and then shows the _live_ ledger rows for a filter, so the UI never
asserts a status it invented. A surface that is available for one thing but
withholds another (the Runs surface, which reads a run's lifecycle but cannot
delegate a task) states that withheld part in the same vocabulary and through
the same rendering, so the client never has two voices for "unavailable".

The distinction matters and is preserved: the local mesh proof and the role
bindings are **Current** in Abbey — they are simply not on this desktop
invoke surface. Memory summaries are Current on the daemon and now on this
read path. Only Training is marked `capability_not_implemented`.
Calling the Current ones "Proposed" would be a fabricated claim in the
opposite direction.

View availability is computed from the `CapabilitySet` returned by
`app_status`, not from a hardcoded boolean. Additional protocol-v2
capabilities never auto-create a view or widen the ten-command Tauri
allowlist; each new desktop operation needs an explicit narrow invoke, UI,
and evidence slice.

## Unmet bars

Phase 7 in `tasks/todo.md` asks for more than this slice delivers:

- **Not notarized.** `tauri build` bundles are produced and signed (ad-hoc, with
  the hardened runtime) by `scripts/release-macos.sh` for both editions, and the
  packaged app is accepted by `scripts/prove-packaged-macos.sh`. What is still
  missing is Apple Developer ID signing plus notarization (and Windows/Linux
  signing keys) — all owner-controlled release blockers; no credential was
  manufactured or embedded.
- **The existing backend smoke remains process-level.**
  `desktop/scripts/prove-daemon-read.sh` starts an owner-only scratch daemon
  and drives the shipped desktop `status` / `run_status` / `run_events`
  functions over that socket. A bearer with no listener still fails closed
  (`a_configured_daemon_never_falls_back_to_the_in_process_core`). No WebView
  is opened by that older script.
- **Windowed runtime proof is Routes-only on macOS ARM64.** On 2026-09-06,
  `scripts/prove-routes-macos.sh` opened the ordinary safe-edition Tauri window
  and passed all seven scenario checks against real scratch daemons. Thirteen
  assertion/isolation tests, the complete desktop gate with native acceptance
  required, and root `./check.sh` passed. The same seven scenarios now also
  pass against the **packaged** app (`scripts/prove-packaged-macos.sh`, 2026-10-20).
  Other views, the personal edition GUI, and other platforms remain unproven.
- **Not run on Ubuntu ARM64 or Win11 ARM.** `ClientError::UnsupportedPlatform`
  is surfaced honestly on Windows, where `abbeyd` has no named-pipe transport.
  Native adapters (macOS accessibility, etc.) are unavailable (report-only inventory).
- **The Linux desktop graph retains one upstream dependency advisory.** The
  current Tauri 2.11.5 graph selects GTK 0.18.2 and `glib` 0.18.5; GitHub's
  medium-severity `VariantStrIter` advisory requires `glib` 0.20 or newer.
  That incompatible transitive upgrade is not available through the current
  Tauri release, so the proposed desktop must not be described as
  vulnerability-free or release-ready.
- **Governed tasks, Tools, Automations, Training, and Cluster execution remain unavailable.** Chat submits bounded turns only through the daemon's configured provider route. Settings/Doctor, Claims, Routes, Runs & trace, Memory, and Models consume real bridge contracts. Cancel, tool invocation, and obsolete are not
  on the invoke surface.
- **No per-edition capability manifest split.** Both editions ship the same
  `capabilities/default.json`; `CapabilitySet` is identical in both editions
  today, so there is nothing to differentiate yet.
- **Icons and the native menu are now real, and their packaged proof is
  macOS-only.** The icon chain is rendered from the tracked 1024 master and
  `scripts/prove-menu-icon-macos.sh` checks the menu bar, the icon identity and
  the per-edition window title against the packaged app. No Windows or Linux
  packaging has been produced. The generated Windows `.ico` is listed in the
  bundle configuration, but that is preparation, not Windows runtime evidence.
- `ClaimRecord` carries no stable claim `id`, only display text, so placeholder
  views cite the ledger by substring filter rather than by id. Fixing that means
  changing `src/app_core/`, which this slice deliberately did not do.

Desktop Stop is available for an active selected conversation turn when the daemon grants `cancel_turns`. It preserves drafts and transcript content, blocks duplicate Stop requests immediately, and reconciles requested or ambiguous outcomes through history/content reads. A completion race reports the actual terminal state. Cancellation never targets a legacy run or another conversation, and transport ambiguity never replays the request.
