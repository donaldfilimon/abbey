//! Internal permission-profile configuration records; no execution grants.
use super::ValidationError;
use serde::{Deserialize, Serialize};
use std::fmt;
const MAX_V4_ITEMS: usize = 64;
const MAX_V4_ROOT: usize = 512;
const MAX_V4_DOMAIN: usize = 253;
const MAX_V4_RUNTIME_MS: u64 = 3_600_000;
const MAX_V4_OUTPUT_BYTES: u64 = 1_048_576;
fn validate_id(value: &str) -> Result<(), ValidationError> {
    super::v4::route(value)
}
/// Bounded permission profile. Lists name authority; they do not execute it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V4PermissionProfile {
    pub profile_id: String,
    pub tool_ids: Vec<String>,
    pub application_ids: Vec<String>,
    pub filesystem_roots: Vec<String>,
    pub browser_domains: Vec<String>,
    pub max_runtime_ms: u64,
    pub max_output_bytes: u64,
}

impl V4PermissionProfile {
    /// Validate bounded identifiers, absolute roots, hostnames, and ceilings.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_id(&self.profile_id)?;
        validate_id_list(&self.tool_ids)?;
        validate_id_list(&self.application_ids)?;
        validate_roots(&self.filesystem_roots)?;
        validate_domains(&self.browser_domains)?;
        positive_at_most(
            self.max_runtime_ms,
            MAX_V4_RUNTIME_MS,
            "invalid v4 runtime limit",
        )?;
        positive_at_most(
            self.max_output_bytes,
            MAX_V4_OUTPUT_BYTES,
            "invalid v4 output limit",
        )
    }
}

impl fmt::Debug for V4PermissionProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4PermissionProfile([redacted])")
    }
}
fn positive_at_most(value: u64, max: u64, message: &'static str) -> Result<(), ValidationError> {
    if value == 0 || value > max {
        Err(ValidationError::new(message))
    } else {
        Ok(())
    }
}

fn validate_id_list(values: &[String]) -> Result<(), ValidationError> {
    if values.len() > MAX_V4_ITEMS {
        return Err(ValidationError::new("invalid v4 permission profile"));
    }
    values.iter().try_for_each(|value| validate_id(value))
}

fn validate_roots(values: &[String]) -> Result<(), ValidationError> {
    if values.len() > MAX_V4_ITEMS {
        return Err(ValidationError::new("invalid v4 permission profile"));
    }
    for value in values {
        if value.is_empty()
            || value.len() > MAX_V4_ROOT
            || value.chars().any(char::is_control)
            || !absolute_root(value)
        {
            return Err(ValidationError::new("invalid v4 filesystem root"));
        }
    }
    Ok(())
}

fn absolute_root(value: &str) -> bool {
    if value.starts_with('/') {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn validate_domains(values: &[String]) -> Result<(), ValidationError> {
    if values.len() > MAX_V4_ITEMS {
        return Err(ValidationError::new("invalid v4 permission profile"));
    }
    for value in values {
        if value.is_empty()
            || value.len() > MAX_V4_DOMAIN
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        {
            return Err(ValidationError::new("invalid v4 browser domain"));
        }
    }
    Ok(())
}
