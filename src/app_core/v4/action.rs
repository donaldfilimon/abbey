//! Schema 3 bounded records. Data references never grant execution authority.
use super::*;

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ActionQuery {
    pub task_id: String,
    pub attempt_id: String,
    pub action_id: String,
}
impl V4ActionQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.task_id)?;
        route(&self.attempt_id)?;
        route(&self.action_id)
    }
}
validated_admin!(V4ActionQuery {
    task_id: String,
    attempt_id: String,
    action_id: String
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4Action {
    pub action_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub run_id: RunId,
    pub revision: u64,
    pub operation_id: String,
    pub resource_label: String,
    pub preview_digest: String,
    pub input_digest: String,
    pub target_digest: String,
    pub binding_digest: String,
    pub expires_at_ms: u64,
    pub state: V4ActionState,
}
impl V4Action {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.action_id)?;
        route(&self.task_id)?;
        route(&self.attempt_id)?;
        positive(self.revision)?;
        route(&self.operation_id)?;
        label(&self.resource_label)?;
        digest(&self.preview_digest)?;
        digest(&self.input_digest)?;
        digest(&self.target_digest)?;
        digest(&self.binding_digest)?;
        timestamp(self.expires_at_ms)
    }
}
validated_admin!(V4Action {
    action_id: String,
    task_id: String,
    attempt_id: String,
    run_id: RunId,
    revision: u64,
    operation_id: String,
    resource_label: String,
    preview_digest: String,
    input_digest: String,
    target_digest: String,
    binding_digest: String,
    expires_at_ms: u64,
    state: V4ActionState
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ActionDecision {
    pub action_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub expected_revision: u64,
    pub preview_digest: String,
    pub decision_id: IdempotencyKey,
    pub kind: V4ActionDecisionKind,
}
impl V4ActionDecision {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.action_id)?;
        route(&self.task_id)?;
        route(&self.attempt_id)?;
        positive(self.expected_revision)?;
        digest(&self.preview_digest)
    }
}
validated_admin!(V4ActionDecision {
    action_id: String,
    task_id: String,
    attempt_id: String,
    expected_revision: u64,
    preview_digest: String,
    decision_id: IdempotencyKey,
    kind: V4ActionDecisionKind
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4ActionDecisionKind {
    Approve,
    Deny,
}
/// Uncertain is distinct from failure/cancellation and never grants replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4ActionState {
    AwaitingApproval,
    Approved,
    Denied,
    Running,
    Succeeded,
    Failed,
    Interrupted,
    Uncertain,
}

/// Durable decision acknowledgement; applying a decision is not executing an effect.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ActionDecisionReceipt {
    pub decision: V4ActionDecision,
    pub applied_revision: u64,
}
impl V4ActionDecisionReceipt {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.decision.validate()?;
        positive(self.applied_revision)?;
        ensure(self.applied_revision > self.decision.expected_revision)
    }
}
validated_admin!(V4ActionDecisionReceipt {
    decision: V4ActionDecision,
    applied_revision: u64
});
