//! Data-only desired-plan construction for closed synthetic guild recordings.

use serde::Serialize;

use super::{Alternative, GuildIntelligenceError, GuildRecording, SEND_MESSAGES, digest};

/// An exact observed or expected permission state bound to one observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionCondition {
    pub source_observation_digest: String,
    pub scope_ref: String,
    pub subject_ref: String,
    pub allow: u64,
    pub deny: u64,
}

/// Desired permission state; it has no execution behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesiredPermissionState {
    pub scope_ref: String,
    pub subject_ref: String,
    pub allow: u64,
    pub deny: u64,
}

/// Before-state retained only as metadata for a rollback preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RollbackPermissionState {
    pub scope_ref: String,
    pub subject_ref: String,
    pub allow: u64,
    pub deny: u64,
}

/// Non-executable desired-state plan from an explicit selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesiredStatePlan {
    pub plan_digest: String,
    pub source_observation_digest: String,
    pub selected_option_id: String,
    pub preconditions: Vec<PermissionCondition>,
    pub desired_states: Vec<DesiredPermissionState>,
    pub postconditions: Vec<PermissionCondition>,
    pub rollback_preview: Vec<RollbackPermissionState>,
}

pub(super) fn make_plan(
    recording: &GuildRecording,
    observation_digest: &str,
    alternatives: &[Alternative],
    id: &str,
) -> Result<DesiredStatePlan, GuildIntelligenceError> {
    if !alternatives.iter().any(|item| item.id == id) {
        return Err(GuildIntelligenceError::UnknownSelection);
    }
    let role = recording
        .roles
        .iter()
        .find(|role| role.position == 0)
        .ok_or_else(|| GuildIntelligenceError::InvalidRecording("everyone role missing".into()))?;
    let (preconditions, desired_states, postconditions, rollback_preview) = match id {
        "do-nothing" => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        "least-privilege" if role.permissions & SEND_MESSAGES != 0 => {
            let desired_allow = role.permissions & !SEND_MESSAGES;
            (
                vec![PermissionCondition {
                    source_observation_digest: observation_digest.into(),
                    scope_ref: recording.guild_ref.clone(),
                    subject_ref: role.ref_id.clone(),
                    allow: role.permissions,
                    deny: 0,
                }],
                vec![DesiredPermissionState {
                    scope_ref: recording.guild_ref.clone(),
                    subject_ref: role.ref_id.clone(),
                    allow: desired_allow,
                    deny: 0,
                }],
                vec![PermissionCondition {
                    source_observation_digest: observation_digest.into(),
                    scope_ref: recording.guild_ref.clone(),
                    subject_ref: role.ref_id.clone(),
                    allow: desired_allow,
                    deny: 0,
                }],
                vec![RollbackPermissionState {
                    scope_ref: recording.guild_ref.clone(),
                    subject_ref: role.ref_id.clone(),
                    allow: role.permissions,
                    deny: 0,
                }],
            )
        }
        "focused-overwrite" if role.permissions & SEND_MESSAGES != 0 => {
            let mut preconditions = Vec::with_capacity(recording.channels.len());
            let mut desired_states = Vec::with_capacity(recording.channels.len());
            let mut postconditions = Vec::with_capacity(recording.channels.len());
            let mut rollback_preview = Vec::with_capacity(recording.channels.len());
            for channel in &recording.channels {
                let prior = channel
                    .overwrites
                    .iter()
                    .find(|overwrite| overwrite.target.is_everyone(&role.ref_id));
                let observed_allow = prior.map_or(0, |overwrite| overwrite.allow);
                let observed_deny = prior.map_or(0, |overwrite| overwrite.deny);
                let desired_allow = observed_allow & !SEND_MESSAGES;
                let desired_deny = observed_deny | SEND_MESSAGES;
                preconditions.push(PermissionCondition {
                    source_observation_digest: observation_digest.into(),
                    scope_ref: channel.ref_id.clone(),
                    subject_ref: role.ref_id.clone(),
                    allow: observed_allow,
                    deny: observed_deny,
                });
                desired_states.push(DesiredPermissionState {
                    scope_ref: channel.ref_id.clone(),
                    subject_ref: role.ref_id.clone(),
                    allow: desired_allow,
                    deny: desired_deny,
                });
                postconditions.push(PermissionCondition {
                    source_observation_digest: observation_digest.into(),
                    scope_ref: channel.ref_id.clone(),
                    subject_ref: role.ref_id.clone(),
                    allow: desired_allow,
                    deny: desired_deny,
                });
                rollback_preview.push(RollbackPermissionState {
                    scope_ref: channel.ref_id.clone(),
                    subject_ref: role.ref_id.clone(),
                    allow: observed_allow,
                    deny: observed_deny,
                });
            }
            (
                preconditions,
                desired_states,
                postconditions,
                rollback_preview,
            )
        }
        "least-privilege" | "focused-overwrite" => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        _ => return Err(GuildIntelligenceError::UnknownSelection),
    };
    let plan_digest = digest(&(
        observation_digest,
        id,
        &preconditions,
        &desired_states,
        &postconditions,
        &rollback_preview,
    ))?;
    Ok(DesiredStatePlan {
        plan_digest,
        source_observation_digest: observation_digest.into(),
        selected_option_id: id.into(),
        preconditions,
        desired_states,
        postconditions,
        rollback_preview,
    })
}
