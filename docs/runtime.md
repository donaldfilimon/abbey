# Durable runtime architecture

Abbey has two deliberately bounded runtime protocols over one local daemon:

1. Protocol v1 remains an authenticated read-only compatibility surface accepting only
   `Status` and `Claims`.
2. Protocol v2 adds typed run submission, durable status/cancellation, and paged sanitized
   lifecycle events for startup-bound ABI-local and Foundation Models recipes on Unix.

The v2 request can select only a closed backend and mode plus literal input. It cannot
choose a program, argv prefix, environment, workspace, trust flag, tool, or shell.
Provider-neutral model/tool ownership remains a separate Proposed capability.

## Contracts

`app_core` owns canonical `RunId`, `ConversationId`, and `IdempotencyKey` values plus
bounded `RunRequest`, `RunSnapshot`, `RunFailure`, and lifecycle-event types. Requests
select only a closed backend enum. They cannot provide an executable path, arbitrary
arguments, environment variables, trust flags, or an `AgentConfig`.

The lifecycle is:

```text
Queued -> Starting -> Running -> Succeeded | Failed | Cancelled | Interrupted
   |          |          |
   +----------+----------+-> CancelRequested -> Succeeded | Failed | Cancelled | Interrupted
```

Terminal states are immutable. Every transition is checked in the shared contract and
again by the transactional store.

## Storage

`RuntimeStore` uses `<state>/daemon/runtime.sqlite`. It is independent from
`memory.sqlite` and from the optional WDBX semantic-memory backend.

The version-4 schema contains:

- `schema_migrations`
- `conversations`
- `conversation_backends`
- `runs`
- `run_events`
- `audit_events`
- `legacy_conversation_imports`
- `legacy_conversation_aliases`
- `legacy_conversation_entries`
- `conversation_identity_aliases`
- `conversation_identity_scopes`
- `conversation_identity_commit`
- `conversation_identity_tombstones`
- `conversation_identity_clear_all`
- `conversation_identity_mutations`
- `conversation_identity_mutation_scopes`
- `conversation_identity_migrated_scopes`

SQLite runs with foreign keys enabled, WAL journaling, `synchronous=FULL`, and a bounded
busy timeout. Migrations and run transitions use immediate transactions. A state change
and its lifecycle event commit atomically, with one-based strictly increasing event
sequences.

Idempotency keys are bound to a SHA-256 digest computed by `RunManager` from the validated
serialized `RunRequest`. Reusing a key with the same request returns the original run;
reusing it with different request data fails with `IdempotencyConflict`.

## Legacy conversation metadata migration

On Unix, daemon startup inspects only the active edition's canonical `history.log`,
`chat-id`, and direct regular files under `by-cwd`. `chat-id.export` participates in the
byte-exact backup but is never parsed, sourced, evaluated, or imported. Environment
overrides that name other chat/history paths are deliberately ignored by this automatic
migration.

The snapshot is bounded to 1,024 source files, 2 MiB per file, 8 MiB total, and 4,096
parsed observations. Files are opened with `O_NOFOLLOW`; symlinks, non-regular files,
unsafe ownership/permissions, oversize sources, and a snapshot that remains unstable
after three attempts fail closed. The second pass compares both metadata fingerprints and
bytes because legacy writers do not share a migration lock.

Before SQLite changes, Abbey finalizes an owner-only content-addressed backup at
`<state>/daemon/legacy-conversation-backups/v1-<digest>/`. It contains exact source bytes
and a deterministic manifest with the retained capture time, source roles, sizes, and
per-file SHA-256 values. Files and directories are synchronized around the atomic rename;
an existing destination is verified rather than overwritten. The original metadata and
every finalized backup are retained.

Only digest-addressed opaque UUIDv8 identities and normalized UTC timestamp envelopes
enter `runtime.sqlite` in one immediate transaction. Raw legacy IDs, cwd values, source
paths, transcript/prompt/provider output, memory/embeddings, route audit, backend
bindings, inferred titles, and reconstructed runs do not. Native UUID collisions roll
back the entire batch. An unchanged snapshot reuses the manifest timestamp and import
marker; a changed snapshot creates another retained backup and may widen only the
trustworthy timestamp envelope. This startup-only migration adds no `AppCommand`,
`AppEvent`, CLI/TUI/desktop invoke, MCP tool, process, model, or network authority.

## Canonical conversation identity mutations and compatibility mirrors

On Unix, conversation-identity saves and clears first validate and domain-separate the
active edition and affected global/per-working-directory scopes. A save stores only
opaque alias, edition, scope, scope-set, and mutation-token digests; the deterministic
UUIDv8 conversation identity; a monotonic revision; and the commit timestamp. Schema v4
extends the operation-specific commit marker with `clear_scope` and `clear_all`, durable
opaque mutation receipts plus their exact scope mapping, per-scope tombstones, an
edition-wide clear-all marker, and immutable provenance for selections migrated from v3.
Clear markers contain no alias or conversation identity. Raw external identities,
working directories, and mutation tokens never enter `runtime.sqlite`, and metadata-only
access does not perform unrelated run recovery.

Only after the canonical transaction commits does Abbey project a save or clear through
the sole coordinator at `<state>/daemon/conversation-mirror-journal/`. The directory is
`0700`; its stable `lock` and transient atomic `pending.json` are `0600`. The same `fs4`
advisory lock serializes saves, clears, legacy capture, history observation, and
compaction. A pending plan may transiently contain the exact raw mirror material required
for recovery, so it remains private and its `Debug` and error surfaces are redacted.

Save recovery compares the journal's opaque marker with the canonical SQLite commit,
discards prepared-but-uncommitted plans, and completes exactly marked saves once without
duplicating history. Clear recovery follows the same commit-first rule: an uncommitted
plan removes nothing, while an authenticated committed or partially applied plan removes
each exact mirror idempotently. Current per-working-directory clear removes only the
uniquely authorized active mirror and preserves the global fallback. In non-per-cwd mode,
current clear removes the global and matching export mirrors. Clear-all commits one
active-edition marker and removes global/export plus a sorted, exact, bounded inventory
of direct regular files under `by-cwd`; it neither walks recursively nor affects another
edition.

Every planned destination is absolute, pairwise distinct, outside the runtime database
and reserved journal subtree, and bound to its expected role and before-state. Divergent,
shared, omitted, duplicated, symlinked, non-regular, unsafe-parent, malformed, or
permission-unsafe targets fail closed. Durable mutation receipts authenticate the clear
effect even after a later mutation advances the singleton marker, so deleting or forging
a tombstone/effect row cannot authorize a recreated stale mirror. A later authenticated
save may supersede a tombstone through a higher revision.

Identity clear retains opaque aliases and conversation provenance. It does not remove
`history.log`, backend transcripts, SQLite/WDBX semantic memory, models, route/audit or
run data, or finalized legacy-migration backups. Reads recover the pending projection,
select the working-directory scope before global from canonical edition-scoped evidence,
continue past a cwd tombstone, stop at a global tombstone, and accept a selected mirror
only when its external identity matches the canonical digest. A corrupt selected cwd
mirror never falls back to a valid global mirror. Scopes with no post-cutover evidence
retain the bounded legacy cwd-then-global behavior. Provider create, resume, retry, and
capture paths use the fallible resolver; presentation-only status surfaces may use the
lossy wrapper. This coordinator adds no backend/title/run inference, `AppCommand`,
`AppEvent`, CLI/TUI/desktop invoke, MCP capability, model/tool execution authority,
network surface, or Windows runtime claim.

## Recovery

Opening the store preserves `Queued` runs. `Starting`, `Running`, and
`CancelRequested` runs become `Interrupted` with a durable recovery event. Abbey does not
silently replay them because a pre-crash provider or tool may already have produced an
external side effect.

A client may explicitly resubmit the same validated request. The idempotency binding
then identifies the existing durable run rather than inventing a second logical action.

## Manager

`RunManager` uses one standard-library worker and a synchronous queue clamped to 1–32
entries. An injected `Executor` and `Clock` make ordering, cancellation, failure, panic,
and shutdown behavior deterministic in tests.

The manager:

- executes an idempotent request at most once per manager admission;
- rejects queue overflow with a durable `queue_full` failure;
- cancels queued work without invoking the executor;
- signals running work through a monotonic cancellation token;
- contains executor panics as per-run failures;
- stores stable failure codes without persisting arbitrary executor error text; and
- returns from a successful shutdown only after admitted work has a terminal state.

Cancellation is cooperative at the `Executor` trait, while the delegated adapter makes it
enforceable for its owned Unix children. The crate-private supervisor provides bounded
stdout/stderr, deadlines, a fresh Unix process group, graceful `SIGTERM` followed by
`SIGKILL`, descendant teardown, direct-child reaping, and a drop guard. It keeps monitoring
the process group after the leader exits so an inherited pipe cannot hang the manager.
After the group is gone, capture collection uses a separate doubled scheduling window capped
at the same five-second hard maximum. This keeps a descheduled reader from being mislabeled
as an open-pipe leak while preserving a fixed failure bound for a genuine inherited pipe.

`DelegatedExecutor` exposes exactly two startup-bound recipes: ABI local completion and
Apple Foundation Models response. A request supplies only one literal input argument; it
cannot select the executable, argv prefix, model, workspace, environment, or trust policy.
Cursor/Grok and interactive/automation modes fail before spawn. The child environment is
cleared and rebuilt from a fixed benign allowlist, excluding daemon and provider secrets.
Captured bytes and configured paths are redacted from `Debug` and durable errors.

Protocol v1 still cannot submit a run. Protocol v2 reaches only these fixed delegated
recipes. Windows delegated execution remains fail-closed until named-pipe and Job Object
implementation plus runtime evidence exist.

## Protocol-v2 paging and compatibility

`RunEvents` returns at most 16 sanitized lifecycle records. The first page captures a
durable high-water sequence; continuations use an exclusive cursor and must repeat that
watermark. Gaps, future cursors, invalid lifecycle kinds, or disagreement between the
latest event and durable run state fail closed. Later appends never move an in-progress
page traversal, and raw prompts or provider stdout/stderr never enter a snapshot or page.

`DaemonClient` starts with protocol v2. It may retry the replay-safe `Status` or `Claims`
request once with v1 only after an explicit `unsupported_version`. It never retries
`SubmitRun` or `CancelRun`, because a lost mutation response does not prove the operation
was not committed.

## Protocol-v3 contracts and first daemon authority

`app_core::v3` defines a separate protocol-v3 command and event family. It covers explicit
grant negotiation, tools and digest-bound approval decisions, memory reads, immutable model
revision actions, reproducible training starts, worker/job reads and cancellation, stable
claim-ID lookup, and fixed-watermark polling. Every free-text, identifier, JSON, progress,
metric, and page field is bounded and validated. Capability sets are canonically ordered,
duplicate-free, and deny all authority unless the exact command grant is present; a server
negotiation cannot return an unrequested grant.

The legacy client constant remains 2, and the existing `AppCommand`, `AppEvent`, v1/v2
request envelope, fixtures, and downgrade behavior are unchanged. The daemon's supported
version list is now `[1, 2, 3]`. Version 3 uses a separate envelope carrying schema version,
bearer, canonical echoed grants, and `V3Command`; it cannot be decoded as a legacy command.

The served authority is deliberately smaller than the contract catalog. Negotiation may
always grant `list_tools` and `invoke_tools`; `ListTools` returns bounded fixed-watermark
descriptors. The first three are projected from the same structurally read-only `SAFE_TOOLS`
registry used by MCP. The default safe daemon appends one local-only mutating descriptor,
`abbey_memory_mark_obsolete`; the personal build and MCP do not. At daemon startup those
schemas are compiled separately;
`InvokeTool` validates a registered tool and its bounded object input before ABI's
`EffectScopedPolicy` authorizes it, persists digest-only authorization before an ABI
`ToolExecutor` adapter runs the handler, and returns terminal correlated JSON bounded to
32 KiB. A second audit row records only result metadata and digests. Call IDs remain
single-use after daemon reopen because the authority recovers reservations from that audit.
The one-second deadline is cooperative and rejects late completion; it cannot pre-empt a
synchronous handler mid-call. For the local mutating descriptor, schema validation and
ABI policy must produce `RequireConfirmation`; the daemon then computes a domain-separated
digest over call ID, tool ID, and canonical JSON input, stores one bounded pending record,
and returns a typed approval status without performing an effect. Runtime schema v5 provides
the transactional approval ledger: one lowercase call digest, bounded expiry, single-use
decision/cancellation identifiers, durable terminal states, and append-only transition
evidence. The default daemon alone negotiates exact approval decisions and cancellation.
Only an identical explicit `InvokeTool` resubmission of a still-approved call can execute:
schema v6 atomically consumes that approval with raw-free prepared intent before opening
the exact startup-selected edition memory backend. Unknown or unavailable backends fail
terminally without SQLite substitution. The effect marks one record obsolete without
deleting provenance, and retains only a bounded result digest and ordered terminal evidence.
Process failpoints after prepare and after effect both reopen as interrupted, reject replay
of the consumed call, and require a fresh call and approval; no automatic mutation replay or
legacy downgrade occurs. The personal daemon and MCP retain only the three read-only tools.
Negotiation may also always grant `read_claims_by_id`; `ClaimById`
projects one exact stable-ID match from Abbey's canonical claim registry and returns
`not_found` for a missing or non-exact ID. There is no fuzzy claim search. When daemon
startup binds an ABI-local fixed provider, negotiation may additionally grant `read_models`,
and `ListModels` returns bounded fixed-watermark inventory derived from the same
startup-owned `ModelProvider` object used by protocol-v2 execution. A separately configured
signed local-model authority can also supply inventory and the model lifecycle described
below. Without either startup authority, model grants remain denied while the canonical
claim read remains available. A non-negotiation request missing its exact grant is rejected
before dispatch; echoing an unsupported grant cannot manufacture daemon authority. There is
no fuzzy claim search, served prompt-inference command, personal-shell tool, training,
worker, polling, CLI lifecycle presentation, desktop, remote, or Windows authority yet.

### Signed local-model lifecycle

The optional edition-scoped `*_MODEL_RUNTIME_CONFIG` variable names one owner-only regular
JSON file read exactly once at daemon startup. That document contains only startup-owned
authority: an absolute signed-registry path, explicit publisher public keys, an absolute
license-acceptance ledger, an absolute external storage root, one accepting principal, one
exact device (`cpu`, `metal`, or `cuda`), and an artifact-size ceiling. The whole document
fails closed if its version, permissions, signature chain, manifest validation, paths,
principal, device, storage containment, or bound is invalid. Requests cannot override any
of those values.

With that authority open, v3 may grant `read_models`, `download_models`, and
`manage_models`. `DownloadModel`, `LoadModel`, and `UnloadModel` select only an exact
manifest ID, immutable revision, and globally single-use operation ID. The daemon checks
the exact principal's acceptance before admitting download or load and serializes all three
lifecycle actions per model revision. ABI owns HTTPS range validation, redirect refusal,
resumable partial state, maximum-size enforcement, SHA-256 verification, atomic
publication, artifact re-verification, tokenizer/tensor loading, and exact device
initialization. Abbey never substitutes a model or device and does not turn a loaded model
into a protocol-v2 or global default route.

Schema v7 persists at most 4,096 operation records with monotonic state and bounded
0–10,000 progress. Operation IDs never become reusable. Any queued or running record found
at daemon reopen becomes failed; no ambiguous download/load is silently replayed. Terminal
records remain queryable after restart, while the in-memory loaded provider deliberately
does not pretend to survive and must be loaded again. `InferenceStatus` can read a
load/unload operation or report that an exact model is loaded but has no inference evidence
as `available` with zero progress. There is no prompt-inference command in this slice.

`V3DaemonSession` provides correlated one-shot methods for download, download status,
load, unload, and inference status. Audit rows retain bounded model/revision/operation,
kind, progress, requested device, and no-fallback evidence. Registry URLs, storage paths,
the accepting principal, license text, artifact bytes, tokenizer bytes, and credentials do
not enter those rows. The generated real-process fixture uses scratch prepublished artifacts
to prove the production daemon path offline; it is not live HTTPS publisher evidence or a
production model-quality claim.

`DaemonClient::negotiate_v3` sends one strict negotiation request and returns a typed
`V3DaemonSession`. The session keeps the validated daemon-returned grants private; its tool,
model, stable-claim, and safe-tool invocation requests echo that exact set. Tool and model reads validate the
response kind, versions, request correlation, page query, watermark, bounds, and record or
descriptor schema. Claim reads additionally require the returned stable ID to equal the
requested ID. Tool invocation additionally validates terminal output plus exact tool and
call correlation. A denied grant stops locally before a second request. Negotiation and every
operation make one correlated request without the legacy v1 downgrade path or automatic retry.

`abbey daemon negotiate` presents the explicit negotiation, while `abbey daemon models`
negotiates `read_models` and reads one bounded page. Both human and JSON views consume only
validated `V3Event` values. The safe-tool and stable-claim reads are currently typed client
authorities, not new CLI commands. These commands do not select a provider, executable,
model, workspace, or grant, and their addition does not expand daemon authority.

## Shared presentation reducer

`src/run_control.rs` is the single presentation-neutral reducer used by
`abbey daemon run submit|status|cancel|events` and the TUI's `/daemon run ...` slash
path. Both surfaces build the same closed `AppCommand`, use the same authenticated
`DaemonClient`, and reduce the correlated `AppEvent` into a sanitized `RunControlView`.
The reducer performs no I/O or execution itself.

Snapshots may advance over states a status poll did not observe, but their run identity,
idempotency key, conversation identity, creation timestamp, terminal state, and durable
event count cannot regress. Event pages are stricter: sequence one is `Queued`, every
contiguous event follows the canonical transition graph, and continuation requests must
use the prior page's exact cursor and fixed watermark. `next_events_command` creates one
explicit continuation only; it is not polling or a live subscription.

Human and JSON renderers consume only this validated state. Prompt text, provider output,
bearer material, and provider paths never enter the reducer view or its diagnostics. The
desktop bridge remains separately wired and does not gain run control from this module.

## Audit boundary

Audit metadata is a bounded JSON object. Sensitive field names and recognizable bearer,
API-key, and private-key values are redacted before persistence. Prompts, provider output,
tool arguments, and credentials are not copied into the audit ledger by default.

Lifecycle events and audit events serve different purposes: lifecycle events reconstruct
run state; audit events record bounded operational facts. Neither is a transcript store.

## Claim boundary

The Current product surface is exact v1 read compatibility, authenticated protocol-v2
fixed-recipe local run control, and authenticated protocol-v3 safe tools, memory reads,
stable claims, startup-owned model inventory, and signed local-model lifecycle control on
Unix. Training, workers, polling, and prompt inference remain unserved contracts. A real scratch-daemon test proves idempotent single
launch, terminal persistence, cancellation and descendant death, fixed-watermark paging,
restart/reopen, and absence of bearer, prompt, provider-output, and executable-path
disclosure. A second real process proof covers the shared CLI/TUI-slash command grammar,
reducer, and sanitized renderer; the TUI evidence is deterministic slash-process parity,
not an interactive-terminal or desktop proof. A separate real scratch startup/restart
proof covers the canonical legacy-metadata backup/import boundary, including unchanged
transcript/route/memory/WDBX canaries and absence of raw identifiers or cwd values in
SQLite and output. Tool dispatch, automations, approvals,
live subscriptions, the desktop run bridge, Windows named pipes/Job Objects, production
model architectures/weights, and full provider-neutral tool ownership remain separate incomplete or Proposed
evidence slices.
