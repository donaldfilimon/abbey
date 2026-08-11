//! Authenticated, bounded local control-plane transport for `abbeyd`.
//!
//! Protocol v1 preserves its read-only contract — Status, Claims, and the
//! sanitized ReadRoutes audit tail. Protocol v2 adds
//! durable run submission, status, cancellation, and sanitized paged lifecycle
//! events for startup-bound fixed local providers. Protocol v3 is a separate
//! authenticated envelope and currently negotiates daemon-local safe tool
//! inventory, bounded ABI-local model inventory when that provider is
//! configured, and exact stable-ID reads from Abbey's canonical claims
//! registry. The default safe daemon can persist one digest-bound pending
//! request to mark a memory record obsolete, but cannot decide, cancel, or
//! execute it. Requests never choose a program, argument recipe, environment,
//! or workspace; shell, approval decisions, daemon-owned memory execution,
//! automations, live subscriptions, and non-Unix transports remain unavailable.

mod client;
mod config;
mod protocol;
mod runtime_config;
mod runtime_handler;
mod runtime_v3;
mod server;

pub use client::{ClientError, DaemonClient, V3DaemonSession};
pub use config::{AuthenticatedRateLimit, BearerSecret, DaemonConfig};
pub use protocol::{
    CURRENT_PROTOCOL_VERSION, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope, ResponsePayload,
    SUPPORTED_PROTOCOL_VERSIONS, V3RequestEnvelope, V3ResponseEnvelope, V3ResponsePayload,
};
pub use runtime_config::{RuntimeConfigError, RuntimeDaemonConfig};
pub use runtime_handler::RuntimeHandler;
pub use server::{
    DaemonHandler, DaemonServer, HandlerFailure, ReadOnlyHandler, ServerError, Shutdown,
};
