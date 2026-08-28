import { useEffect, useMemo, useState } from "react";
import { appClaimById, appClaims, toIpcError } from "../ipc/client";
import type {
  ClaimRecord,
  ClaimStatus,
  ClaimsQuery,
  IpcError,
  V3StableClaim,
} from "../ipc/generated";
import { ErrorCard } from "./ErrorCard";

const STATUSES: readonly (ClaimStatus | "")[] = [
  "",
  "current",
  "partial",
  "proposed",
  "blocked",
  "out_of_scope",
  "failed",
  "revoked",
  "superseded",
  "expired",
];

const STATUS_LABEL: Record<ClaimStatus, string> = {
  current: "current",
  partial: "partial",
  proposed: "proposed",
  blocked: "blocked",
  out_of_scope: "out of scope",
  failed: "failed",
  revoked: "revoked",
  superseded: "superseded",
  expired: "expired",
};

/**
 * Exact-ID lookup over protocol-v3 `ReadClaimsById`.
 *
 * Deliberately part of the Claims surface rather than its own SurfaceId: it is
 * a lookup refinement of the same ledger, not a second concept. The Claims
 * surface itself gates on the protocol-v1 `read_claims` capability, so this
 * section carries its own v3 gate — a daemon may serve the ledger snapshot
 * while this process holds no v3 session at all (no configured bearer), in
 * which case the section explains itself instead of failing on every keystroke.
 */
export function StableClaimLookup({ granted }: { granted: boolean }) {
  const [draft, setDraft] = useState("");
  const [claim, setClaim] = useState<V3StableClaim | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [loading, setLoading] = useState(false);

  if (!granted) {
    return (
      <div className="card muted">
        <strong>Exact-ID lookup unavailable</strong>
        <p className="claim-note">
          This process did not negotiate <code>read_claims_by_id</code>, which
          requires a configured <code>abbeyd</code>. The ledger above is a
          protocol-v1 read and is unaffected.
        </p>
      </div>
    );
  }

  function lookup() {
    const resourceId = draft.trim();
    if (resourceId === "") return;
    setLoading(true);
    setError(null);
    setClaim(null);
    appClaimById({ resource_id: resourceId })
      .then(setClaim)
      .catch((thrown: unknown) => {
        setError(toIpcError(thrown));
      })
      .finally(() => {
        setLoading(false);
      });
  }

  return (
    <div className="card">
      <h2>Exact-ID lookup</h2>
      <p className="claim-note">
        Resolves one canonical claim by stable ID over{" "}
        <code>app_claim_by_id</code>. A non-exact ID reports not found rather
        than a fuzzy match.
      </p>
      <div className="filters">
        <label>
          Claim ID{" "}
          <input
            type="search"
            spellCheck={false}
            autoComplete="off"
            placeholder="stable claim id"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") lookup();
            }}
          />
        </label>
        <button
          type="button"
          className="nav-button"
          disabled={draft.trim() === "" || loading}
          onClick={lookup}
        >
          {loading ? "looking up…" : "Look up"}
        </button>
      </div>
      {error !== null && <ErrorCard error={error} />}
      {claim !== null && (
        <dl className="kv">
          <dt>id</dt>
          <dd>
            <code>{claim.id}</code>
          </dd>
          <dt>name</dt>
          <dd>{claim.name}</dd>
          <dt>status</dt>
          <dd>
            <ClaimBadge status={claim.status} />
          </dd>
          <dt>note</dt>
          <dd>{claim.note}</dd>
        </dl>
      )}
    </div>
  );
}

export function ClaimBadge({ status }: { status: ClaimStatus }) {
  return <span className={`badge ${status}`}>{STATUS_LABEL[status]}</span>;
}

export function ClaimRow({ claim }: { claim: ClaimRecord }) {
  return (
    <div className="claim">
      <div className="claim-head">
        <span className="claim-name">{claim.name}</span>
        <ClaimBadge status={claim.status} />
      </div>
      <p className="claim-note">{claim.note}</p>
      {claim.instead !== null && (
        <p className="claim-instead">
          Use instead: <code>{claim.instead}</code>
        </p>
      )}
    </div>
  );
}

/** Query the ledger. Returns `null` while loading. */
export function useClaims(query: ClaimsQuery): {
  claims: ClaimRecord[] | null;
  matched: number;
  error: IpcError | null;
} {
  const [claims, setClaims] = useState<ClaimRecord[] | null>(null);
  const [matched, setMatched] = useState(0);
  const [error, setError] = useState<IpcError | null>(null);
  const key = JSON.stringify(query);

  useEffect(() => {
    let live = true;
    setClaims(null);
    setError(null);
    appClaims(JSON.parse(key) as ClaimsQuery)
      .then((snapshot) => {
        if (!live) return;
        setClaims(snapshot.claims);
        setMatched(snapshot.matched);
      })
      .catch((thrown: unknown) => {
        if (live) setError(toIpcError(thrown));
      });
    return () => {
      live = false;
    };
  }, [key]);

  return { claims, matched, error };
}

export function ClaimsView({ claimByIdGranted }: { claimByIdGranted: boolean }) {
  const [status, setStatus] = useState<ClaimStatus | "">("");
  const [contains, setContains] = useState("");
  const trimmed = contains.trim();

  const query = useMemo<ClaimsQuery>(
    () => ({
      status: status === "" ? null : status,
      // The application core rejects an empty or control-bearing filter. The
      // UI sends `null` rather than pre-validating a second copy of its rules.
      contains: trimmed === "" ? null : trimmed,
    }),
    [status, trimmed],
  );

  const { claims, matched, error } = useClaims(query);

  return (
    <>
      <h1>Capabilities</h1>
      <p className="subtitle">
        Abbey's canonical capability ledger, read live over <code>app_claims</code>.
        The filter is validated by the application core, not by this window.
      </p>

      <div className="filters">
        <label>
          Status{" "}
          <select
            value={status}
            onChange={(event) => setStatus(event.target.value as ClaimStatus | "")}
          >
            {STATUSES.map((value) => (
              <option key={value} value={value}>
                {value === "" ? "all" : STATUS_LABEL[value]}
              </option>
            ))}
          </select>
        </label>
        <input
          type="search"
          placeholder="contains…"
          value={contains}
          onChange={(event) => setContains(event.target.value)}
          size={28}
        />
        <span className="muted">
          {claims === null ? "loading…" : `${matched} matching`}
        </span>
      </div>

      {error !== null && <ErrorCard error={error} />}
      {claims !== null && claims.length === 0 && error === null && (
        <div className="card muted">No ledger row matches this filter.</div>
      )}
      {claims !== null && claims.length > 0 && (
        <div className="card">
          {claims.map((claim) => (
            <ClaimRow key={claim.name} claim={claim} />
          ))}
        </div>
      )}

      <StableClaimLookup granted={claimByIdGranted} />
    </>
  );
}
