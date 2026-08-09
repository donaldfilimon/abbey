//! Desktop-only transport types.
//!
//! Everything Abbey itself defines — `RuntimeStatus`, `ClaimsSnapshot`,
//! `ClaimsQuery`, `Edition` — crosses IPC as the `abbey::app_core` type, not a
//! desktop copy. Only the envelope describing *how the desktop reached the app
//! core* lives here, and it is projected into TypeScript by `desktop/codegen`
//! from this very file, so it cannot drift either.
//!
//! Secret hygiene is a type-level property of this module: no field on any type
//! below can hold a bearer token. The daemon bearer is reported as a *boolean*
//! and a *source kind*, never as a value and never as the token file's
//! contents.

use abbey::app_core::Edition;
use serde::{Deserialize, Serialize};

/// Which transport served a desktop read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionSource {
    /// An authenticated `abbeyd` over its owner-only Unix socket.
    Daemon,
    /// Abbey's application core linked into this process.
    ///
    /// This is only used when no daemon bearer is configured at all. A daemon
    /// that *is* configured but fails is surfaced as an error — Abbey's
    /// documented invariant is that client failures never silently fall back
    /// to in-process claims.
    InProcess,
}

/// Where the daemon bearer was configured, by *kind* — never its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BearerSource {
    /// Inline environment variable.
    InlineEnv,
    /// Owner-only token file named by an environment variable.
    TokenFile,
    /// Both were set — a fail-closed configuration error, not a preference.
    Conflicting,
}

/// Non-secret description of how this desktop process reaches Abbey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub source: ConnectionSource,
    /// Daemon socket path, when a daemon is configured. Never a token.
    pub socket_path: Option<String>,
    pub bearer_configured: bool,
    pub bearer_source: Option<BearerSource>,
    /// Human-readable explanation, safe to render and safe to log.
    pub detail: String,
}

/// Packaged identity of the running build, derived from `abbey::edition`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleIdentity {
    pub edition: Edition,
    pub product_name: String,
    pub binary_name: String,
    pub daemon_binary_name: String,
    pub bundle_id: String,
    /// Bundle identifier the running window was actually configured with.
    ///
    /// Equal to `bundle_id` in a correctly built app; the startup check in
    /// `main.rs` refuses to run if they differ, so a personal build can never
    /// present itself under the safe edition's identity.
    pub configured_bundle_id: String,
}

/// Category of an IPC failure. The frontend switches on this, so the strings
/// are part of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorKind {
    /// The daemon is configured but its configuration is invalid.
    Configuration,
    /// The daemon could not be reached, or the exchange failed.
    Transport,
    /// The daemon answered with a well-formed but unexpected event.
    Protocol,
    /// The request was rejected before any data was read.
    Rejected,
    /// This platform has no daemon transport yet (Windows named pipes).
    UnsupportedPlatform,
}

/// A failure the frontend can render. Carries no secret material: every
/// message originates from an authored `Display` string in Abbey's daemon
/// error types, none of which interpolate bearer values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcError {
    pub kind: IpcErrorKind,
    pub message: String,
    pub remedy: Option<String>,
}

impl IpcError {
    pub fn new(kind: IpcErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            remedy: None,
        }
    }

    #[must_use]
    pub fn with_remedy(mut self, remedy: impl Into<String>) -> Self {
        self.remedy = Some(remedy.into());
        self
    }
}
