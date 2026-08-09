//! Sanitized read-only view over Abbey's persona/role routing audit log.
//!
//! # Why this file exists rather than a raw [`crate::route_log::RouteRecord`]
//!
//! The on-disk record carries `cwd` as a **raw absolute filesystem path**, and
//! `reason` as free text assembled next to model routing. Neither may cross the
//! daemon socket or reach the desktop client: keeping raw working directories
//! out of `runtime.sqlite` and out of daemon diagnostics is an invariant several
//! earlier phases were spent establishing, and re-introducing it through a new
//! read command would regress that boundary silently.
//!
//! So this module owns a *projection*, not a re-export:
//!
//! * `cwd` never appears. It is replaced by an opaque domain-separated
//!   [`RouteAuditEntry::workspace`] digest (`ws-<12 lowercase hex>`) that is
//!   stable enough to group entries in a UI and carries no path segment.
//! * every free-text field is bounded, control-stripped, and has filesystem
//!   paths redacted to `[path]` before it is ever serialized.
//! * `confidence` is quantized to whole percent. That keeps [`AppEvent`] `Eq`
//!   (an `f32` field would not), and a malformed `NaN`/`1e30` in the log clamps
//!   instead of wrapping.
//!
//! Sanitization happens on the **producer** side ([`sanitize_record`]) and the
//! invariants are re-checked on the **consumer** side ([`RouteAuditPage::validate`]).
//! The second check is not redundant: a client validates pages it did not build,
//! so the absolute-path rejection has to be a wire invariant, not a promise.
//!
//! [`AppEvent`]: super::AppEvent

use super::ValidationError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Largest number of audit entries one read may return.
pub const MAX_ROUTE_AUDIT_PAGE: u16 = 50;

const MAX_ROUTE_FIELD_BYTES: usize = 64;
const MAX_ROUTE_REASON_BYTES: usize = 240;
const MAX_ROUTE_TIMESTAMP_BYTES: usize = 64;
const MAX_ROUTE_TOOLS: usize = 8;

/// `ws-` plus this many lowercase hex characters. 48 bits distinguishes the
/// handful of workspaces one user routes from; it is not a recoverable path.
const WORKSPACE_DIGEST_HEX: usize = 12;
const WORKSPACE_PREFIX: &str = "ws-";
const WORKSPACE_DOMAIN: &[u8] = b"abbey:route-audit-workspace:v1\0";

const fn default_route_audit_limit() -> u16 {
    MAX_ROUTE_AUDIT_PAGE
}

/// Bounded tail query over the append-only routing audit log.
///
/// This is deliberately *not* a cursor. The route log is append-only JSONL with
/// no stable sequence column, so the honest contract is "the most recent N
/// decisions", the same thing `abbey routes -n` has always shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteAuditQuery {
    /// Maximum entries to return, from 1 through [`MAX_ROUTE_AUDIT_PAGE`].
    #[serde(default = "default_route_audit_limit")]
    pub limit: u16,
}

impl Default for RouteAuditQuery {
    fn default() -> Self {
        Self {
            limit: default_route_audit_limit(),
        }
    }
}

impl RouteAuditQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.limit == 0 || self.limit > MAX_ROUTE_AUDIT_PAGE {
            return Err(ValidationError::new(
                "route audit limit must be from 1 through 50",
            ));
        }
        Ok(())
    }
}

/// One sanitized routing decision. Contains no path, prompt, or provider output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteAuditEntry {
    /// RFC 3339 instant the decision was appended.
    pub recorded_at: String,
    /// Opaque `ws-<12 hex>` digest of the working directory. Never a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub persona: String,
    pub role: String,
    pub model: String,
    /// Routing confidence, quantized to whole percent (0 through 100).
    pub confidence_percent: u8,
    /// Bounded, control-stripped, path-redacted routing rationale.
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    /// Closed tool markers the router attached (`media`, `mcp`, `subagents`, …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
}

impl RouteAuditEntry {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_timestamp(&self.recorded_at)?;
        if let Some(workspace) = &self.workspace {
            validate_workspace(workspace)?;
        }
        validate_field(&self.persona, MAX_ROUTE_FIELD_BYTES)?;
        validate_field(&self.role, MAX_ROUTE_FIELD_BYTES)?;
        validate_field(&self.model, MAX_ROUTE_FIELD_BYTES)?;
        validate_field(&self.reason, MAX_ROUTE_REASON_BYTES)?;
        for value in [
            &self.stage,
            &self.correlation,
            &self.alternate,
            &self.fallback,
        ]
        .into_iter()
        .flatten()
        {
            validate_field(value, MAX_ROUTE_FIELD_BYTES)?;
        }
        if self.confidence_percent > 100 {
            return Err(ValidationError::new(
                "route audit confidence exceeds 100 percent",
            ));
        }
        if self.tools.len() > MAX_ROUTE_TOOLS {
            return Err(ValidationError::new("route audit entry has too many tools"));
        }
        for tool in &self.tools {
            validate_field(tool, MAX_ROUTE_FIELD_BYTES)?;
        }
        Ok(())
    }
}

/// One bounded tail of the routing audit log, oldest entry first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteAuditPage {
    pub entries: Vec<RouteAuditEntry>,
    /// `entries.len()`, carried explicitly so a truncated frame fails validation.
    pub returned: u16,
    /// The limit the request asked for, echoed back.
    pub limit: u16,
}

impl RouteAuditPage {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.limit == 0 || self.limit > MAX_ROUTE_AUDIT_PAGE {
            return Err(ValidationError::new(
                "route audit page limit must be from 1 through 50",
            ));
        }
        if usize::from(self.returned) != self.entries.len() || self.returned > self.limit {
            return Err(ValidationError::new(
                "route audit page count is inconsistent with its limit",
            ));
        }
        for entry in &self.entries {
            entry.validate()?;
        }
        Ok(())
    }
}

/// Project one on-disk record into the sanitized view.
///
/// Returns `None` when the record cannot be represented honestly — today that
/// means a timestamp that is not RFC 3339, or an empty persona/role/model after
/// sanitization. A dropped record is preferable to an entry whose fields the
/// consumer would have to reject anyway.
pub(crate) fn sanitize_record(record: &crate::route_log::RouteRecord) -> Option<RouteAuditEntry> {
    let recorded_at = record.ts.trim().to_owned();
    validate_timestamp(&recorded_at).ok()?;

    let persona = sanitize_text(&record.persona, MAX_ROUTE_FIELD_BYTES);
    let role = sanitize_text(&record.role, MAX_ROUTE_FIELD_BYTES);
    let model = sanitize_text(&record.model, MAX_ROUTE_FIELD_BYTES);
    if persona.is_empty() || role.is_empty() || model.is_empty() {
        return None;
    }

    let reason = {
        let sanitized = sanitize_text(&record.reason, MAX_ROUTE_REASON_BYTES);
        if sanitized.is_empty() {
            "(no reason recorded)".to_owned()
        } else {
            sanitized
        }
    };

    let entry = RouteAuditEntry {
        recorded_at,
        workspace: workspace_digest(&record.cwd),
        persona,
        role,
        model,
        confidence_percent: quantize_confidence(record.confidence),
        reason,
        stage: sanitize_optional(record.stage.as_deref()),
        correlation: sanitize_optional(record.correlation.as_deref()),
        alternate: sanitize_optional(record.alternate.as_deref()),
        fallback: sanitize_optional(record.fallback.as_deref()),
        tools: record
            .tools
            .iter()
            .filter_map(|tool| sanitize_optional(Some(tool)))
            .take(MAX_ROUTE_TOOLS)
            .collect(),
    };
    entry.validate().ok()?;
    Some(entry)
}

/// The state root the audit log lives under, resolved **without creating it**.
///
/// Deliberately not `AbbeyState::load()`: that creates directories and reads the
/// process working directory, neither of which a read-only command may do. The
/// resolution order is otherwise identical, and stays edition-scoped so a
/// personal build never reads the safe build's audit log.
pub(crate) fn audit_state_root() -> Option<PathBuf> {
    let edition = crate::edition::ACTIVE;
    std::env::var_os(edition.state_dir_env())
        .map(PathBuf::from)
        .or_else(|| edition.default_state_root())
}

/// Domain-separated, truncated digest of a working directory.
fn workspace_digest(cwd: &str) -> Option<String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut digest = Sha256::new();
    digest.update(WORKSPACE_DOMAIN);
    digest.update(trimmed.as_bytes());
    let bytes: [u8; 32] = digest.finalize().into();

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(WORKSPACE_PREFIX.len() + WORKSPACE_DIGEST_HEX);
    encoded.push_str(WORKSPACE_PREFIX);
    for byte in bytes.iter().take(WORKSPACE_DIGEST_HEX / 2) {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Some(encoded)
}

fn quantize_confidence(confidence: f32) -> u8 {
    if !confidence.is_finite() {
        return 0;
    }
    // `clamp` before the cast: an unclamped `as u8` saturates in current Rust
    // but relying on saturation for out-of-range provider data is not a contract.
    (confidence * 100.0).round().clamp(0.0, 100.0) as u8
}

fn sanitize_optional(value: Option<&str>) -> Option<String> {
    let sanitized = sanitize_text(value?, MAX_ROUTE_FIELD_BYTES);
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

/// Collapse whitespace, drop control characters, redact filesystem paths, and
/// truncate on a character boundary.
fn sanitize_text(raw: &str, max_bytes: usize) -> String {
    let mut out = String::new();
    for token in raw.split_whitespace() {
        let cleaned: String = if is_redactable_path(token) {
            "[path]".to_owned()
        } else {
            token.chars().filter(|c| !c.is_control()).collect()
        };
        if cleaned.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&cleaned);
    }
    truncate_on_boundary(out, max_bytes)
}

fn truncate_on_boundary(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

/// A token the producer replaces with `[path]`.
///
/// Broader than [`is_structural_path`]: it also catches the user's own home
/// directory, which is only knowable on the machine that produced the record.
fn is_redactable_path(token: &str) -> bool {
    if is_structural_path(token) {
        return true;
    }
    home_marker().is_some_and(|home| token.contains(home))
}

/// A token any consumer must reject, on any machine.
///
/// Only structural markers: a leading `/`, a `~` prefix, a UNC prefix, or a
/// `C:\`-style drive. The home-directory check is deliberately absent — the
/// receiving desktop has a different `HOME` than the daemon that produced the
/// page, so a `HOME`-relative rule would be a machine-dependent wire invariant.
///
/// Every `key=value` segment of the token is tested, not just its start. Abbey's
/// own reasons are `key=value` shaped (`persona=`, `class=`, `stage=`, `exit=`),
/// so a prefix-only test let `log=/var/log/abbey.jsonl` through — caught by the
/// redaction test before this shipped.
fn is_structural_path(token: &str) -> bool {
    token.split('=').any(segment_is_path)
}

fn segment_is_path(segment: &str) -> bool {
    let segment =
        segment.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';'));
    if segment.starts_with('/') || segment.starts_with('~') || segment.starts_with("\\\\") {
        return true;
    }
    let bytes = segment.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn home_marker() -> Option<&'static str> {
    static HOME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        for variable in ["HOME", "USERPROFILE"] {
            if let Ok(value) = std::env::var(variable) {
                let trimmed = value.trim_end_matches(['/', '\\']).to_owned();
                // A one- or two-character "home" would redact ordinary words.
                if trimmed.len() > 3 {
                    return Some(trimmed);
                }
            }
        }
        None
    })
    .as_deref()
}

fn validate_timestamp(value: &str) -> Result<(), ValidationError> {
    if value.is_empty() || value.len() > MAX_ROUTE_TIMESTAMP_BYTES {
        return Err(ValidationError::new("invalid route audit timestamp"));
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| ValidationError::new("invalid route audit timestamp"))
}

fn validate_workspace(value: &str) -> Result<(), ValidationError> {
    let Some(digest) = value.strip_prefix(WORKSPACE_PREFIX) else {
        return Err(ValidationError::new(
            "route audit workspace is not an opaque digest",
        ));
    };
    if digest.len() != WORKSPACE_DIGEST_HEX
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ValidationError::new(
            "route audit workspace is not an opaque digest",
        ));
    }
    Ok(())
}

fn validate_field(value: &str, max_bytes: usize) -> Result<(), ValidationError> {
    if value.is_empty() || value.len() > max_bytes {
        return Err(ValidationError::new(
            "route audit field is empty or exceeds its bound",
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(ValidationError::new(
            "route audit field cannot contain control characters",
        ));
    }
    if value.split_whitespace().any(is_structural_path) {
        return Err(ValidationError::new(
            "route audit field cannot contain a filesystem path",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route_log::RouteRecord;

    fn record(cwd: &str, reason: &str) -> RouteRecord {
        let mut record = RouteRecord::new(cwd, "Abbey", "max", "fable", reason, 0.82);
        record.ts = "2026-08-08T12:00:00Z".into();
        record
    }

    #[test]
    fn wire_shapes_are_tagged_and_reject_unknown_fields() {
        let command = super::super::AppCommand::ReadRoutes(RouteAuditQuery { limit: 7 });
        assert_eq!(
            serde_json::to_value(&command).unwrap(),
            serde_json::json!({"type": "read_routes", "payload": {"limit": 7}})
        );
        // Round-trip, then prove the extra key is refused on both sides.
        assert_eq!(
            serde_json::from_value::<super::super::AppCommand>(
                serde_json::to_value(&command).unwrap()
            )
            .unwrap(),
            command
        );
        assert!(
            serde_json::from_value::<super::super::AppCommand>(serde_json::json!({
                "type": "read_routes",
                "payload": {"limit": 7, "cwd": "/Users/someone"}
            }))
            .is_err()
        );

        let event = super::super::AppEvent::RouteAudit(RouteAuditPage {
            entries: vec![sanitize_record(&record("/tmp/project", "persona=Abbey")).unwrap()],
            returned: 1,
            limit: 50,
        });
        let encoded = serde_json::to_value(&event).unwrap();
        assert_eq!(encoded["type"], "route_audit");
        assert_eq!(
            serde_json::from_value::<super::super::AppEvent>(encoded.clone()).unwrap(),
            event
        );
        let mut hostile = encoded;
        hostile["payload"]["cwd"] = serde_json::json!("/tmp/project");
        assert!(serde_json::from_value::<super::super::AppEvent>(hostile).is_err());
    }

    #[test]
    fn the_working_directory_becomes_an_opaque_digest_and_never_a_path() {
        let entry = sanitize_record(&record("/Users/someone/code/abbey", "persona=Abbey")).unwrap();
        let workspace = entry.workspace.clone().expect("a digest");
        assert!(workspace.starts_with("ws-"));
        assert_eq!(
            workspace.len(),
            WORKSPACE_PREFIX.len() + WORKSPACE_DIGEST_HEX
        );
        assert!(workspace[3..].bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(!workspace.chars().any(|c| c.is_ascii_uppercase()));

        // Stable for the same path, different for another.
        assert_eq!(
            sanitize_record(&record("/Users/someone/code/abbey", "x"))
                .unwrap()
                .workspace,
            entry.workspace
        );
        assert_ne!(
            sanitize_record(&record("/Users/someone/code/other", "x"))
                .unwrap()
                .workspace,
            entry.workspace
        );
        // An empty cwd omits the field rather than digesting "".
        assert!(
            sanitize_record(&record("   ", "x"))
                .unwrap()
                .workspace
                .is_none()
        );
    }

    #[test]
    fn free_text_is_bounded_control_stripped_and_path_redacted() {
        let entry = sanitize_record(&record(
            "/tmp/project",
            "persona=Abbey\u{7}\n role=max wrote /etc/passwd and C:\\Windows\\System32 and ~/.ssh/id_rsa",
        ))
        .unwrap();
        assert!(!entry.reason.chars().any(char::is_control));
        assert!(!entry.reason.contains("/etc/passwd"));
        assert!(!entry.reason.contains("C:\\Windows"));
        assert!(!entry.reason.contains("~/.ssh"));
        assert_eq!(entry.reason.matches("[path]").count(), 3);
        assert!(entry.reason.contains("persona=Abbey"));

        // A path hidden behind a `key=` prefix must be caught too — a
        // prefix-only detector missed exactly this shape.
        let keyed =
            sanitize_record(&record("/tmp/p", "stage=gate log=/var/log/abbey.jsonl")).unwrap();
        assert!(!keyed.reason.contains("/var/log"));
        assert!(keyed.reason.contains("stage=gate"));
        assert!(keyed.reason.contains("[path]"));
        keyed.validate().unwrap();

        let long = sanitize_record(&record("/tmp/p", &"x".repeat(4_000))).unwrap();
        assert!(long.reason.len() <= MAX_ROUTE_REASON_BYTES);
        long.validate().unwrap();

        // Multi-byte truncation must land on a character boundary.
        let wide = sanitize_record(&record("/tmp/p", &"日".repeat(400))).unwrap();
        assert!(wide.reason.len() <= MAX_ROUTE_REASON_BYTES);
        wide.validate().unwrap();
    }

    #[test]
    fn confidence_is_quantized_and_clamps_instead_of_wrapping() {
        assert_eq!(quantize_confidence(0.82), 82);
        assert_eq!(quantize_confidence(0.0), 0);
        assert_eq!(quantize_confidence(1.0), 100);
        assert_eq!(quantize_confidence(f32::NAN), 0);
        assert_eq!(quantize_confidence(f32::INFINITY), 0);
        assert_eq!(quantize_confidence(f32::NEG_INFINITY), 0);
        assert_eq!(quantize_confidence(1e30), 100);
        assert_eq!(quantize_confidence(-5.0), 0);
    }

    #[test]
    fn validation_rejects_an_unsanitized_page_from_any_peer() {
        let good = sanitize_record(&record("/tmp/project", "persona=Abbey")).unwrap();
        good.validate().unwrap();

        // The whole point of consumer-side validation: a differently-built
        // daemon cannot push a raw absolute path through the socket.
        for poisoned in [
            RouteAuditEntry {
                reason: "routed in /Users/someone/secret".into(),
                ..good.clone()
            },
            RouteAuditEntry {
                model: "C:\\models\\local".into(),
                ..good.clone()
            },
            RouteAuditEntry {
                persona: "Ab\u{7}bey".into(),
                ..good.clone()
            },
            RouteAuditEntry {
                workspace: Some("/Users/someone/code".into()),
                ..good.clone()
            },
            RouteAuditEntry {
                workspace: Some("ws-NOTHEXAAAAA".into()),
                ..good.clone()
            },
            RouteAuditEntry {
                recorded_at: "yesterday".into(),
                ..good.clone()
            },
            RouteAuditEntry {
                confidence_percent: 101,
                ..good.clone()
            },
            RouteAuditEntry {
                tools: vec!["media".into(); MAX_ROUTE_TOOLS + 1],
                ..good.clone()
            },
        ] {
            assert!(
                poisoned.validate().is_err(),
                "validate accepted {poisoned:?}"
            );
        }

        // Page-level consistency.
        let page = RouteAuditPage {
            entries: vec![good.clone()],
            returned: 1,
            limit: 50,
        };
        page.validate().unwrap();
        for invalid in [
            RouteAuditPage {
                returned: 2,
                ..page.clone()
            },
            RouteAuditPage {
                limit: 0,
                ..page.clone()
            },
            RouteAuditPage {
                limit: MAX_ROUTE_AUDIT_PAGE + 1,
                ..page.clone()
            },
            RouteAuditPage {
                entries: vec![good; 2],
                returned: 2,
                limit: 1,
            },
        ] {
            assert!(invalid.validate().is_err(), "validate accepted {invalid:?}");
        }
    }

    #[test]
    fn a_record_that_cannot_be_represented_honestly_is_dropped() {
        let mut undated = record("/tmp/p", "persona=Abbey");
        undated.ts = "not-a-timestamp".into();
        assert!(sanitize_record(&undated).is_none());

        let mut blank = record("/tmp/p", "persona=Abbey");
        blank.persona = "  \u{7} ".into();
        assert!(sanitize_record(&blank).is_none());

        // A reason that sanitizes to nothing still yields an entry — the
        // decision itself is audit-worthy even when its rationale is not.
        let mut pathy = record("/tmp/p", "/Users/someone/only-a-path");
        pathy.reason = "/Users/someone/only-a-path".into();
        let entry = sanitize_record(&pathy).unwrap();
        assert_eq!(entry.reason, "[path]");
        entry.validate().unwrap();
    }

    #[test]
    fn query_bounds_are_enforced() {
        RouteAuditQuery::default().validate().unwrap();
        assert_eq!(RouteAuditQuery::default().limit, MAX_ROUTE_AUDIT_PAGE);
        assert!(RouteAuditQuery { limit: 0 }.validate().is_err());
        assert!(
            RouteAuditQuery {
                limit: MAX_ROUTE_AUDIT_PAGE + 1
            }
            .validate()
            .is_err()
        );
        // An omitted limit deserializes to the cap, not to zero.
        assert_eq!(
            serde_json::from_value::<RouteAuditQuery>(serde_json::json!({}))
                .unwrap()
                .limit,
            MAX_ROUTE_AUDIT_PAGE
        );
    }
}
