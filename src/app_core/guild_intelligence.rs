//! Pure Program 3 guild intelligence over closed synthetic metadata recordings.
//!
//! This module deliberately has no transport, persistence, command, or effect
//! surface. It produces data-only desired states for later, separately governed
//! programs to interpret.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

mod plan;
mod validation;

use plan::make_plan;
pub use plan::{
    DesiredPermissionState, DesiredStatePlan, PermissionCondition, RollbackPermissionState,
};

const SCHEMA_VERSION: u16 = 1;
const MAX_OBJECTS: usize = 2_048;
const MAX_REF_LEN: usize = 96;
const ADMINISTRATOR: u64 = 1 << 3;
const VIEW_CHANNEL: u64 = 1 << 10;
const SEND_MESSAGES: u64 = 1 << 11;

/// Fail-closed errors produced by the local recording engine.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuildIntelligenceError {
    #[error("recording is invalid: {0}")]
    InvalidRecording(String),
    #[error("only explicitly synthetic recordings are accepted")]
    NonSyntheticRecording,
    #[error("owner or administrator authority is required")]
    Unauthorized,
    #[error("the selected alternative does not exist")]
    UnknownSelection,
    #[error("canonical serialization failed: {0}")]
    Serialization(String),
}

/// The only operation a recording source can expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadOperation {
    /// Read one already-loaded synthetic observation.
    ReadSyntheticObservation,
}

/// A closed, in-memory source used for C2 closed synthetic replay evidence.
#[derive(Debug, Clone)]
pub struct RecordingGuildSource {
    recording: GuildRecording,
    operations: Vec<ReadOperation>,
}

impl RecordingGuildSource {
    /// Parses and validates a closed synthetic metadata recording.
    pub fn from_json(input: &str) -> Result<Self, GuildIntelligenceError> {
        let mut recording: GuildRecording = serde_json::from_str(input)
            .map_err(|error| GuildIntelligenceError::InvalidRecording(error.to_string()))?;
        if !recording.synthetic {
            return Err(GuildIntelligenceError::NonSyntheticRecording);
        }
        recording.validate()?;
        recording.normalize();
        Ok(Self {
            recording,
            operations: Vec::new(),
        })
    }

    /// Produces a deterministic read-only replay, optionally planning one
    /// explicitly selected alternative.
    pub fn replay(
        &mut self,
        selection: Option<&str>,
    ) -> Result<ReplayArtifact, GuildIntelligenceError> {
        self.operations
            .push(ReadOperation::ReadSyntheticObservation);
        analyze(&self.recording, selection)
    }

    /// Returns the typed local read log. No identifiers or content are logged.
    #[must_use]
    pub fn read_operations(&self) -> &[ReadOperation] {
        &self.operations
    }
}

/// Deterministic replay result suitable for a presentation adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayArtifact {
    pub twin: GuildTwin,
    pub findings: Vec<Finding>,
    pub alternatives: Vec<Alternative>,
    pub plan: Option<DesiredStatePlan>,
    pub status: RedactedGuildStatus,
}

impl ReplayArtifact {
    /// Serializes the normalized replay for byte-for-byte comparison.
    pub fn canonical_json(&self) -> Result<Vec<u8>, GuildIntelligenceError> {
        serde_json::to_vec(self)
            .map_err(|error| GuildIntelligenceError::Serialization(error.to_string()))
    }
}

/// The five required twin views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    Structure,
    Authority,
    Workflow,
    Goal,
    Health,
}

/// One view-level assertion with provenance and no free-form content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TwinView {
    pub kind: ViewKind,
    pub assertion_count: u32,
    pub metadata: AssertionMetadata,
}

/// Provenance carried by every view assertion set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssertionMetadata {
    pub source: &'static str,
    pub observed_at_unix: u64,
    pub confidence_basis_points: u16,
    pub stale: bool,
    pub contradiction: bool,
    pub privacy_class: &'static str,
    pub schema_version: u16,
    pub digest: String,
}

/// Explicitly bounded observation coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Coverage {
    pub role_count: u32,
    pub channel_count: u32,
    pub active_thread_count: u32,
    pub audit_log: AuditCoverage,
    pub content_excluded: bool,
    pub member_enumeration_excluded: bool,
}

/// Five-view twin plus per-object source watermarks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GuildTwin {
    pub schema_version: u16,
    pub observation_digest: String,
    pub views: Vec<TwinView>,
    pub coverage: Coverage,
    pub watermarks: Vec<ObjectWatermark>,
    pub effective_channel_permissions: BTreeMap<String, u64>,
}

/// Stable metadata-object watermark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectWatermark {
    pub object_kind: &'static str,
    pub object_ref: String,
    pub observed_at_unix: u64,
    pub schema_version: u16,
    pub digest: String,
}

/// Stable finding identifiers; no generated prose is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCode {
    EveryoneCanSend,
    BotChannelVisibilityRestored,
    CoverageLimited,
}

/// Metadata-only finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: FindingCode,
    pub severity: &'static str,
    pub affected_refs: Vec<String>,
    pub evidence_digests: Vec<String>,
}

/// Deterministic operator-selectable alternative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Alternative {
    pub id: String,
    pub strategy: &'static str,
    pub affected_findings: Vec<FindingCode>,
    pub reversible: bool,
}

/// Local evidence vocabulary for the redacted status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel {
    C2ClosedSyntheticReplay,
}

/// Fixed, identifier-free status projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RedactedGuildStatus {
    pub schema_version: u16,
    pub evidence_level: EvidenceLevel,
    pub source_kind: &'static str,
    pub authorization_basis: &'static str,
    pub read_only: bool,
    pub fresh: bool,
    pub role_count: u32,
    pub channel_count: u32,
    pub finding_count: u32,
    pub alternative_count: u32,
    pub selection_present: bool,
    pub plan_present: bool,
    pub excluded_surfaces: [&'static str; 5],
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GuildRecording {
    schema_version: u16,
    synthetic: bool,
    observed_at_unix: u64,
    guild_ref: String,
    operator_ref: String,
    operator_authority: OperatorAuthority,
    owner_ref: String,
    roles: Vec<RoleRecord>,
    channels: Vec<ChannelRecord>,
    active_threads: Vec<ThreadRecord>,
    bot_self: BotSelfRecord,
    coverage: RecordingCoverage,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum OperatorAuthority {
    Owner,
    Administrator,
    Manager,
    Member,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RoleRecord {
    ref_id: String,
    position: i32,
    permissions: u64,
    managed: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChannelRecord {
    ref_id: String,
    parent_ref: Option<String>,
    kind: ChannelKind,
    overwrites: Vec<PermissionOverwrite>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ChannelKind {
    Category,
    Text,
    Voice,
    Forum,
    Thread,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PermissionOverwrite {
    target: OverwriteTarget,
    allow: u64,
    deny: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum OverwriteTarget {
    Everyone { ref_id: String },
    Role { ref_id: String },
    Member { ref_id: String },
    Unrecognized { ref_id: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ThreadRecord {
    ref_id: String,
    parent_ref: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BotSelfRecord {
    ref_id: String,
    role_refs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecordingCoverage {
    guild: bool,
    roles: bool,
    channels: bool,
    active_threads: bool,
    audit_log: AuditCoverage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditCoverage {
    Observed,
    UnavailableByPolicy,
    UnavailableByCapability,
}

fn analyze(
    recording: &GuildRecording,
    selection: Option<&str>,
) -> Result<ReplayArtifact, GuildIntelligenceError> {
    if !matches!(
        recording.operator_authority,
        OperatorAuthority::Owner | OperatorAuthority::Administrator
    ) {
        return Err(GuildIntelligenceError::Unauthorized);
    }
    let observation_digest = digest(recording)?;
    let watermarks = watermarks(recording)?;
    let effective_channel_permissions = effective_permissions(recording);
    let findings = findings(recording, &watermarks, &effective_channel_permissions)?;
    let alternatives = alternatives(&findings);
    let coverage = Coverage {
        role_count: recording.roles.len() as u32,
        channel_count: recording.channels.len() as u32,
        active_thread_count: recording.active_threads.len() as u32,
        audit_log: recording.coverage.audit_log,
        content_excluded: true,
        member_enumeration_excluded: true,
    };
    let metadata = |kind: ViewKind, count: u32| -> Result<TwinView, GuildIntelligenceError> {
        Ok(TwinView {
            kind,
            assertion_count: count,
            metadata: AssertionMetadata {
                source: "synthetic_recording",
                observed_at_unix: recording.observed_at_unix,
                confidence_basis_points: 10_000,
                stale: true,
                contradiction: false,
                privacy_class: "structural_metadata",
                schema_version: SCHEMA_VERSION,
                digest: digest(&(observation_digest.as_str(), kind as u8, count))?,
            },
        })
    };
    let views = vec![
        metadata(
            ViewKind::Structure,
            coverage.role_count + coverage.channel_count + coverage.active_thread_count,
        )?,
        metadata(
            ViewKind::Authority,
            effective_channel_permissions.len() as u32 + 2,
        )?,
        metadata(ViewKind::Workflow, 1)?,
        metadata(ViewKind::Goal, u32::from(selection.is_some()))?,
        metadata(ViewKind::Health, findings.len() as u32 + 1)?,
    ];
    let twin = GuildTwin {
        schema_version: SCHEMA_VERSION,
        observation_digest: observation_digest.clone(),
        views,
        coverage: coverage.clone(),
        watermarks,
        effective_channel_permissions,
    };
    let plan = match selection {
        None => None,
        Some(id) => Some(make_plan(
            recording,
            &observation_digest,
            &alternatives,
            id,
        )?),
    };
    let status = RedactedGuildStatus {
        schema_version: SCHEMA_VERSION,
        evidence_level: EvidenceLevel::C2ClosedSyntheticReplay,
        source_kind: "synthetic_recording",
        authorization_basis: "synthetic_fixture_claim",
        read_only: true,
        fresh: false,
        role_count: coverage.role_count,
        channel_count: coverage.channel_count,
        finding_count: findings.len() as u32,
        alternative_count: alternatives.len() as u32,
        selection_present: selection.is_some(),
        plan_present: plan.is_some(),
        excluded_surfaces: [
            "discord_transport",
            "durable_state",
            "member_enumeration",
            "private_content",
            "effects",
        ],
    };
    Ok(ReplayArtifact {
        twin,
        findings,
        alternatives,
        plan,
        status,
    })
}

fn effective_permissions(recording: &GuildRecording) -> BTreeMap<String, u64> {
    let owner = recording.bot_self.ref_id == recording.owner_ref;
    let bot_roles: BTreeSet<_> = recording
        .bot_self
        .role_refs
        .iter()
        .map(String::as_str)
        .collect();
    let everyone = recording
        .roles
        .iter()
        .find(|role| role.position == 0)
        .map_or(0, |role| role.permissions);
    let everyone_ref = recording
        .roles
        .iter()
        .find(|role| role.position == 0)
        .map_or("", |role| role.ref_id.as_str());
    let mut base = everyone;
    for role in &recording.roles {
        if bot_roles.contains(role.ref_id.as_str()) {
            base |= role.permissions;
        }
    }
    recording.channels.iter().map(|channel| {
        let mut permissions = if owner || base & ADMINISTRATOR != 0 { u64::MAX } else { base };
        if permissions != u64::MAX {
            for overwrite in channel
                .overwrites
                .iter()
                .filter(|item| item.target.is_everyone(everyone_ref))
            {
                permissions = (permissions & !overwrite.deny) | overwrite.allow;
            }
            let mut deny = 0; let mut allow = 0;
            for overwrite in channel.overwrites.iter().filter(|item| matches!(&item.target, OverwriteTarget::Role { ref_id } if bot_roles.contains(ref_id.as_str()))) {
                deny |= overwrite.deny; allow |= overwrite.allow;
            }
            permissions = (permissions & !deny) | allow;
            for overwrite in channel.overwrites.iter().filter(|item| matches!(&item.target, OverwriteTarget::Member { ref_id } if ref_id == &recording.bot_self.ref_id)) {
                permissions = (permissions & !overwrite.deny) | overwrite.allow;
            }
        }
        (channel.ref_id.clone(), permissions)
    }).collect()
}

fn findings(
    recording: &GuildRecording,
    watermarks: &[ObjectWatermark],
    effective: &BTreeMap<String, u64>,
) -> Result<Vec<Finding>, GuildIntelligenceError> {
    let everyone_ref = recording
        .roles
        .iter()
        .find(|role| role.position == 0)
        .map_or("", |role| role.ref_id.as_str());
    let evidence = |reference: &str| {
        watermarks
            .iter()
            .find(|item| item.object_ref == reference)
            .map(|item| vec![item.digest.clone()])
            .ok_or_else(|| {
                GuildIntelligenceError::InvalidRecording(
                    "validated metadata object is missing its watermark".into(),
                )
            })
    };
    let mut result = Vec::new();
    if let Some(role) = recording
        .roles
        .iter()
        .find(|role| role.position == 0 && role.permissions & SEND_MESSAGES != 0)
    {
        result.push(Finding {
            code: FindingCode::EveryoneCanSend,
            severity: "medium",
            affected_refs: vec![role.ref_id.clone()],
            evidence_digests: evidence(&role.ref_id)?,
        });
    }
    for channel in &recording.channels {
        if effective
            .get(&channel.ref_id)
            .is_some_and(|bits| bits & VIEW_CHANNEL != 0)
            && channel
                .overwrites
                .iter()
                .any(|item| item.target.is_everyone(everyone_ref) && item.deny & VIEW_CHANNEL != 0)
        {
            result.push(Finding {
                code: FindingCode::BotChannelVisibilityRestored,
                severity: "informational",
                affected_refs: vec![channel.ref_id.clone()],
                evidence_digests: evidence(&channel.ref_id)?,
            });
        }
    }
    if recording.coverage.audit_log != AuditCoverage::Observed {
        result.push(Finding {
            code: FindingCode::CoverageLimited,
            severity: "informational",
            affected_refs: Vec::new(),
            evidence_digests: vec![digest(&recording.coverage)?],
        });
    }
    result.sort_by_key(|item| item.code);
    Ok(result)
}

fn alternatives(findings: &[Finding]) -> Vec<Alternative> {
    let addressed = findings
        .iter()
        .filter_map(|item| (item.code == FindingCode::EveryoneCanSend).then_some(item.code))
        .collect::<Vec<_>>();
    vec![
        Alternative {
            id: "least-privilege".into(),
            strategy: "reduce_broad_permission",
            affected_findings: addressed.clone(),
            reversible: true,
        },
        Alternative {
            id: "focused-overwrite".into(),
            strategy: "normalize_scoped_overwrite",
            affected_findings: addressed,
            reversible: true,
        },
        Alternative {
            id: "do-nothing".into(),
            strategy: "retain_observed_state",
            affected_findings: Vec::new(),
            reversible: true,
        },
    ]
}

fn watermarks(recording: &GuildRecording) -> Result<Vec<ObjectWatermark>, GuildIntelligenceError> {
    let mut result = vec![watermark(
        "guild",
        &recording.guild_ref,
        recording.observed_at_unix,
        recording,
    )?];
    for role in &recording.roles {
        result.push(watermark(
            "role",
            &role.ref_id,
            recording.observed_at_unix,
            role,
        )?);
    }
    for channel in &recording.channels {
        result.push(watermark(
            "channel",
            &channel.ref_id,
            recording.observed_at_unix,
            channel,
        )?);
    }
    for thread in &recording.active_threads {
        result.push(watermark(
            "active_thread",
            &thread.ref_id,
            recording.observed_at_unix,
            thread,
        )?);
    }
    result.sort_by(|a, b| (a.object_kind, &a.object_ref).cmp(&(b.object_kind, &b.object_ref)));
    Ok(result)
}

fn watermark<T: Serialize>(
    kind: &'static str,
    reference: &str,
    observed_at: u64,
    value: &T,
) -> Result<ObjectWatermark, GuildIntelligenceError> {
    Ok(ObjectWatermark {
        object_kind: kind,
        object_ref: reference.into(),
        observed_at_unix: observed_at,
        schema_version: SCHEMA_VERSION,
        digest: digest(value)?,
    })
}

fn digest<T: Serialize>(value: &T) -> Result<String, GuildIntelligenceError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| GuildIntelligenceError::Serialization(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn invalid<T>(message: &str) -> Result<T, GuildIntelligenceError> {
    Err(GuildIntelligenceError::InvalidRecording(message.into()))
}
