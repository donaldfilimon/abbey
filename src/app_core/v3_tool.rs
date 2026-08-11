//! Tool-specific protocol-v3 request and descriptor contracts.

use super::v3::{validate_digest, validate_id, validate_text};
use super::{MAX_V3_PAGE, ValidationError};
use serde::{Deserialize, Serialize};

const MAX_TOOL_DESCRIPTION_BYTES: usize = 1_024;
const MAX_TOOL_JSON_BYTES: usize = 32 * 1_024;
const MAX_TOOL_JSON_DEPTH: usize = 16;

/// One advertised tool and its bounded JSON input schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ToolDescriptor {
    pub tool_id: String,
    pub description: String,
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

impl V3ToolCall {
    /// Validate identifiers and structural JSON bounds before policy evaluation.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.tool_id)?;
        validate_id(&self.call_id)?;
        validate_json_object(&self.input, "invalid or oversized v3 tool input")
    }
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
    if !value.is_object()
        || serde_json::to_vec(value).map_or(true, |encoded| encoded.len() > MAX_TOOL_JSON_BYTES)
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
