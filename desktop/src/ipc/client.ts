// Typed wrappers over the eleven enumerated Tauri commands.
//
// Every type below comes from `./generated`, which is projected from Abbey's
// Rust source. Nothing in this file declares a shape of its own.

import { invoke } from "@tauri-apps/api/core";
import type {
  BundleIdentity,
  ClaimsQuery,
  ClaimsSnapshot,
  ConnectionInfo,
  IpcError,
  RouteAuditPage,
  RouteAuditQuery,
  RunEventPage,
  RunEventsQuery,
  RunQuery,
  RunSnapshot,
  RuntimeStatus,
  V3CapabilitySet,
  V3EntityPage,
  V3EntityRecord,
  V3PageQuery,
  V3ResourceQuery,
  V3SearchRequest,
} from "./generated";

/**
 * The complete IPC surface. This mirrors `generate_handler!` in
 * `src-tauri/src/main.rs`; there is no dynamic command name anywhere in the
 * frontend, and no command that executes anything.
 */
export const COMMANDS = [
  "app_status",
  "app_claims",
  "app_routes",
  "app_run_status",
  "app_run_events",
  "app_v3_grants",
  "app_memory_search",
  "app_memory_metadata",
  "app_models_list",
  "app_connection",
  "app_bundle_identity",
] as const;

export type CommandName = (typeof COMMANDS)[number];

/** True when the page is running inside the Tauri webview. */
export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Partial<IpcError>;
  return typeof candidate.kind === "string" && typeof candidate.message === "string";
}

/** Normalise anything thrown by `invoke` into an `IpcError`. */
export function toIpcError(thrown: unknown): IpcError {
  if (isIpcError(thrown)) return thrown;
  if (!inTauri()) {
    return {
      kind: "transport",
      message: "This page is not running inside the Abbey desktop shell.",
      remedy: "Launch it with `bun run tauri dev` rather than opening the Vite URL directly.",
    };
  }
  return {
    kind: "transport",
    message: thrown instanceof Error ? thrown.message : String(thrown),
    remedy: null,
  };
}

export function appStatus(): Promise<RuntimeStatus> {
  return invoke<RuntimeStatus>("app_status");
}

export function appClaims(query: ClaimsQuery): Promise<ClaimsSnapshot> {
  return invoke<ClaimsSnapshot>("app_claims", { query });
}

export function appRoutes(query: RouteAuditQuery): Promise<RouteAuditPage> {
  return invoke<RouteAuditPage>("app_routes", { query });
}

export function appRunStatus(query: RunQuery): Promise<RunSnapshot> {
  return invoke<RunSnapshot>("app_run_status", { query });
}

export function appRunEvents(query: RunEventsQuery): Promise<RunEventPage> {
  return invoke<RunEventPage>("app_run_events", { query });
}

export function appV3Grants(): Promise<V3CapabilitySet> {
  return invoke<V3CapabilitySet>("app_v3_grants");
}

export function appMemorySearch(request: V3SearchRequest): Promise<V3EntityPage> {
  return invoke<V3EntityPage>("app_memory_search", { request });
}

export function appMemoryMetadata(query: V3ResourceQuery): Promise<V3EntityRecord> {
  return invoke<V3EntityRecord>("app_memory_metadata", { query });
}

/**
 * `ReadModels` inventory. Read-only: the desktop never negotiates
 * `download_models` or `manage_models`, so there is no command here that
 * could mutate model state.
 */
export function appModelsList(query: V3PageQuery): Promise<V3EntityPage> {
  return invoke<V3EntityPage>("app_models_list", { query });
}

export function appConnection(): Promise<ConnectionInfo> {
  return invoke<ConnectionInfo>("app_connection");
}

export function appBundleIdentity(): Promise<BundleIdentity> {
  return invoke<BundleIdentity>("app_bundle_identity");
}
