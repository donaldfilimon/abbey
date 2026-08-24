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
    match super::routes::audit_state_root() {
        Some(root) => route_audit_page_in(&root, query),
        None => empty_route_audit_page(query.limit),
    }
}

/// The state-directory-explicit half of [`route_audit_page`].
///
/// Split out so tests can drive a scratch log directly: `std::env::set_var` is
/// `unsafe` in edition 2024 and this crate denies `unsafe_code`, so a unit test
/// cannot point `ABBEY_STATE_DIR` at a temporary directory. The env-resolved
/// path is covered end-to-end by `tests/daemon_cli.rs`, which sets the variable
/// on a real spawned process.
fn route_audit_page_in(state_dir: &std::path::Path, query: &RouteAuditQuery) -> RouteAuditPage {
    let limit = query.limit;
    let Ok(records) = crate::route_log::recent_routes(state_dir, usize::from(limit)) else {
        return empty_route_audit_page(limit);
    };
    let entries = records
        .iter()
        .filter_map(super::routes::sanitize_record)
        .collect::<Vec<_>>();
    RouteAuditPage {
        // `recent_routes` already caps at `limit`, and sanitization only ever
        // drops records, so this cannot exceed `u16::from(limit)`.
        returned: u16::try_from(entries.len()).unwrap_or(limit),
        limit,
        entries,
    }
}

const fn empty_route_audit_page(limit: u16) -> RouteAuditPage {
    RouteAuditPage {
        entries: Vec::new(),
        returned: 0,
        limit,
    }
}

fn claim_status(status: crate::claims::Status) -> ClaimStatus {
    match status {
        crate::claims::Status::Current => ClaimStatus::Current,
        crate::claims::Status::Partial => ClaimStatus::Partial,
        crate::claims::Status::Proposed => ClaimStatus::Proposed,
        crate::claims::Status::Blocked => ClaimStatus::Blocked,
        crate::claims::Status::OutOfScope => ClaimStatus::OutOfScope,
        crate::claims::Status::Failed => ClaimStatus::Failed,
        crate::claims::Status::Revoked => ClaimStatus::Revoked,
        crate::claims::Status::Superseded => ClaimStatus::Superseded,
        crate::claims::Status::Expired => ClaimStatus::Expired,
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
        assert_eq!(status.capabilities.as_slice().len(), 3);
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

    /// Owner-only scratch directory that removes itself.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "abbey-route-audit-{tag}-{}-{}",
                std::process::id(),
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            ));
            std::fs::create_dir_all(&path).expect("create scratch state dir");
            Self(path)
        }

        fn write_log(&self, lines: &[String]) {
            std::fs::write(
                crate::route_log::route_log_path(&self.0),
                format!("{}\n", lines.join("\n")),
            )
            .expect("write route log");
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn record_line(index: usize, cwd: &str, reason: &str) -> String {
        let mut record =
            crate::route_log::RouteRecord::new(cwd, "Abbey", "max", "fable", reason, 0.5);
        record.ts = format!("2026-08-08T12:{:02}:00Z", index % 60);
        serde_json::to_string(&record).expect("serialize route record")
    }

    #[test]
    fn an_absent_log_reads_as_an_empty_page_rather_than_an_error() {
        let scratch = Scratch::new("empty");
        let page = route_audit_page_in(&scratch.0, &RouteAuditQuery::default());
        assert!(page.entries.is_empty());
        assert_eq!(page.returned, 0);
        assert_eq!(page.limit, super::super::MAX_ROUTE_AUDIT_PAGE);
        page.validate().unwrap();

        // An empty file is the same answer, not a parse failure.
        scratch.write_log(&[String::new()]);
        let page = route_audit_page_in(&scratch.0, &RouteAuditQuery::default());
        assert_eq!(page.returned, 0);
        page.validate().unwrap();
    }

    #[test]
    fn reads_are_capped_and_return_the_most_recent_decisions() {
        let scratch = Scratch::new("cap");
        let lines = (0..80)
            .map(|index| record_line(index, "/tmp/project", &format!("decision-{index}")))
            .collect::<Vec<_>>();
        scratch.write_log(&lines);

        let capped = route_audit_page_in(&scratch.0, &RouteAuditQuery::default());
        assert_eq!(
            capped.returned,
            super::super::MAX_ROUTE_AUDIT_PAGE,
            "the page must never exceed its cap"
        );
        assert_eq!(capped.entries.len(), 50);
        capped.validate().unwrap();
        // Tail semantics: the newest decision is last, the oldest 30 are gone.
        assert!(
            capped
                .entries
                .last()
                .unwrap()
                .reason
                .contains("decision-79")
        );
        assert!(
            capped
                .entries
                .first()
                .unwrap()
                .reason
                .contains("decision-30")
        );

        let narrow = route_audit_page_in(&scratch.0, &RouteAuditQuery { limit: 3 });
        assert_eq!(narrow.returned, 3);
        assert_eq!(narrow.limit, 3);
        narrow.validate().unwrap();
    }

    #[test]
    fn one_malformed_jsonl_line_does_not_poison_the_read() {
        let scratch = Scratch::new("malformed");
        scratch.write_log(&[
            record_line(1, "/tmp/project", "first"),
            "{ this is not json".to_owned(),
            "null".to_owned(),
            r#"{"ts":"2026-08-08T12:05:00Z"}"#.to_owned(),
            record_line(2, "/tmp/project", "second"),
        ]);

        let page = route_audit_page_in(&scratch.0, &RouteAuditQuery::default());
        assert_eq!(page.returned, 2, "well-formed records must survive");
        assert!(page.entries[0].reason.contains("first"));
        assert!(page.entries[1].reason.contains("second"));
        page.validate().unwrap();
    }

    /// The redaction bar: nothing that reaches the wire may carry a path.
    #[test]
    fn no_absolute_path_home_segment_or_control_character_survives_serialization() {
        let home = std::env::var("HOME").expect("HOME is set in the test environment");
        let secret = format!("{home}/abbey-route-audit/secret-project");
        let scratch = Scratch::new("redaction");
        scratch.write_log(&[record_line(
            1,
            &secret,
            &format!("persona=Abbey class=Code cwd={secret} log=/var/log/abbey.jsonl\u{7}"),
        )]);

        let page = route_audit_page_in(&scratch.0, &RouteAuditQuery::default());
        assert_eq!(page.returned, 1);
        page.validate().unwrap();
        let serialized =
            serde_json::to_string(&AppEvent::RouteAudit(page.clone())).expect("serialize page");

        for leaked in [secret.as_str(), home.as_str(), "/var/log/abbey.jsonl"] {
            assert!(
                !serialized.contains(leaked),
                "route audit leaked {leaked} in {serialized}"
            );
        }
        // No whitespace-delimited token anywhere in the payload is an absolute
        // path, and nothing in it is a control character.
        let entry = &page.entries[0];
        for field in [
            entry.recorded_at.as_str(),
            entry.persona.as_str(),
            entry.role.as_str(),
            entry.model.as_str(),
            entry.reason.as_str(),
            entry.workspace.as_deref().unwrap_or_default(),
        ] {
            assert!(
                !field.chars().any(char::is_control),
                "control char in {field}"
            );
            assert!(
                !field.split_whitespace().any(|token| token.starts_with('/')),
                "absolute path in {field}"
            );
        }
        assert!(entry.reason.contains("[path]"));
        assert!(entry.workspace.as_deref().unwrap().starts_with("ws-"));
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
