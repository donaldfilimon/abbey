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

The version-3 schema contains:

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

## Canonical conversation identity saves and compatibility mirrors

On Unix, a new conversation-identity save first validates and domain-separates its
external identity, active edition, and global/per-working-directory scopes. One immediate
schema-v3 transaction stores only the alias, edition, scope, scope-set, and mutation-token
digests; the deterministic UUIDv8 conversation identity; one monotonic revision; and the
commit timestamp. The raw external identity, working directory, and mutation token never
enter `runtime.sqlite`. Opening this metadata path also does not perform unrelated run
recovery.

After that canonical commit, Abbey projects the configured `chat-id`, `chat-id.export`,
direct `by-cwd`, and `history.log` compatibility files through the sole coordinator at
`<state>/daemon/conversation-mirror-journal/`. The directory is `0700`; its stable `lock`
and transient atomic `pending.json` are `0600`. An `fs4` advisory lock serializes writers
and history observation. The pending plan may transiently contain the exact raw mirror
material required for recovery, so it remains private and its `Debug` and error surfaces
are redacted.

Recovery compares the journal's opaque mutation marker with the canonical SQLite commit.
A prepared plan that never committed is discarded without touching mirrors. A committed
plan is completed idempotently. Each mirror uses before/after digests and atomic
same-parent replacement; divergent existing data fails closed instead of being
overwritten, and full-file history replacement prevents replay from duplicating an entry.
Symlinks, reserved journal aliases, unsafe ancestors, controls, and unsafe permissions
fail before canonical commit or mirror mutation.

This slice changes only save/new-identity coordination. Reads still consult the legacy
compatibility mirrors, and `clear_chat` retains its existing mirror-only semantics under
the shared lock; canonical tombstones are the next separately evidenced slice. It adds no
transcript or semantic-memory ownership, backend/title/run inference, `AppCommand`,
`AppEvent`, CLI/TUI/desktop invoke, MCP capability, model/tool execution authority, or
Windows runtime claim.

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

The Current product surface is exact v1 read compatibility plus authenticated protocol-v2
fixed-recipe local run control on Unix. A real scratch-daemon test proves idempotent single
launch, terminal persistence, cancellation and descendant death, fixed-watermark paging,
restart/reopen, and absence of bearer, prompt, provider-output, and executable-path
disclosure. A second real process proof covers the shared CLI/TUI-slash command grammar,
reducer, and sanitized renderer; the TUI evidence is deterministic slash-process parity,
not an interactive-terminal or desktop proof. A separate real scratch startup/restart
proof covers the canonical legacy-metadata backup/import boundary, including unchanged
transcript/route/memory/WDBX canaries and absence of raw identifiers or cwd values in
SQLite and output. Tool dispatch, automations, approvals,
live subscriptions, daemon-owned memory, the desktop run bridge, Windows named pipes/Job
Objects, and provider-neutral model ownership remain separate incomplete or Proposed
evidence slices.
