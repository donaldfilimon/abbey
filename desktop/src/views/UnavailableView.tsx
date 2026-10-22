import type { MissingReason, Surface } from "../surfaces";
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

/**
 * The refusal itself: what is missing, and why.
 *
 * This is the one place a surface states that something is unavailable. A
 * surface that is unavailable entirely and a surface that is available for
 * one thing but withholds another both render their refusal here, so the
 * desktop never has two voices for the same word.
 */
export function UnavailableNotice({
  label,
  reason,
  detail,
}: {
  label: string;
  reason: MissingReason;
  detail: string;
}) {
  return (
    <div className="card">
      <div className="claim-head">
        <span className="claim-name">{label}</span>
        <span className="badge">Unavailable</span>
      </div>
      <strong>{REASON_HEADLINE[reason]}</strong>
      <details className="unavailable-details">
        <summary>Why this isn’t available yet</summary>
        <p className="claim-note">{detail}</p>
        <p className="claim-note">{REASON_NOTE[reason]}</p>
        <p className="claim-note">
          This page can’t start or change work. The live capability ledger below
          provides the current runtime evidence.
        </p>
      </details>
    </div>
  );
}

/**
 * The live ledger rows behind a refusal.
 *
 * Read from Abbey, never asserted here: if the ledger says a capability is
 * Current, this shows Current — the missing piece is the contract, not the
 * capability.
 */
export function LedgerEvidence({ ledgerFilter }: { ledgerFilter: string }) {
  const { claims, error } = useClaims({ status: null, contains: ledgerFilter });

  return (
    <>
      <h2>
        What the ledger says (<code>contains: "{ledgerFilter}"</code>)
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

export function UnavailableView({ surface }: { surface: Surface }) {
  const unavailable = surface.unavailable;
  if (!unavailable) {
    throw new Error(`surface ${surface.id} is available; render its real view`);
  }

  return (
    <>
      <h1>{surface.label}</h1>
      <p className="subtitle">{surface.blurb}</p>

      <UnavailableNotice
        label={surface.label}
        reason={unavailable.reason}
        detail={unavailable.detail}
      />
      <LedgerEvidence ledgerFilter={unavailable.ledgerFilter} />
    </>
  );
}
