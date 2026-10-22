// The desktop's view catalogue, and — for the views that have no data behind
// them — the honest reason.
//
// Two rules govern this file:
//
//   1. A view is available only when the *live* `CapabilitySet` returned by
//      `app_status` or the separate v3/v4 session grants what it needs. Availability is never a hardcoded
//      boolean, so the day the app core grants more, the UI follows.
//   2. An unavailable view never asserts a claim status of its own. It states
//      the contract-level reason, and renders whatever the *live* ledger
//      returns for `ledgerFilter` via `app_claims`. If the ledger says a
//      capability is Current, the UI says Current — the missing piece is the
//      contract, not the capability.

import type { AppCapability, V3Capability, V4Capability } from "./ipc/generated";

export type SurfaceId =
  | "automations"
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
  /** Abbey ships this, but the desktop contract does not carry it. */
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

/**
 * A capability a surface deliberately does not expose.
 *
 * A surface can be available for one thing and unavailable for another: the
 * Runs surface reads a durable run's lifecycle metadata, but it cannot
 * delegate a task. That refusal is stated in the same vocabulary — and
 * through the same rendering — as a surface that is unavailable entirely,
 * so this desktop never says "unavailable" in two different voices.
 */
export interface WithheldCapability {
  /** The thing that is missing, in the reader's words. */
  label: string;
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
  /** Capability a validated v1/v2, v3, or v4 session must report. */
  requires: AppCapability | V3Capability | V4Capability | null;
  unavailable?: UnavailableSurface;
  /** Present on an available surface that withholds part of its work. */
  withheld?: WithheldCapability;
}

export const SURFACES: readonly Surface[] = [
  { id: "automations", label: "Automations", blurb: "A home for recurring work.", requires: null, unavailable: { reason: "not_on_contract", detail: "Automation scheduling and execution are not exposed by the current desktop bridge. No automations can be created or managed here yet.", ledgerFilter: "automation" } },
  {
    id: "doctor",
    label: "Settings",
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
    blurb: "Send turns through the daemon’s configured route and read the authorized conversation history.",
    requires: "read_conversations",
  },
  {
    id: "runs",
    label: "Tasks",
    blurb: "Inspect daemon run status and lifecycle events. Task delegation is not available here yet.",
    requires: "read_run",
    // The surface itself is live — it reads a durable run's lifecycle
    // metadata. What it cannot do is delegate a task, and that refusal is
    // stated through the shared unavailable mechanism rather than a badge
    // invented here.
    withheld: {
      label: "Task delegation",
      reason: "not_on_contract",
      detail:
        "Task delegation is not on the desktop invoke surface: no command creates, " +
        "lists, or schedules a task, so there is nothing here to show. This page " +
        "reads one durable run's lifecycle metadata instead.",
      ledgerFilter: "task",
    },
  },
  {
    id: "tools",
    label: "Tools",
    blurb:
      "The tool inventory the connected runtime advertises — read-only enumeration, never an invocation.",
    // The inventory itself needs the daemon's `list_tools` grant, which the
    // desktop negotiates in its own tool session rather than in the shared
    // v3 read session. `read_status` is the honest floor for *reaching* the
    // surface: the view then reports, in its own words, whether the daemon
    // negotiated a tool inventory or refused to answer at all.
    requires: "read_status",
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
    blurb: "Model inventory and exact signed-model lifecycle management.",
    requires: "read_models",
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
