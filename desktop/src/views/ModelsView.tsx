// Protocol-v3 model inventory, read-only.
//
// The page comes from `app_models_list`, whose session negotiates exactly
// `read_memory` and `read_models`. `download_models` and `manage_models` are
// never requested, so download, load, and unload are unreachable from this
// process — there is no button here to omit, because there is no command to
// call. Presence in this inventory is not evidence that a model is loaded,
// licensed, or that any inference has run.

import { useState } from "react";
import { appModelsList, toIpcError } from "../ipc/client";
import type { IpcError, V3EntityRecord } from "../ipc/generated";
import { ErrorCard } from "./ErrorCard";

const LIMITS = [8, 16, 32] as const;

export function ModelsView() {
  const [limit, setLimit] = useState(16);
  const [records, setRecords] = useState<V3EntityRecord[] | null>(null);
  const [through, setThrough] = useState<number | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [loading, setLoading] = useState(false);

  function load() {
    setLoading(true);
    setError(null);
    appModelsList({ after: 0, through: null, limit })
      .then((page) => {
        setRecords(page.records);
        setThrough(page.through);
      })
      .catch((thrown: unknown) => {
        setError(toIpcError(thrown));
        setRecords(null);
      })
      .finally(() => {
        setLoading(false);
      });
  }

  return (
    <>
      <h1>Models</h1>
      <p className="subtitle">
        Bounded model inventory over <code>app_models_list</code>. Read-only:
        this build never negotiates <code>download_models</code> or{" "}
        <code>manage_models</code>, so download, load, and unload are not on the
        desktop invoke surface. Listing a model is not evidence that it is
        loaded or that inference has run.
      </p>

      <div className="filters">
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
          disabled={loading}
          onClick={load}
        >
          {loading ? "loading…" : "List models"}
        </button>
        {through !== null && <span className="muted">watermark {through}</span>}
      </div>

      {error !== null && <ErrorCard error={error} />}

      {records !== null && records.length === 0 && error === null && (
        <div className="card muted">
          The daemon granted <code>read_models</code> but returned an empty
          inventory. No model is registered for this edition.
        </div>
      )}
      {records !== null && records.length > 0 && (
        <div className="card">
          {records.map((record) => (
            <div key={record.id} className="claim">
              <div className="claim-head">
                <span className="claim-name">{record.label}</span>
                <span className={`badge ${record.state}`}>{record.state}</span>
              </div>
              <p className="claim-note">
                <code>{record.id}</code>
              </p>
            </div>
          ))}
        </div>
      )}
    </>
  );
}
