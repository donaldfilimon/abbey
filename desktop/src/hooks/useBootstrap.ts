import { useCallback, useEffect, useRef, useState } from "react";
import {
  appBundleIdentity,
  appConnection,
  appStatus,
  appV3Grants,
  toIpcError,
} from "../ipc/client";
import type {
  BundleIdentity,
  ConnectionInfo,
  IpcError,
  RuntimeStatus,
  V3Capability,
} from "../ipc/generated";
export interface Bootstrap {
  status: RuntimeStatus;
  connection: ConnectionInfo;
  identity: BundleIdentity | null;
  v3Grants: V3Capability[];
}
export function useBootstrap() {
  const [boot, setBoot] = useState<Bootstrap | null>(null);
  const [error, setError] = useState<IpcError | null>(null);
  const [loading, setLoading] = useState(true);
  const [attempt, setAttempt] = useState(0);
  const generation = useRef(0);
  const retry = useCallback(() => {
    generation.current++;
    setBoot(null);
    setError(null);
    setLoading(true);
    setAttempt((n) => n + 1);
  }, []);
  useEffect(() => {
    const current = ++generation.current;
    // Start independent requests together; optional failures are nonfatal.
    const identity = appBundleIdentity().catch(() => null);
    const grants = appV3Grants().catch(() => ({
      capabilities: [] as V3Capability[],
    }));
    Promise.all([appStatus(), appConnection(), identity, grants])
      .then(([status, connection, identity, grants]) => {
        if (generation.current === current) {
          setBoot({
            status,
            connection,
            identity,
            v3Grants: grants.capabilities,
          });
          setLoading(false);
        }
      })
      .catch((cause: unknown) => {
        if (generation.current === current) {
          setError(toIpcError(cause));
          setLoading(false);
        }
      });
    return () => {
      generation.current++;
    };
  }, [attempt]);
  return { boot, error, loading, retry };
}
