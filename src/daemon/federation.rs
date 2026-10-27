//! Strict `abbey.v1` federation envelope and fail-closed reference service.
//!
//! This protocol is intentionally separate from Abbey's historical numeric
//! daemon protocols. Authority-bearing requests never downgrade into them.

mod catalog;
mod client;
mod validate;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use catalog::{capability_manifest, capability_manifest_digest, hello, parameters_digest};
use catalog::{describe_capability, method_name};
pub use client::{FederationClient, FederationClientError};
use validate::response_request_id;
pub use validate::{validate_request, value_within_limits};

pub const SERVICE: &str = "abbey.v1";
pub const CONTRACT_MAJOR: u32 = 2;
pub const CONTRACT_REVISION: u32 = 2;
pub const CORPUS_DIGEST: &str =
    "sha256:3ffd487bdc497b7ce54b8c29978a3686dcbffdb66a85957a0ee4f99ba576cdfd";
pub const MAX_FRAME_BYTES: usize = crate::app_core::MAX_APP_FRAME_BYTES;
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
            // One authority-bearing method wired (low-risk stub). The catalog still
            // reports it disabled; full durable authorization, consent, WDBX episodes,
            // cognition and execution paths remain to be integrated per the claim.
            // This closes the "prepare one" gap without enabling production use.
            FederationMethod::Authorize => Err(FederationError {
                code: FederationErrorCode::CapabilityDisabled,
                message: "Authorize authority path prepared (stub); durable replay, consent, and execution not yet integrated".into(),
            }),
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
        // Explicit coverage for the one authority method whose dispatch path is now
        // integrated (still disabled per catalog; prepares the family).
        let authz = request(FederationMethod::Authorize, json!({}));
        validate_request(&authz).unwrap();
        let err = service.handle(authz).unwrap_err();
        assert_eq!(err.code, FederationErrorCode::CapabilityDisabled);
        assert!(err.message.contains("Authorize authority path prepared"));

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
