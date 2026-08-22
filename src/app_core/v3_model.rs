//! Strict protocol-v3 contracts for exact loaded-model inference.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ValidationError, v3::validate_digest, v3::validate_id};

/// Maximum prompt bytes accepted by exact-model inference.
pub const MAX_V3_MODEL_PROMPT_BYTES: usize = 32 * 1024;
/// Maximum output bytes returned by exact-model inference.
pub const MAX_V3_MODEL_OUTPUT_BYTES: usize = 32 * 1024;
/// Maximum requested output tokens.
pub const MAX_V3_MODEL_OUTPUT_TOKENS: u16 = 256;

const REQUEST_DIGEST_DOMAIN: &[u8] = b"abbey.protocol-v3.exact-model-inference.request.v1\0";
const OUTPUT_DIGEST_DOMAIN: &[u8] = b"abbey.protocol-v3.exact-model-inference.output.v1\0";

/// Exact device requested at daemon startup or evidenced during execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3ModelDevice {
    Cpu,
    Metal,
    Cuda,
}

/// One bounded request for an already-loaded immutable model revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ModelInferenceRequest {
    pub model_id: String,
    pub revision: String,
    pub prompt: String,
    pub max_output_tokens: u16,
    pub request_digest: String,
}

impl V3ModelInferenceRequest {
    /// Build a request and bind every field to its domain-separated digest.
    pub fn new(
        model_id: impl Into<String>,
        revision: impl Into<String>,
        prompt: impl Into<String>,
        max_output_tokens: u16,
    ) -> Result<Self, ValidationError> {
        let mut request = Self {
            model_id: model_id.into(),
            revision: revision.into(),
            prompt: prompt.into(),
            max_output_tokens,
            request_digest: String::new(),
        };
        request.request_digest = request.computed_digest();
        request.validate()?;
        Ok(request)
    }

    /// Recompute the digest over the exact request fields.
    #[must_use]
    pub fn computed_digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(REQUEST_DIGEST_DOMAIN);
        digest_field(&mut digest, self.model_id.as_bytes());
        digest_field(&mut digest, self.revision.as_bytes());
        digest_field(&mut digest, self.prompt.as_bytes());
        digest.update(self.max_output_tokens.to_be_bytes());
        format!("{:x}", digest.finalize())
    }

    /// Validate all bounds and the exact request digest.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.model_id)?;
        validate_id(&self.revision)?;
        if self.prompt.trim().is_empty()
            || self.prompt.len() > MAX_V3_MODEL_PROMPT_BYTES
            || self.prompt.chars().any(char::is_control)
        {
            return Err(ValidationError::new("invalid v3 model prompt"));
        }
        if self.max_output_tokens == 0 || self.max_output_tokens > MAX_V3_MODEL_OUTPUT_TOKENS {
            return Err(ValidationError::new("invalid v3 model output-token limit"));
        }
        validate_digest(&self.request_digest)?;
        if self.request_digest != self.computed_digest() {
            return Err(ValidationError::new("v3 model request digest mismatch"));
        }
        Ok(())
    }
}

/// Bounded output and native execution evidence for one exact request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ModelInferenceResult {
    pub model_id: String,
    pub revision: String,
    pub request_digest: String,
    pub output_digest: String,
    pub output: String,
    pub requested_output_tokens: u16,
    pub prompt_tokens: u32,
    pub output_tokens: u32,
    pub requested_device: V3ModelDevice,
    pub executed_device: V3ModelDevice,
    pub native_operations: u64,
    pub fallback_used: bool,
    pub mixed_execution: bool,
}

impl V3ModelInferenceResult {
    /// Recompute the domain-separated digest over the returned output.
    #[must_use]
    pub fn computed_output_digest(&self) -> String {
        output_digest(
            &self.model_id,
            &self.revision,
            &self.request_digest,
            &self.output,
        )
    }

    /// Validate exact identities, output bounds, digests, and execution evidence.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.model_id)?;
        validate_id(&self.revision)?;
        validate_digest(&self.request_digest)?;
        validate_digest(&self.output_digest)?;
        if self.output.len() > MAX_V3_MODEL_OUTPUT_BYTES
            || self.requested_output_tokens == 0
            || self.requested_output_tokens > MAX_V3_MODEL_OUTPUT_TOKENS
            || self.prompt_tokens == 0
            || self.output_tokens > u32::from(self.requested_output_tokens)
            || self.native_operations == 0
            || self.requested_device != self.executed_device
            || self.fallback_used
            || self.mixed_execution
            || self.output_digest != self.computed_output_digest()
        {
            return Err(ValidationError::new("invalid v3 model inference result"));
        }
        Ok(())
    }

    /// Validate this result against the exact request that caused it.
    pub fn validate_for(&self, request: &V3ModelInferenceRequest) -> Result<(), ValidationError> {
        request.validate()?;
        self.validate()?;
        if self.model_id != request.model_id
            || self.revision != request.revision
            || self.request_digest != request.request_digest
            || self.requested_output_tokens != request.max_output_tokens
        {
            return Err(ValidationError::new(
                "v3 model inference correlation mismatch",
            ));
        }
        Ok(())
    }
}

pub(crate) fn output_digest(
    model_id: &str,
    revision: &str,
    request_digest: &str,
    output: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(OUTPUT_DIGEST_DOMAIN);
    digest_field(&mut digest, model_id.as_bytes());
    digest_field(&mut digest, revision.as_bytes());
    digest_field(&mut digest, request_digest.as_bytes());
    digest_field(&mut digest, output.as_bytes());
    format!("{:x}", digest.finalize())
}

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(value);
}
