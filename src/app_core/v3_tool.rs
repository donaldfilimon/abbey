//! Tool-specific protocol-v3 request and descriptor contracts.

use super::v3::{validate_digest, validate_id, validate_text};
use super::{MAX_V3_PAGE, V3OperationState, ValidationError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_TOOL_DESCRIPTION_BYTES: usize = 1_024;
const MAX_TOOL_JSON_BYTES: usize = 32 * 1_024;
const MAX_TOOL_JSON_DEPTH: usize = 16;
const TOOL_CALL_DIGEST_DOMAIN: &[u8] = b"abbey:v3-tool-call:v1";

/// Declared effect of one protocol-v3 tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3ToolEffect {
    /// Observes state and may run without an approval.
    ReadOnly,
    /// Changes state reversibly and requires an exact-call approval.
    Mutating,
    /// Changes state irreversibly and is not served by the safe edition.
    Destructive,
}

/// One advertised tool and its bounded JSON input schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ToolDescriptor {
    pub tool_id: String,
    pub description: String,
    pub effect: V3ToolEffect,
    pub input_schema: serde_json::Value,
}

impl V3ToolDescriptor {
    /// Validate the identifier, description, and structural schema bounds.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.tool_id)?;
        validate_text(
            &self.description,
            MAX_TOOL_DESCRIPTION_BYTES,
            "invalid v3 tool description",
        )?;
        validate_json_object(&self.input_schema, "invalid or oversized v3 tool schema")
    }
}

/// Fixed-watermark page of tool descriptors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ToolPage {
    pub after: u64,
    pub through: u64,
    pub tools: Vec<V3ToolDescriptor>,
}

impl V3ToolPage {
    /// Validate page bounds and every advertised schema.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.after > self.through || self.tools.len() > usize::from(MAX_V3_PAGE) {
            return Err(ValidationError::new("invalid v3 tool page"));
        }
        self.tools.iter().try_for_each(V3ToolDescriptor::validate)
    }
}

/// Schema-validated tool call request. Input must be one bounded JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ToolCall {
    pub tool_id: String,
    pub call_id: String,
    pub input: serde_json::Value,
}

/// Bounded terminal result from one schema-validated safe tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ToolResult {
    pub tool_id: String,
    pub call_id: String,
    pub state: V3OperationState,
    pub output: serde_json::Value,
}

impl V3ToolResult {
    /// Validate correlation, terminal state, and bounded JSON output.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.tool_id)?;
        validate_id(&self.call_id)?;
        if !matches!(
            self.state,
            V3OperationState::Succeeded | V3OperationState::Failed
        ) {
            return Err(ValidationError::new("v3 tool result must be terminal"));
        }
        validate_json_value(&self.output, "invalid or oversized v3 tool output")
    }
}

impl V3ToolCall {
    /// Validate identifiers and structural JSON bounds before policy evaluation.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.tool_id)?;
        validate_id(&self.call_id)?;
        validate_json_object(&self.input, "invalid or oversized v3 tool input")
    }

    /// Compute the domain-separated digest used by exact-call approvals.
    pub fn approval_digest(&self) -> Result<String, ValidationError> {
        self.validate()?;
        let input = serde_json::to_vec(&canonical_json(&self.input))
            .map_err(|_| ValidationError::new("invalid or oversized v3 tool input"))?;
        let mut digest = Sha256::new();
        digest.update(TOOL_CALL_DIGEST_DOMAIN);
        digest_field(&mut digest, self.call_id.as_bytes());
        digest_field(&mut digest, self.tool_id.as_bytes());
        digest_field(&mut digest, &input);
        Ok(format!("{:x}", digest.finalize()))
    }
}

/// Durable lifecycle state of one digest-bound tool approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3ToolApprovalState {
    Pending,
    Approved,
    Denied,
    Cancelled,
    Expired,
    Consumed,
}

/// Typed status of one exact tool-call approval without raw tool input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ToolApprovalStatus {
    pub tool_id: String,
    pub call_id: String,
    pub call_digest: String,
    pub state: V3ToolApprovalState,
    pub expires_at_ms: u64,
}

impl V3ToolApprovalStatus {
    /// Validate correlation, exact digest, and a nonzero server expiry.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.tool_id)?;
        validate_id(&self.call_id)?;
        validate_digest(&self.call_digest)?;
        if self.expires_at_ms == 0 {
            return Err(ValidationError::new("invalid v3 tool approval expiry"));
        }
        Ok(())
    }
}

/// Outcome of one non-replayed protocol-v3 tool request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum V3ToolInvocation {
    /// A read-only tool completed immediately.
    Completed(V3ToolResult),
    /// A mutating tool was not executed and awaits an exact-call approval.
    ApprovalRequired(V3ToolApprovalStatus),
}

/// Approval decision bound to the exact tool call digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ToolDecision {
    pub call_id: String,
    pub call_digest: String,
    pub decision_id: String,
}

impl V3ToolDecision {
    /// Validate identifiers and a lowercase SHA-256 digest.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.call_id)?;
        validate_digest(&self.call_digest)?;
        validate_id(&self.decision_id)
    }
}

fn validate_json_object(
    value: &serde_json::Value,
    message: &'static str,
) -> Result<(), ValidationError> {
    if !value.is_object() {
        return Err(ValidationError::new(message));
    }
    validate_json_value(value, message)
}

fn validate_json_value(
    value: &serde_json::Value,
    message: &'static str,
) -> Result<(), ValidationError> {
    if serde_json::to_vec(value).map_or(true, |encoded| encoded.len() > MAX_TOOL_JSON_BYTES)
        || json_depth(value) > MAX_TOOL_JSON_DEPTH
    {
        return Err(ValidationError::new(message));
    }
    Ok(())
}

fn json_depth(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Array(values) => {
            1 + values.iter().map(json_depth).max().unwrap_or_default()
        }
        serde_json::Value::Object(values) => {
            1 + values.values().map(json_depth).max().unwrap_or_default()
        }
        _ => 0,
    }
}

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(value);
}

fn canonical_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonical_json).collect())
        }
        serde_json::Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = serde_json::Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonical_json(&values[key]));
            }
            serde_json::Value::Object(canonical)
        }
        scalar => scalar.clone(),
    }
}
