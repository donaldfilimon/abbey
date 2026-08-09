# Durable runtime architecture

Abbey has two deliberately separate runtime layers:

1. The shipped protocol-v1 `abbeyd` control plane, which is authenticated and read-only.
   It accepts only `Status` and `Claims`.
2. The `abbey::runtime` lifecycle foundation, which can persist and coordinate typed test
   runs but is not connected to a real model, tool, shell, daemon command, or network
   protocol.

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

Cancellation is cooperative in this foundation. No real provider is permitted until a
process supervisor adds bounded stdout/stderr, deadlines, Unix process groups, graceful
termination followed by forced termination, descendant teardown, reaping, and daemon
credential scrubbing. Windows execution additionally requires Job Object runtime proof.

## Audit boundary

Audit metadata is a bounded JSON object. Sensitive field names and recognizable bearer,
API-key, and private-key values are redacted before persistence. Prompts, provider output,
tool arguments, and credentials are not copied into the audit ledger by default.

Lifecycle events and audit events serve different purposes: lifecycle events reconstruct
run state; audit events record bounded operational facts. Neither is a transcript store.

## Claim boundary

The current product claim remains the authenticated read-only daemon. Phase 4A proves a
durable lifecycle library foundation, not background model execution, tool dispatch,
automations, live event streaming, daemon-owned memory, Windows named pipes, or a desktop
client. Those surfaces remain Proposed or incomplete until their own source, automated
tests, and required live evidence exist.
