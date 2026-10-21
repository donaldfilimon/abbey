// The daemon's tool inventory, read-only.
//
// This view lists what the connected runtime advertises over protocol-v3 and
// says, per tool, which effect class the daemon assigned. It deliberately has
// no way to *do* anything with a tool: there is no invoke, approve, deny, or
// cancel command on this surface, and the safe edition's mutating tool is
// listed with its approval requirement and nothing else.
//
// When the inventory cannot be read, this view says why. It never shows an
// empty list as if the runtime had no tools, and it never shows a list it
// made up.

import { useEffect, useRef, useState } from "react";
import { appToolsList, toIpcError } from "../ipc/client";
import type { IpcError, V3ToolDescriptor, V3ToolPage } from "../ipc/generated";
import { ErrorCard } from "./ErrorCard";
import {
  INVENTORY_UNAVAILABLE,
  INVENTORY_UNAVAILABLE_DETAIL,
  effectCounts,
  effectLabel,
  inventoryState,
} from "./toolInventory";

const PAGE_LIMIT = 32;

interface Props {
  /** Injected for tests; the live view reads the inventory itself. */
  page?: V3ToolPage | null;
  error?: IpcError | null;
}

export function ToolsView({ page: injectedPage, error: injectedError }: Props = {}) {
  const [page, setPage] = useState<V3ToolPage | null>(injectedPage ?? null);
  const [error, setError] = useState<IpcError | null>(injectedError ?? null);
  const [loading, setLoading] = useState(false);
  const generation = useRef(0);
  const mounted = useRef(true);

  function load() {
    const current = ++generation.current;
    setLoading(true);
    setError(null);
    void appToolsList({ after: 0, through: null, limit: PAGE_LIMIT })
      .then((result) => {
        if (mounted.current && current === generation.current) setPage(result);
      })
      .catch((cause: unknown) => {
        if (mounted.current && current === generation.current) {
          setPage(null);
          setError(toIpcError(cause));
        }
      })
      .finally(() => {
        if (mounted.current && current === generation.current) setLoading(false);
      });
  }

  useEffect(() => {
    if (injectedPage !== undefined || injectedError !== undefined) return;
    mounted.current = true;
    load();
    return () => {
      mounted.current = false;
      generation.current++;
    };
  }, []);

  const unavailable = error === null ? null : inventoryState(error);
  const tools: readonly V3ToolDescriptor[] = page?.tools ?? [];
  const counts = effectCounts(tools);

  return (
    <>
      <h1>Tools</h1>
      <p className="subtitle">
        The inventory <code>abbeyd</code> advertises over protocol-v3, read with{" "}
        <code>app_tools_list</code>. Listing a tool is not calling it: this window has no
        invoke, approve, deny, or cancel command, so nothing here can run a tool or
        answer an approval.
      </p>

      <div className="filters">
        <button type="button" disabled={loading} onClick={load}>
          {loading ? "reading…" : "Read inventory"}
        </button>
        {page !== null && (
          <span className="muted">
            {tools.length} advertised · {counts.read_only ?? 0} read-only ·{" "}
            {counts.mutating ?? 0} mutating · {counts.destructive ?? 0} destructive
          </span>
        )}
      </div>

      {error !== null && unavailable !== null && (
        <div className="card">
          <strong>{INVENTORY_UNAVAILABLE[unavailable]}</strong>
          <p className="claim-note">{INVENTORY_UNAVAILABLE_DETAIL[unavailable]}</p>
          <ErrorCard error={error} />
        </div>
      )}

      {error === null && page !== null && tools.length === 0 && (
        <div className="card muted">
          The daemon answered with an empty inventory. That is what it advertises — this view
          does not substitute a list of its own.
        </div>
      )}

      {error === null && tools.length > 0 && (
        <div className="card">
          {tools.map((tool) => {
            const effect = effectLabel(tool.effect);
            return (
              <div className="claim" key={tool.tool_id}>
                <div className="claim-head">
                  <span className="claim-name">
                    <code>{tool.tool_id}</code>
                  </span>
                  <span className={`badge ${tool.effect}`}>{effect.label}</span>
                </div>
                <p className="claim-note">{tool.description}</p>
                <p className="claim-note">{effect.detail}</p>
              </div>
            );
          })}
        </div>
      )}

      {error === null && tools.length > 0 && (
        <p className="muted">
          These are descriptors, not capabilities granted to this window. The desktop
          negotiated exactly one read capability for this page — the inventory itself — and
          the mutating tool above is shown with the approval the daemon requires, not with
          a control that could supply one.
        </p>
      )}
    </>
  );
}
