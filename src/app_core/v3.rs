//! Additive protocol-v3 contracts for capability-gated application authority.
//!
//! These types deliberately do not extend [`super::AppCommand`] or
//! [`super::AppEvent`]. Protocols v1 and v2 therefore retain their exact enum
//! and wire shapes. Defining a v3 grant is not evidence that the daemon serves
//! it: the daemon must explicitly negotiate and advertise every grant before a
//! command can be dispatched.

use super::{
    ClaimStatus, V3ToolApprovalStatus, V3ToolCall, V3ToolDecision, V3ToolPage, V3ToolResult,
    ValidationError,
};
use serde::{Deserialize, Serialize};

/// Additive application protocol reserved for capability-gated authority.
pub const APP_PROTOCOL_V3: u16 = 3;
/// Serialized schema used by protocol-v3 contracts.
pub const APP_SCHEMA_V3: u16 = 3;
/// Maximum number of records in any protocol-v3 page.
pub const MAX_V3_PAGE: u16 = 32;
const MAX_ID_BYTES: usize = 128;
const MAX_LABEL_BYTES: usize = 256;
const MAX_QUERY_BYTES: usize = 4 * 1_024;
const MAX_ERROR_MESSAGE_BYTES: usize = 2 * 1_024;
/// Individually negotiated protocol-v3 authority.
///
/// Declaration order is the canonical serialized order for
/// [`V3CapabilitySet`]. New grants must be appended to retain stable ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3Capability {
    ListTools,
    InvokeTools,
    DecideToolApprovals,
    CancelTools,
    ReadMemory,
    ReadModels,
    DownloadModels,
    ManageModels,
    ReadTraining,
    ManageTraining,
    ReadWorkers,
    CancelJobs,
    ReadClaimsById,
    PollEvents,
}

/// Ordered, duplicate-free set of negotiated v3 grants.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "V3CapabilitySetWire", into = "V3CapabilitySetWire")]
pub struct V3CapabilitySet {
    capabilities: Vec<V3Capability>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct V3CapabilitySetWire {
    capabilities: Vec<V3Capability>,
}

impl TryFrom<V3CapabilitySetWire> for V3CapabilitySet {
    type Error = ValidationError;

    fn try_from(wire: V3CapabilitySetWire) -> Result<Self, Self::Error> {
        Self::from_sorted(wire.capabilities)
    }
}

impl From<V3CapabilitySet> for V3CapabilitySetWire {
    fn from(set: V3CapabilitySet) -> Self {
        Self {
            capabilities: set.capabilities,
        }
    }
}

impl V3CapabilitySet {
    /// Construct the fail-closed grant set.
    #[must_use]
    pub const fn deny_all() -> Self {
        Self {
            capabilities: Vec::new(),
        }
    }

    /// Construct a grant set only when its supplied order is canonical.
    pub fn from_sorted(capabilities: Vec<V3Capability>) -> Result<Self, ValidationError> {
        let set = Self { capabilities };
        set.validate()?;
        Ok(set)
    }

    /// Test whether one authority was explicitly granted.
    #[must_use]
    pub fn contains(&self, capability: V3Capability) -> bool {
        self.capabilities.binary_search(&capability).is_ok()
    }

    /// Return grants in their canonical order.
    #[must_use]
    pub fn as_slice(&self) -> &[V3Capability] {
        &self.capabilities
    }

    /// Validate canonical ordering and uniqueness.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.capabilities.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ValidationError::new(
                "v3 capabilities must be strictly ordered and duplicate-free",
            ));
        }
        Ok(())
    }

    /// Authorize negotiation itself or require the command's exact grant.
    #[must_use]
    pub fn permits(&self, command: &V3Command) -> bool {
        command
            .required_capability()
            .is_none_or(|required| self.contains(required))
    }
}

/// Client request for an explicit v3 version and capability negotiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3GrantRequest {
    pub supported_versions: Vec<u16>,
    pub requested: V3CapabilitySet,
}

impl V3GrantRequest {
    /// Validate a bounded, ordered version declaration containing v3.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.supported_versions.is_empty()
            || self.supported_versions.len() > 8
            || self
                .supported_versions
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || !self.supported_versions.contains(&APP_PROTOCOL_V3)
        {
            return Err(ValidationError::new(
                "supported versions must be ordered, unique, bounded, and include v3",
            ));
        }
        self.requested.validate()
    }
}

/// Server result of protocol-v3 negotiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3GrantNegotiation {
    pub protocol_version: u16,
    pub schema_version: u16,
    pub granted: V3CapabilitySet,
}

impl V3GrantNegotiation {
    /// Validate the exact v3 contract versions and canonical grants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != APP_PROTOCOL_V3 || self.schema_version != APP_SCHEMA_V3 {
            return Err(ValidationError::new("invalid v3 negotiation versions"));
        }
        self.granted.validate()
    }

    /// Validate versions and prove every returned grant was requested.
    pub fn validate_for(&self, request: &V3GrantRequest) -> Result<(), ValidationError> {
        request.validate()?;
        self.validate()?;
        if self
            .granted
            .as_slice()
            .iter()
            .any(|grant| !request.requested.contains(*grant))
        {
            return Err(ValidationError::new(
                "v3 negotiation granted an unrequested capability",
            ));
        }
        Ok(())
    }
}

/// Fixed-watermark page request shared by v3 list and polling commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct V3PageQuery {
    pub after: u64,
    pub through: Option<u64>,
    pub limit: u16,
}

impl Default for V3PageQuery {
    fn default() -> Self {
        Self {
            after: 0,
            through: None,
            limit: MAX_V3_PAGE,
        }
    }
}

impl V3PageQuery {
    /// Validate the page limit and fixed-watermark relationship.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.limit == 0 || self.limit > MAX_V3_PAGE {
            return Err(ValidationError::new("invalid v3 page limit"));
        }
        if self.through.is_some_and(|through| self.after > through) {
            return Err(ValidationError::new("v3 cursor exceeds watermark"));
        }
        Ok(())
    }
}

/// Resource-scoped action with a caller-generated operation identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3Action {
    pub resource_id: String,
    pub operation_id: String,
}

impl V3Action {
    /// Validate opaque resource and operation identifiers.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.resource_id)?;
        validate_id(&self.operation_id)
    }
}

/// Read-only query for one opaque resource identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ResourceQuery {
    pub resource_id: String,
}

impl V3ResourceQuery {
    /// Validate the opaque resource identifier.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.resource_id)
    }
}

/// Bounded search within one explicitly selected memory space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3SearchRequest {
    pub space_id: String,
    pub query: String,
    pub page: V3PageQuery,
}

impl V3SearchRequest {
    /// Validate the selected space, bounded query, and page.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.space_id)?;
        validate_text(&self.query, MAX_QUERY_BYTES, "invalid v3 search query")?;
        self.page.validate()
    }
}

/// Fixed-watermark metric-page query for one training operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3MetricQuery {
    pub operation_id: String,
    pub page: V3PageQuery,
}

impl V3MetricQuery {
    /// Validate the operation identity and bounded page.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.operation_id)?;
        self.page.validate()
    }
}

/// Immutable model revision selected for a bounded lifecycle action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ModelAction {
    pub model_id: String,
    pub revision: String,
    pub operation_id: String,
}

impl V3ModelAction {
    /// Validate model, immutable revision, and operation identifiers.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.model_id)?;
        validate_id(&self.revision)?;
        validate_id(&self.operation_id)
    }
}

/// Reproducible request to start one adapter-training operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3TrainingStart {
    pub dataset_id: String,
    pub base_model_id: String,
    pub base_revision: String,
    pub operation_id: String,
    pub seed: u64,
}

impl V3TrainingStart {
    /// Validate every immutable input identity and the operation identifier.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.dataset_id)?;
        validate_id(&self.base_model_id)?;
        validate_id(&self.base_revision)?;
        validate_id(&self.operation_id)
    }
}

/// Protocol-v3 commands. Their presence grants no daemon authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum V3Command {
    Negotiate(V3GrantRequest),
    ListTools(V3PageQuery),
    InvokeTool(V3ToolCall),
    ApproveTool(V3ToolDecision),
    DenyTool(V3ToolDecision),
    CancelTool(V3Action),
    ListMemorySpaces(V3PageQuery),
    SearchMemory(V3SearchRequest),
    ReadMemoryMetadata(V3ResourceQuery),
    ListModels(V3PageQuery),
    DownloadModel(V3ModelAction),
    ModelDownloadStatus(V3ResourceQuery),
    LoadModel(V3ModelAction),
    UnloadModel(V3ModelAction),
    InferenceStatus(V3ResourceQuery),
    TrainingDatasetStatus(V3ResourceQuery),
    StartTraining(V3TrainingStart),
    TrainingStatus(V3ResourceQuery),
    CancelTraining(V3Action),
    TrainingMetrics(V3MetricQuery),
    PromoteAdapter(V3Action),
    RollbackAdapter(V3Action),
    ListClusters(V3PageQuery),
    ListWorkers(V3PageQuery),
    WorkerHealth(V3ResourceQuery),
    JobStatus(V3ResourceQuery),
    CancelJob(V3Action),
    ClaimById(V3ResourceQuery),
    PollEvents(V3PageQuery),
}

impl V3Command {
    /// Protocol version required for every command in this separate family.
    #[must_use]
    pub const fn minimum_protocol_version(&self) -> u16 {
        APP_PROTOCOL_V3
    }

    /// Exact grant required before this command may be dispatched.
    #[must_use]
    pub const fn required_capability(&self) -> Option<V3Capability> {
        Some(match self {
            Self::Negotiate(_) => return None,
            Self::ListTools(_) => V3Capability::ListTools,
            Self::InvokeTool(_) => V3Capability::InvokeTools,
            Self::ApproveTool(_) | Self::DenyTool(_) => V3Capability::DecideToolApprovals,
            Self::CancelTool(_) => V3Capability::CancelTools,
            Self::ListMemorySpaces(_) | Self::SearchMemory(_) | Self::ReadMemoryMetadata(_) => {
                V3Capability::ReadMemory
            }
            Self::ListModels(_) | Self::ModelDownloadStatus(_) | Self::InferenceStatus(_) => {
                V3Capability::ReadModels
            }
            Self::DownloadModel(_) => V3Capability::DownloadModels,
            Self::LoadModel(_) | Self::UnloadModel(_) => V3Capability::ManageModels,
            Self::TrainingDatasetStatus(_) | Self::TrainingStatus(_) | Self::TrainingMetrics(_) => {
                V3Capability::ReadTraining
            }
            Self::StartTraining(_)
            | Self::CancelTraining(_)
            | Self::PromoteAdapter(_)
            | Self::RollbackAdapter(_) => V3Capability::ManageTraining,
            Self::ListClusters(_)
            | Self::ListWorkers(_)
            | Self::WorkerHealth(_)
            | Self::JobStatus(_) => V3Capability::ReadWorkers,
            Self::CancelJob(_) => V3Capability::CancelJobs,
            Self::ClaimById(_) => V3Capability::ReadClaimsById,
            Self::PollEvents(_) => V3Capability::PollEvents,
        })
    }

    /// Validate the command before grant evaluation or dispatch.
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Negotiate(request) => request.validate(),
            Self::ListTools(page)
            | Self::ListMemorySpaces(page)
            | Self::ListModels(page)
            | Self::ListClusters(page)
            | Self::ListWorkers(page)
            | Self::PollEvents(page) => page.validate(),
            Self::InvokeTool(call) => call.validate(),
            Self::SearchMemory(search) => search.validate(),
            Self::ApproveTool(decision) | Self::DenyTool(decision) => decision.validate(),
            Self::DownloadModel(action) | Self::LoadModel(action) | Self::UnloadModel(action) => {
                action.validate()
            }
            Self::StartTraining(request) => request.validate(),
            Self::ReadMemoryMetadata(query)
            | Self::ModelDownloadStatus(query)
            | Self::InferenceStatus(query)
            | Self::TrainingDatasetStatus(query)
            | Self::TrainingStatus(query)
            | Self::WorkerHealth(query)
            | Self::JobStatus(query)
            | Self::ClaimById(query) => query.validate(),
            Self::TrainingMetrics(query) => query.validate(),
            Self::CancelTool(action)
            | Self::CancelTraining(action)
            | Self::PromoteAdapter(action)
            | Self::RollbackAdapter(action)
            | Self::CancelJob(action) => action.validate(),
        }
    }
}

/// Sanitized state for a bounded operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3OperationState {
    Available,
    Queued,
    Running,
    InputRequired,
    Succeeded,
    Failed,
    Denied,
    Cancelled,
}

/// Sanitized record used by inventory and metadata pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3EntityRecord {
    pub id: String,
    pub label: String,
    pub state: V3OperationState,
}

impl V3EntityRecord {
    /// Validate the opaque ID and presentation-bounded label.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.id)?;
        validate_text(&self.label, MAX_LABEL_BYTES, "invalid v3 entity label")
    }
}

/// Fixed-watermark inventory page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3EntityPage {
    pub after: u64,
    pub through: u64,
    pub records: Vec<V3EntityRecord>,
}

impl V3EntityPage {
    /// Validate page bounds, watermark, and sanitized records.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.after > self.through || self.records.len() > usize::from(MAX_V3_PAGE) {
            return Err(ValidationError::new("invalid v3 entity page"));
        }
        self.records.iter().try_for_each(V3EntityRecord::validate)
    }
}

/// Current status of one tool, model, training, adapter, or worker job operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3OperationStatus {
    pub operation_id: String,
    pub resource_id: String,
    pub state: V3OperationState,
    pub progress_basis_points: u16,
}

impl V3OperationStatus {
    /// Validate opaque identifiers and progress in the inclusive 0..=10000 range.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.operation_id)?;
        validate_id(&self.resource_id)?;
        if self.progress_basis_points > 10_000 {
            return Err(ValidationError::new("invalid v3 operation progress"));
        }
        Ok(())
    }
}

/// One finite training metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3Metric {
    pub name: String,
    pub step: u64,
    pub value: f64,
}

impl V3Metric {
    /// Validate a bounded metric name and finite value.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.name)?;
        if !self.value.is_finite() {
            return Err(ValidationError::new("v3 metric must be finite"));
        }
        Ok(())
    }
}

/// Bounded metric page for one training operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3MetricPage {
    pub operation_id: String,
    pub after: u64,
    pub through: u64,
    pub metrics: Vec<V3Metric>,
}

impl V3MetricPage {
    /// Validate operation identity, page bounds, ordering, and metric values.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.operation_id)?;
        if self.metrics.len() > usize::from(MAX_V3_PAGE)
            || self.after > self.through
            || self
                .metrics
                .windows(2)
                .any(|pair| pair[0].step > pair[1].step)
            || self
                .metrics
                .last()
                .is_some_and(|metric| metric.step > self.through)
            || self
                .metrics
                .first()
                .is_some_and(|metric| metric.step <= self.after)
        {
            return Err(ValidationError::new("invalid v3 metric page"));
        }
        self.metrics.iter().try_for_each(V3Metric::validate)
    }
}

/// One sanitized event in a fixed-watermark polling page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3EventRecord {
    pub sequence: u64,
    pub operation: V3OperationStatus,
}

/// Fixed-watermark protocol-v3 event page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3EventPage {
    pub after: u64,
    pub through: u64,
    pub events: Vec<V3EventRecord>,
}

impl V3EventPage {
    /// Validate bounded, strictly increasing event sequences and the watermark.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.after > self.through
            || self.events.len() > usize::from(MAX_V3_PAGE)
            || self
                .events
                .windows(2)
                .any(|pair| pair[0].sequence >= pair[1].sequence)
            || self
                .events
                .first()
                .is_some_and(|event| event.sequence <= self.after)
            || self
                .events
                .last()
                .is_some_and(|event| event.sequence > self.through)
        {
            return Err(ValidationError::new("invalid v3 event page"));
        }
        self.events
            .iter()
            .try_for_each(|event| event.operation.validate())
    }
}

/// Stable machine-readable protocol-v3 error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3ErrorCode {
    UnsupportedVersion,
    Unauthorized,
    CapabilityDenied,
    InvalidCommand,
    NotFound,
    Conflict,
    Cancelled,
    DeadlineExceeded,
    BudgetExceeded,
    RateLimited,
    ResponseTooLarge,
    Internal,
}

/// Bounded, presentation-safe protocol-v3 failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3Error {
    pub code: V3ErrorCode,
    pub message: String,
}

/// Stable claim lookup result retaining the canonical claim identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3StableClaim {
    pub id: String,
    pub name: String,
    pub status: ClaimStatus,
    pub note: String,
}

impl V3StableClaim {
    /// Validate the stable ID and bounded presentation text.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.id)?;
        validate_text(&self.name, MAX_LABEL_BYTES, "invalid v3 claim name")?;
        validate_text(&self.note, MAX_ERROR_MESSAGE_BYTES, "invalid v3 claim note")
    }
}

impl V3Error {
    /// Validate the bounded error message.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text(
            &self.message,
            MAX_ERROR_MESSAGE_BYTES,
            "invalid v3 error message",
        )
    }
}

/// Protocol-v3 events corresponding to the command families above.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum V3Event {
    Negotiated(V3GrantNegotiation),
    Tools(V3ToolPage),
    ToolResult(V3ToolResult),
    ToolStatus(V3OperationStatus),
    ToolApprovalStatus(V3ToolApprovalStatus),
    MemorySpaces(V3EntityPage),
    MemorySearchResults(V3EntityPage),
    MemoryMetadata(V3EntityRecord),
    Models(V3EntityPage),
    ModelStatus(V3OperationStatus),
    TrainingDatasetStatus(V3EntityRecord),
    TrainingStatus(V3OperationStatus),
    TrainingMetrics(V3MetricPage),
    AdapterStatus(V3OperationStatus),
    Clusters(V3EntityPage),
    Workers(V3EntityPage),
    WorkerHealth(V3EntityRecord),
    JobStatus(V3OperationStatus),
    Claim(V3StableClaim),
    Events(V3EventPage),
    Error(V3Error),
}

impl V3Event {
    /// Validate every bounded event payload before presentation.
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Negotiated(value) => value.validate(),
            Self::Tools(value) => value.validate(),
            Self::ToolResult(value) => value.validate(),
            Self::MemorySpaces(value)
            | Self::MemorySearchResults(value)
            | Self::Models(value)
            | Self::Clusters(value)
            | Self::Workers(value) => value.validate(),
            Self::ToolStatus(value)
            | Self::ModelStatus(value)
            | Self::TrainingStatus(value)
            | Self::AdapterStatus(value)
            | Self::JobStatus(value) => value.validate(),
            Self::ToolApprovalStatus(value) => value.validate(),
            Self::MemoryMetadata(value)
            | Self::TrainingDatasetStatus(value)
            | Self::WorkerHealth(value) => value.validate(),
            Self::Claim(value) => value.validate(),
            Self::TrainingMetrics(value) => value.validate(),
            Self::Events(value) => value.validate(),
            Self::Error(value) => value.validate(),
        }
    }
}

pub(super) fn validate_id(value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(ValidationError::new("invalid v3 identifier"));
    }
    Ok(())
}

pub(super) fn validate_digest(value: &str) -> Result<(), ValidationError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(ValidationError::new("invalid v3 sha256 digest"));
    }
    Ok(())
}

pub(super) fn validate_text(
    value: &str,
    max_bytes: usize,
    message: &'static str,
) -> Result<(), ValidationError> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(ValidationError::new(message));
    }
    Ok(())
}
