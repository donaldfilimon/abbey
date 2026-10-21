import { test, expect } from "bun:test";
import { createOperationStore } from "../src/models/operations";
import { createModelRunner } from "../src/models/runner";
import type { V3ModelAction, V3OperationStatus } from "../src/ipc/generated";
const action: V3ModelAction = { model_id: "fixture", revision: "rev-1", operation_id: "operation-1" };
const result: V3OperationStatus = {operation_id: action.operation_id, resource_id: action.model_id, state: "succeeded", progress_basis_points: 10000};
test("interrupted effect is sent once; recovery only reads the retained operation", async () => {
  const store = createOperationStore(); let effects = 0; const queries: string[] = [];
  const effect = async () => { effects++; throw new Error("lost response after effect"); };
  const read = async (query: {resource_id: string}) => { queries.push(query.resource_id); return result; };
  const runner = createModelRunner(store, {download: effect, load: effect, unload: effect}, {download: read, load: read, unload: read});
  await runner.submit("download", action);
  const attempt = store.snapshot().get(action.model_id)!;
  expect(attempt.phase).toBe("uncertain");
  await runner.recover(attempt);
  expect(effects).toBe(1); expect(queries).toEqual([action.operation_id]);
  expect(store.snapshot().get(action.model_id)?.status?.state).toBe("succeeded");
});
test("unmounted or obsolete read cannot update retained state", async () => {
  const store = createOperationStore(); store.begin("load", action); store.uncertain(action);
  let resolve!: (status: V3OperationStatus) => void;
  const read = () => new Promise<V3OperationStatus>(done => { resolve = done; });
  const effect = async () => result;
  const runner = createModelRunner(store, {download: effect, load: effect, unload: effect}, {download: read, load: read, unload: read});
  let valid = true; const pending = runner.recover(store.snapshot().get(action.model_id)!, () => valid);
  valid = false; resolve(result); await pending;
  expect(store.snapshot().get(action.model_id)?.phase).toBe("uncertain");
});
test("double submit is prevented synchronously before an effect completes", async () => {
  const store = createOperationStore(); let effects = 0;
  const effect = async () => { effects++; return result; }; const read = effect;
  const runner = createModelRunner(store, {download: effect, load: effect, unload: effect}, {download: read, load: read, unload: read});
  const first = runner.submit("load", action);
  expect(() => runner.submit("load", {...action, operation_id: "other"})).toThrow();
  await first; expect(effects).toBe(1);
});
