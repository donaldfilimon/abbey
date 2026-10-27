//! `abbey.v1` capability catalog, canonical digests, and method names.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::validate::error;
use super::{
    CONTRACT_MAJOR, CONTRACT_REVISION, CORPUS_DIGEST, FederationError, FederationErrorCode,
    FederationMethod, SERVICE,
};

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

pub(super) fn describe_capability(parameters: &Value) -> Result<Value, FederationError> {
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

pub(super) fn method_name(method: FederationMethod) -> &'static str {
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
