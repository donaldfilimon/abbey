//! Strict `abbey.v1` federation envelope and fail-closed reference service.
//!
//! This protocol is intentionally separate from Abbey's historical numeric
//! daemon protocols. Authority-bearing requests never downgrade into them.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

pub const SERVICE: &str = "abbey.v1";
pub const CONTRACT_MAJOR: u32 = 2;
pub const CONTRACT_REVISION: u32 = 2;
pub const CORPUS_DIGEST: &str =
    "sha256:3ffd487bdc497b7ce54b8c29978a3686dcbffdb66a85957a0ee4f99ba576cdfd";
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_JSON_CONTAINER_DEPTH: usize = 32;
pub const MAX_IDENTIFIER_BYTES: usize = 64;
pub const MAX_COLLECTION_ITEMS: usize = 2_048;
pub const MAX_PARAMETER_PROPERTIES: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum FederationMethod {
    Hello,
    GetStatus,
    Authorize,
    Cognize,
    ProposeChange,
    ApproveChange,
    ExecuteChange,
    CompensateChange,
    RetrieveEpisodes,
    ProposeEpisodeWrite,
    ListCapabilities,
    DescribeCapability,
    PreviewManifest,
    ApplyManifest,
    OpenConsentEpoch,
    AttestConsent,
    CloseConsentEpoch,
    ResumeConsentEpoch,
    WatchEvents,
}

impl FederationMethod {
    #[must_use]
    pub const fn is_authority_bearing(self) -> bool {
        !matches!(
            self,
            Self::Hello
                | Self::GetStatus
                | Self::ListCapabilities
                | Self::DescribeCapability
                | Self::PreviewManifest
                | Self::WatchEvents
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FederationRequest {
    pub service: String,
    pub contract_major: u32,
    pub contract_revision: u32,
    pub corpus_digest: String,
    pub capability_manifest_digest: String,
    pub request_id: String,
    pub method: FederationMethod,
    pub parameters_digest: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FederationResponse {
    pub service: String,
    pub contract_major: u32,
    pub contract_revision: u32,
    pub request_id: String,
    pub payload: FederationPayload,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum FederationPayload {
    Ok { result: Value },
    Error { error: FederationError },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FederationError {
    pub code: FederationErrorCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FederationErrorCode {
    MalformedRequest,
    LimitExceeded,
    ContractMismatch,
    CapabilityManifestMismatch,
    ParametersDigestMismatch,
    InvalidRequestId,
    InvalidParameters,
    RateLimited,
    CapabilityDisabled,
    Internal,
}

impl FederationResponse {
    #[must_use]
    pub fn ok(request_id: impl Into<String>, result: Value) -> Self {
        Self {
            service: SERVICE.to_owned(),
            contract_major: CONTRACT_MAJOR,
            contract_revision: CONTRACT_REVISION,
            request_id: response_request_id(request_id.into()),
            payload: FederationPayload::Ok { result },
        }
    }

    #[must_use]
    pub fn error(
        request_id: impl Into<String>,
        code: FederationErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            service: SERVICE.to_owned(),
            contract_major: CONTRACT_MAJOR,
            contract_revision: CONTRACT_REVISION,
            request_id: response_request_id(request_id.into()),
            payload: FederationPayload::Error {
                error: FederationError {
                    code,
                    message: message.into(),
                },
            },
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FederationService;

impl FederationService {
    pub fn handle(&self, request: FederationRequest) -> Result<Value, FederationError> {
        validate_request(&request)?;
        match request.method {
            FederationMethod::Hello => Ok(hello()),
            FederationMethod::GetStatus => Ok(json!({
                "daemon": "ready",
                "deployment_profile": "developer",
                "effects_enabled": false,
                "best_effort_executors_registered": false,
                "contract_digest": CORPUS_DIGEST,
                "capability_manifest_digest": capability_manifest_digest(),
            })),
            FederationMethod::ListCapabilities => Ok(capability_manifest()),
            FederationMethod::DescribeCapability => describe_capability(&request.parameters),
            FederationMethod::PreviewManifest => Ok(json!({
                "manifest": capability_manifest(),
                "digest": capability_manifest_digest(),
                "apply_requires_separate_authorization": true,
            })),
            FederationMethod::WatchEvents => Ok(json!({
                "events": [],
                "next_cursor": null,
                "live_subscription": false,
            })),
            method => Err(FederationError {
                code: FederationErrorCode::CapabilityDisabled,
                message: format!(
                    "{} is not registered in this deployment profile",
                    method_name(method)
                ),
            }),
        }
    }
}

/// Send-once client for the separate contract-governed local interface.
///
/// It has no legacy protocol list, retry loop, or alternate backend.
#[derive(Clone, Debug)]
pub struct FederationClient {
    socket_path: PathBuf,
    timeout: Duration,
}

impl FederationClient {
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            timeout: Duration::from_secs(5),
        }
    }

    pub fn request(
        &self,
        method: FederationMethod,
        parameters: Value,
    ) -> Result<FederationResponse, FederationClientError> {
        let request = FederationRequest {
            service: SERVICE.to_owned(),
            contract_major: CONTRACT_MAJOR,
            contract_revision: CONTRACT_REVISION,
            corpus_digest: CORPUS_DIGEST.to_owned(),
            capability_manifest_digest: capability_manifest_digest(),
            request_id: format!("request_{}", uuid::Uuid::new_v4().simple()),
            method,
            parameters_digest: parameters_digest(&parameters),
            parameters,
        };
        self.request_exact(&request)
    }

    #[cfg(unix)]
    pub fn request_exact(
        &self,
        request: &FederationRequest,
    ) -> Result<FederationResponse, FederationClientError> {
        use std::io::{Read as _, Write as _};
        use std::os::unix::net::UnixStream;

        let bytes = serde_json::to_vec(request).map_err(FederationClientError::Json)?;
        if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
            return Err(FederationClientError::RequestTooLarge);
        }
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|source| {
            FederationClientError::Connect {
                path: self.socket_path.clone(),
                source,
            }
        })?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(FederationClientError::Io)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(FederationClientError::Io)?;
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .and_then(|()| stream.write_all(&bytes))
            .and_then(|()| stream.flush())
            .map_err(FederationClientError::Io)?;

        let mut prefix = [0_u8; 4];
        stream
            .read_exact(&mut prefix)
            .map_err(FederationClientError::Io)?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length == 0 || length > MAX_FRAME_BYTES {
            return Err(FederationClientError::ResponseTooLarge);
        }
        let mut response_bytes = vec![0_u8; length];
        stream
            .read_exact(&mut response_bytes)
            .map_err(FederationClientError::Io)?;
        let value: Value =
            serde_json::from_slice(&response_bytes).map_err(FederationClientError::Json)?;
        if !value_within_limits(&value, 1) {
            return Err(FederationClientError::InvalidResponse);
        }
        let response: FederationResponse =
            serde_json::from_value(value).map_err(FederationClientError::Json)?;
        if response.service != SERVICE
            || response.contract_major != CONTRACT_MAJOR
            || response.contract_revision != CONTRACT_REVISION
            || response.request_id != request.request_id
        {
            return Err(FederationClientError::InvalidResponse);
        }
        Ok(response)
    }

    #[cfg(not(unix))]
    pub fn request_exact(
        &self,
        _request: &FederationRequest,
    ) -> Result<FederationResponse, FederationClientError> {
        Err(FederationClientError::UnsupportedPlatform)
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

#[derive(Debug, Error)]
pub enum FederationClientError {
    #[error("abbey.v1 local transport requires Unix domain sockets")]
    UnsupportedPlatform,
    #[error("cannot connect to abbey.v1 socket at {path}: {source}")]
    Connect {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("abbey.v1 I/O failed: {0}")]
    Io(std::io::Error),
    #[error("abbey.v1 JSON failed: {0}")]
    Json(serde_json::Error),
    #[error("abbey.v1 request exceeds the frame limit")]
    RequestTooLarge,
    #[error("abbey.v1 response exceeds the frame limit")]
    ResponseTooLarge,
    #[error("abbey.v1 response failed correlation or structural validation")]
    InvalidResponse,
}

#[must_use]
pub fn hello() -> Value {
    json!({
        "service": SERVICE,
        "supported_contract_majors": [CONTRACT_MAJOR],
        "contract_revision": CONTRACT_REVISION,
        "corpus_digest": CORPUS_DIGEST,
        "capability_manifest_digest": capability_manifest_digest(),
        "deployment_profile": "developer",
    })
}

#[must_use]
pub fn capability_manifest() -> Value {
    json!({
        "schema_version": 1,
        "service": SERVICE,
        "contract_major": CONTRACT_MAJOR,
        "contract_revision": CONTRACT_REVISION,
        "capabilities": [
            {"id":"federation.metadata.read","methods":["Hello","GetStatus"],"effect":"read_only","enabled":true},
            {"id":"federation.capabilities.read","methods":["ListCapabilities","DescribeCapability","PreviewManifest"],"effect":"read_only","enabled":true},
            {"id":"federation.events.read","methods":["WatchEvents"],"effect":"read_only","enabled":true,"live_subscription":false},
            {"id":"federation.authorization","methods":["Authorize"],"effect":"authority","enabled":false},
            {"id":"federation.cognition","methods":["Cognize"],"effect":"cognition","enabled":false},
            {"id":"federation.changes","methods":["ProposeChange","ApproveChange","ExecuteChange","CompensateChange"],"effect":"platform_effect","enabled":false,"best_effort_executors_registered":false},
            {"id":"federation.episodes","methods":["RetrieveEpisodes","ProposeEpisodeWrite"],"effect":"durable_write","enabled":false},
            {"id":"federation.manifests.apply","methods":["ApplyManifest"],"effect":"local_effect","enabled":false},
            {"id":"federation.consent","methods":["OpenConsentEpoch","AttestConsent","CloseConsentEpoch","ResumeConsentEpoch"],"effect":"consent_state","enabled":false}
        ]
    })
}

#[must_use]
pub fn capability_manifest_digest() -> String {
    digest_value(&capability_manifest())
}

#[must_use]
pub fn parameters_digest(parameters: &Value) -> String {
    digest_value(parameters)
}

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

fn describe_capability(parameters: &Value) -> Result<Value, FederationError> {
    let id = parameters
        .as_object()
        .filter(|map| map.len() == 1)
        .and_then(|map| map.get("capability_id"))
        .and_then(Value::as_str)
        .filter(|id| id.len() <= 128)
        .ok_or_else(|| {
            error(
                FederationErrorCode::InvalidParameters,
                "capability_id is required",
            )
        })?;
    capability_manifest()["capabilities"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == id))
        .cloned()
        .ok_or_else(|| {
            error(
                FederationErrorCode::CapabilityDisabled,
                "capability is not declared",
            )
        })
}

fn digest_value(value: &Value) -> String {
    let bytes = serde_json::to_vec(&canonical_json(value)).expect("JSON value serializes");
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = serde_json::Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonical_json(&values[key]));
            }
            Value::Object(canonical)
        }
        other => other.clone(),
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

fn response_request_id(value: String) -> String {
    if valid_identifier(&value) {
        value
    } else {
        String::new()
    }
}

fn method_name(method: FederationMethod) -> &'static str {
    match method {
        FederationMethod::Hello => "Hello",
        FederationMethod::GetStatus => "GetStatus",
        FederationMethod::Authorize => "Authorize",
        FederationMethod::Cognize => "Cognize",
        FederationMethod::ProposeChange => "ProposeChange",
        FederationMethod::ApproveChange => "ApproveChange",
        FederationMethod::ExecuteChange => "ExecuteChange",
        FederationMethod::CompensateChange => "CompensateChange",
        FederationMethod::RetrieveEpisodes => "RetrieveEpisodes",
        FederationMethod::ProposeEpisodeWrite => "ProposeEpisodeWrite",
        FederationMethod::ListCapabilities => "ListCapabilities",
        FederationMethod::DescribeCapability => "DescribeCapability",
        FederationMethod::PreviewManifest => "PreviewManifest",
        FederationMethod::ApplyManifest => "ApplyManifest",
        FederationMethod::OpenConsentEpoch => "OpenConsentEpoch",
        FederationMethod::AttestConsent => "AttestConsent",
        FederationMethod::CloseConsentEpoch => "CloseConsentEpoch",
        FederationMethod::ResumeConsentEpoch => "ResumeConsentEpoch",
        FederationMethod::WatchEvents => "WatchEvents",
    }
}

fn error(code: FederationErrorCode, message: &str) -> FederationError {
    FederationError {
        code,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: FederationMethod, parameters: Value) -> FederationRequest {
        FederationRequest {
            service: SERVICE.into(),
            contract_major: CONTRACT_MAJOR,
            contract_revision: CONTRACT_REVISION,
            corpus_digest: CORPUS_DIGEST.into(),
            capability_manifest_digest: capability_manifest_digest(),
            request_id: "request_ref".into(),
            method,
            parameters_digest: parameters_digest(&parameters),
            parameters,
        }
    }

    #[test]
    fn manifest_and_parameter_digests_are_deterministic_across_key_order() {
        assert_eq!(capability_manifest_digest(), capability_manifest_digest());
        assert_eq!(
            parameters_digest(&json!({"b": 2, "a": 1})),
            parameters_digest(&json!({"a": 1, "b": 2}))
        );
    }

    #[test]
    fn mismatches_fail_before_a_method_can_run() {
        let service = FederationService;
        let mut stale = request(FederationMethod::GetStatus, json!({}));
        stale.contract_revision = 1;
        assert_eq!(
            validate_request(&stale).unwrap_err().code,
            FederationErrorCode::ContractMismatch
        );
        assert_eq!(
            service.handle(stale).unwrap_err().code,
            FederationErrorCode::ContractMismatch,
            "the service boundary must not rely on transport validation"
        );
    }

    #[test]
    fn read_only_catalog_works_and_authority_methods_are_disabled() {
        let service = FederationService;
        let list = request(FederationMethod::ListCapabilities, json!({}));
        validate_request(&list).unwrap();
        assert_eq!(
            service.handle(list).unwrap()["capabilities"]
                .as_array()
                .unwrap()
                .len(),
            9
        );
        let change = request(FederationMethod::ExecuteChange, json!({}));
        validate_request(&change).unwrap();
        assert_eq!(
            service.handle(change).unwrap_err().code,
            FederationErrorCode::CapabilityDisabled
        );
    }

    #[test]
    fn depth_and_collection_limits_are_exact() {
        assert!(value_within_limits(
            &Value::Array(vec![Value::Null; 2_048]),
            1
        ));
        assert!(!value_within_limits(
            &Value::Array(vec![Value::Null; 2_049]),
            1
        ));
        let mut value = json!({});
        for _ in 0..31 {
            value = json!({"nested": value});
        }
        assert!(value_within_limits(&value, 1));
        value = json!({"nested": value});
        assert!(!value_within_limits(&value, 1));
    }

    #[test]
    fn request_parameters_respect_the_contract_property_bound() {
        let parameters = Value::Object(
            (0..33)
                .map(|index| (format!("key_{index}"), Value::Null))
                .collect(),
        );
        let invalid = request(FederationMethod::GetStatus, parameters);
        assert_eq!(
            validate_request(&invalid).unwrap_err().code,
            FederationErrorCode::InvalidParameters
        );
    }
}
