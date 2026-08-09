// The desktop's view catalogue, and — for the views that have no data behind
// them — the honest reason.
//
// Two rules govern this file:
//
//   1. A view is available only when the *live* `CapabilitySet` returned by
//      `app_status` grants what it needs. Availability is never a hardcoded
//      boolean, so the day the app core grants more, the UI follows.
//   2. An unavailable view never asserts a claim status of its own. It states
//      the contract-level reason, and renders whatever the *live* ledger
//      returns for `ledgerFilter` via `app_claims`. If the ledger says a
//      capability is Current, the UI says Current — the missing piece is the
//      contract, not the capability.

import type { AppCapability } from "./ipc/generated";

export type SurfaceId =
  | "doctor"
  | "claims"
  | "chat"
  | "runs"
  | "tools"
  | "memory"
  | "models"
  | "training"
  | "cluster"
  | "routes";

export type MissingReason =
  /** Abbey ships this, but the read-only contract does not carry it. */
  | "not_on_contract"
  /** The underlying capability itself is not implemented anywhere yet. */
  | "capability_not_implemented";

export interface UnavailableSurface {
  reason: MissingReason;
  /** Contract-level explanation. Describes app_core, never a claim status. */
  detail: string;
  /** Substring passed to `app_claims` so the ledger speaks for itself. */
  ledgerFilter: string;
}

export interface Surface {
  id: SurfaceId;
  label: string;
  blurb: string;
  /** Capability `app_status` must report before this view holds real data. */
  requires: AppCapability | null;
  unavailable?: UnavailableSurface;
}

const CONTRACT_SHAPE =
  "`AppCommand` has exactly two variants — `Status` and `Claims` — and " +
  "`CapabilitySet::standard()` grants exactly `ReadStatus` and `ReadClaims`.";

export const SURFACES: readonly Surface[] = [
  {
    id: "doctor",
    label: "Doctor",
    blurb: "Runtime status, build stamp, edition identity, granted capabilities.",
    requires: "read_status",
  },
  {
    id: "claims",
    label: "Capabilities",
    blurb: "The canonical capability ledger, read live from the application core.",
    requires: "read_claims",
  },
  {
    id: "chat",
    label: "Chat",
    blurb: "Conversational agent runs.",
    requires: null,
    unavailable: {
      reason: "not_on_contract",
      detail:
        `${CONTRACT_SHAPE} There is no command that starts, streams, or cancels an ` +
        "agent run, so a chat view here would have nothing to send and nothing to " +
        "receive. Abbey's CLI and TUI reach the agent through `actions::run_agent` " +
        "inside their own process; the desktop is a client of the app core and has " +
        "no such path.",
      ledgerFilter: "provider-neutral",
    },
  },
  {
    id: "runs",
    label: "Runs & trace",
    blurb: "Run lifecycle, events, failures.",
    requires: null,
    unavailable: {
      reason: "not_on_contract",
      detail:
        "`src/app_core/run.rs` already defines `RunRequest`, `RunSnapshot`, " +
        "`RunEventRecord`, and `RunLifecycleEvent` — but no `AppCommand` submits a " +
        "run and no `AppEvent` streams one. The types exist; the transport does not. " +
        "Reading them would require a command that has not been designed yet.",
      ledgerFilter: "provider-neutral",
    },
  },
  {
    id: "tools",
    label: "Tools & approvals",
    blurb: "Sensitive-operation prompts.",
    requires: null,
    unavailable: {
      reason: "not_on_contract",
      detail:
        "`AppEvent::ApprovalRequested` exists, and its own doc comment says the type " +
        "grants no authority. Nothing emits it today, and there is no approve or deny " +
        "command to pair with it. Rendering an approvals queue would be inventing both " +
        "the queue and the authority to answer it.",
      ledgerFilter: "allowlist",
    },
  },
  {
    id: "memory",
    label: "Memory",
    blurb: "STM/LTM, self-learn, similarity search.",
    requires: null,
    unavailable: {
      reason: "not_on_contract",
      detail:
        "Abbey's memory backends are not missing — they are not on this contract. " +
        "`abbeyd` explicitly does not own memory, and no `AppCommand` reads it. See " +
        "the ledger rows below for what memory actually ships today.",
      ledgerFilter: "memory",
    },
  },
  {
    id: "models",
    label: "Models",
    blurb: "Backends and role bindings.",
    requires: null,
    unavailable: {
      reason: "not_on_contract",
      detail:
        "Backend selection and the Max/Gemma role bindings are resolved inside the " +
        "agent process. `RuntimeStatus` carries the build stamp and capability set, " +
        "not the model configuration, and no command exposes it.",
      ledgerFilter: "role bindings",
    },
  },
  {
    id: "training",
    label: "Training",
    blurb: "Fine-tuning and LoRA.",
    requires: null,
    unavailable: {
      reason: "capability_not_implemented",
      detail:
        "Unlike the views above, this one has no implementation anywhere in Abbey to " +
        "expose. `train_candidate` is curation substrate and performs no weight " +
        "updates. A training view would be a picture of something that does not run.",
      ledgerFilter: "lora",
    },
  },
  {
    id: "cluster",
    label: "Cluster",
    blurb: "Multi-process and multi-host compute.",
    requires: null,
    unavailable: {
      reason: "not_on_contract",
      detail:
        "The local multi-process proof is a CLI command on Unix hosts, not an app-core " +
        "contract. Nothing here can enumerate nodes, and the production multi-host mesh " +
        "is a separate ledger row — read both below rather than trusting this pane.",
      ledgerFilter: "mesh",
    },
  },
  {
    id: "routes",
    label: "Routes",
    blurb: "Persona/role routing audit.",
    requires: null,
    unavailable: {
      reason: "not_on_contract",
      detail:
        "Routing decisions are appended to Abbey's JSONL route log by `hybrid_run`. " +
        "The app core exposes no reader for it, and the desktop deliberately does not " +
        "have filesystem access to go read it behind the contract's back.",
      ledgerFilter: "route audit",
    },
  },
] as const;

export function surfaceById(id: SurfaceId): Surface {
  const found = SURFACES.find((surface) => surface.id === id);
  if (!found) throw new Error(`unknown surface ${id}`);
  return found;
}
