import type { Surface } from "../surfaces";
import { ClaimRow, useClaims } from "./ClaimsView";
import { ErrorCard } from "./ErrorCard";

const REASON_HEADLINE = {
  not_on_contract:
    "Not available — the desktop does not expose this contract yet, so there is nothing to show.",
  capability_not_implemented:
    "Not available — this capability is not implemented anywhere in Abbey yet.",
} as const;

const REASON_NOTE = {
  not_on_contract:
    "This is a limitation of the desktop's read-only invoke surface, not a statement " +
    "about whether Abbey can do it. The ledger rows below are read live from Abbey " +
    "and say what actually ships.",
  capability_not_implemented:
    "The ledger rows below are read live from Abbey and are the authority on status.",
} as const;

export function UnavailableView({ surface }: { surface: Surface }) {
  const unavailable = surface.unavailable;
  if (!unavailable) {
    throw new Error(`surface ${surface.id} is available; render its real view`);
  }

  const { claims, error } = useClaims({ status: null, contains: unavailable.ledgerFilter });

  return (
    <>
      <h1>{surface.label}</h1>
      <p className="subtitle">{surface.blurb}</p>

      <div className="card">
        <strong>{REASON_HEADLINE[unavailable.reason]}</strong>
        <p className="claim-note">{unavailable.detail}</p>
        <p className="claim-note">{REASON_NOTE[unavailable.reason]}</p>
        <p className="claim-note">
          No placeholder data is rendered on this screen. There is no sample
          conversation, no fake run, and no mock metric anywhere in this window.
        </p>
      </div>

      <h2>
        What the ledger says (<code>contains: "{unavailable.ledgerFilter}"</code>)
      </h2>
      {error !== null && <ErrorCard error={error} />}
      {claims === null && error === null && <div className="card muted">loading…</div>}
      {claims !== null && claims.length === 0 && (
        <div className="card muted">
          No ledger row matched this filter. Rather than guess a status, this view
          shows nothing — open Capabilities and search the ledger directly.
        </div>
      )}
      {claims !== null && claims.length > 0 && (
        <div className="card">
          {claims.map((claim) => (
            <ClaimRow key={claim.name} claim={claim} />
          ))}
        </div>
      )}
    </>
  );
}
