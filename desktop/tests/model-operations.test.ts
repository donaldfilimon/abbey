import { describe, test, expect } from "bun:test";
import { createOperationStore, blocksNewAttempt } from "../src/models/operations";
import type { V3ModelAction, V3OperationStatus } from "../src/ipc/generated";
const action: V3ModelAction = { model_id: "fixture", revision: "rev-1", operation_id: "operation-1" };
const status = (state: V3OperationStatus["state"], progress = 0): V3OperationStatus => ({ operation_id: action.operation_id, resource_id: action.model_id, state, progress_basis_points: progress });
describe("window-session exact model attempts", () => {
  test("ambiguous submission retains exact identity and blocks a fresh effect", () => {
    const store = createOperationStore(); store.begin("download", action); store.uncertain(action);
    expect(store.snapshot().get("fixture")?.action).toEqual(action);
    expect(store.snapshot().get("fixture")?.phase).toBe("uncertain");
    expect(() => store.begin("load", { ...action, operation_id: "new" })).toThrow();
    expect(store.accept(action, status("running", 2300))).toBe(true);
    expect(store.snapshot().get("fixture")?.phase).toBe("tracking");
  });
  test("navigation subscriptions do not erase operations; terminal recovery permits a fresh ID", () => {
    const store = createOperationStore(); let updates = 0;
    const unmount = store.subscribe(() => { updates++; });
    store.begin("load", action); unmount(); store.uncertain(action);
    expect(updates).toBe(1);
    expect(blocksNewAttempt(store.snapshot().get("fixture"))).toBe(true);
    store.accept(action, status("succeeded", 10000));
    expect(blocksNewAttempt(store.snapshot().get("fixture"))).toBe(false);
    store.begin("unload", { ...action, operation_id: "operation-2" });
    expect(store.accept(action, status("failed"))).toBe(false);
    expect(store.snapshot().get("fixture")?.action.operation_id).toBe("operation-2");
  });
  test("wrong correlation, impossible states and progress stay unresolved", () => {
    for (const invalid of [{ ...status("running"), operation_id: "wrong" }, { ...status("running"), resource_id: "wrong" }, status("available"), status("running", 10001)]) {
      const store = createOperationStore(); store.begin("load", action);
      expect(store.accept(action, invalid)).toBe(false);
      expect(store.snapshot().get("fixture")?.phase).toBe("uncertain");
    }
  });
  test("late reads cannot regress running or terminal evidence", () => {
    const store = createOperationStore(); store.begin("load", action);
    store.accept(action, status("running", 5000));
    expect(store.accept(action, status("queued"))).toBe(false);
    store.accept(action, status("succeeded", 10000)); store.uncertain(action);
    store.accept(action, status("running", 9000));
    expect(store.snapshot().get("fixture")?.status?.state).toBe("succeeded");
  });
  test("bounded store refuses new models without discarding unresolved identity", () => {
    const store = createOperationStore(1); store.begin("download", action);
    expect(() => store.begin("load", { ...action, model_id: "other" })).toThrow();
    expect(store.snapshot().get("fixture")?.action).toEqual(action);
  });
});
