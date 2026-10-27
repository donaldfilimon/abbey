//! `abbey.v1` request validation and structural limits.

use serde_json::Value;

use super::{
    CONTRACT_MAJOR, CONTRACT_REVISION, CORPUS_DIGEST, FederationError, FederationErrorCode,
    FederationRequest, MAX_COLLECTION_ITEMS, MAX_IDENTIFIER_BYTES, MAX_JSON_CONTAINER_DEPTH,
    MAX_PARAMETER_PROPERTIES, SERVICE, capability_manifest_digest, parameters_digest,
};

pub fn validate_request(request: &FederationRequest) -> Result<(), FederationError> {
    if request.service != SERVICE
        || request.contract_major != CONTRACT_MAJOR
        || request.contract_revision != CONTRACT_REVISION
        || request.corpus_digest != CORPUS_DIGEST
    {
        return Err(error(
            FederationErrorCode::ContractMismatch,
            "contract identity does not match this daemon",
        ));
    }
    if request.capability_manifest_digest != capability_manifest_digest() {
        return Err(error(
            FederationErrorCode::CapabilityManifestMismatch,
            "capability manifest digest does not match this daemon",
        ));
    }
    if !valid_identifier(&request.request_id) {
        return Err(error(
            FederationErrorCode::InvalidRequestId,
            "request_id is invalid",
        ));
    }
    if request
        .parameters
        .as_object()
        .is_none_or(|parameters| parameters.len() > MAX_PARAMETER_PROPERTIES)
        || !value_within_limits(&request.parameters, 1)
    {
        return Err(error(
            FederationErrorCode::InvalidParameters,
            "parameters exceed their structural limits",
        ));
    }
    if request.parameters_digest != parameters_digest(&request.parameters) {
        return Err(error(
            FederationErrorCode::ParametersDigestMismatch,
            "parameters digest does not match the request body",
        ));
    }
    Ok(())
}

#[must_use]
pub fn value_within_limits(value: &Value, depth: usize) -> bool {
    if depth > MAX_JSON_CONTAINER_DEPTH {
        return false;
    }
    match value {
        Value::Array(items) => {
            items.len() <= MAX_COLLECTION_ITEMS
                && items
                    .iter()
                    .all(|item| value_within_limits(item, depth + 1))
        }
        Value::Object(map) => {
            map.len() <= MAX_COLLECTION_ITEMS
                && map
                    .values()
                    .all(|item| value_within_limits(item, depth + 1))
        }
        _ => true,
    }
}

fn valid_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_IDENTIFIER_BYTES
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(byte))
}

pub(super) fn response_request_id(value: String) -> String {
    if valid_identifier(&value) {
        value
    } else {
        String::new()
    }
}

pub(super) fn error(code: FederationErrorCode, message: &str) -> FederationError {
    FederationError {
        code,
        message: message.to_owned(),
    }
}
