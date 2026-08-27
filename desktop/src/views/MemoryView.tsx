// Protocol-v3 sanitized memory summaries.
//
// Search and metadata go through `app_memory_search` / `app_memory_metadata`.
// Payload, provenance, paths, and raw record IDs are not on `V3EntityRecord`,
// so this view cannot echo them. Mutation (`abbey_memory_mark_obsolete`) is
// not registered.

import { useState } from "react";
import { appMemoryMetadata, appMemorySearch, toIpcError } from "../ipc/client";
import type { IpcError, V3EntityRecord } from "../ipc/generated";
import { ErrorCard } from "./ErrorCard";

const SPACE_ID = "memory-v1-summary";
const LIMITS = [8, 16, 32] as const;

export function MemoryView() {
  const [draft, setDraft] = useState("");
  const [limit, setLimit] = useState(16);
  const [records, setRecords] = useState<V3EntityRecord[] | null>(null);
  const [through, setThrough] = useState<number | null>(null);
  const [selected, setSelected] = useState<V3EntityRecord | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [loading, setLoading] = useState(false);

  function search() {
    const query = draft.trim();
    if (query === "") return;
    setLoading(true);
    setError(null);
    setSelected(null);
    appMemorySearch({
      space_id: SPACE_ID,
      query,
      page: { after: 0, through: null, limit },
    })
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

  function showMetadata(record: V3EntityRecord) {
    setError(null);
    appMemoryMetadata({ resource_id: record.id })
      .then((metadata) => {
        setSelected(metadata);
      })
      .catch((thrown: unknown) => {
        setError(toIpcError(thrown));
      });
  }

  return (
    <>
      <h1>Memory</h1>
      <p className="subtitle">
        Sanitized summaries from <code>{SPACE_ID}</code> over{" "}
        <code>app_memory_search</code>. Labels are presentation-bounded; payload
        and provenance never cross this contract. Obsolete/delete is not on the
        desktop invoke surface.
      </p>

      <div className="filters">
        <label>
          Query{" "}
          <input
            type="search"
            spellCheck={false}
            autoComplete="off"
            placeholder="summary text"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") search();
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
          disabled={draft.trim() === "" || loading}
          onClick={search}
        >
          {loading ? "searching…" : "Search"}
        </button>
        {through !== null && (
          <span className="muted">watermark {through}</span>
        )}
      </div>

      {error !== null && <ErrorCard error={error} />}

      {records !== null && records.length === 0 && error === null && (
        <div className="card muted">No sanitized summaries matched.</div>
      )}
      {records !== null && records.length > 0 && (
        <div className="card">
          {records.map((record) => (
            <button
              key={record.id}
              type="button"
              className="nav-button"
              onClick={() => showMetadata(record)}
            >
              <div className="claim-head">
                <span className="claim-name">{record.label}</span>
                <span className={`badge ${record.state}`}>{record.state}</span>
              </div>
              <p className="claim-note">
                <code>{record.id}</code>
              </p>
            </button>
          ))}
        </div>
      )}

      {selected !== null && (
        <div className="card">
          <h2>Metadata</h2>
          <dl className="kv">
            <dt>id</dt>
            <dd>
              <code>{selected.id}</code>
            </dd>
            <dt>label</dt>
            <dd>{selected.label}</dd>
            <dt>state</dt>
            <dd>{selected.state}</dd>
          </dl>
        </div>
      )}
    </>
  );
}
