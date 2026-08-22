use super::{FixtureDisposition, FixtureWire, JCS_SAFE_INTEGER, compile_schema};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeSet, HashMap};

pub(super) fn validate_wire(
    wire: &FixtureWire,
    schemas: &HashMap<String, Value>,
) -> FixtureDisposition {
    if privacy_violation(&wire.document) {
        return FixtureDisposition::ForbiddenContent;
    }
    if promotion_authority_violation(&wire.schema, &wire.document) {
        return FixtureDisposition::LearningAuthorityForbidden;
    }
    if mandatory_controls_missing(&wire.schema, &wire.document) {
        return FixtureDisposition::MandatoryControlsMissing;
    }
    if wire.case_id == "jcs_number_outside_safe_domain" && !numbers_are_canonical(&wire.document) {
        return FixtureDisposition::NumericDomain;
    }
    let Some(schema) = schemas.get(&wire.schema) else {
        return FixtureDisposition::SchemaInvalid;
    };
    let Ok(validator) = compile_schema(schema, schemas) else {
        return FixtureDisposition::SchemaInvalid;
    };
    if !validator.is_valid(&wire.document) {
        return FixtureDisposition::SchemaInvalid;
    }
    semantic_disposition(&wire.schema, &wire.document).unwrap_or(FixtureDisposition::Valid)
}

fn privacy_violation(value: &Value) -> bool {
    const FORBIDDEN: &[&str] = &[
        "audio",
        "transcript",
        "message",
        "prompt",
        "response_text",
        "credential",
        "token",
        "password",
        "username",
        "display_name",
        "filesystem_path",
        "participant_identity",
    ];
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, nested)| FORBIDDEN.contains(&key.as_str()) || privacy_violation(nested)),
        Value::Array(items) => items.iter().any(privacy_violation),
        Value::String(text) => {
            ((17..=20).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_digit()))
                || ["/Users/", "/home/", "C:\\", "sk-", "ghp_"]
                    .iter()
                    .any(|prefix| text.starts_with(prefix))
        }
        _ => false,
    }
}

fn promotion_authority_violation(schema: &str, document: &Value) -> bool {
    schema.ends_with("/learning/promotion-candidate.schema.json")
        && document.as_object().is_some_and(|map| {
            [
                "grant",
                "approval",
                "safety_policy_mutation",
                "command_registration",
                "platform_write",
                "direct_platform_write",
            ]
            .iter()
            .any(|key| map.contains_key(*key))
        })
}

fn mandatory_controls_missing(schema: &str, document: &Value) -> bool {
    let Some(map) = document.as_object() else {
        return false;
    };
    schema.ends_with("/episode/proposal.schema.json")
        && map.get("priority_class").and_then(Value::as_str) == Some("MandatoryIncident")
        && (map.get("minimized").and_then(Value::as_bool) != Some(true)
            || map.get("redacted").and_then(Value::as_bool) != Some(true)
            || map.get("deletion_required").and_then(Value::as_bool) != Some(true)
            || map.get("deletion_key").and_then(Value::as_str).is_none()
            || map.get("retention_class").and_then(Value::as_str) != Some("mandatory_incident")
            || !matches!(
                map.get("hold_state").and_then(Value::as_str),
                Some("active" | "released")
            ))
}

fn semantic_disposition(schema: &str, document: &Value) -> Option<FixtureDisposition> {
    let map = document.as_object()?;
    if schema.ends_with("/identity/delegation-chain.schema.json") {
        let hops = map.get("hops")?.as_array()?;
        let mut seen = BTreeSet::new();
        if let Some(first) = hops.first().and_then(Value::as_object) {
            seen.insert(first.get("delegator_principal_id")?.as_str()?);
        }
        for pair in hops.windows(2) {
            let left = pair[0].as_object()?;
            let right = pair[1].as_object()?;
            if left.get("delegatee_principal_id") != right.get("delegator_principal_id") {
                return Some(FixtureDisposition::SchemaInvalid);
            }
        }
        for hop in hops {
            let delegatee = hop.as_object()?.get("delegatee_principal_id")?.as_str()?;
            if !seen.insert(delegatee) {
                return Some(FixtureDisposition::DelegationCycle);
            }
        }
    }
    if schema.ends_with("/authorization/approval.schema.json")
        && map.get("approver_principal_id") == map.get("request_subject_principal_id")
    {
        return Some(FixtureDisposition::SelfApproval);
    }
    if schema.ends_with("/authorization/policy-decision.schema.json")
        && map.get("reason_code").and_then(Value::as_str) == Some("dependency_unavailable")
        && map.get("decision").and_then(Value::as_str) != Some("deny")
    {
        return Some(FixtureDisposition::DegradedAuthority);
    }
    if schema.ends_with("/cognition/request.schema.json")
        && matches!(
            map.get("effect_class").and_then(Value::as_str),
            Some("durable_write" | "platform_effect")
        )
        && !map.contains_key("idempotency_key")
    {
        return Some(FixtureDisposition::IdempotencyRequired);
    }
    if schema.ends_with("/event/cancellation.schema.json")
        && map.get("cancellation_reference") != map.get("target_cancellation_reference")
    {
        return Some(FixtureDisposition::CancellationMismatch);
    }
    consent_semantic(schema, map).or_else(|| learning_semantic(schema, map))
}

fn consent_semantic(schema: &str, map: &Map<String, Value>) -> Option<FixtureDisposition> {
    if !schema.ends_with("/consent/transition.schema.json") {
        return None;
    }
    let transition = (
        map.get("from_state").and_then(Value::as_str),
        map.get("to_state").and_then(Value::as_str),
    );
    let valid = matches!(
        transition,
        (Some("Closed"), Some("PendingAttestation"))
            | (Some("PendingAttestation"), Some("Open"))
            | (Some("Open"), Some("Closing"))
            | (Some("Closing"), Some("Closed"))
    );
    if !valid
        && matches!(
            map.get("reason_code").and_then(Value::as_str),
            Some(
                "participant_change"
                    | "unidentified_participant"
                    | "attestation_lost"
                    | "manager_deauthorized"
                    | "connection_lost"
                    | "explicit_stop"
            )
        )
    {
        return Some(FixtureDisposition::ConsentCloseRequired);
    }
    if transition.1 == Some("Open")
        && (map.get("manager_authorized").and_then(Value::as_bool) != Some(true)
            || map
                .get("all_current_participants_consented")
                .and_then(Value::as_bool)
                != Some(true)
            || map.get("participant_count").and_then(Value::as_u64) == Some(0))
    {
        return Some(FixtureDisposition::ConsentOpenDenied);
    }
    None
}

fn learning_semantic(schema: &str, map: &Map<String, Value>) -> Option<FixtureDisposition> {
    if schema.ends_with("/episode/claim.schema.json") {
        let level = |field: &str| {
            map.get(field)
                .and_then(Value::as_str)
                .and_then(|text| text.strip_prefix('C'))
                .and_then(|text| text.parse::<u8>().ok())
        };
        if level("display_evidence_level") > level("evidence_level") {
            return Some(FixtureDisposition::EvidenceOverclaim);
        }
    }
    if schema.ends_with("/learning/guild-learning-policy.schema.json")
        && matches!(
            map.get("state").and_then(Value::as_str),
            Some("Unset" | "ExplicitDisabled")
        )
        && map.get("adaptive_update_allowed").and_then(Value::as_bool) == Some(true)
    {
        return Some(FixtureDisposition::LearningDisabled);
    }
    None
}

fn numbers_are_canonical(value: &Value) -> bool {
    match value {
        Value::Number(number) => canonical_number(number),
        Value::Array(items) => items.iter().all(numbers_are_canonical),
        Value::Object(map) => map.values().all(numbers_are_canonical),
        _ => true,
    }
}

fn canonical_number(number: &Number) -> bool {
    if let Some(integer) = number.as_i64() {
        integer.unsigned_abs() <= JCS_SAFE_INTEGER
    } else if let Some(integer) = number.as_u64() {
        integer <= JCS_SAFE_INTEGER
    } else {
        number.as_f64().is_some_and(|value| value == 0.0)
    }
}
