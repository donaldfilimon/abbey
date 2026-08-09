//! Stable serializable types at Abbey's application boundary.

use super::{
    RouteAuditPage, RouteAuditQuery, RunEventPage, RunEventsQuery, RunId, RunQuery, RunRequest,
    RunRouteCapability, RunSnapshot, RunSubmission,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Original read-only Status/Claims command/event protocol.
pub const APP_PROTOCOL_V1: u16 = 1;
/// Original read-only serialized application-state schema.
pub const APP_SCHEMA_V1: u16 = 1;
/// Current command/event exchange protocol.
pub const APP_PROTOCOL_VERSION: u16 = 2;
/// Current serialized application-state schema.
pub const APP_SCHEMA_VERSION: u16 = 2;

const MAX_CLAIMS_FILTER_BYTES: usize = 256;
const MAX_APPROVAL_SUMMARY_BYTES: usize = 1_024;

/// Abbey edition represented by this public-safe contract.
///
/// Mirrors the compile-time `edition::Edition` so a client is never told it is
/// talking to the safe edition when it is not. Neither variant grants extra
/// capabilities — [`CapabilitySet`] is identical in both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edition {
    /// Safe public edition — the default build.
    Standard,
    /// Separately packaged personal edition (`--features personal-edition`).
    Personal,
}

/// Commands accepted by the initial shared application service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AppCommand {
    Status,
    Claims(ClaimsQuery),
    /// Bounded tail of the persona/role routing audit log. Read-only, and the
    /// response omits the raw working directory — see [`RouteAuditPage`].
    ReadRoutes(RouteAuditQuery),
    SubmitRun(RunRequest),
    GetRun(RunQuery),
    CancelRun(RunQuery),
    RunEvents(RunEventsQuery),
}

impl AppCommand {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Status => Ok(()),
            Self::Claims(query) => query.validate(),
            Self::ReadRoutes(query) => query.validate(),
            Self::SubmitRun(request) => request.validate(),
            Self::GetRun(query) | Self::CancelRun(query) => query.validate(),
            Self::RunEvents(query) => query.validate(),
        }
    }

    /// Lowest application protocol version that can carry this command.
    ///
    /// The route audit is a protocol-v1 read: it touches no run, no store, and
    /// no provider, so the read-only daemon surface can answer it. Placing it at
    /// v2 would make it unreachable through `RuntimeHandler`'s v1 path, which is
    /// what `AppService::default()` serves.
    #[must_use]
    pub const fn minimum_protocol_version(&self) -> u16 {
        match self {
            Self::Status | Self::Claims(_) | Self::ReadRoutes(_) => APP_PROTOCOL_V1,
            Self::SubmitRun(_) | Self::GetRun(_) | Self::CancelRun(_) | Self::RunEvents(_) => {
                APP_PROTOCOL_VERSION
            }
        }
    }
}

/// Events emitted by the presentation-neutral application service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AppEvent {
    Status(RuntimeStatus),
    Claims(ClaimsSnapshot),
    RouteAudit(RouteAuditPage),
    ApprovalRequested(ApprovalRequest),
    RunSubmitted(RunSubmission),
    RunStatus(RunSnapshot),
    CancellationAcknowledged(RunSnapshot),
    RunEvents(RunEventPage),
}

impl AppEvent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Status(status) => status.validate(),
            Self::Claims(snapshot) => snapshot.validate(),
            Self::RouteAudit(page) => page.validate(),
            Self::ApprovalRequested(request) => request.validate(),
            Self::RunSubmitted(submission) => submission.validate(),
            Self::RunStatus(snapshot) | Self::CancellationAcknowledged(snapshot) => {
                snapshot.validate()
            }
            Self::RunEvents(page) => page.validate(),
        }
    }
}

/// Runtime state exposed without process or user-state details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Ready,
}

/// Safe, immutable process identity and supported application operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeStatus {
    pub protocol_version: u16,
    pub schema_version: u16,
    pub edition: Edition,
    pub state: RuntimeState,
    pub version: String,
    pub build_git: String,
    pub build_target: String,
    pub capabilities: CapabilitySet,
    /// Startup-bound execution routes. Empty and omitted for protocol v1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run_routes: Vec<RunRouteCapability>,
}

impl RuntimeStatus {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.capabilities.validate()?;
        if self
            .run_routes
            .windows(2)
            .any(|pair| pair[0].backend >= pair[1].backend)
        {
            return Err(ValidationError::new(
                "run routes must be strictly ordered and duplicate-free",
            ));
        }
        for route in &self.run_routes {
            route.validate()?;
        }
        // `run_routes` is tied to submission authority and to nothing else.
        // `AppCapability::ReadRoutes` is deliberately NOT part of this rule
        // despite the name: it reads the routing *audit log*, which exists
        // whether or not a provider route is bound.
        if self.capabilities.contains(AppCapability::SubmitRun) != !self.run_routes.is_empty() {
            return Err(ValidationError::new(
                "run submission capability and configured routes are inconsistent",
            ));
        }
        Ok(())
    }
}

/// Application operations granted by this edition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppCapability {
    ReadStatus,
    ReadClaims,
    ReadRun,
    ReadRunEvents,
    SubmitRun,
    CancelRun,
    /// Read the persona/role routing **audit log**.
    ///
    /// Unrelated to [`RuntimeStatus::run_routes`], which declares startup-bound
    /// *execution* routes. This capability grants no execution authority; it
    /// reads a sanitized tail of an append-only JSONL file.
    ///
    /// Declared last on purpose: [`CapabilitySet`] is ordered by this enum's
    /// declaration order, so appending keeps every existing pair's relative
    /// order — and therefore every serialized set — unchanged.
    ReadRoutes,
}

/// Ordered, duplicate-free capability declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitySet {
    capabilities: Vec<AppCapability>,
}

impl CapabilitySet {
    #[must_use]
    pub fn standard() -> Self {
        Self {
            capabilities: vec![
                AppCapability::ReadStatus,
                AppCapability::ReadClaims,
                AppCapability::ReadRoutes,
            ],
        }
    }

    /// Complete Phase 4B.2 operation set. Configured routes remain a separate,
    /// strictly bounded declaration on [`RuntimeStatus`].
    #[must_use]
    pub fn runtime_v2() -> Self {
        Self::runtime_v2_for_routes(true)
    }

    /// Phase 4B.2 operations available for the startup-bound route set.
    /// Submission is advertised only when at least one executable route exists;
    /// durable reads and cancellation remain available without a provider.
    #[must_use]
    pub fn runtime_v2_for_routes(has_routes: bool) -> Self {
        let mut capabilities = vec![
            AppCapability::ReadStatus,
            AppCapability::ReadClaims,
            AppCapability::ReadRun,
            AppCapability::ReadRunEvents,
        ];
        if has_routes {
            capabilities.push(AppCapability::SubmitRun);
        }
        capabilities.push(AppCapability::CancelRun);
        // Always granted: the audit read depends on no provider route.
        capabilities.push(AppCapability::ReadRoutes);
        Self { capabilities }
    }

    #[must_use]
    pub fn contains(&self, capability: AppCapability) -> bool {
        self.capabilities.binary_search(&capability).is_ok()
    }

    #[must_use]
    pub fn as_slice(&self) -> &[AppCapability] {
        &self.capabilities
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.capabilities.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ValidationError::new(
                "capabilities must be strictly ordered and duplicate-free",
            ));
        }
        Ok(())
    }
}

/// Canonical claim status, independent of terminal rendering labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Current,
    Partial,
    Proposed,
    Blocked,
    OutOfScope,
}

/// Bounded, typed query over the canonical claims table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClaimsQuery {
    pub status: Option<ClaimStatus>,
    pub contains: Option<String>,
}

impl ClaimsQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let Some(filter) = self.contains.as_deref() else {
            return Ok(());
        };
        let filter = filter.trim();
        if filter.is_empty() {
            return Err(ValidationError::new("claims filter cannot be empty"));
        }
        if filter.len() > MAX_CLAIMS_FILTER_BYTES {
            return Err(ValidationError::new("claims filter exceeds 256 bytes"));
        }
        if filter.chars().any(char::is_control) {
            return Err(ValidationError::new(
                "claims filter cannot contain control characters",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRecord {
    pub name: String,
    pub status: ClaimStatus,
    pub note: String,
    pub instead: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimsSnapshot {
    pub claims: Vec<ClaimRecord>,
    pub matched: usize,
}

impl ClaimsSnapshot {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.matched != self.claims.len() {
            return Err(ValidationError::new(
                "claims snapshot matched count is inconsistent",
            ));
        }
        Ok(())
    }
}

/// Sensitive operation class shown to a user before any future execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    FileSystemMutation,
    NetworkAccess,
    ProcessExecution,
    PrivilegeElevation,
}

/// A presentation-neutral approval prompt. This type grants no authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRequest {
    pub run_id: RunId,
    pub kind: ApprovalKind,
    pub summary: String,
}

impl ApprovalRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let summary = self.summary.trim();
        if summary.is_empty() {
            return Err(ValidationError::new("approval summary cannot be empty"));
        }
        if summary.len() > MAX_APPROVAL_SUMMARY_BYTES {
            return Err(ValidationError::new("approval summary exceeds 1024 bytes"));
        }
        if summary.chars().any(char::is_control) {
            return Err(ValidationError::new(
                "approval summary cannot contain control characters",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    message: &'static str,
}

impl ValidationError {
    #[must_use]
    pub const fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_have_a_stable_tagged_wire_shape() {
        let command = AppCommand::Claims(ClaimsQuery {
            status: Some(ClaimStatus::Current),
            contains: Some("memory".into()),
        });
        assert_eq!(
            serde_json::to_value(command).unwrap(),
            serde_json::json!({
                "type": "claims",
                "payload": {"status": "current", "contains": "memory"}
            })
        );
        assert!(
            serde_json::from_value::<AppCommand>(serde_json::json!({
                "type": "status",
                "extra": true
            }))
            .is_err()
        );
    }

    #[test]
    fn events_have_a_stable_tagged_wire_shape() {
        let event = AppEvent::Claims(ClaimsSnapshot {
            claims: Vec::new(),
            matched: 0,
        });
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({
                "type": "claims",
                "payload": {"claims": [], "matched": 0}
            })
        );
        assert!(
            serde_json::from_value::<AppEvent>(serde_json::json!({
                "type": "claims",
                "payload": {"claims": [], "matched": 0},
                "extra": true
            }))
            .is_err()
        );
    }

    #[test]
    fn free_text_fields_are_bounded_and_reject_controls() {
        let empty = ClaimsQuery {
            contains: Some("  ".into()),
            ..ClaimsQuery::default()
        };
        assert!(empty.validate().is_err());

        let control = ApprovalRequest {
            run_id: RunId::new(),
            kind: ApprovalKind::NetworkAccess,
            summary: "request\nsecret".into(),
        };
        assert!(control.validate().is_err());
    }

    #[test]
    fn standard_capabilities_are_read_only() {
        let capabilities = CapabilitySet::standard();
        capabilities.validate().unwrap();
        assert_eq!(
            capabilities.as_slice(),
            &[AppCapability::ReadStatus, AppCapability::ReadClaims]
        );
    }

    #[test]
    fn runtime_capabilities_and_routes_are_strictly_ordered() {
        let capabilities = CapabilitySet::runtime_v2();
        capabilities.validate().unwrap();
        assert_eq!(
            capabilities.as_slice(),
            &[
                AppCapability::ReadStatus,
                AppCapability::ReadClaims,
                AppCapability::ReadRun,
                AppCapability::ReadRunEvents,
                AppCapability::SubmitRun,
                AppCapability::CancelRun,
            ]
        );

        let status = RuntimeStatus {
            protocol_version: APP_PROTOCOL_VERSION,
            schema_version: APP_SCHEMA_VERSION,
            edition: Edition::Standard,
            state: RuntimeState::Ready,
            version: "test".into(),
            build_git: "test".into(),
            build_target: "test".into(),
            capabilities,
            run_routes: vec![
                RunRouteCapability {
                    backend: crate::app_core::BackendSelection::Abi,
                    modes: vec![
                        crate::app_core::RunMode::OneShot,
                        crate::app_core::RunMode::Background,
                    ],
                },
                RunRouteCapability {
                    backend: crate::app_core::BackendSelection::FoundationModels,
                    modes: vec![
                        crate::app_core::RunMode::OneShot,
                        crate::app_core::RunMode::Background,
                    ],
                },
            ],
        };
        status.validate().unwrap();

        let mut invalid = status;
        invalid.run_routes.reverse();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn runtime_without_routes_omits_only_submission_authority() {
        let capabilities = CapabilitySet::runtime_v2_for_routes(false);
        capabilities.validate().unwrap();
        assert_eq!(
            capabilities.as_slice(),
            &[
                AppCapability::ReadStatus,
                AppCapability::ReadClaims,
                AppCapability::ReadRun,
                AppCapability::ReadRunEvents,
                AppCapability::CancelRun,
            ]
        );
        assert!(!capabilities.contains(AppCapability::SubmitRun));
        assert_eq!(
            CapabilitySet::runtime_v2_for_routes(true),
            CapabilitySet::runtime_v2()
        );
    }
}
