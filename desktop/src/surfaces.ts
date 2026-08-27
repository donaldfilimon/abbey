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

import type { AppCapability, V3Capability } from "./ipc/generated";

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
  /** Capability `app_status` (v1/v2) or `app_v3_grants` must report. */
  requires: AppCapability | V3Capability | null;
  unavailable?: UnavailableSurface;
}

const CONTRACT_SHAPE =
  "The desktop exposes read-only Status, Claims, Routes, protocol-v2 run " +
  "status/events, and protocol-v3 memory summary search/metadata. In-process " +
  "`CapabilitySet::standard()` grants `ReadStatus`, `ReadClaims`, and " +
  "`ReadRoutes`. `ReadRun`/`ReadRunEvents` appear only on a protocol-v2 " +
  "daemon. `ReadMemory` is a v3 grant, never an in-process store open. " +
  "Submit, cancel, invoke, and obsolete stay off this invoke surface.";

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
        `${CONTRACT_SHAPE} Protocol v2 does not stream conversational output, and ` +
        "the desktop has no command that starts or cancels an agent run, so a chat " +
        "view here would still have nothing to send or receive.",
      ledgerFilter: "provider-neutral",
    },
  },
  {
    id: "runs",
    label: "Runs & trace",
    blurb: "Sanitized protocol-v2 run lifecycle and fixed-watermark events.",
    requires: "read_run",
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
    blurb: "Sanitized protocol-v3 summary search and metadata.",
    requires: "read_memory",
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
    blurb:
      "Persona/role routing audit — sanitized, with workspaces shown as opaque digests.",
    requires: "read_routes",
  },
] as const;

export function surfaceById(id: SurfaceId): Surface {
  const found = SURFACES.find((surface) => surface.id === id);
  if (!found) throw new Error(`unknown surface ${id}`);
  return found;
}
