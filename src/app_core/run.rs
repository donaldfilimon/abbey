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
pub const MAX_RUN_EVENT_PAGE: u16 = 16;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    Interactive,
    OneShot,
    Background,
    Automation,
}

/// Closed backend selection; arbitrary executable paths are intentionally absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendSelection {
    Cursor,
    Abi,
    FoundationModels,
    Grok,
}

/// Bounded model request description. This type does not grant execution authority.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    pub idempotency_key: IdempotencyKey,
    pub conversation_id: Option<ConversationId>,
    pub mode: RunMode,
    pub backend: BackendSelection,
    pub input: String,
    pub labels: Vec<String>,
}

impl fmt::Debug for RunRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunRequest")
            .field("idempotency_key", &self.idempotency_key)
            .field("conversation_id", &self.conversation_id)
            .field("mode", &self.mode)
            .field("backend", &self.backend)
            .field("input", &"[REDACTED]")
            .field("input_bytes", &self.input.len())
            .field("label_count", &self.labels.len())
            .finish()
    }
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

/// Identity-only query for one durable run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunQuery {
    pub run_id: RunId,
}

impl RunQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

const fn default_run_event_limit() -> u16 {
    MAX_RUN_EVENT_PAGE
}

/// Bounded, snapshot-consistent query over one run's append-only event ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunEventsQuery {
    pub run_id: RunId,
    /// Exclusive cursor. Zero begins with the one-based first event.
    #[serde(default)]
    pub after_sequence: u64,
    /// Optional fixed high-water mark returned by an earlier page.
    #[serde(default)]
    pub through_sequence: Option<u64>,
    #[serde(default = "default_run_event_limit")]
    pub limit: u16,
}

impl RunEventsQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.limit == 0 || self.limit > MAX_RUN_EVENT_PAGE {
            return Err(ValidationError::new(
                "run event page limit must be from 1 through 16",
            ));
        }
        if self
            .through_sequence
            .is_some_and(|through| self.after_sequence > through)
        {
            return Err(ValidationError::new(
                "run event cursor exceeds its snapshot watermark",
            ));
        }
        Ok(())
    }
}

/// One startup-bound backend and its explicitly supported run modes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRouteCapability {
    pub backend: BackendSelection,
    pub modes: Vec<RunMode>,
}

impl RunRouteCapability {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !matches!(
            self.backend,
            BackendSelection::Abi | BackendSelection::FoundationModels
        ) {
            return Err(ValidationError::new(
                "run route backend is not supported by protocol v2",
            ));
        }
        if self.modes.is_empty() || self.modes.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ValidationError::new(
                "run route modes must be nonempty, strictly ordered, and duplicate-free",
            ));
        }
        if self
            .modes
            .iter()
            .any(|mode| matches!(mode, RunMode::Interactive | RunMode::Automation))
        {
            return Err(ValidationError::new(
                "run route mode is not supported by protocol v2",
            ));
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
    pub conversation_id: Option<ConversationId>,
    pub idempotency_key: IdempotencyKey,
    pub state: RunState,
    pub created_at: String,
    pub updated_at: String,
    pub failure: Option<RunFailure>,
    pub event_count: u64,
}

impl RunSnapshot {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let created = parse_timestamp(&self.created_at)?;
        let updated = parse_timestamp(&self.updated_at)?;
        if updated < created {
            return Err(ValidationError::new("run timestamps are out of order"));
        }

        if self.event_count == 0 {
            return Err(ValidationError::new(
                "run snapshot must contain at least one lifecycle event",
            ));
        }

        if let Some(failure) = &self.failure {
            failure.validate()?;
        }

        let shape_valid = (self.state == RunState::Failed) == self.failure.is_some();
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
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RunLifecycleEvent {
    Queued,
    Starting,
    Running,
    CancelRequested,
    Succeeded,
    Failed { failure: RunFailure },
    Cancelled { reason: RunCancellationReason },
    Interrupted { reason: RunInterruptionReason },
}

impl RunLifecycleEvent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Failed { failure } => failure.validate(),
            Self::Queued
            | Self::Starting
            | Self::Running
            | Self::CancelRequested
            | Self::Succeeded
            | Self::Cancelled { .. }
            | Self::Interrupted { .. } => Ok(()),
        }
    }
}

/// Closed cancellation reason; arbitrary manager text never crosses app-core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunCancellationReason {
    Requested,
    ManagerShutdown,
}

/// Closed interruption reason; arbitrary process/provider details stay internal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunInterruptionReason {
    DaemonRestart,
    ManagerShutdown,
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

/// Whether submission admitted new work or returned an existing durable run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSubmissionDisposition {
    Enqueued,
    Existing,
    QueueFull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSubmission {
    pub disposition: RunSubmissionDisposition,
    pub run: RunSnapshot,
}

impl RunSubmission {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.run.validate()?;
        if self.disposition == RunSubmissionDisposition::QueueFull
            && (self.run.state != RunState::Failed
                || self
                    .run
                    .failure
                    .as_ref()
                    .map(|failure| failure.code.as_str())
                    != Some("queue_full"))
        {
            return Err(ValidationError::new(
                "queue-full submission must contain a failed run",
            ));
        }
        Ok(())
    }
}

/// One bounded, immutable page through a fixed lifecycle high-water mark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunEventPage {
    pub run_id: RunId,
    pub events: Vec<RunEventRecord>,
    pub after_sequence: u64,
    pub next_after_sequence: u64,
    pub through_sequence: u64,
    pub has_more: bool,
}

impl RunEventPage {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.events.len() > usize::from(MAX_RUN_EVENT_PAGE) {
            return Err(ValidationError::new("run event page exceeds 16 records"));
        }
        if self.after_sequence > self.next_after_sequence
            || self.next_after_sequence > self.through_sequence
        {
            return Err(ValidationError::new("run event page cursors are invalid"));
        }

        let mut expected = self
            .after_sequence
            .checked_add(1)
            .ok_or(ValidationError::new("run event sequence overflowed"))?;
        for record in &self.events {
            record.validate()?;
            if record.run_id != self.run_id || record.sequence != expected {
                return Err(ValidationError::new(
                    "run event page is not contiguous for the requested run",
                ));
            }
            expected = expected
                .checked_add(1)
                .ok_or(ValidationError::new("run event sequence overflowed"))?;
        }

        let expected_next = self
            .events
            .last()
            .map_or(self.after_sequence, |record| record.sequence);
        if self.next_after_sequence != expected_next
            || self.has_more != (self.next_after_sequence < self.through_sequence)
            || (self.events.is_empty() && self.has_more)
        {
            return Err(ValidationError::new(
                "run event page continuation metadata is inconsistent",
            ));
        }
        Ok(())
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
#[path = "run_tests.rs"]
mod tests;
