//! Authenticated, bounded local control-plane transport for `abbeyd`.
//!
//! Protocol v1 preserves its read-only Status/Claims contract. Protocol v2 adds
//! durable run submission, status, cancellation, and sanitized paged lifecycle
//! events for startup-bound fixed local providers. Requests never choose a
//! program, argument recipe, environment, or workspace; tools, shell, memory,
//! automations, live subscriptions, and non-Unix transports remain unavailable.

mod client;
mod config;
mod protocol;
mod runtime_config;
mod runtime_handler;
mod server;

pub use client::{ClientError, DaemonClient};
pub use config::{AuthenticatedRateLimit, BearerSecret, DaemonConfig};
pub use protocol::{
    CURRENT_PROTOCOL_VERSION, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope, ResponsePayload,
    SUPPORTED_PROTOCOL_VERSIONS,
};
pub use runtime_config::{RuntimeConfigError, RuntimeDaemonConfig};
pub use runtime_handler::RuntimeHandler;
pub use server::{
    DaemonHandler, DaemonServer, HandlerFailure, ReadOnlyHandler, ServerError, Shutdown,
};
