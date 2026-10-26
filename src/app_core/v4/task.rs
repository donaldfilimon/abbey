//! Schema 3 bounded records. Data references never grant execution authority.
use super::*;

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4TaskCreate {
    pub operation_id: IdempotencyKey,
    pub conversation_id: ConversationId,
    pub goal: String,
    pub workspace_id: String,
    pub route_id: String,
    pub permission_profile_id: String,
    pub profile_revision: u64,
    pub budget_tokens: u32,
    pub execution_kind: V4TaskExecutionKind,
}
impl V4TaskCreate {
    pub fn validate(&self) -> Result<(), ValidationError> {
        text(&self.goal, 1024)?;
        route(&self.workspace_id)?;
        route(&self.route_id)?;
        route(&self.permission_profile_id)?;
        positive(self.profile_revision)?;
        budget(self.budget_tokens)
    }
}
validated_admin!(V4TaskCreate {
    operation_id: IdempotencyKey,
    conversation_id: ConversationId,
    goal: String,
    workspace_id: String,
    route_id: String,
    permission_profile_id: String,
    profile_revision: u64,
    budget_tokens: u32,
    execution_kind: V4TaskExecutionKind
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4TaskAttempt {
    pub attempt_id: String,
    pub run_id: RunId,
    pub ordinal: u32,
    pub state: RunState,
    pub admitted_at_ms: u64,
    pub finished_at_ms: Option<u64>,
}
impl V4TaskAttempt {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.attempt_id)?;
        ensure(self.ordinal > 0)?;
        timestamp(self.admitted_at_ms)?;
        ensure(self.state.is_terminal() == self.finished_at_ms.is_some())?;
        if let Some(t) = self.finished_at_ms {
            timestamp(t)?;
            ensure(t >= self.admitted_at_ms)?;
        }
        Ok(())
    }
}
validated_admin!(V4TaskAttempt { attempt_id: String, run_id: RunId, ordinal: u32, state: RunState, admitted_at_ms: u64, finished_at_ms: Option<u64> });

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4Task {
    pub task_id: String,
    pub revision: u64,
    pub sequence: u64,
    pub conversation_id: ConversationId,
    pub goal: String,
    pub workspace_id: String,
    pub route_id: String,
    pub permission_profile_id: String,
    pub profile_revision: u64,
    pub budget_tokens: u32,
    pub execution_kind: V4TaskExecutionKind,
    pub state: V4TaskState,
    pub current_attempt: V4TaskAttempt,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}
impl V4Task {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.task_id)?;
        positive(self.revision)?;
        positive(self.sequence)?;
        text(&self.goal, 1024)?;
        route(&self.workspace_id)?;
        route(&self.route_id)?;
        route(&self.permission_profile_id)?;
        positive(self.profile_revision)?;
        budget(self.budget_tokens)?;
        self.current_attempt.validate()?;
        timestamp(self.created_at_ms)?;
        timestamp(self.updated_at_ms)?;
        ensure(
            self.updated_at_ms >= self.created_at_ms
                && self.current_attempt.admitted_at_ms >= self.created_at_ms
                && self.current_attempt.admitted_at_ms <= self.updated_at_ms
                && self
                    .current_attempt
                    .finished_at_ms
                    .is_none_or(|t| t <= self.updated_at_ms),
        )?;
        ensure(self.state.matches_run(self.current_attempt.state))
    }
}
validated_admin!(V4Task {
    task_id: String,
    revision: u64,
    sequence: u64,
    conversation_id: ConversationId,
    goal: String,
    workspace_id: String,
    route_id: String,
    permission_profile_id: String,
    profile_revision: u64,
    budget_tokens: u32,
    execution_kind: V4TaskExecutionKind,
    state: V4TaskState,
    current_attempt: V4TaskAttempt,
    created_at_ms: u64,
    updated_at_ms: u64
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4TaskPage {
    pub after: u64,
    pub through: u64,
    pub tasks: Vec<V4Task>,
}
impl V4TaskPage {
    pub fn validate(&self) -> Result<(), ValidationError> {
        page(self.after, self.through, self.tasks.len())?;
        let mut last = self.after;
        let mut ids = std::collections::BTreeSet::new();
        for t in &self.tasks {
            t.validate()?;
            ensure(t.sequence > last && t.sequence <= self.through && ids.insert(&t.task_id))?;
            last = t.sequence;
        }
        admin_bytes(self)
    }
}
validated_admin!(V4TaskPage { after: u64, through: u64, tasks: Vec<V4Task> });

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4TaskMutation {
    pub task_id: String,
    pub expected_revision: u64,
    pub operation_id: IdempotencyKey,
}
impl V4TaskMutation {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.task_id)?;
        positive(self.expected_revision)
    }
}
validated_admin!(V4TaskMutation {
    task_id: String,
    expected_revision: u64,
    operation_id: IdempotencyKey
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4TaskRetry {
    pub task_id: String,
    pub expected_revision: u64,
    pub expected_attempt_id: String,
    pub operation_id: IdempotencyKey,
}
impl V4TaskRetry {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.task_id)?;
        route(&self.expected_attempt_id)?;
        positive(self.expected_revision)
    }
}
validated_admin!(V4TaskRetry {
    task_id: String,
    expected_revision: u64,
    expected_attempt_id: String,
    operation_id: IdempotencyKey
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4TaskEventQuery {
    pub task_id: String,
    pub page: V4PageQuery,
}
impl V4TaskEventQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.task_id)?;
        self.page.validate()
    }
}
validated_admin!(V4TaskEventQuery {
    task_id: String,
    page: V4PageQuery
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4TaskEvent {
    pub task_id: String,
    pub revision: u64,
    pub sequence: u64,
    pub attempt_id: String,
    pub run_id: RunId,
    pub timestamp_ms: u64,
    pub kind: V4TaskEventKind,
}
impl V4TaskEvent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.task_id)?;
        route(&self.attempt_id)?;
        positive(self.revision)?;
        positive(self.sequence)?;
        timestamp(self.timestamp_ms)?;
        self.kind.validate()
    }
}
validated_admin!(V4TaskEvent {
    task_id: String,
    revision: u64,
    sequence: u64,
    attempt_id: String,
    run_id: RunId,
    timestamp_ms: u64,
    kind: V4TaskEventKind
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4TaskEventPage {
    pub task_id: String,
    pub after: u64,
    pub through: u64,
    pub events: Vec<V4TaskEvent>,
}
impl V4TaskEventPage {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.task_id)?;
        page(self.after, self.through, self.events.len())?;
        let mut last = self.after;
        for e in &self.events {
            e.validate()?;
            ensure(e.task_id == self.task_id && e.sequence > last && e.sequence <= self.through)?;
            last = e.sequence;
        }
        admin_bytes(self)
    }
}
validated_admin!(V4TaskEventPage { task_id: String, after: u64, through: u64, events: Vec<V4TaskEvent> });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4TaskExecutionKind {
    ReadOnly,
    Governed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4TaskState {
    Queued,
    Starting,
    Running,
    AwaitingApproval,
    Paused,
    Succeeded,
    Failed,
    Cancelled,
    CancelRequested,
    Interrupted,
}
impl V4TaskState {
    pub(super) fn matches_run(self, run: RunState) -> bool {
        match self {
            Self::Queued => run == RunState::Queued,
            Self::Starting => run == RunState::Starting,
            Self::Running | Self::AwaitingApproval | Self::Paused => run == RunState::Running,
            Self::Succeeded => run == RunState::Succeeded,
            Self::Failed => run == RunState::Failed,
            Self::Cancelled => run == RunState::Cancelled,
            Self::CancelRequested => run == RunState::CancelRequested,
            Self::Interrupted => run == RunState::Interrupted,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4TurnState {
    Queued,
    Starting,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    CancelRequested,
    Interrupted,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum V4TaskEventKind {
    StateChanged {
        state: V4TaskState,
    },
    ActionChanged {
        action_id: String,
        state: V4ActionState,
    },
    ArtifactAvailable {
        artifact: V4ArtifactReference,
    },
}
impl V4TaskEventKind {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::StateChanged { .. } => Ok(()),
            Self::ActionChanged { action_id, .. } => route(action_id),
            Self::ArtifactAvailable { artifact } => artifact.validate(),
        }
    }
}
impl std::fmt::Debug for V4TaskEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("V4TaskEventKind([redacted])")
    }
}
