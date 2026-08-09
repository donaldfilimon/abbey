# Durable runtime architecture

Abbey has two deliberately separate runtime layers:

1. The shipped protocol-v1 `abbeyd` control plane, which is authenticated and read-only.
   It accepts only `Status` and `Claims`.
2. The `abbey::runtime` lifecycle foundation, which can persist and coordinate typed runs
   and now includes a crate-private Unix supervisor plus fixed ABI-local/Foundation Models
   adapters. Those adapters are not reachable through the protocol-v1 daemon, CLI, TUI,
   MCP, or any network protocol.

The second layer exists so durability and cancellation semantics can be proven before
any execution authority is added.

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

The version-1 schema contains:

- `schema_migrations`
- `conversations`
- `conversation_backends`
- `runs`
- `run_events`
- `audit_events`

SQLite runs with foreign keys enabled, WAL journaling, `synchronous=FULL`, and a bounded
busy timeout. Migrations and run transitions use immediate transactions. A state change
and its lifecycle event commit atomically, with one-based strictly increasing event
sequences.

Idempotency keys are bound to a SHA-256 digest computed by `RunManager` from the validated
serialized `RunRequest`. Reusing a key with the same request returns the original run;
reusing it with different request data fails with `IdempotencyConflict`.

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

This is a library-level Phase 4B.1 proof. Protocol v1 still cannot submit a run. Windows
delegated execution remains fail-closed until named-pipe and Job Object implementation plus
runtime evidence exist.

## Audit boundary

Audit metadata is a bounded JSON object. Sensitive field names and recognizable bearer,
API-key, and private-key values are redacted before persistence. Prompts, provider output,
tool arguments, and credentials are not copied into the audit ledger by default.

Lifecycle events and audit events serve different purposes: lifecycle events reconstruct
run state; audit events record bounded operational facts. Neither is a transcript store.

## Claim boundary

The current product claim remains the authenticated read-only daemon. Phase 4A proves the
durable lifecycle foundation and Phase 4B.1 proves bounded fixed-recipe Unix delegation as
an internal library seam. Neither adds daemon submission authority, tool dispatch,
automations, live event streaming, daemon-owned memory, Windows named pipes/Job Objects,
or a desktop client. Those surfaces remain Proposed or incomplete until their own source,
automated tests, and required live evidence exist.
