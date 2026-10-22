// Protocol-v2 run status and fixed-watermark lifecycle events. Serves as
// the "Tasks" surface (id: runs) for minimal Task 6.
//
// The surface is live: it reads one durable run's snapshot and its
// fixed-watermark lifecycle page. What it cannot do is delegate a task —
// that refusal is not invented here. It is declared once in the surface
// catalogue (`surfaces.ts`) and rendered through the same mechanism the
// unavailable pages use, so the desktop states it in one voice and cites
// the live ledger beside it. No execution.

import { useMemo, useState } from "react";
import { appRunEvents, appRunStatus, toIpcError } from "../ipc/client";
import type {
  IpcError,
  RunEventPage,
  RunLifecycleEvent,
  RunSnapshot,
} from "../ipc/generated";
import { surfaceById } from "../surfaces";
import { ErrorCard } from "./ErrorCard";
import { LedgerEvidence, UnavailableNotice } from "./UnavailableView";

/** Mirrors `MAX_RUN_EVENT_PAGE` in `src/app_core/run.rs`. */
const LIMITS = [4, 8, 16] as const;

const CANONICAL_UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/**
 * The human label for one lifecycle event.
 *
 * A failure, a cancellation, and an interruption each carry the reason the
 * daemon recorded. Showing only the event kind would hide *why* a run
 * stopped, which is the one thing a reader of this page needs.
 */
export function eventLabel(event: RunLifecycleEvent): string {
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

/**
 * One lifecycle event.
 *
 * Exported so the honesty contract is testable directly: a failed run must
 * show the failure code and the daemon's own message, never a bare
 * "failed".
 */
export function EventRow({
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
  // The one withheld capability of this surface, declared in the catalogue.
  const withheld = surfaceById("runs").withheld;

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

      {withheld && (
        <UnavailableNotice
          label={withheld.label}
          reason={withheld.reason}
          detail={withheld.detail}
        />
      )}

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

      {withheld && <LedgerEvidence ledgerFilter={withheld.ledgerFilter} />}
    </>
  );
}
