import { useEffect, useSyncExternalStore } from "react";
import { appModelDownload, appModelLoad, appModelUnload, appModelDownloadStatus, appModelLifecycleStatus } from "../ipc/client";
import type { V3ModelDetails } from "../ipc/generated";
import { modelOperations, type ModelEffect } from "./operations";
import { createModelRunner } from "./runner";
import { startModelPolling } from "./polling";

const effects = { download: appModelDownload, load: appModelLoad, unload: appModelUnload };
const reads = { download: appModelDownloadStatus, load: appModelLifecycleStatus, unload: appModelLifecycleStatus };

const runner = createModelRunner(modelOperations, effects, reads);
// The store records identity synchronously before the one and only effect call.
export function startModelOperation(kind: ModelEffect, details: V3ModelDetails) {
  const action = { model_id: details.model_id, revision: details.revision, operation_id: `desktop-${crypto.randomUUID()}` };
  void runner.submit(kind, action);
}
export const recoverModelOperation = runner.recover;
export function useModelOperations() {
  const attempts = useSyncExternalStore(modelOperations.subscribe, modelOperations.snapshot);
  useEffect(() => startModelPolling(modelOperations, recoverModelOperation), []);
  return attempts;
}
