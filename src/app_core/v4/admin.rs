//! Reserved administrative wire records. These declarations grant no authority.
//! Task and automation dispatch remains unavailable until durable implementations ship.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct V4PageQuery {
    pub after: u64,
    pub through: Option<u64>,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4SearchQuery {
    pub query: String,
    pub page: V4PageQuery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4ResourceQuery {
    pub resource_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4Task {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4TaskPage {
    pub tasks: Vec<V4Task>,
    pub next_after: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4TaskCreate {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4TaskState {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4Automation {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4AutomationPage {
    pub automations: Vec<V4Automation>,
    pub next_after: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum V4ActionDecisionKind {
    Approve,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4Action {
    pub resource_id: String,
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum V4ScheduleKind {
    Cron,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V4ActionDecision {
    pub action_id: String,
    pub decision_id: String,
    pub kind: V4ActionDecisionKind,
}
