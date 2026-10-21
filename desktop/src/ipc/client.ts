// Typed wrappers over the enumerated Tauri commands.
//
// Every type below comes from `./generated`, which is projected from Abbey's
// Rust source. Nothing in this file declares a shape of its own.

import { invoke } from "@tauri-apps/api/core";
import type {
  V4CapabilitySet, V4ContentPage, V4ContentQuery, V4Conversation, V4ConversationCreate,
  V4ConversationQuery, V4ConversationRename, V4ConversationPatch, V4ConversationPage, V4ResourceQuery, V4Route,
  V4MessagePage, V4MessageQuery, V4SubmitTurn, V4TurnSubmission,
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
  RunState,
  ConversationId,
  RunId,
  RuntimeStatus,
  SystemIntegration,
  V3CapabilitySet,
  V3EntityPage,
  V3EntityRecord,
  V3PageQuery,
  V3ResourceQuery,
  V3SearchRequest,
  V3StableClaim,
  V3ToolPage,
  V3ModelAction, V3ModelDetails, V3OperationStatus,
} from "./generated";

/**
 * The complete IPC surface. This mirrors `generate_handler!` in
 * `src-tauri/src/main.rs`; there is no dynamic command name anywhere in the
 * frontend. Conversation submit, content, and route reads call the daemon. This
 * process does not execute a model or a tool.
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
  "app_model_grants",
  "app_model_details",
  "app_model_download",
  "app_model_load",
  "app_model_unload",
  "app_model_download_status",
  "app_model_lifecycle_status",

  "app_claim_by_id",
  "app_connection",
  "app_bundle_identity",
  "app_conversation_grants",
  "app_conversations_list",
  "app_conversations_search",
  "app_conversation_read",
  "app_conversation_create",
  "app_conversation_rename",
  "app_conversation_archive",
  "app_conversation_delete",
  "app_conversation_messages",
  "app_conversation_content",
  "app_conversation_routes",
  "app_conversation_submit_turn",
  "app_conversation_cancel_turn",
  "app_conversation_lookup_turn",
  "app_conversation_update",

  // Read-only tool inventory and this build's own host-OS facts. The order
  // mirrors the `generate_handler!` block in `src-tauri/src/main.rs`, which
  // `tools/check_claims_sync.py` compares entry by entry.
  "app_tools_list",
  "app_system_integration",

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

/** Bounded read-only inventory. Lifecycle uses separate grants below. */
export function appModelsList(query: V3PageQuery): Promise<V3EntityPage> {
  return invoke<V3EntityPage>("app_models_list", { query });
}

/**
 * `ReadClaimsById` — one canonical claim by exact stable ID. Distinct from
 * `appClaims`, which filters the protocol-v1 ledger snapshot.
 */
export function appClaimById(query: V3ResourceQuery): Promise<V3StableClaim> {
  return invoke<V3StableClaim>("app_claim_by_id", { query });
}

export function appConnection(): Promise<ConnectionInfo> {
  return invoke<ConnectionInfo>("app_connection");
}

/**
 * `ListTools` — the tool inventory the daemon advertises.
 *
 * Read-only enumeration: this call lists descriptors and nothing else. There
 * is no invoke, approve, deny, or cancel command on this surface, so a tool
 * that appears here cannot be run from this window.
 */
export function appToolsList(query: V3PageQuery): Promise<V3ToolPage> {
  return invoke<V3ToolPage>("app_tools_list", { query });
}

/**
 * This build's own host-OS integration, as read-only facts.
 *
 * The declared permissions come from the compiled-in capability manifest;
 * nothing here registers, installs, or starts anything.
 */
export function appSystemIntegration(): Promise<SystemIntegration> {
  return invoke<SystemIntegration>("app_system_integration");
}

export function appBundleIdentity(): Promise<BundleIdentity> {
  return invoke<BundleIdentity>("app_bundle_identity");
}

export function appModelGrants(): Promise<V3CapabilitySet> {
  return invoke<V3CapabilitySet>("app_model_grants");
}

export function appModelDetails(query: V3ResourceQuery): Promise<V3ModelDetails> {
  return invoke<V3ModelDetails>("app_model_details", { query });
}

export function appModelDownload(action: V3ModelAction): Promise<V3OperationStatus> {
  return invoke<V3OperationStatus>("app_model_download", { action });
}

export function appModelLoad(action: V3ModelAction): Promise<V3OperationStatus> {
  return invoke<V3OperationStatus>("app_model_load", { action });
}

export function appModelUnload(action: V3ModelAction): Promise<V3OperationStatus> {
  return invoke<V3OperationStatus>("app_model_unload", { action });
}

export function appModelDownloadStatus(query: V3ResourceQuery): Promise<V3OperationStatus> {
  return invoke<V3OperationStatus>("app_model_download_status", { query });
}

export function appModelLifecycleStatus(query: V3ResourceQuery): Promise<V3OperationStatus> {
  return invoke<V3OperationStatus>("app_model_lifecycle_status", { query });
}

export function appConversationGrants(): Promise<V4CapabilitySet> {
  return invoke<V4CapabilitySet>("app_conversation_grants");
}

export function appConversationsList(query: V4ConversationQuery): Promise<V4ConversationPage> {
  return invoke<V4ConversationPage>("app_conversations_list", { query });
}

export function appConversationsSearch(query: V4ConversationQuery): Promise<V4ConversationPage> {
  return invoke<V4ConversationPage>("app_conversations_search", { query });
}

export function appConversationRead(query: V4ResourceQuery): Promise<V4Conversation> {
  return invoke<V4Conversation>("app_conversation_read", { query });
}

export function appConversationCreate(query: V4ConversationCreate): Promise<V4Conversation> {
  return invoke<V4Conversation>("app_conversation_create", { query });
}

export function appConversationRename(query: V4ConversationRename): Promise<V4Conversation> {
  return invoke<V4Conversation>("app_conversation_rename", { query });
}

export function appConversationArchive(query: V4ResourceQuery): Promise<V4Conversation> {
  return invoke<V4Conversation>("app_conversation_archive", { query });
}

export function appConversationDelete(query: V4ResourceQuery): Promise<void> {
  return invoke<void>("app_conversation_delete", { query });
}

export function appConversationMessages(query: V4MessageQuery): Promise<V4MessagePage> {
  return invoke<V4MessagePage>("app_conversation_messages", { query });
}

export function appConversationContent(query: V4ContentQuery): Promise<V4ContentPage> {
  return invoke<V4ContentPage>("app_conversation_content", { query });
}

export function appConversationRoutes(): Promise<V4Route[]> {
  return invoke<V4Route[]>("app_conversation_routes");
}

export function appConversationSubmitTurn(request: V4SubmitTurn): Promise<V4TurnSubmission> {
  return invoke<V4TurnSubmission>("app_conversation_submit_turn", { request });
}

/** Send once; the Rust client checks exact response conversation/run correlation. */
export function appConversationCancelTurn(conversationId: ConversationId, runId: RunId): Promise<RunState> {
  return invoke<RunState>("app_conversation_cancel_turn", { conversationId, runId });
}

/** Read-only recovery; a missing receipt never triggers submission. */
export function appConversationLookupTurn(conversationId: ConversationId, idempotencyKey: string): Promise<V4TurnSubmission | null> {
  return invoke<V4TurnSubmission | null>("app_conversation_lookup_turn", { conversationId, idempotencyKey });
}

/** Existing daemon metadata patch; no execution or arbitrary configuration authority. */
export function appConversationUpdate(conversationId: ConversationId, patch: V4ConversationPatch): Promise<V4Conversation> {
  return invoke<V4Conversation>("app_conversation_update", { conversationId, patch });
}
