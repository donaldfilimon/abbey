// Presentation rules for the read-only tool inventory.
//
// The daemon owns the registry, the effect classes, and the descriptions. This
// module only *labels* what the daemon sent: it never decides what a tool
// does, never reorders the inventory, and never adds a way to run one.
//
// Two rules govern everything below:
//
//   1. Listing a tool is not calling it. There is no invoke, approve, deny,
//      or cancel affordance here, and nothing in this file produces one.
//   2. An effect class is a fact about the daemon's catalogue, not a promise
//      about what a call would do. The label says which class the daemon
//      advertised; the mutating class additionally says that the daemon
//      requires an exact-call approval — and that this window still cannot
//      give one.

import type { IpcError, V3ToolDescriptor, V3ToolEffect } from "../ipc/generated";

export interface EffectLabel {
  /** Short class name, e.g. "Read-only". */
  label: string;
  /** One sentence about what the class means for this window. */
  detail: string;
}

/** The daemon's own effect class, rendered. */
export function effectLabel(effect: V3ToolEffect): EffectLabel {
  switch (effect) {
    case "read_only":
      return {
        label: "Read-only",
        detail:
          "Observes state. The daemon serves it without an approval, and this window only lists it.",
      };
    case "mutating":
      return {
        label: "Mutating — approval required",
        detail:
          "Changes state. The daemon requires an exact-call approval before it runs, and this " +
          "window has no approve, deny, or invoke command to give one.",
      };
    case "destructive":
      return {
        label: "Destructive",
        detail:
          "Changes state irreversibly. The safe edition does not serve this class, and this " +
          "window cannot invoke it either.",
      };
  }
}

/** How many tools of each class the page carries. */
export function effectCounts(tools: readonly V3ToolDescriptor[]): Record<string, number> {
  const counts: Record<string, number> = { read_only: 0, mutating: 0, destructive: 0 };
  for (const tool of tools) counts[tool.effect] = (counts[tool.effect] ?? 0) + 1;
  return counts;
}

/** Why the inventory could not be read, in the view's own words. */
export type InventoryState =
  /** No daemon is configured, or the daemon could not be reached. */
  | "unreachable"
  /** The daemon is reachable but did not negotiate a tool inventory. */
  | "not_negotiated"
  /** The command this build needs is not registered. */
  | "not_exposed";

/**
 * Classify a failed inventory read.
 *
 * The distinctions matter: "the daemon is not running" and "the daemon does
 * not offer a tool inventory" are different facts, and a surface that
 * collapsed them into one empty list would be lying about which one it is.
 */
export function inventoryState(error: IpcError): InventoryState {
  const message = error.message.toLowerCase();
  if (message.includes("not found") || message.includes("unknown command")) {
    return "not_exposed";
  }
  if (error.kind === "rejected") return "not_negotiated";
  return "unreachable";
}

/** The headline for each unavailable state. */
export const INVENTORY_UNAVAILABLE: Record<InventoryState, string> = {
  unreachable:
    "The tool inventory is unavailable because the Abbey daemon could not be reached.",
  not_negotiated:
    "The connected runtime did not negotiate a tool inventory, so there is no list to show.",
  not_exposed:
    "This desktop build does not expose the tool-inventory command, so there is no list to show.",
};

/** The explanation that goes with each unavailable state. */
export const INVENTORY_UNAVAILABLE_DETAIL: Record<InventoryState, string> = {
  unreachable:
    "Tool inventory is daemon-only: it is read over the authenticated owner-only socket. " +
    "Start abbeyd, then read this page again. This surface never falls back to a " +
    "hardcoded list.",
  not_negotiated:
    "The daemon answered, but it did not grant the read-only list_tools capability this " +
    "read needs. That is the runtime's decision, not a missing list.",
  not_exposed:
    "The tool-inventory command must be registered in the Tauri handler for this build " +
    "before this window can read it.",
};
