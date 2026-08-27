// Protocol-v2 run status and fixed-watermark lifecycle events.
//
// This window looks up one durable run by canonical UUID and pages its
// sanitized lifecycle ledger. It cannot submit or cancel: those commands are
// absent from `generate_handler!`. User input and provider output are not on
// `RunSnapshot` or `RunEventPage`, so there is nothing here that could echo
// them even by accident.

import { useMemo, useState } from "react";
import { appRunEvents, appRunStatus, toIpcError } from "../ipc/client";
import type {
  IpcError,
  RunEventPage,
  RunLifecycleEvent,
  RunSnapshot,
} from "../ipc/generated";
import { ErrorCard } from "./ErrorCard";

/** Mirrors `MAX_RUN_EVENT_PAGE` in `src/app_core/run.rs`. */
const LIMITS = [4, 8, 16] as const;

const CANONICAL_UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

function eventLabel(event: RunLifecycleEvent): string {
  switch (event.type) {
    case "failed":
      return `failed (${event.payload.failure.code})`;
    case "cancelled":
      return `cancelled (${event.payload.reason})`;
    case "interrupted":
      return `interrupted (${event.payload.reason})`;
    default:
      return event.type;
  }
}

function EventRow({
  sequence,
  recorded_at,
  event,
}: {
  sequence: number;
  recorded_at: string;
  event: RunLifecycleEvent;
}) {
  const detail =
    event.type === "failed" ? event.payload.failure.message : null;
  return (
    <div className="claim">
      <div className="claim-head">
        <span className="claim-name">
          #{sequence} {eventLabel(event)}
        </span>
        <span className="muted">{recorded_at}</span>
      </div>
      {detail !== null && <p className="claim-note">{detail}</p>}
    </div>
  );
}

export function RunsView() {
  const [draft, setDraft] = useState("");
  const [runId, setRunId] = useState<string | null>(null);
  const [limit, setLimit] = useState<number>(16);
  const [snapshot, setSnapshot] = useState<RunSnapshot | null>(null);
  const [page, setPage] = useState<RunEventPage | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [loading, setLoading] = useState(false);

  const canonical = useMemo(() => {
    const trimmed = draft.trim();
    return CANONICAL_UUID.test(trimmed) ? trimmed : null;
  }, [draft]);

  function lookup(afterSequence = 0, through: number | null = null) {
    if (canonical === null) return;
    setLoading(true);
    setError(null);
    const eventsQuery = {
      run_id: canonical,
      after_sequence: afterSequence,
      through_sequence: through,
      limit,
    };
    Promise.all([
      afterSequence === 0
        ? appRunStatus({ run_id: canonical })
        : Promise.resolve(snapshot),
      appRunEvents(eventsQuery),
    ])
      .then(([nextSnapshot, nextPage]) => {
        setRunId(canonical);
        if (nextSnapshot !== null) setSnapshot(nextSnapshot);
        setPage(nextPage);
      })
      .catch((thrown: unknown) => {
        setError(toIpcError(thrown));
        if (afterSequence === 0) {
          setSnapshot(null);
          setPage(null);
          setRunId(null);
        }
      })
      .finally(() => {
        setLoading(false);
      });
  }

  return (
    <>
      <h1>Runs &amp; trace</h1>
      <p className="subtitle">
        One durable run at a time, over <code>app_run_status</code> and{" "}
        <code>app_run_events</code>. The snapshot is lifecycle metadata only —
        prompt text and provider output never cross this contract. Submit and
        cancel are not on the desktop invoke surface.
      </p>

      <div className="filters">
        <label>
          Run id{" "}
          <input
            type="search"
            spellCheck={false}
            autoComplete="off"
            placeholder="canonical uuid"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") lookup(0, null);
            }}
          />
        </label>
        <label>
          Page{" "}
          <select
            value={limit}
            onChange={(event) => setLimit(Number(event.target.value))}
          >
            {LIMITS.map((value) => (
              <option key={value} value={value}>
                {value}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="nav-button"
          disabled={canonical === null || loading}
          onClick={() => lookup(0, null)}
        >
          {loading ? "loading…" : "Look up"}
        </button>
        {canonical === null && draft.trim() !== "" && (
          <span className="muted">needs a lower-case UUID</span>
        )}
      </div>

      {error !== null && <ErrorCard error={error} />}

      {snapshot !== null && (
        <div className="card">
          <div className="claim-head">
            <span className="claim-name">
              <code>{snapshot.run_id}</code>
            </span>
            <span className={`badge ${snapshot.state}`}>{snapshot.state}</span>
          </div>
          <dl className="kv">
            <dt>updated</dt>
            <dd>{snapshot.updated_at}</dd>
            <dt>created</dt>
            <dd>{snapshot.created_at}</dd>
            <dt>events</dt>
            <dd>{snapshot.event_count}</dd>
            <dt>idempotency</dt>
            <dd>
              <code>{snapshot.idempotency_key}</code>
            </dd>
            {snapshot.conversation_id !== null && (
              <>
                <dt>conversation</dt>
                <dd>
                  <code>{snapshot.conversation_id}</code>
                </dd>
              </>
            )}
            {snapshot.failure !== null && (
              <>
                <dt>failure</dt>
                <dd>
                  <code>{snapshot.failure.code}</code>
                  {snapshot.failure.retryable ? " · retryable" : ""} —{" "}
                  {snapshot.failure.message}
                </dd>
              </>
            )}
          </dl>
        </div>
      )}

      {page !== null && page.events.length === 0 && (
        <div className="card muted">No lifecycle events in this page.</div>
      )}
      {page !== null && page.events.length > 0 && (
        <div className="card">
          {page.events.map((record) => (
            <EventRow
              key={`${record.run_id}-${record.sequence}`}
              sequence={record.sequence}
              recorded_at={record.recorded_at}
              event={record.event}
            />
          ))}
        </div>
      )}
      {page !== null && page.has_more && (
        <div className="filters">
          <button
            type="button"
            className="nav-button"
            disabled={loading}
            onClick={() => lookup(page.next_after_sequence, page.through_sequence)}
          >
            Next {limit} events
          </button>
          <span className="muted">
            through sequence {page.through_sequence}
            {runId !== null && runId !== canonical && " · id changed, look up again"}
          </span>
        </div>
      )}
    </>
  );
}
