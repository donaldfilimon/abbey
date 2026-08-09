// The routing audit view.
//
// Everything rendered here comes from `app_routes`, which returns the
// `RouteAuditPage` Abbey's application core already sanitized. This window has
// no filesystem plugin, so there is no path by which it could read the raw
// `route.jsonl` and show a real working directory even if it wanted to — the
// `ws-<digest>` label below is genuinely all the contract carries.

import { useEffect, useMemo, useState } from "react";
import { appRoutes, toIpcError } from "../ipc/client";
import type { IpcError, RouteAuditEntry, RouteAuditPage } from "../ipc/generated";
import { ErrorCard } from "./ErrorCard";

/** Mirrors `MAX_ROUTE_AUDIT_PAGE` in `src/app_core/routes.rs`. */
const LIMITS = [10, 25, 50] as const;

/** Stable colour-free grouping key so one workspace reads as one column. */
function workspaceLabel(entry: RouteAuditEntry): string {
  return entry.workspace ?? "unrecorded";
}

function RouteRow({ entry }: { entry: RouteAuditEntry }) {
  return (
    <div className="claim">
      <div className="claim-head">
        <span className="claim-name">
          {entry.persona} / {entry.role}
        </span>
        <span className="badge current">{entry.confidence_percent}%</span>
      </div>
      <p className="claim-note">
        <code>{entry.model}</code> · {entry.recorded_at} ·{" "}
        <code title="Opaque digest of the working directory — never a path.">
          {workspaceLabel(entry)}
        </code>
        {entry.stage !== null && entry.stage !== undefined && <> · stage {entry.stage}</>}
        {entry.correlation !== null && entry.correlation !== undefined && (
          <> · run {entry.correlation}</>
        )}
      </p>
      <p className="claim-note">{entry.reason}</p>
      {(entry.alternate ?? null) !== null && (
        <p className="claim-instead">
          Considered instead: <code>{entry.alternate}</code>
          {(entry.fallback ?? null) !== null && <> · {entry.fallback}</>}
        </p>
      )}
      {entry.tools !== undefined && entry.tools.length > 0 && (
        <p className="claim-instead">tools: {entry.tools.join(", ")}</p>
      )}
    </div>
  );
}

export function RoutesView() {
  const [limit, setLimit] = useState<number>(25);
  const [page, setPage] = useState<RouteAuditPage | null>(null);
  const [error, setError] = useState<IpcError | null>(null);

  useEffect(() => {
    let live = true;
    setPage(null);
    setError(null);
    appRoutes({ limit })
      .then((next) => {
        if (live) setPage(next);
      })
      .catch((thrown: unknown) => {
        if (live) setError(toIpcError(thrown));
      });
    return () => {
      live = false;
    };
  }, [limit]);

  // Newest first reads better in a UI than the log's oldest-first order.
  const entries = useMemo(() => (page === null ? [] : [...page.entries].reverse()), [page]);
  const workspaces = useMemo(
    () => new Set(entries.map(workspaceLabel)).size,
    [entries],
  );

  return (
    <>
      <h1>Routes</h1>
      <p className="subtitle">
        The most recent persona/role routing decisions, read live over{" "}
        <code>app_routes</code>. Working directories are shown as opaque{" "}
        <code>ws-</code> digests: the raw path never leaves the application core.
      </p>

      <div className="filters">
        <label>
          Show{" "}
          <select value={limit} onChange={(event) => setLimit(Number(event.target.value))}>
            {LIMITS.map((value) => (
              <option key={value} value={value}>
                last {value}
              </option>
            ))}
          </select>
        </label>
        <span className="muted">
          {page === null
            ? "loading…"
            : `${page.returned} decision(s) across ${workspaces} workspace(s)`}
        </span>
      </div>

      {error !== null && <ErrorCard error={error} />}
      {page !== null && entries.length === 0 && error === null && (
        <div className="card muted">
          No routing has been audited in this state directory yet. Abbey appends a
          row each time <code>hybrid_run</code> picks a persona and role — note that
          the two headless capture commands (<code>abbey print</code> and{" "}
          <code>abbey commit</code>) deliberately bypass it.
        </div>
      )}
      {entries.length > 0 && (
        <div className="card">
          {entries.map((entry, index) => (
            <RouteRow key={`${entry.recorded_at}-${index}`} entry={entry} />
          ))}
        </div>
      )}
    </>
  );
}
