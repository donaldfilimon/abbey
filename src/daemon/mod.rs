//! Authenticated, bounded local control-plane transport for `abbeyd`.
//!
//! This first slice is deliberately read-only. It owns neither model workers,
//! tools, child processes, nor memory stores. Presentation clients can query
//! status and claims through an injected [`ReadOnlyHandler`].

mod config;
mod protocol;
mod server;

pub use config::{BearerSecret, DaemonConfig};
pub use protocol::{PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope, ResponsePayload};
pub use server::{DaemonServer, ReadOnlyHandler, ServerError, Shutdown};
