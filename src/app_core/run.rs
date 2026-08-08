//! Validated run-lifecycle contracts without execution authority.

use super::{ConversationId, RunId, ValidationError};
use chrono::DateTime;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{fmt, str::FromStr};
use uuid::Uuid;

const MAX_INPUT_BYTES: usize = 32 * 1_024;
const MAX_LABELS: usize = 16;
const MAX_LABEL_BYTES: usize = 64;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;
const MAX_FAILURE_CODE_BYTES: usize = 64;
const MAX_FAILURE_MESSAGE_BYTES: usize = 2_048;
const MAX_CONVERSATION_TITLE_BYTES: usize = 256;
const MAX_TIMESTAMP_BYTES: usize = 64;

/// Caller-provided retry identity. It is data identity, not authorization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Generate a collision-resistant key suitable for a new local request.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for IdempotencyKey {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for IdempotencyKey {
    type Err = ValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate_identifier(value, MAX_IDEMPOTENCY_KEY_BYTES, "invalid idempotency key")?;
        Ok(Self(value.to_owned()))
    }
}

impl Serialize for IdempotencyKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for IdempotencyKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Durable lifecycle state for one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Starting,
    Running,
    CancelRequested,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl RunState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    /// Enforce the lifecycle graph. Terminal states are immutable.
    pub fn validate_transition(self, next: Self) -> Result<(), ValidationError> {
        let valid = match self {
            Self::Queued => matches!(
                next,
                Self::Starting
                    | Self::CancelRequested
                    | Self::Cancelled
                    | Self::Failed
                    | Self::Interrupted
            ),
            Self::Starting => matches!(
                next,
                Self::Running
                    | Self::CancelRequested
                    | Self::Cancelled
                    | Self::Failed
                    | Self::Interrupted
            ),
            Self::Running => matches!(
                next,
                Self::CancelRequested
                    | Self::Succeeded
                    | Self::Failed
                    | Self::Cancelled
                    | Self::Interrupted
            ),
            Self::CancelRequested => matches!(
                next,
                Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
            ),
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted => false,
        };
        if valid {
            Ok(())
        } else {
            Err(ValidationError::new("invalid run state transition"))
        }
    }
}

/// User-visible scheduling mode. It carries no executable configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    Interactive,
    OneShot,
    Background,
    Automation,
}

/// Closed backend selection; arbitrary executable paths are intentionally absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendSelection {
    Cursor,
    Abi,
    FoundationModels,
    Grok,
}

/// Bounded model request description. This type does not grant execution authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    pub idempotency_key: IdempotencyKey,
    pub conversation_id: Option<ConversationId>,
    pub mode: RunMode,
    pub backend: BackendSelection,
    pub input: String,
    pub labels: Vec<String>,
}

impl RunRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text(&self.input, MAX_INPUT_BYTES, "invalid run input")?;
        if self.labels.len() > MAX_LABELS {
            return Err(ValidationError::new("run request has too many labels"));
        }
        for label in &self.labels {
            validate_identifier(label, MAX_LABEL_BYTES, "invalid run label")?;
        }
        Ok(())
    }
}

/// Stable failure data suitable for persistence and presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl RunFailure {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_identifier(
            &self.code,
            MAX_FAILURE_CODE_BYTES,
            "invalid run failure code",
        )?;
        validate_text(
            &self.message,
            MAX_FAILURE_MESSAGE_BYTES,
            "invalid run failure message",
        )
    }
}

/// Persistable summary of a run. User input and provider output are not echoed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSnapshot {
    pub run_id: RunId,
    pub conversation_id: ConversationId,
    pub idempotency_key: IdempotencyKey,
    pub mode: RunMode,
    pub backend: BackendSelection,
    pub state: RunState,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub failure: Option<RunFailure>,
    pub event_count: u64,
}

impl RunSnapshot {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let created = parse_timestamp(&self.created_at)?;
        let started = self
            .started_at
            .as_deref()
            .map(parse_timestamp)
            .transpose()?;
        let finished = self
            .finished_at
            .as_deref()
            .map(parse_timestamp)
            .transpose()?;

        if started.is_some_and(|value| value < created)
            || finished.is_some_and(|value| value < started.unwrap_or(created))
        {
            return Err(ValidationError::new("run timestamps are out of order"));
        }

        if let Some(failure) = &self.failure {
            failure.validate()?;
        }

        let shape_valid = match self.state {
            RunState::Queued => {
                self.started_at.is_none() && self.finished_at.is_none() && self.failure.is_none()
            }
            RunState::Starting | RunState::Running => {
                self.started_at.is_some() && self.finished_at.is_none() && self.failure.is_none()
            }
            RunState::CancelRequested => self.finished_at.is_none() && self.failure.is_none(),
            RunState::Succeeded => {
                self.started_at.is_some() && self.finished_at.is_some() && self.failure.is_none()
            }
            RunState::Failed => self.finished_at.is_some() && self.failure.is_some(),
            RunState::Cancelled | RunState::Interrupted => {
                self.finished_at.is_some() && self.failure.is_none()
            }
        };
        if !shape_valid {
            return Err(ValidationError::new(
                "run snapshot fields do not match its state",
            ));
        }
        Ok(())
    }
}

/// One append-only lifecycle fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum RunLifecycleEvent {
    Accepted { request: RunRequest },
    StateChanged { from: RunState, to: RunState },
    FailureRecorded { failure: RunFailure },
}

impl RunLifecycleEvent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Accepted { request } => request.validate(),
            Self::StateChanged { from, to } => from.validate_transition(*to),
            Self::FailureRecorded { failure } => failure.validate(),
        }
    }
}

/// Sequenced event persisted for one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunEventRecord {
    pub run_id: RunId,
    pub sequence: u64,
    pub recorded_at: String,
    pub event: RunLifecycleEvent,
}

impl RunEventRecord {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.sequence == 0 {
            return Err(ValidationError::new("run event sequence must start at one"));
        }
        parse_timestamp(&self.recorded_at)?;
        self.event.validate()
    }
}

/// Minimal durable conversation metadata shared by all presentations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationMetadata {
    pub conversation_id: ConversationId,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub run_count: u64,
}

impl ConversationMetadata {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let created = parse_timestamp(&self.created_at)?;
        let updated = parse_timestamp(&self.updated_at)?;
        if updated < created {
            return Err(ValidationError::new(
                "conversation timestamps are out of order",
            ));
        }
        if let Some(title) = &self.title {
            validate_text(
                title,
                MAX_CONVERSATION_TITLE_BYTES,
                "invalid conversation title",
            )?;
        }
        Ok(())
    }
}

fn validate_text(
    value: &str,
    max_bytes: usize,
    message: &'static str,
) -> Result<(), ValidationError> {
    if value.trim().is_empty()
        || value.len() > max_bytes
        || value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(ValidationError::new(message));
    }
    Ok(())
}

fn validate_identifier(
    value: &str,
    max_bytes: usize,
    message: &'static str,
) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > max_bytes
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(ValidationError::new(message));
    }
    Ok(())
}

fn parse_timestamp(value: &str) -> Result<DateTime<chrono::FixedOffset>, ValidationError> {
    if value.is_empty() || value.len() > MAX_TIMESTAMP_BYTES {
        return Err(ValidationError::new("invalid RFC 3339 timestamp"));
    }
    DateTime::parse_from_rfc3339(value)
        .map_err(|_| ValidationError::new("invalid RFC 3339 timestamp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_core::{
        AppCapability, AppCommand, AppEvent, CapabilitySet, ClaimsQuery, ClaimsSnapshot,
    };

    fn request() -> RunRequest {
        RunRequest {
            idempotency_key: "client:request-1".parse().unwrap(),
            conversation_id: Some(ConversationId::new()),
            mode: RunMode::Background,
            backend: BackendSelection::Abi,
            input: "Summarize the checked workspace.".into(),
            labels: vec!["workspace".into(), "summary".into()],
        }
    }

    #[test]
    fn protocol_v1_read_only_fixtures_remain_exact() {
        assert_eq!(
            serde_json::to_value(AppCommand::Claims(ClaimsQuery::default())).unwrap(),
            serde_json::json!({
                "type": "claims",
                "payload": {"status": null, "contains": null}
            })
        );
        assert_eq!(
            serde_json::to_value(AppEvent::Claims(ClaimsSnapshot {
                claims: Vec::new(),
                matched: 0,
            }))
            .unwrap(),
            serde_json::json!({
                "type": "claims",
                "payload": {"claims": [], "matched": 0}
            })
        );
        assert_eq!(
            CapabilitySet::standard().as_slice(),
            &[AppCapability::ReadStatus, AppCapability::ReadClaims]
        );
    }

    #[test]
    fn idempotency_keys_are_bounded_and_wire_validated() {
        let key: IdempotencyKey = "client:request-1".parse().unwrap();
        assert_eq!(serde_json::to_string(&key).unwrap(), "\"client:request-1\"");
        assert!("".parse::<IdempotencyKey>().is_err());
        assert!("contains space".parse::<IdempotencyKey>().is_err());
        assert!(
            serde_json::from_value::<IdempotencyKey>(serde_json::json!("x".repeat(129))).is_err()
        );
    }

    #[test]
    fn requests_bound_input_labels_and_unknown_fields() {
        request().validate().unwrap();
        let mut oversized = request();
        oversized.input = "x".repeat(MAX_INPUT_BYTES + 1);
        assert!(oversized.validate().is_err());

        let mut too_many_labels = request();
        too_many_labels.labels = vec!["label".into(); MAX_LABELS + 1];
        assert!(too_many_labels.validate().is_err());

        let mut json = serde_json::to_value(request()).unwrap();
        json.as_object_mut()
            .unwrap()
            .insert("argv".into(), serde_json::json!(["sh", "-c"]));
        assert!(serde_json::from_value::<RunRequest>(json).is_err());
    }

    #[test]
    fn lifecycle_rejects_skips_repeats_and_terminal_mutation() {
        RunState::Queued
            .validate_transition(RunState::Starting)
            .unwrap();
        RunState::Starting
            .validate_transition(RunState::Running)
            .unwrap();
        RunState::Running
            .validate_transition(RunState::Succeeded)
            .unwrap();
        assert!(
            RunState::Queued
                .validate_transition(RunState::Running)
                .is_err()
        );
        assert!(
            RunState::Running
                .validate_transition(RunState::Running)
                .is_err()
        );
        assert!(
            RunState::Succeeded
                .validate_transition(RunState::Running)
                .is_err()
        );
    }

    #[test]
    fn snapshots_require_state_consistent_monotonic_timestamps() {
        let snapshot = RunSnapshot {
            run_id: RunId::new(),
            conversation_id: ConversationId::new(),
            idempotency_key: IdempotencyKey::new(),
            mode: RunMode::OneShot,
            backend: BackendSelection::Cursor,
            state: RunState::Succeeded,
            created_at: "2026-08-08T12:00:00Z".into(),
            started_at: Some("2026-08-08T12:00:01Z".into()),
            finished_at: Some("2026-08-08T12:00:02Z".into()),
            failure: None,
            event_count: 4,
        };
        snapshot.validate().unwrap();

        let mut invalid = snapshot.clone();
        invalid.failure = Some(RunFailure {
            code: "provider-error".into(),
            message: "provider rejected request".into(),
            retryable: false,
        });
        assert!(invalid.validate().is_err());

        invalid.failure = None;
        invalid.finished_at = Some("2026-08-08T11:59:59Z".into());
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn event_records_validate_sequence_timestamp_and_transition() {
        let record = RunEventRecord {
            run_id: RunId::new(),
            sequence: 2,
            recorded_at: "2026-08-08T12:00:01Z".into(),
            event: RunLifecycleEvent::StateChanged {
                from: RunState::Starting,
                to: RunState::Running,
            },
        };
        record.validate().unwrap();

        let mut invalid = record;
        invalid.sequence = 0;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn conversation_metadata_is_bounded_and_monotonic() {
        let mut metadata = ConversationMetadata {
            conversation_id: ConversationId::new(),
            title: Some("Abbey completion".into()),
            created_at: "2026-08-08T12:00:00Z".into(),
            updated_at: "2026-08-08T12:00:01Z".into(),
            run_count: 1,
        };
        metadata.validate().unwrap();
        metadata.updated_at = "2026-08-08T11:59:59Z".into();
        assert!(metadata.validate().is_err());
    }
}
