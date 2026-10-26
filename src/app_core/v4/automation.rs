//! Schema 3 bounded records. Data references never grant execution authority.
use super::*;

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4AutomationUpsert {
    pub operation_id: IdempotencyKey,
    pub automation_id: String,
    pub expected_revision: Option<u64>,
    pub instruction: String,
    pub schedule: V4ScheduleKind,
    pub timezone: String,
    pub workspace_id: String,
    pub route_id: String,
    pub permission_profile_id: String,
    pub profile_revision: u64,
    pub budget_tokens: u32,
    pub enabled: bool,
}
impl V4AutomationUpsert {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.automation_id)?;
        if let Some(r) = self.expected_revision {
            positive(r)?;
        }
        text(&self.instruction, 1024)?;
        route(&self.workspace_id)?;
        route(&self.route_id)?;
        route(&self.permission_profile_id)?;
        positive(self.profile_revision)?;
        budget(self.budget_tokens)?;
        self.schedule.validate(&self.timezone)
    }
}
validated_admin!(V4AutomationUpsert { operation_id: IdempotencyKey, automation_id: String, expected_revision: Option<u64>, instruction: String, schedule: V4ScheduleKind, timezone: String, workspace_id: String, route_id: String, permission_profile_id: String, profile_revision: u64, budget_tokens: u32, enabled: bool });

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4Automation {
    pub revision: u64,
    pub sequence: u64,
    pub spec: V4AutomationUpsert,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub next_run_at_ms: Option<u64>,
}
impl V4Automation {
    pub fn validate(&self) -> Result<(), ValidationError> {
        positive(self.revision)?;
        positive(self.sequence)?;
        self.spec.validate()?;
        timestamp(self.created_at_ms)?;
        timestamp(self.updated_at_ms)?;
        ensure(self.updated_at_ms >= self.created_at_ms)?;
        if let Some(t) = self.next_run_at_ms {
            timestamp(t)?;
        }
        Ok(())
    }
}
validated_admin!(V4Automation { revision: u64, sequence: u64, spec: V4AutomationUpsert, created_at_ms: u64, updated_at_ms: u64, next_run_at_ms: Option<u64> });

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4AutomationPage {
    pub after: u64,
    pub through: u64,
    pub automations: Vec<V4Automation>,
}
impl V4AutomationPage {
    pub fn validate(&self) -> Result<(), ValidationError> {
        page(self.after, self.through, self.automations.len())?;
        let mut last = self.after;
        let mut ids = std::collections::BTreeSet::new();
        for a in &self.automations {
            a.validate()?;
            ensure(
                a.sequence > last
                    && a.sequence <= self.through
                    && ids.insert(&a.spec.automation_id),
            )?;
            last = a.sequence;
        }
        admin_bytes(self)
    }
}
validated_admin!(V4AutomationPage { after: u64, through: u64, automations: Vec<V4Automation> });

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4AutomationToggle {
    pub automation_id: String,
    pub expected_revision: u64,
    pub operation_id: IdempotencyKey,
    pub enabled: bool,
}
impl V4AutomationToggle {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.automation_id)?;
        positive(self.expected_revision)
    }
}
validated_admin!(V4AutomationToggle {
    automation_id: String,
    expected_revision: u64,
    operation_id: IdempotencyKey,
    enabled: bool
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4AutomationRun {
    pub automation_id: String,
    pub expected_revision: u64,
    pub operation_id: IdempotencyKey,
}
impl V4AutomationRun {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.automation_id)?;
        positive(self.expected_revision)
    }
}
validated_admin!(V4AutomationRun {
    automation_id: String,
    expected_revision: u64,
    operation_id: IdempotencyKey
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4AutomationOccurrence {
    pub automation_id: String,
    pub revision: u64,
    pub occurrence_id: String,
    pub source: V4OccurrenceSource,
    pub task_id: String,
    pub attempt_id: String,
    pub run_id: RunId,
    pub state: V4OccurrenceState,
}
impl V4AutomationOccurrence {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.automation_id)?;
        positive(self.revision)?;
        route(&self.occurrence_id)?;
        self.source.validate()?;
        route(&self.task_id)?;
        route(&self.attempt_id)
    }
}
validated_admin!(V4AutomationOccurrence {
    automation_id: String,
    revision: u64,
    occurrence_id: String,
    source: V4OccurrenceSource,
    task_id: String,
    attempt_id: String,
    run_id: RunId,
    state: V4OccurrenceState
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum V4ScheduleKind {
    Once { at_ms: u64 },
    Hourly { minute: u8 },
    Daily { hour: u8, minute: u8 },
    Weekly { weekday: u8, hour: u8, minute: u8 },
    Cron { expression: String },
}
impl V4ScheduleKind {
    fn validate(&self, timezone: &str) -> Result<(), ValidationError> {
        use crate::automation_schedule::{CompiledSchedule, ScheduleKind, ScheduleSpec};
        let kind = match self {
            Self::Once { at_ms } => {
                timestamp(*at_ms)?;
                ScheduleKind::Once { at_ms: *at_ms }
            }
            Self::Hourly { minute } => ScheduleKind::Hourly { minute: *minute },
            Self::Daily { hour, minute } => ScheduleKind::Daily {
                hour: *hour,
                minute: *minute,
            },
            Self::Weekly {
                weekday,
                hour,
                minute,
            } => ScheduleKind::Weekly {
                weekday: *weekday,
                hour: *hour,
                minute: *minute,
            },
            Self::Cron { expression } => {
                text(expression, 128)?;
                ScheduleKind::Cron {
                    expression: expression.clone(),
                }
            }
        };
        CompiledSchedule::new(&ScheduleSpec {
            kind,
            timezone: timezone.into(),
        })
        .map_err(|_| ValidationError::new("invalid v4 schedule"))?;
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V4OccurrenceState {
    Reserved,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
    ReconciliationRequired,
}

/// Manual requests and scheduled occurrences have disjoint retry identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum V4OccurrenceSource {
    Scheduled { scheduled_at_ms: u64 },
    Manual { operation_id: IdempotencyKey },
}
impl V4OccurrenceSource {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Scheduled { scheduled_at_ms } => timestamp(*scheduled_at_ms),
            Self::Manual { .. } => Ok(()),
        }
    }
}
