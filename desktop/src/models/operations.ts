import type { V3ModelAction, V3OperationStatus } from "../ipc/generated";

export type ModelEffect = "download" | "load" | "unload";
export interface ModelAttempt {
  readonly kind: ModelEffect;
  readonly action: Readonly<V3ModelAction>;
  readonly phase: "submitting" | "uncertain" | "tracking" | "terminal";
  readonly status: V3OperationStatus | null;
  readonly note: string | null;
}
export const isActiveStatus = (status: V3OperationStatus) =>
  status.state === "queued" || status.state === "running";
const terminalStates = new Set(["succeeded", "failed", "cancelled", "denied"]);
export const blocksNewAttempt = (attempt: ModelAttempt | undefined) =>
  attempt !== undefined && attempt.phase !== "terminal";

// Window-session memory only. This survives view unmount/navigation, never restart.
// Bound the inventory of tracked models; never evict an ambiguous/active attempt.
export function createOperationStore(maxModels = 128) {
  let snapshot: ReadonlyMap<string, ModelAttempt> = new Map();
  const listeners = new Set<() => void>();
  const emit = (model: string, attempt: ModelAttempt) => {
    snapshot = new Map(snapshot).set(model, Object.freeze(attempt));
    for (const listener of listeners) listener();
  };
  const current = (action: V3ModelAction) => {
    const attempt = snapshot.get(action.model_id);
    return attempt?.action.operation_id === action.operation_id &&
      attempt.action.revision === action.revision ? attempt : undefined;
  };
  return {
    snapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    begin(kind: ModelEffect, action: V3ModelAction) {
      if (blocksNewAttempt(snapshot.get(action.model_id))) throw new Error("This model already has an unresolved operation.");
      if (!snapshot.has(action.model_id) && snapshot.size >= maxModels) throw new Error("This window has reached its model-operation limit. Keep this window open to recover pending operations.");
      emit(action.model_id, { kind, action: Object.freeze({ ...action }), phase: "submitting", status: null, note: null });
    },
    accept(action: V3ModelAction, status: V3OperationStatus) {
      const attempt = current(action);
      // Late responses must not overwrite a newer operation or terminal evidence.
      if (!attempt || attempt.phase === "terminal") return false;
      if (status.operation_id !== action.operation_id || status.resource_id !== action.model_id ||
          !Number.isInteger(status.progress_basis_points) || status.progress_basis_points < 0 || status.progress_basis_points > 10000 ||
          (!isActiveStatus(status) && !terminalStates.has(status.state))) {
        emit(action.model_id, { ...attempt, phase: "uncertain", note: "The status did not match this exact operation. Check status again; do not repeat the action." });
        return false;
      }
      // Ignore a regressing active snapshot (including a delayed submission response).
      if (attempt.status && isActiveStatus(status) &&
          ((attempt.status.state === "running" && status.state === "queued") || status.progress_basis_points < attempt.status.progress_basis_points)) return false;
      emit(action.model_id, { ...attempt, status: { ...status }, phase: isActiveStatus(status) ? "tracking" : "terminal", note: null });
      return true;
    },
    uncertain(action: V3ModelAction) {
      const attempt = current(action);
      if (!attempt || attempt.phase === "terminal") return;
      emit(action.model_id, { ...attempt, phase: "uncertain", note: "The response was interrupted. The action may have happened. Check this operation's status; it will not be sent again." });
    },
  };
}
export const modelOperations = createOperationStore();
