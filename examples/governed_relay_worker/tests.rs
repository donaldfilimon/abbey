use super::*;
use abi_agent_host::{GovernedFactProvider, Postconditions, ToolExecutionContext};
use abi_agent_runtime::ToolCall;
use abi_capability::{
    AuthorizationContext, Decision, Kernel,
    lifecycle::{AuthorizationBinding, PreparedExecution},
};
use std::collections::BTreeSet;

fn certificate() -> NodeCertificate {
    // Configuration validation uses immutable identity/expiry. Actual startup
    // verifies signatures and enrollment through RelayRuntime::new_tools.
    NodeCertificate {
        protocol: "abi.node-certificate.v1".into(),
        audience: "https://example.invalid".into(),
        node_id: uuid::Uuid::new_v4().to_string(),
        expires_at: now().unwrap() + 3600,
        claim: abi_control_plane::PairClaim {
            protocol: "abi.node-pair.v1".into(),
            enrollment_id: uuid::Uuid::new_v4().to_string(),
            account_id: uuid::Uuid::new_v4().to_string(),
            tenant_id: uuid::Uuid::new_v4().to_string(),
            public_key: [1; 32],
            encryption_public_key: Some([2; 32]),
            capabilities: vec!["tool.execute".into()],
            expires_at: now().unwrap() + 3600,
        },
        signature: vec![],
    }
}
#[test]
fn source_and_grant_compile_with_exact_actor_approval_and_real_file_evidence() {
    let directory =
        std::env::temp_dir().join(format!("abbey-native-governed-{}", uuid::Uuid::new_v4()));
    private::directory(&directory).unwrap();
    let target = directory.join("fixed-effect.txt");
    let cert = certificate();
    let (sources, registration, value) = source::compile(&cert, &target).unwrap();
    let compiled = sources.get("local.scratch.create", "1.0.0").unwrap();
    assert!(compiled.validates_input(&serde_json::json!({"content":"private fixture"})));
    assert!(!compiled.validates_input(&serde_json::json!({"content":"x","path":"/elsewhere"})));
    let registry = sources.capability_registry().unwrap();
    let kernel = Kernel::new(&registry);
    let cancellation = CancellationToken::new();
    let evaluated = kernel.evaluate_source_bound(
        &registration.request,
        &AuthorizationContext {
            now_ms: &WallClock,
            cancellation: &cancellation,
            grants: &value.grants,
            approval: None,
            used_decision_ids: &BTreeSet::new(),
            platform: &value.platform,
        },
        compiled,
        abi_capability::Digest::default(),
        None,
    );
    assert_eq!(evaluated.decision, Decision::ApprovalRequired);
    let binding = AuthorizationBinding::from_evaluation(
        &registration.request,
        &evaluated,
        compiled.schema_digest(),
        abi_capability::Digest::default(),
    )
    .unwrap();
    let prepared = PreparedExecution::after_commit(adapter::digest(b"fixture-intent"), binding);
    let evidence = Evidence::default();
    let mut executor = FileAdapter {
        path: target.clone(),
        evidence: evidence.clone(),
    };
    let mut facts = FileFacts {
        value,
        path: target.clone(),
        evidence,
    };
    let call = ToolCall::new(
        "fixture-call",
        "create_scratch",
        "{\"content\":\"private fixture\"}",
    );
    let output = executor
        .execute(
            &call,
            &registration.request,
            compiled,
            &prepared,
            ToolExecutionContext {
                cancellation: &cancellation,
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(2),
            },
        )
        .unwrap();
    assert!(compiled.validates_output(&serde_json::from_str(output.payload()).unwrap()));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "private fixture");
    let observed = facts
        .postconditions(&registration.request, &BTreeSet::from(["written".into()]))
        .unwrap();
    assert_eq!(observed[0].status, Postconditions::Verified);
    assert_eq!(
        observed[0].source,
        abi_agent_host::ObservationSource::TrustedLocalAdapter
    );
    assert!(
        !facts
            .observe(&registration.request)
            .unwrap()
            .predicates
            .contains("target-absent")
    );
    assert!(
        executor
            .execute(
                &call,
                &registration.request,
                compiled,
                &prepared,
                ToolExecutionContext {
                    cancellation: &cancellation,
                    deadline: std::time::Instant::now() + std::time::Duration::from_secs(2),
                }
            )
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "private fixture");
    std::fs::remove_dir_all(&directory).unwrap();
}
