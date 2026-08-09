//! Read-only service facade over Abbey's canonical runtime and claims data.

use super::{
    AppCommand, AppContext, AppEvent, ClaimRecord, ClaimStatus, ClaimsQuery, ClaimsSnapshot,
    RouteAuditPage, RouteAuditQuery, StandardPolicy, ValidationError,
};
use std::fmt;

#[derive(Debug, Clone)]
pub struct AppService {
    context: AppContext,
    policy: StandardPolicy,
}

impl AppService {
    #[must_use]
    pub fn new(context: AppContext) -> Self {
        Self {
            context,
            policy: StandardPolicy,
        }
    }

    /// Execute one validated, read-only application command.
    pub fn handle(&self, command: AppCommand) -> Result<AppEvent, AppServiceError> {
        command
            .validate()
            .map_err(AppServiceError::InvalidCommand)?;
        if !self
            .policy
            .permits(&command, &self.context.status().capabilities)
        {
            return Err(AppServiceError::NotPermitted);
        }

        match command {
            AppCommand::Status => Ok(AppEvent::Status(self.context.status().clone())),
            AppCommand::Claims(query) => Ok(AppEvent::Claims(claims_snapshot(&query))),
            AppCommand::ReadRoutes(query) => Ok(AppEvent::RouteAudit(route_audit_page(&query))),
            AppCommand::SubmitRun(_)
            | AppCommand::GetRun(_)
            | AppCommand::CancelRun(_)
            | AppCommand::RunEvents(_) => Err(AppServiceError::NotPermitted),
        }
    }
}

impl Default for AppService {
    fn default() -> Self {
        Self::new(AppContext::active())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppServiceError {
    InvalidCommand(ValidationError),
    NotPermitted,
}

impl fmt::Display for AppServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCommand(error) => {
                write!(formatter, "invalid application command: {error}")
            }
            Self::NotPermitted => formatter.write_str("application command is not permitted"),
        }
    }
}

impl std::error::Error for AppServiceError {}

fn claims_snapshot(query: &ClaimsQuery) -> ClaimsSnapshot {
    let contains = query
        .contains
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    let claims = crate::claims::CLAIMS
        .iter()
        .filter(|claim| {
            query
                .status
                .is_none_or(|status| claim_status(claim.status) == status)
        })
        .filter(|claim| {
            contains.as_ref().is_none_or(|needle| {
                claim.name.to_ascii_lowercase().contains(needle)
                    || claim.note.to_ascii_lowercase().contains(needle)
                    || claim
                        .instead
                        .is_some_and(|value| value.to_ascii_lowercase().contains(needle))
            })
        })
        .map(|claim| ClaimRecord {
            name: claim.name.to_owned(),
            status: claim_status(claim.status),
            note: claim.note.to_owned(),
            instead: claim.instead.map(str::to_owned),
        })
        .collect::<Vec<_>>();

    ClaimsSnapshot {
        matched: claims.len(),
        claims,
    }
}

/// Read a bounded tail of the routing audit log and sanitize every record.
///
/// Fails soft to an empty page: a missing state root or an unreadable log is
/// "no routing has been audited here", not an error worth propagating to a
/// read-only caller. Individual malformed JSONL lines are already isolated by
/// [`crate::route_log::recent_routes`], and records this projection cannot
/// represent honestly are dropped by `sanitize_record` rather than emitted.
fn route_audit_page(query: &RouteAuditQuery) -> RouteAuditPage {
    let limit = query.limit;
    let entries = super::routes::audit_state_root()
        .and_then(|root| crate::route_log::recent_routes(&root, usize::from(limit)).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(super::routes::sanitize_record)
        .collect::<Vec<_>>();
    RouteAuditPage {
        returned: u16::try_from(entries.len()).unwrap_or(limit),
        limit,
        entries,
    }
}

fn claim_status(status: crate::claims::Status) -> ClaimStatus {
    match status {
        crate::claims::Status::Current => ClaimStatus::Current,
        crate::claims::Status::Partial => ClaimStatus::Partial,
        crate::claims::Status::Proposed => ClaimStatus::Proposed,
        crate::claims::Status::Blocked => ClaimStatus::Blocked,
        crate::claims::Status::OutOfScope => ClaimStatus::OutOfScope,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reports_versioned_standard_read_only_surface() {
        let event = AppService::default().handle(AppCommand::Status).unwrap();
        let AppEvent::Status(status) = event else {
            panic!("expected status event");
        };
        assert_eq!(status.protocol_version, super::super::APP_PROTOCOL_V1);
        assert_eq!(status.schema_version, super::super::APP_SCHEMA_V1);
        assert_eq!(status.capabilities.as_slice().len(), 2);
    }

    #[test]
    fn claims_reads_the_canonical_ledger_with_typed_filters() {
        let event = AppService::default()
            .handle(AppCommand::Claims(ClaimsQuery {
                status: Some(ClaimStatus::Blocked),
                contains: Some("linux".into()),
            }))
            .unwrap();
        let AppEvent::Claims(snapshot) = event else {
            panic!("expected claims event");
        };
        assert_eq!(snapshot.matched, 1);
        assert_eq!(snapshot.claims[0].status, ClaimStatus::Blocked);
    }

    #[test]
    fn invalid_claims_filter_fails_before_ledger_access() {
        let error = AppService::default()
            .handle(AppCommand::Claims(ClaimsQuery {
                status: None,
                contains: Some("\n".into()),
            }))
            .unwrap_err();
        assert!(matches!(error, AppServiceError::InvalidCommand(_)));
    }
}
