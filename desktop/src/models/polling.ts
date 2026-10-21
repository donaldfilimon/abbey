import type { ModelAttempt, createOperationStore } from "./operations";

// One serial poll loop per mounted view. Store notifications may add/remove work
// but never reset an in-flight batch, so the first model cannot starve later ones.
export function startModelPolling(
  store: ReturnType<typeof createOperationStore>,
  read: (attempt: ModelAttempt, valid: () => boolean) => Promise<void>,
  schedule: (callback: () => void) => () => void = callback => {
    const timer = setTimeout(callback, 2000); return () => clearTimeout(timer);
  },
) {
  let alive = true;
  let inFlight = false;
  let cancelTimer: (() => void) | null = null;
  const hasWork = () => [...store.snapshot().values()].some(attempt => attempt.phase === "tracking");
  function reconcile() {
    if (!alive) return;
    if (!hasWork()) { cancelTimer?.(); cancelTimer = null; return; }
    if (!cancelTimer && !inFlight) cancelTimer = schedule(() => { cancelTimer = null; void tick(); });
  }
  async function tick() {
    if (!alive || inFlight) return;
    inFlight = true;
    try {
      for (const attempt of store.snapshot().values()) {
        if (!alive) break;
        const current = store.snapshot().get(attempt.action.model_id);
        if (current?.phase === "tracking" && current.action.operation_id === attempt.action.operation_id) {
          await read(current, () => alive);
        }
      }
    } finally { inFlight = false; reconcile(); }
  }
  const unsubscribe = store.subscribe(reconcile);
  reconcile();
  return () => { alive = false; cancelTimer?.(); cancelTimer = null; unsubscribe(); };
}
