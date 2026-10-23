//! Immutable demonstration source, scoped to one tenant/account and exact local file.
use super::{adapter::digest, private::Result};
use abi_agent_host::{GovernedFacts, GovernedRegistration};
use abi_capability::{
    ApprovalLevel, Digest, Grant, PlatformFacts, Principal, PrincipalKind, Request,
    source::{DataClass, PackageSource, ResidencyPolicy, RetentionClass, SourceRegistry},
};
use abi_control_plane::NodeCertificate;
use serde_json::json;
use std::{collections::BTreeSet, path::Path};

pub fn compile(
    cert: &NodeCertificate,
    target: &Path,
) -> Result<(SourceRegistry, GovernedRegistration, GovernedFacts)> {
    let expires = u64::try_from(cert.expires_at)?
        .checked_mul(1000)
        .ok_or("certificate time overflow")?;
    let source: PackageSource = serde_json::from_value(json!({
        "id":"local.scratch.create","version":"1.0.0",
        "input_schema":{"type":"object","properties":{"content":{"type":"string","maxLength":1024}},"required":["content"],"additionalProperties":false},
        "output_schema":{"type":"object","properties":{"written":{"const":true},"digest":{"type":"string","maxLength":71}},"required":["written","digest"],"additionalProperties":false},"error_schema":{"type":"null"},
        "streaming":"None","cancellation":"BeforeEffect","required_platform_permissions":[],
        "resource_scope":{"organization_id":"local-demo","deployment_id":cert.node_id,"tenant_id":cert.claim.tenant_id,"platform":"local","guild_ref":null,"resource_digest":digest(target.to_string_lossy().as_bytes()),"subject_digest":digest(cert.claim.account_id.as_bytes())},
        "side_effect":"PlatformWrite","reversibility":"ReversibleWithLoss","risk_class":"Low",
        "data_classes":["Operational"],"residency":"LocalOnly","retention":"Session",
        "preconditions":["target-absent"],"invariants":["scoped"],"deadline_ms":10000,"rate_class":"one-call",
        "budgets":{"max_input_bytes":8192,"max_output_bytes":512,"max_events":4,"max_calls":1},
        "confirmation_policy":"A1Actor","delegation_policy":"Forbidden","expiry_policy_ms":900000,
        "revocation_policy":"RecheckBeforeEffect","approval_policy":"A1Actor","idempotency":"RequiresKey",
        "compensation":null,"rollback":"None","postconditions":["written"],
        "adapter_bindings":[{"adapter_id":"scratch-create-new","api_version":"1"}],
        "receipt_requirements":{"redacted":true,"require_postcondition_evidence":true},
        "fixtures":{"deterministic":digest(b"scratch-create-schema-v1"),"adversarial":digest(b"scratch-create-refusal-v1"),"failure":digest(b"scratch-create-unknown-v1")},
        "promotion_thresholds":{"minimum_passed":1,"maximum_failed":0},
        "expiration_criteria":{"expires_at_ms":expires,"disable_on_schema_drift":true,"disable_on_missing_evidence":true}
    }))?;
    let sources = SourceRegistry::compile([source])?;
    let compiled = sources
        .get("local.scratch.create", "1.0.0")
        .ok_or("source missing")?;
    let principal = Principal::new(&cert.claim.account_id, PrincipalKind::HumanSubject)?;
    let request = Request::new(
        "startup",
        "startup",
        principal.clone(),
        compiled.source().resource_scope.clone(),
        "local.scratch.create",
        "1.0.0",
        compiled.package().digest,
        Digest::default(),
        "demo-policy-v1",
        "demo-guild-v1",
        "demo-safety-v1",
        0,
    )?;
    let grant = Grant::active(
        "startup-scratch-grant",
        Principal::new("local-owner", PrincipalKind::OrganizationOwner)?,
        principal,
        request.scope.clone(),
        compiled.package(),
        0,
        expires,
        ApprovalLevel::A1Actor,
        0,
        "demo-policy-v1",
        "demo-guild-v1",
        "demo-safety-v1",
    )?;
    let facts = GovernedFacts {
        platform: PlatformFacts::new(
            BTreeSet::new(),
            0,
            "demo-policy-v1",
            "demo-guild-v1",
            "demo-safety-v1",
        ),
        grants: vec![grant],
        used_decision_ids: BTreeSet::new(),
        predicates: BTreeSet::from(["target-absent".into(), "scoped".into()]),
        captured_pre_state: None,
        observed_at_ms: 0,
        expires_at_ms: expires,
        permitted_data_classes: BTreeSet::from([DataClass::Operational]),
        residency: ResidencyPolicy::LocalOnly,
        retention: RetentionClass::Session,
        safety_allows: true,
        rate_allows: true,
    };
    Ok((
        sources,
        GovernedRegistration {
            tool_name: "create_scratch".into(),
            request,
        },
        facts,
    ))
}
