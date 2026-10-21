import type { V3ModelAction, V3OperationStatus, V3ResourceQuery } from "../ipc/generated";
import { type ModelAttempt, type ModelEffect, type createOperationStore } from "./operations";
type Effect = (action: V3ModelAction) => Promise<V3OperationStatus>;
type Read = (query: V3ResourceQuery) => Promise<V3OperationStatus>;
export function createModelRunner(
  store: ReturnType<typeof createOperationStore>,
  effects: Record<ModelEffect, Effect>, reads: Record<ModelEffect, Read>,
) {
  return {
    // begin is synchronous: a rapid double click cannot submit twice.
    submit(kind: ModelEffect, action: V3ModelAction) {
      store.begin(kind, action);
      return Promise.resolve().then(() => effects[kind](action)).then(
        status => { store.accept(action, status); },
        () => { store.uncertain(action); },
      );
    },
    async recover(attempt: ModelAttempt, valid: () => boolean = () => true) {
      try {
        const status = await reads[attempt.kind]({ resource_id: attempt.action.operation_id });
        if (valid()) store.accept(attempt.action, status);
      } catch {
        if (valid()) store.uncertain(attempt.action);
      }
    },
  };
}
