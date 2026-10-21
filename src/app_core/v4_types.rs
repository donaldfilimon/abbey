//! Protocol-v4 payload records. Commands and events stay in `v4`.

use super::ValidationError;
use super::v4::{
    MAX_V4_ITEMS, MAX_V4_LABEL, MAX_V4_PAGE, MAX_V4_TEXT, V4PageQuery, validate_conversation_id,
    validate_id, validate_text,
};
use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_V4_SHORT_TEXT: usize = 1024;
const MAX_V4_CRON: usize = 128;
const MAX_V4_TIMEZONE: usize = 64;
const MAX_V4_ROOT: usize = 512;
const MAX_V4_DOMAIN: usize = 253;
const MAX_V4_BUDGET: u32 = 100_000;
const MAX_V4_RUNTIME_MS: u64 = 3_600_000;
const MAX_V4_OUTPUT_BYTES: u64 = 1_048_576;

/// New conversation title. This request carries no message body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4ConversationCreate {
    pub title: String,
}

impl V4ConversationCreate {
    /// Validate a trimmed, control-free title.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text(&self.title, MAX_V4_LABEL, "invalid v4 title")
    }
}

/// Rename one conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4ConversationRename {
    pub conversation_id: String,
    pub title: String,
}

impl V4ConversationRename {
    /// Validate the conversation identifier and replacement title.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_conversation_id(&self.conversation_id)?;
        validate_text(&self.title, MAX_V4_LABEL, "invalid v4 title")
    }
}

/// Conversation summary. This record has no message body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4Conversation {
    pub id: String,
    pub title: String,
    pub archived: bool,
    pub pinned: bool,
    /// False when a legacy identity was listed without imported message bodies.
    pub imported_history: bool,
    pub sequence: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub message_count: u64,
}

impl V4Conversation {
    /// Validate identity, title, and timestamp order.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_conversation_id(&self.id)?;
        validate_text(&self.title, MAX_V4_LABEL, "invalid v4 title")?;
        if self.sequence == 0 || self.updated_at_ms < self.created_at_ms {
            return Err(ValidationError::new("invalid v4 conversation timestamp"));
        }
        Ok(())
    }
}

/// Fixed-watermark page of conversation summaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4ConversationPage {
    pub after: u64,
    pub through: u64,
    pub conversations: Vec<V4Conversation>,
}

impl V4ConversationPage {
    /// Validate page bounds and each summary.
    pub fn validate(&self) -> Result<(), ValidationError> {
        bounded_page(
            self.after,
            self.through,
            self.conversations.len(),
            "invalid v4 conversation page",
        )?;
        self.conversations
            .iter()
            .try_for_each(V4Conversation::validate)
    }
}

/// Title search. Matches titles only; this query has no message body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4SearchQuery {
    pub query: String,
    pub page: V4PageQuery,
}

impl V4SearchQuery {
    /// Validate the bounded title query and page.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text(&self.query, MAX_V4_SHORT_TEXT, "invalid v4 search query")?;
        self.page.validate()
    }
}

/// Message page query for one conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4MessageQuery {
    pub conversation_id: String,
    pub page: V4PageQuery,
}

impl V4MessageQuery {
    /// Validate the conversation identifier and page.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_conversation_id(&self.conversation_id)?;
        self.page.validate()
    }
}

/// Role of one conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

/// Authorized conversation content. Do not place this body on an audit record.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4Message {
    pub id: String,
    pub conversation_id: String,
    pub role: V4MessageRole,
    pub sequence: u64,
    pub body: String,
}

impl fmt::Debug for V4Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("V4Message")
            .field("id", &self.id)
            .field("conversation_id", &self.conversation_id)
            .field("role", &self.role)
            .field("sequence", &self.sequence)
            .field("body", &"[redacted]")
            .finish()
    }
}

impl V4Message {
    /// Validate identifiers and the role-bounded body.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.id)?;
        validate_conversation_id(&self.conversation_id)?;
        loose_text(&self.body, self.role == V4MessageRole::Tool)
    }
}

/// Fixed-watermark page of authorized conversation messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4MessagePage {
    pub conversation_id: String,
    pub after: u64,
    pub through: u64,
    pub messages: Vec<V4Message>,
}

impl V4MessagePage {
    /// Validate bounds, identity, and strictly increasing sequences.
    pub fn validate(&self) -> Result<(), ValidationError> {
        bounded_page(
            self.after,
            self.through,
            self.messages.len(),
            "invalid v4 message page",
        )?;
        validate_conversation_id(&self.conversation_id)?;
        let mut previous = self.after;
        for message in &self.messages {
            message.validate()?;
            if message.conversation_id != self.conversation_id
                || message.sequence <= previous
                || message.sequence > self.through
            {
                return Err(ValidationError::new("invalid v4 message page"));
            }
            previous = message.sequence;
        }
        Ok(())
    }
}

/// One turn submission. The prompt is the only caller-supplied text.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4TurnSubmit {
    pub conversation_id: String,
    pub request_id: String,
    pub route_id: String,
    pub permission_profile_id: String,
    pub prompt: String,
}

impl fmt::Debug for V4TurnSubmit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("V4TurnSubmit")
            .field("conversation_id", &self.conversation_id)
            .field("request_id", &self.request_id)
            .field("route_id", &self.route_id)
            .field("permission_profile_id", &self.permission_profile_id)
            .field("prompt", &"[redacted]")
            .finish()
    }
}

impl V4TurnSubmit {
    /// Validate identifiers and a non-empty bounded prompt.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_conversation_id(&self.conversation_id)?;
        validate_id(&self.request_id)?;
        validate_id(&self.route_id)?;
        validate_id(&self.permission_profile_id)?;
        loose_text(&self.prompt, false)
    }
}

/// Lifecycle of an accepted turn. This is not transcript text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4TurnState {
    Queued,
    Running,
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}

/// Acceptance record with no prompt and no assistant text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4TurnReceipt {
    pub conversation_id: String,
    pub request_id: String,
    pub message_id: String,
    pub state: V4TurnState,
}

impl V4TurnReceipt {
    /// Validate the correlated identifiers.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_conversation_id(&self.conversation_id)?;
        validate_id(&self.request_id)?;
        validate_id(&self.message_id)
    }
}

/// Kind of one content-progress event. None of these carry text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4ContentKind {
    Queued,
    Started,
    OutputBounded,
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}

/// Bounded content progress. `byte_count` is a count only; this record has no text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4ContentEvent {
    pub sequence: u64,
    pub conversation_id: String,
    pub request_id: String,
    pub kind: V4ContentKind,
    pub byte_count: u64,
}

impl V4ContentEvent {
    /// Validate the correlated identifiers.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_conversation_id(&self.conversation_id)?;
        validate_id(&self.request_id)
    }
}

/// Fixed-watermark page of text-free content events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4ContentEventPage {
    pub conversation_id: String,
    pub after: u64,
    pub through: u64,
    pub events: Vec<V4ContentEvent>,
}

impl V4ContentEventPage {
    /// Validate bounds, identity, and strictly increasing sequences.
    pub fn validate(&self) -> Result<(), ValidationError> {
        bounded_page(
            self.after,
            self.through,
            self.events.len(),
            "invalid v4 content event page",
        )?;
        validate_conversation_id(&self.conversation_id)?;
        let mut previous = self.after;
        for event in &self.events {
            event.validate()?;
            if event.conversation_id != self.conversation_id
                || event.sequence <= previous
                || event.sequence > self.through
            {
                return Err(ValidationError::new("invalid v4 content event page"));
            }
            previous = event.sequence;
        }
        Ok(())
    }
}

/// Content-event query for one conversation. It does not select text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4EventQuery {
    pub conversation_id: String,
    pub page: V4PageQuery,
}

impl V4EventQuery {
    /// Validate the conversation identifier and page.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_conversation_id(&self.conversation_id)?;
        self.page.validate()
    }
}

/// Lifecycle of one bounded task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4TaskState {
    Queued,
    Running,
    AwaitingApproval,
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}

/// Request to create one bounded task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4TaskCreate {
    pub task_id: String,
    pub conversation_id: String,
    pub goal: String,
    pub route_id: String,
    pub permission_profile_id: String,
    pub budget_tokens: u32,
}

impl V4TaskCreate {
    /// Validate identities, the goal, and the token budget.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_task_fields(
            &self.task_id,
            &self.conversation_id,
            &self.goal,
            &self.route_id,
            &self.permission_profile_id,
            self.budget_tokens,
        )
    }
}

/// Bounded task record. The goal is a label, not a transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4Task {
    pub task_id: String,
    pub conversation_id: String,
    pub goal: String,
    pub route_id: String,
    pub permission_profile_id: String,
    pub state: V4TaskState,
    pub budget_tokens: u32,
    pub attempt: u32,
}

impl V4Task {
    /// Validate identities, the goal, the budget, and a positive attempt.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_task_fields(
            &self.task_id,
            &self.conversation_id,
            &self.goal,
            &self.route_id,
            &self.permission_profile_id,
            self.budget_tokens,
        )?;
        if self.attempt == 0 {
            return Err(ValidationError::new("invalid v4 task attempt"));
        }
        Ok(())
    }
}

/// Fixed-watermark page of tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4TaskPage {
    pub after: u64,
    pub through: u64,
    pub tasks: Vec<V4Task>,
}

impl V4TaskPage {
    /// Validate page bounds and each task.
    pub fn validate(&self) -> Result<(), ValidationError> {
        bounded_page(
            self.after,
            self.through,
            self.tasks.len(),
            "invalid v4 task page",
        )?;
        self.tasks.iter().try_for_each(V4Task::validate)
    }
}

/// When an automation may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4ScheduleKind {
    Once,
    Hourly,
    Daily,
    Weekly,
    Cron,
}

/// Create or replace one automation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4AutomationUpsert {
    pub automation_id: String,
    pub instruction: String,
    pub schedule: V4ScheduleKind,
    pub cron: Option<String>,
    pub timezone: String,
    pub route_id: String,
    pub permission_profile_id: String,
    pub budget_tokens: u32,
    pub enabled: bool,
}

impl V4AutomationUpsert {
    /// Validate the schedule, instruction, and bounded budget.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.automation_id)?;
        validate_text(
            &self.instruction,
            MAX_V4_SHORT_TEXT,
            "invalid v4 instruction",
        )?;
        validate_schedule(self.schedule, &self.cron)?;
        validate_timezone(&self.timezone)?;
        validate_id(&self.route_id)?;
        validate_id(&self.permission_profile_id)?;
        budget(self.budget_tokens)
    }
}

/// Stored automation plus its next projected run, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4Automation {
    pub automation_id: String,
    pub instruction: String,
    pub schedule: V4ScheduleKind,
    pub cron: Option<String>,
    pub timezone: String,
    pub route_id: String,
    pub permission_profile_id: String,
    pub budget_tokens: u32,
    pub enabled: bool,
    pub next_run_at_ms: Option<u64>,
}

impl V4Automation {
    /// Validate the same bounds as an upsert. The next run is unconstrained.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.automation_id)?;
        validate_text(
            &self.instruction,
            MAX_V4_SHORT_TEXT,
            "invalid v4 instruction",
        )?;
        validate_schedule(self.schedule, &self.cron)?;
        validate_timezone(&self.timezone)?;
        validate_id(&self.route_id)?;
        validate_id(&self.permission_profile_id)?;
        budget(self.budget_tokens)
    }
}

/// Fixed-watermark page of automations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4AutomationPage {
    pub after: u64,
    pub through: u64,
    pub automations: Vec<V4Automation>,
}

impl V4AutomationPage {
    /// Validate page bounds and each automation.
    pub fn validate(&self) -> Result<(), ValidationError> {
        bounded_page(
            self.after,
            self.through,
            self.automations.len(),
            "invalid v4 automation page",
        )?;
        self.automations.iter().try_for_each(V4Automation::validate)
    }
}

/// Enable or disable one automation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4AutomationToggle {
    pub automation_id: String,
    pub enabled: bool,
}

impl V4AutomationToggle {
    /// Validate the automation identifier.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.automation_id)
    }
}

/// Bounded permission profile. Lists name authority; they do not execute it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4PermissionProfile {
    pub profile_id: String,
    pub tool_ids: Vec<String>,
    pub application_ids: Vec<String>,
    pub filesystem_roots: Vec<String>,
    pub browser_domains: Vec<String>,
    pub max_runtime_ms: u64,
    pub max_output_bytes: u64,
}

impl V4PermissionProfile {
    /// Validate bounded identifiers, absolute roots, hostnames, and ceilings.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.profile_id)?;
        validate_id_list(&self.tool_ids)?;
        validate_id_list(&self.application_ids)?;
        validate_roots(&self.filesystem_roots)?;
        validate_domains(&self.browser_domains)?;
        positive_at_most(
            self.max_runtime_ms,
            MAX_V4_RUNTIME_MS,
            "invalid v4 runtime limit",
        )?;
        positive_at_most(
            self.max_output_bytes,
            MAX_V4_OUTPUT_BYTES,
            "invalid v4 output limit",
        )
    }
}

/// Decision applied to one pending action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4ActionDecisionKind {
    Approve,
    Deny,
}

/// Action decision. This record does not carry a tool payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4ActionDecision {
    pub action_id: String,
    pub decision_id: String,
    pub kind: V4ActionDecisionKind,
}

impl V4ActionDecision {
    /// Validate the action and decision identifiers.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.action_id)?;
        validate_id(&self.decision_id)
    }
}

fn bounded_page(
    after: u64,
    through: u64,
    len: usize,
    message: &'static str,
) -> Result<(), ValidationError> {
    if after > through || len > usize::from(MAX_V4_PAGE) {
        return Err(ValidationError::new(message));
    }
    Ok(())
}

fn loose_text(value: &str, allow_empty: bool) -> Result<(), ValidationError> {
    if value.len() > MAX_V4_TEXT || (!allow_empty && value.is_empty()) {
        return Err(ValidationError::new("invalid v4 text"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
    {
        return Err(ValidationError::new("invalid v4 text"));
    }
    Ok(())
}

fn budget(tokens: u32) -> Result<(), ValidationError> {
    if (1..=MAX_V4_BUDGET).contains(&tokens) {
        Ok(())
    } else {
        Err(ValidationError::new("invalid v4 token budget"))
    }
}

fn validate_task_fields(
    task_id: &str,
    conversation_id: &str,
    goal: &str,
    route_id: &str,
    permission_profile_id: &str,
    budget_tokens: u32,
) -> Result<(), ValidationError> {
    validate_id(task_id)?;
    validate_conversation_id(conversation_id)?;
    validate_text(goal, MAX_V4_SHORT_TEXT, "invalid v4 goal")?;
    validate_id(route_id)?;
    validate_id(permission_profile_id)?;
    budget(budget_tokens)
}

fn validate_schedule(
    schedule: V4ScheduleKind,
    cron: &Option<String>,
) -> Result<(), ValidationError> {
    match (schedule, cron) {
        (V4ScheduleKind::Cron, Some(expression)) => {
            validate_text(expression, MAX_V4_CRON, "invalid v4 cron")
        }
        (V4ScheduleKind::Cron, None) => Err(ValidationError::new(
            "v4 cron schedule requires an expression",
        )),
        (_, Some(_)) => Err(ValidationError::new(
            "v4 cron expression is only valid for a cron schedule",
        )),
        (_, None) => Ok(()),
    }
}

fn validate_timezone(value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > MAX_V4_TIMEZONE
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'+' | b'-'))
    {
        return Err(ValidationError::new("invalid v4 timezone"));
    }
    Ok(())
}

fn positive_at_most(value: u64, max: u64, message: &'static str) -> Result<(), ValidationError> {
    if value == 0 || value > max {
        Err(ValidationError::new(message))
    } else {
        Ok(())
    }
}

fn validate_id_list(values: &[String]) -> Result<(), ValidationError> {
    if values.len() > MAX_V4_ITEMS {
        return Err(ValidationError::new("invalid v4 permission profile"));
    }
    values.iter().try_for_each(|value| validate_id(value))
}

fn validate_roots(values: &[String]) -> Result<(), ValidationError> {
    if values.len() > MAX_V4_ITEMS {
        return Err(ValidationError::new("invalid v4 permission profile"));
    }
    for value in values {
        if value.is_empty()
            || value.len() > MAX_V4_ROOT
            || value.chars().any(char::is_control)
            || !absolute_root(value)
        {
            return Err(ValidationError::new("invalid v4 filesystem root"));
        }
    }
    Ok(())
}

fn absolute_root(value: &str) -> bool {
    if value.starts_with('/') {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn validate_domains(values: &[String]) -> Result<(), ValidationError> {
    if values.len() > MAX_V4_ITEMS {
        return Err(ValidationError::new("invalid v4 permission profile"));
    }
    for value in values {
        if value.is_empty()
            || value.len() > MAX_V4_DOMAIN
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        {
            return Err(ValidationError::new("invalid v4 browser domain"));
        }
    }
    Ok(())
}
