import { useEffect, useRef, useState } from "react";
import { appModelsList, appModelGrants, appModelDetails, toIpcError } from "../ipc/client";
import type { IpcError, V3Capability, V3EntityPage, V3EntityRecord, V3ModelDetails } from "../ipc/generated";
import { blocksNewAttempt, type ModelAttempt, type ModelEffect } from "../models/operations";
import { recoverModelOperation, startModelOperation, useModelOperations } from "../models/useModelOperations";
import { ErrorCard } from "./ErrorCard";

/**
 * The four lifecycle controls, and exactly when each one is offered.
 *
 * The matrix *is* the safety property, so it lives in one place: an action
 * needs its own grant, an accepted licence, and a model that is not
 * already loaded — and while an operation for this model is unresolved,
 * nothing new may be started at all.
 */
export function ModelActionButtons({
  details,
  canDownload,
  canManage,
  blocked,
  detailLoading,
  onConfirm,
  onRefresh,
}: {
  details: V3ModelDetails;
  canDownload: boolean;
  canManage: boolean;
  blocked: boolean;
  detailLoading: boolean;
  onConfirm: (effect: ModelEffect) => void;
  onRefresh: () => void;
}) {
  return (
    <div className="filters">
      <button disabled={blocked || !canDownload || !details.license_accepted || details.loaded} onClick={() => onConfirm("download")}>Download exact revision</button>
      <button disabled={blocked || !canManage || !details.license_accepted || details.loaded} onClick={() => onConfirm("load")}>Load exact revision</button>
      <button disabled={blocked || !canManage || !details.loaded} onClick={() => onConfirm("unload")}>Unload exact revision</button>
      <button disabled={blocked || detailLoading} onClick={onRefresh}>Refresh details</button>
    </div>
  );
}

/**
 * The confirmation gate.
 *
 * Nothing is sent until the reader has been told exactly what will happen
 * — which model, which immutable revision, which configured device — and
 * has confirmed it. The Confirm control stays disabled while an operation
 * for this model is unresolved, so a second action can never race the
 * first.
 */
export function ModelConfirmation({
  effect,
  details,
  blocked,
  onCancel,
  onConfirm,
}: {
  effect: ModelEffect;
  details: V3ModelDetails;
  blocked: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const confirmFocus = useRef<HTMLButtonElement>(null);
  useEffect(() => { confirmFocus.current?.focus(); }, []);
  return (
    <section className="card" role="region" aria-label="Confirm model action" onKeyDown={(event) => { if (event.key === "Escape") onCancel(); }}>
      <h3>Confirm {effect}</h3>
      <p>{effect === "download" ? "Download artifacts for" : effect === "load" ? "Load into the runtime" : "Unload from the runtime"} <code>{details.model_id}</code>, revision <code>{details.revision}</code>, on configured device <strong>{details.configured_device}</strong>.</p>
      <p>The runtime selects storage and device policy. This sends one exact action and does not run inference.</p>
      <div className="filters"><button ref={confirmFocus} onClick={onCancel}>Cancel</button><button disabled={blocked} onClick={onConfirm}>Confirm {effect}</button></div>
    </section>
  );
}

/**
 * The recovery control for an operation whose outcome is unknown.
 *
 * The action may already have happened, so it is never sent again: the only
 * way forward is to read the retained operation's status.
 */
export function ModelRecoveryButton({
  recovery,
  onRecover,
}: {
  recovery: boolean;
  onRecover: () => void;
}) {
  return (
    <button disabled={recovery} onClick={onRecover}>
      {recovery ? "Checking status…" : "Check existing operation status"}
    </button>
  );
}

/**
 * The live state of one model operation.
 *
 * An unresolved outcome is stated as such ("Outcome unknown"), never as a
 * success or a failure the runtime did not report.
 */
export function ModelOperationCard({
  attempt,
  recovery,
  onRecover,
}: {
  attempt: ModelAttempt;
  recovery: boolean;
  onRecover: () => void;
}) {
  return (
    <div className="card" aria-live="polite">
      <h3>{attempt.kind} operation</h3>
      <p><code>{attempt.action.operation_id}</code></p>
      <p>Revision <code>{attempt.action.revision}</code></p>
      <p>{attempt.phase === "uncertain" ? "Outcome unknown" : attempt.phase === "submitting" ? "Submitting once…" : attempt.status?.state}</p>
      {attempt.status && <><progress max={10000} value={attempt.status.progress_basis_points} aria-label="Model operation progress" /><span> {(attempt.status.progress_basis_points / 100).toFixed(1)}%</span></>}
      {attempt.note && <p>{attempt.note}</p>}
      {attempt.phase === "uncertain" && <ModelRecoveryButton recovery={recovery} onRecover={onRecover} />}
    </div>
  );
}

interface Props {
  /** Injected for tests; the live view reads the inventory itself. */
  page?: V3EntityPage | null;
  grants?: V3Capability[];
  selected?: V3EntityRecord | null;
  details?: V3ModelDetails | null;
}

export function ModelsView({
  page: injectedPage,
  grants: injectedGrants,
  selected: injectedSelected,
  details: injectedDetails,
}: Props = {}) {
  const [page, setPage] = useState<V3EntityPage | null>(injectedPage ?? null);
  const [history, setHistory] = useState<number[]>([]);
  const [selected, setSelected] = useState<V3EntityRecord | null>(injectedSelected ?? null);
  const [details, setDetails] = useState<V3ModelDetails | null>(injectedDetails ?? null);
  const [detailError, setDetailError] = useState<IpcError | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [grants, setGrants] = useState<V3Capability[]>(injectedGrants ?? []);
  const [grantError, setGrantError] = useState<IpcError | null>(null);
  const [loading, setLoading] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [confirmation, setConfirmation] = useState<ModelEffect | null>(null);
  const [recovery, setRecovery] = useState(false);
  const inventoryGeneration = useRef(0);
  const detailsGeneration = useRef(0);
  const mounted = useRef(true);
  const attempts = useModelOperations();
  const attempt = selected ? attempts.get(selected.id) : undefined;

  function load(after = 0, through: number | null = null) {
    const generation = ++inventoryGeneration.current;
    setLoading(true); setError(null);
    void appModelsList({ after, through, limit: 16 }).then(result => {
      if (mounted.current && generation === inventoryGeneration.current) setPage(result);
    }).catch((cause: unknown) => {
      if (mounted.current && generation === inventoryGeneration.current) setError(toIpcError(cause));
    }).finally(() => {
      if (mounted.current && generation === inventoryGeneration.current) setLoading(false);
    });
  }
  function inspect(record: V3EntityRecord) {
    const generation = ++detailsGeneration.current;
    setSelected(record); setDetails(null); setDetailError(null); setDetailLoading(true); setConfirmation(null);
    void appModelDetails({ resource_id: record.id }).then(result => {
      if (!mounted.current || generation !== detailsGeneration.current) return;
      if (result.model_id !== record.id) throw new Error("Model details identity mismatch.");
      setDetails(result);
    }).catch((cause: unknown) => {
      if (mounted.current && generation === detailsGeneration.current) setDetailError(toIpcError(cause));
    }).finally(() => {
      if (mounted.current && generation === detailsGeneration.current) setDetailLoading(false);
    });
  }
  useEffect(() => {
    mounted.current = true;
    if (injectedPage === undefined) load();
    let valid = true;
    if (injectedGrants === undefined) {
      void appModelGrants().then(result => { if (valid) setGrants(result.capabilities); })
        .catch((cause: unknown) => { if (valid) setGrantError(toIpcError(cause)); });
    }
    return () => { valid = false; mounted.current = false; inventoryGeneration.current++; detailsGeneration.current++; };
  }, []);
  // Refresh loaded/license snapshots only after a correlated terminal operation.
  const terminalId = attempt?.phase === "terminal" ? attempt.action.operation_id : null;
  useEffect(() => {
    if (terminalId && selected) inspect(selected);
  }, [terminalId]);

  function confirmEffect() {
    if (!details || !confirmation || blocksNewAttempt(attempt)) return;
    const kind = confirmation;
    setConfirmation(null);
    try { startModelOperation(kind, details); }
    catch (cause) { setDetailError(toIpcError(cause)); }
  }
  const canManage = grants.includes("manage_models");
  const canDownload = grants.includes("download_models");
  const blocked = blocksNewAttempt(attempt);
  return <>
    <h1>Models</h1>
    <p className="subtitle">Inspect registered models and manage exact signed revisions when your runtime permits. Loading a model does not mean inference has run.</p>
    <div className="filters">
      <button disabled={loading} onClick={() => { setHistory([]); load(); }}>Refresh inventory</button>
      <button disabled={loading || history.length === 0} onClick={() => {
        const previous = history.at(-1); if (previous === undefined || !page) return;
        setHistory(history.slice(0, -1)); load(previous, page.through);
      }}>Previous page</button>
      <button disabled={loading || !page || page.after + page.records.length >= page.through} onClick={() => {
        if (!page) return; setHistory([...history, page.after]); load(page.after + page.records.length, page.through);
      }}>Next page</button>
      {loading && <span role="status">Loading inventory…</span>}
    </div>
    {error && <ErrorCard error={error} />}
    {grantError && <><p className="muted">Model-management authority could not be checked. Actions remain unavailable.</p><ErrorCard error={grantError} /></>}
    {page && page.records.length === 0 && <p className="card muted">No models are registered in this inventory.</p>}
    {page && page.records.length > 0 && <div className="card">
      {page.records.map(record => <div className="claim" key={record.id}>
        <div className="claim-head"><button aria-pressed={selected?.id === record.id} onClick={() => inspect(record)}>{record.label}</button><span className={`badge ${record.state}`}>{record.state}</span></div>
        <p className="claim-note"><code>{record.id}</code></p>
      </div>)}
    </div>}
    {[...attempts.values()].some(item => item.action.model_id !== selected?.id) && <section className="card" aria-label="Session model operations">
      <h2>Session operations</h2>
      <p className="muted">Operation identities remain in this window across navigation. Status checks never repeat an action.</p>
      {[...attempts.values()].filter(item => item.action.model_id !== selected?.id).map(item => <div className="claim" key={item.action.operation_id}>
        <p>{item.kind}: <code>{item.action.model_id}</code> · revision <code>{item.action.revision}</code></p>
        <p><code>{item.action.operation_id}</code> · {item.phase === "uncertain" ? "Outcome unknown" : item.phase === "submitting" ? "Submitting once…" : item.status?.state}</p>
        {item.phase === "uncertain" && <ModelRecoveryButton recovery={recovery} onRecover={() => {
          setRecovery(true); void recoverModelOperation(item, () => mounted.current).finally(() => { if (mounted.current) setRecovery(false); });
        }} />}
      </div>)}
    </section>}
    {selected && <section className="card" aria-label={`Model details for ${selected.id}`}>
      <h2>{selected.label}</h2>
      {detailLoading && <p role="status">Reading exact model details…</p>}
      {detailError && <><p>Exact signed-model details are unavailable. Inventory-only runtimes can list models but cannot provide these details or lifecycle actions.</p><ErrorCard error={detailError} /><button onClick={() => inspect(selected)}>Retry details</button></>}
      {details && <>
        <dl className="kv">
          <dt>Model</dt><dd><code>{details.model_id}</code></dd>
          <dt>Immutable revision</dt><dd><code>{details.revision}</code></dd>
          <dt>Architecture</dt><dd>{details.architecture}</dd>
          <dt>Configured device</dt><dd>{details.configured_device} (startup policy)</dd>
          <dt>License SHA-256</dt><dd><code>{details.license_sha256}</code></dd>
          <dt>License acceptance</dt><dd>{details.license_accepted ? "Accepted for the runtime principal" : "Not accepted"}</dd>
          <dt>Loaded in runtime</dt><dd>{details.loaded ? "Yes" : "No"}</dd>
        </dl>
        <p className="muted">This is a runtime snapshot. Inventory presence does not verify artifacts or demonstrate inference. License acceptance must be configured outside this window; these controls never accept terms.</p>
        {!details.license_accepted && <p>Download and load require the runtime’s exact license acceptance.</p>}
        {!canDownload && !canManage && <p>This runtime granted inventory access only. Model actions are unavailable.</p>}
        <ModelActionButtons
          details={details}
          canDownload={canDownload}
          canManage={canManage}
          blocked={blocked}
          detailLoading={detailLoading}
          onConfirm={setConfirmation}
          onRefresh={() => inspect(selected)}
        />
        {confirmation && <ModelConfirmation
          effect={confirmation}
          details={details}
          blocked={blocked}
          onCancel={() => setConfirmation(null)}
          onConfirm={confirmEffect}
        />}
      </>}
      {attempt && <ModelOperationCard
        attempt={attempt}
        recovery={recovery}
        onRecover={() => {
          setRecovery(true); void recoverModelOperation(attempt, () => mounted.current).finally(() => { if (mounted.current) setRecovery(false); });
        }}
      />}
    </section>}
  </>;
}
