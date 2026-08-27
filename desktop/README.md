# Abbey desktop (Tauri 2 + React + TypeScript)

A desktop **client of Abbey's shared application core**. It is not a second
Abbey: it owns no agent, no memory, no tools, and no execution path. The current
scaffold exposes read-only `Status`, `Claims`, `ReadRoutes`, and protocol-v2
`GetRun`/`RunEvents` app-core commands, plus connection and bundle-identity
metadata. Submit and cancel remain unregistered: this window can inspect a
durable run, not start or stop one.

Ledger status: `desktop-tauri-react` is **Proposed**. Nothing here changes that.
The client compiles, typechecks, and reads the linked application core when no
bearer is set. With a bearer, process-level reads go through a real scratch
`abbeyd` (`desktop/scripts/prove-daemon-read.sh`). It has not been packaged,
signed, notarized, or opened as a window — see [Unmet bars](#unmet-bars).

## Layout

```
desktop/
  package.json  tsconfig.json  vite.config.ts  index.html   bun + Vite + React 19
  Cargo.toml                                                 separate workspace (see below)
  scripts/verify-bundle.mjs                                  artifact security checks
  codegen/                                                    Rust → TypeScript generator
    src/main.rs        driver, --check drift mode
    src/tsgen.rs       syn projection of serde contracts
    src/samples.rs     real serde output as `as const satisfies` fixtures
    src/editions.rs    bundle identity → tauri configs + TS
  src/
    main.tsx  App.tsx  styles.css  surfaces.ts
    ipc/client.ts                  typed wrappers over the seven commands
    ipc/generated.ts               @generated — IPC types
    ipc/generated.samples.ts       @generated — serde cross-check fixtures
    ipc/generated.editions.ts      @generated — edition identity
    views/DoctorView.tsx  ClaimsView.tsx  RoutesView.tsx  RunsView.tsx
                               UnavailableView.tsx  ErrorCard.tsx
  src-tauri/
    Cargo.toml  build.rs  tauri.conf.json  tauri.personal.conf.json
    capabilities/default.json      core:default only
    icons/                         placeholder marks, not brand assets
    src/main.rs                    generate_handler! — the whole IPC surface
    src/commands.rs                the seven commands
    src/backend.rs                 daemon-first routing, no silent fallback
    src/ipc.rs                     desktop-only transport types
```

`desktop/Cargo.toml` is a **separate workspace**. The root `abbey` manifest has
no `[workspace]` table, so nothing here is absorbed into Abbey's build,
lockfile, `cargo clippy --all-targets`, or `check.sh`'s `src/**/*.rs` file-size
guard. `./check.sh` at the repo root is unaffected.

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
`./check.sh`, which must stay a pure Rust-crate gate needing neither bun nor a
WebView toolchain. The cost is real and worth stating: generated-type drift,
the bundle scan, and the personal-edition build are caught only when somebody
runs `desktop/check.sh`. Nothing automatic enforces them.

Two verification notes:

- `cargo check` does **not** link. `desktop/check.sh` runs `cargo build` as
  well, because a Tauri binary that type-checks can still fail to link against
  the platform WebView frameworks.
- `backend::tests::connection_detail_never_contains_bearer_material` and
  `a_configured_daemon_never_falls_back_to_the_in_process_core` are
  *conditional* on a bearer being exported, and no-op in the default run. Run
  the suite a second time to exercise them
  (`ABBEYD_BEARER_TOKEN=$(python3 -c "print('a'*48)") cargo test -p abbey-desktop`);
  setting the variable from inside a test is not possible because
  `std::env::set_var` is `unsafe` in edition 2024 and this crate denies
  `unsafe_code`.

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
*private*, and the serde wire shape is an object, not a bare array.


<!-- BEGIN abbey-generated:desktop-capability-summary -->
<!-- Generated by tools/check_claims_sync.py; do not edit. -->
Desktop source inventory: **7 enumerated commands · 5 read capabilities · 4 available views · 6 unavailable views**.

- Invoke IDs: `app_status`, `app_claims`, `app_routes`, `app_run_status`, `app_run_events`, `app_connection`, `app_bundle_identity`
- Read capability IDs: `read_status`, `read_claims`, `read_routes`, `read_run`, `read_run_events`
- Available view IDs: `doctor` → `read_status`, `claims` → `read_claims`, `runs` → `read_run`, `routes` → `read_routes`
- Unavailable view IDs: `chat`, `tools`, `memory`, `models`, `training`, `cluster`
<!-- END abbey-generated:desktop-capability-summary -->

## The complete Tauri command surface

Seven commands, all read-only, enumerated in `generate_handler!`:

| Command | Returns | Backed by |
| --- | --- | --- |
| `app_status` | `RuntimeStatus` | `AppCommand::Status` (`ReadStatus`) |
| `app_claims(query: ClaimsQuery)` | `ClaimsSnapshot` | `AppCommand::Claims` (`ReadClaims`) |
| `app_routes(query: RouteAuditQuery)` | `RouteAuditPage` | `AppCommand::ReadRoutes` (`ReadRoutes`) |
| `app_run_status(query: RunQuery)` | `RunSnapshot` | `AppCommand::GetRun` (`ReadRun`) |
| `app_run_events(query: RunEventsQuery)` | `RunEventPage` | `AppCommand::RunEvents` (`ReadRunEvents`) |
| `app_connection` | `ConnectionInfo` | local; describes the route, reads no Abbey state |
| `app_bundle_identity` | `BundleIdentity` | `abbey::edition` + the running Tauri config |

There is no `exec`, no submit, no cancel, no command-name parameter that
resolves to a subprocess, and no plugin that would provide one. In-process
`GetRun`/`RunEvents` are rejected: those grants exist only on a protocol-v2
daemon.

**Routing.** If a daemon bearer is configured for the active edition, every app-core read
goes over `abbeyd`'s authenticated owner-only Unix socket and a failure is
*reported*. It is never retried in-process: `AGENTS.md` requires that client
failures never fall back to in-process claims. In-process is used only when no
bearer is configured at all, and `app_connection().source` always says which
answered.

## Security properties, and how each is checked

| Property | Where | Checked by |
| --- | --- | --- |
| Strict CSP | `tauri.conf.json` `app.security.csp` | `main.rs::tests::the_declared_csp_is_strict`, `verify-bundle.mjs` |
| No remote JS | Vite emits only local assets | `verify-bundle.mjs` scans `dist/` for remote `src`/`href` and remote dynamic `import()` |
| No `eval` | — | `verify-bundle.mjs` scans `dist/` for `eval(` and `new Function(` |
| No execute-shell invoke | no shell/fs/http/process/opener/dialog plugin in `Cargo.toml` | `main.rs::tests::no_execution_capable_plugin_is_declared`, `verify-bundle.mjs` |
| Narrow enumerated commands | `generate_handler!` | `verify-bundle.mjs` rejects any `invoke` outside `src/ipc/client.ts` and any command name off the list |
| Least-privilege capability | `capabilities/default.json` → `core:default` | asserted in `main.rs` tests |
| Secrets never echoed | `ipc.rs` has no field that can hold a bearer | `backend.rs::tests::connection_detail_never_contains_bearer_material` |

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
hold a bearer token. The daemon bearer is reported as a boolean plus a *source
kind* (`inline_env` / `token_file` / `conflicting`) — never a value, never the
token file's contents, and `DaemonConfig` is never formatted into an IPC type.

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

- **Doctor** — runtime state, protocol/schema versions, build stamp, edition,
  granted capabilities, connection route, bundle identity (`ReadStatus`).
- **Capabilities** — the canonical ledger with the core-validated filter (`ReadClaims`).
- **Routes** — a bounded sanitized route-audit tail with opaque workspace digests (`ReadRoutes`).
- **Runs & trace** — one durable run snapshot and a fixed-watermark lifecycle
  page (`ReadRun` / `ReadRunEvents`). In-process `CapabilitySet::standard()`
  does not grant those; a protocol-v2 daemon does. Submit and cancel stay off
  the invoke surface.

**Placeholder** — Chat, Tools & approvals, Memory, Models, Training, Cluster.
These six views render **no data at all**. Each states the contract-level
reason and then shows the *live* ledger rows for a filter, so the UI never
asserts a status it invented.

The distinction matters and is preserved: memory, the local mesh proof, and
the role bindings are **Current** in Abbey — they are simply not on this
desktop invoke surface. Only Training is marked `capability_not_implemented`.
Calling the Current ones "Proposed" would be a fabricated claim in the
opposite direction.

View availability is computed from the `CapabilitySet` returned by
`app_status`, not from a hardcoded boolean. Additional protocol-v2
capabilities never auto-create a view or widen the seven-command Tauri
allowlist; each new desktop operation needs an explicit narrow invoke, UI,
and evidence slice.

## Unmet bars

Phase 7 in `tasks/todo.md` asks for more than this slice delivers:

- **Not packaged, signed, or notarized.** No `tauri build` bundle was produced.
  Apple Developer ID + notarization and Windows/Linux signing keys are
  owner-controlled release blockers; no credential was manufactured or embedded.
- **Live `abbeyd` reads are process-level, not a window.**
  `desktop/scripts/prove-daemon-read.sh` starts an owner-only scratch daemon
  and drives the shipped desktop `status` / `run_status` / `run_events`
  functions over that socket. A bearer with no listener still fails closed
  (`a_configured_daemon_never_falls_back_to_the_in_process_core`). No WebView
  is opened.
- **No windowed runtime proof.** A real `abbey-desktop` binary links
  (`Mach-O 64-bit executable arm64`, macOS ARM64), but the window has never been
  opened, so nothing here has been seen rendering.
- **Not run on Ubuntu ARM64 or Win11 ARM.** `ClientError::UnsupportedPlatform`
  is surfaced honestly on Windows, where `abbeyd` has no named-pipe transport.
- **The Linux desktop graph retains one upstream dependency advisory.** The
  current Tauri 2.11.5 graph selects GTK 0.18.2 and `glib` 0.18.5; GitHub's
  medium-severity `VariantStrIter` advisory requires `glib` 0.20 or newer.
  That incompatible transitive upgrade is not available through the current
  Tauri release, so the proposed desktop must not be described as
  vulnerability-free or release-ready.
- **Six of the ten first-release views have no data source.** Doctor, Claims,
  Routes, and Runs & trace are wired. Chat, Tools, Memory, Models, Training,
  and Cluster stay placeholders. Submit and cancel are not on the invoke
  surface.
- **No per-edition capability manifest split.** Both editions ship the same
  `capabilities/default.json`; `CapabilitySet` is identical in both editions
  today, so there is nothing to differentiate yet.
- **Icons are placeholder marks**, not brand assets, and there is no `.icns`.
- `ClaimRecord` carries no stable claim `id`, only display text, so placeholder
  views cite the ledger by substring filter rather than by id. Fixing that means
  changing `src/app_core/`, which this slice deliberately did not do.
