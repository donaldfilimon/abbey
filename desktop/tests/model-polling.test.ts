import { test, expect } from "bun:test";
import { createOperationStore } from "../src/models/operations";
import { startModelPolling } from "../src/models/polling";
import type { V3ModelAction, V3OperationStatus } from "../src/ipc/generated";
function fixture() {
  const store = createOperationStore();
  for (const id of ["first", "second"]) {
    const action = {model_id: id, revision: "v1", operation_id: `op-${id}`};
    store.begin("load", action);
    store.accept(action, status(action, "running"));
  }
  return store;
}
function status(action: V3ModelAction, state: V3OperationStatus["state"]): V3OperationStatus {
  return {operation_id: action.operation_id, resource_id: action.model_id, state, progress_basis_points: state === "succeeded" ? 10000 : 0};
}
function timers() {
  let callback: (() => void) | null = null;
  return {schedule(fn: () => void) { expect(callback).toBeNull(); callback = fn; return () => { callback = null; }; }, fire() { const next = callback; callback = null; next?.(); }, pending: () => callback !== null};
}
const flush = () => new Promise<void>(resolve => queueMicrotask(resolve));
test("multi-model polls stay serial despite first model store updates and stop when terminal", async () => {
  const store = fixture(); const timer = timers(); const reads: string[] = []; let active = 0;
  const stop = startModelPolling(store, async (attempt, valid) => {
    active++; expect(active).toBe(1); reads.push(attempt.action.model_id);
    await flush(); if (valid()) store.accept(attempt.action, status(attempt.action, "succeeded")); active--;
  }, timer.schedule);
  timer.fire(); await flush(); await flush(); await flush();
  expect(reads).toEqual(["first", "second"]);
  expect(timer.pending()).toBe(false); stop();
});
test("unmount stops pending timer and refuses in-flight results or later reads", async () => {
  const store = fixture(); const timer = timers(); let resolve!: () => void; let reads = 0;
  const stop = startModelPolling(store, async (attempt, valid) => {
    reads++; await new Promise<void>(done => { resolve = done; });
    if (valid()) store.accept(attempt.action, status(attempt.action, "succeeded"));
  }, timer.schedule);
  timer.fire(); stop(); resolve(); await flush(); await flush();
  expect(reads).toBe(1); expect(timer.pending()).toBe(false);
  expect(store.snapshot().get("first")?.phase).toBe("tracking");
});
