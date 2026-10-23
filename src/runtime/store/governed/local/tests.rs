use super::*;
use abi_capability::{PrincipalKind, Scope};
fn digest<T: serde::Serialize>(value: &T) -> Digest {
    Digest::from_bytes(Sha256::digest(serde_json::to_vec(value).unwrap()).into())
}
fn authority(path: &std::path::Path, user: &str) -> AbbeyLocalApprovalAuthority {
    AbbeyLocalApprovalAuthority::new(
        RuntimeStore::open(path).unwrap(),
        "tenant".into(),
        "account".into(),
        "node".into(),
        user.into(),
        Principal::new(user, PrincipalKind::HumanSubject).unwrap(),
    )
    .unwrap()
}
fn preview() -> LocalApprovalPreview {
    let d = Digest::from_bytes([1; 32]);
    let parameters = serde_json::json!({"record":"private-123"});
    let request = abi_capability::Request::new(
        "attempt",
        "local-call",
        Principal::new("alice", PrincipalKind::HumanSubject).unwrap(),
        Scope::new("org", "deployment", "tenant", "local", None, d, d).unwrap(),
        "local.record",
        "1.0.0",
        d,
        digest(&parameters),
        "policy",
        "guild",
        "safety",
        0,
    )
    .unwrap();
    let binding = AuthorizationBinding {
        request_digest: request.request_digest,
        principal_digest: digest(&request.principal),
        scope_digest: digest(&request.scope),
        package_digest: d,
        grant_digest: d,
        policy_digest: d,
        schema_digest: d,
        precondition_digest: d,
        required_approval: ApprovalLevel::A1Actor,
        compensation_origin: None,
    };
    LocalApprovalPreview {
        version: 1,
        tenant_id: "tenant".into(),
        account_id: "account".into(),
        node_id: "node".into(),
        user_id: "alice".into(),
        request,
        binding,
        grant_id: "grant".into(),
        tool_id: "recording".into(),
        operation: "obsolete".into(),
        resource: "private-123".into(),
        parameters,
        expected_effects: vec!["mark record obsolete".into()],
        created_at_ms: 1000,
        expires_at_ms: 5000,
    }
}
#[test]
fn full_preview_reopens_and_approved_actor_cannot_be_fabricated() {
    let scratch = Scratch::new();
    let path = scratch.path().join("local.sqlite");
    let p = preview();
    let a = authority(&path, "alice");
    assert_eq!(
        a.request_local_approval(&p).unwrap().state,
        LocalApprovalState::Pending
    );
    assert!(
        a.exact_local_approval(&p.request.call_id, p.digest(), 1100)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        a.decide_local_approval(&p.request.call_id, p.digest(), "decision", true, 1100)
            .unwrap()
            .state,
        LocalApprovalState::Approved
    );
    drop(a);
    let mut a = authority(&path, "alice");
    assert!(
        a.exact_local_approval(&p.request.call_id, p.digest(), 1050)
            .is_err()
    );
    let exact = a
        .exact_local_approval(&p.request.call_id, p.digest(), 1200)
        .unwrap()
        .unwrap();
    assert_eq!(exact.approval.approver.id, "alice");
    assert_eq!(
        a.read_local_approval(&p.request.call_id).unwrap(),
        Some(p.clone())
    );
    let mut request = PrepareRequest {
        call_id: p.request.call_id.clone(),
        tool_id: p.tool_id.clone(),
        execution_id: Digest::from_bytes([7; 32]),
        binding: p.binding.clone(),
        approval: Some(exact.clone()),
        now_ms: 1200,
    };
    request.approval.as_mut().unwrap().approval.approver.id = "bob".into();
    assert!(matches!(
        a.prepare(&request).unwrap(),
        PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient)
    ));
    request.approval = Some(exact);
    assert!(matches!(
        a.prepare(&request).unwrap(),
        PrepareOutcome::Prepared(_)
    ));
    assert!(
        a.exact_local_approval(&p.request.call_id, p.digest(), 1300)
            .unwrap()
            .is_none()
    );
}
#[test]
fn owner_change_preview_change_expiry_and_denial_fail_closed() {
    let scratch = Scratch::new();
    let path = scratch.path().join("local.sqlite");
    let a = authority(&path, "alice");
    let p = preview();
    a.request_local_approval(&p).unwrap();
    for field in 0..5 {
        let mut changed = p.clone();
        match field {
            0 => changed.resource = "other".into(),
            1 => changed.parameters = serde_json::json!({"record":"other"}),
            2 => changed.expected_effects.push("extra".into()),
            3 => changed.account_id = "other".into(),
            _ => changed.expires_at_ms += 1,
        }
        assert!(a.request_local_approval(&changed).is_err());
        assert!(
            a.decide_local_approval(&p.request.call_id, changed.digest(), "changed", true, 1100)
                .is_err()
        );
    }
    drop(a);
    let wrong = authority(&path, "bob");
    assert!(wrong.read_local_approval(&p.request.call_id).is_err());
    assert!(
        wrong
            .exact_local_approval(&p.request.call_id, p.digest(), 1200)
            .is_err()
    );
    assert!(
        wrong
            .decide_local_approval(&p.request.call_id, p.digest(), "wrong", true, 1200)
            .is_err()
    );
    drop(wrong);
    let a = authority(&path, "alice");
    assert_eq!(
        a.decide_local_approval(&p.request.call_id, p.digest(), "late", true, 5000)
            .unwrap()
            .state,
        LocalApprovalState::Expired
    );
    assert!(
        a.exact_local_approval(&p.request.call_id, p.digest(), 5000)
            .unwrap()
            .is_none()
    );
    let mut p2 = p;
    p2.request.call_id = "denied-call".into();
    p2.request.request_digest = p2.request.computed_digest();
    p2.binding.request_digest = p2.request.request_digest;
    a.request_local_approval(&p2).unwrap();
    assert_eq!(
        a.decide_local_approval(&p2.request.call_id, p2.digest(), "deny", false, 1100)
            .unwrap()
            .state,
        LocalApprovalState::Denied
    );
    assert!(
        a.exact_local_approval(&p2.request.call_id, p2.digest(), 1200)
            .unwrap()
            .is_none()
    );
}

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "abbey-local-approval-{}-{}-{}",
            std::process::id(),
            crate::runtime::store::epoch_ms().unwrap(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn failed_decision_write_rolls_back_and_exact_envelope_cannot_change_grant_or_level() {
    let mut a = authority(std::path::Path::new(":memory:"), "alice");
    let p = preview();
    a.request_local_approval(&p).unwrap();
    a.store.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_local_decision BEFORE INSERT ON tool_approval_events WHEN NEW.state='approved' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(
        a.decide_local_approval(&p.request.call_id, p.digest(), "atomic", true, 1100)
            .is_err()
    );
    assert!(
        a.exact_local_approval(&p.request.call_id, p.digest(), 1200)
            .unwrap()
            .is_none()
    );
    let exact: Option<String> = a
        .store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT exact_json FROM local_governed_approvals", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(exact.is_none());
    a.store
        .conn
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_local_decision")
        .unwrap();
    a.decide_local_approval(&p.request.call_id, p.digest(), "atomic", true, 1100)
        .unwrap();
    let exact = a
        .exact_local_approval(&p.request.call_id, p.digest(), 1200)
        .unwrap()
        .unwrap();
    let request = PrepareRequest {
        call_id: p.request.call_id.clone(),
        tool_id: p.tool_id.clone(),
        execution_id: Digest::from_bytes([8; 32]),
        binding: p.binding.clone(),
        approval: Some(exact),
        now_ms: 1200,
    };
    for mutation in 0..3 {
        let mut changed = request.clone();
        let approval = &mut changed.approval.as_mut().unwrap().approval;
        match mutation {
            0 => approval.grant_id = "forged-grant".into(),
            1 => approval.level = ApprovalLevel::A4Owner,
            _ => {
                approval.coapprover =
                    Some(Principal::new("bob", PrincipalKind::HumanSubject).unwrap())
            }
        }
        assert!(matches!(
            a.prepare(&changed).unwrap(),
            PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient)
        ));
    }
    assert!(matches!(
        a.prepare(&request).unwrap(),
        PrepareOutcome::Prepared(_)
    ));
}

fn approved_request(a: &AbbeyLocalApprovalAuthority) -> PrepareRequest {
    let p = preview();
    a.request_local_approval(&p).unwrap();
    a.decide_local_approval(&p.request.call_id, p.digest(), "owner-decision", true, 1100)
        .unwrap();
    PrepareRequest {
        call_id: p.request.call_id.clone(),
        tool_id: p.tool_id.clone(),
        execution_id: Digest::from_bytes([19; 32]),
        binding: p.binding.clone(),
        approval: a
            .exact_local_approval(&p.request.call_id, p.digest(), 1200)
            .unwrap(),
        now_ms: 1200,
    }
}
fn lifecycle_counts(a: &AbbeyLocalApprovalAuthority) -> (i64, i64, i64) {
    let conn = a.store.conn.lock().unwrap();
    conn.query_row("SELECT (SELECT count(*) FROM governed_stage_events), (SELECT count(*) FROM governed_admission_refusals), (SELECT count(*) FROM governed_outcome_events)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
}
#[test]
fn local_lifecycle_hides_foreign_execution_and_rejects_every_foreign_write() {
    let scratch = Scratch::new();
    let path = scratch.path().join("owners.sqlite");
    let mut alice = authority(&path, "alice");
    let mut bob = authority(&path, "bob");
    // The same human principal under another tenant must also be isolated.
    let mut other_tenant = AbbeyLocalApprovalAuthority::new(
        RuntimeStore::open(&path).unwrap(),
        "other-tenant".into(),
        "account".into(),
        "node".into(),
        "alice".into(),
        Principal::new("alice", PrincipalKind::HumanSubject).unwrap(),
    )
    .unwrap();
    let request = approved_request(&alice);
    let PrepareOutcome::Prepared(prepared) = alice.prepare(&request).unwrap() else {
        panic!("Alice intent not prepared");
    };
    let stage = StageAudit {
        version: 1,
        execution_id: prepared.execution_id(),
        stage: 12,
        binding_digest: digest(&request.binding),
        decision: Decision::Allow,
        reason: LifecycleReason::Authorization(ReasonCode::Authorized),
    };
    let terminal = TerminalOutcome {
        version: 1,
        state: OutcomeState::Succeeded,
        result_digest: Some(Digest::from_bytes([20; 32])),
        reason: LifecycleReason::Authorization(ReasonCode::Authorized),
    };
    let before = lifecycle_counts(&alice);
    for foreign in [&mut bob, &mut other_tenant] {
        assert!(foreign.lookup(prepared.execution_id()).unwrap().is_none());
        assert!(foreign.finish(&prepared, &terminal).is_err());
        assert!(foreign.refuse_replay(prepared.execution_id()).is_err());
        assert!(foreign.append_stage(&stage).is_err());
    }
    assert_eq!(
        lifecycle_counts(&alice),
        before,
        "foreign calls must not append audit/outcome rows"
    );
    assert_eq!(
        alice
            .lookup(prepared.execution_id())
            .unwrap()
            .unwrap()
            .state,
        OutcomeState::Prepared
    );
    alice.append_stage(&stage).unwrap();
    alice.finish(&prepared, &terminal).unwrap();
    assert_eq!(
        alice.lookup(prepared.execution_id()).unwrap(),
        Some(terminal)
    );
    assert!(bob.lookup(prepared.execution_id()).unwrap().is_none());
    let unknown = Digest::from_bytes([21; 32]);
    assert!(bob.lookup(unknown).unwrap().is_none());
    assert!(!bob.refuse_replay(unknown).unwrap());
}
#[test]
fn local_wrapper_refuses_a0_and_nonlocal_intents_without_changing_legacy_store() {
    let mut alice = authority(std::path::Path::new(":memory:"), "alice");
    let p = preview();
    let mut binding = p.binding;
    binding.required_approval = ApprovalLevel::A0None;
    let request = PrepareRequest {
        call_id: "legacy-a0".into(),
        tool_id: p.tool_id,
        execution_id: Digest::from_bytes([22; 32]),
        binding,
        approval: None,
        now_ms: 1200,
    };
    assert!(matches!(
        alice.prepare(&request).unwrap(),
        PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient)
    ));
    let PrepareOutcome::Prepared(prepared) = alice.store.prepare(&request).unwrap() else {
        panic!("legacy RuntimeStore A0 changed");
    };
    assert!(alice.lookup(prepared.execution_id()).unwrap().is_none());
    assert!(alice.refuse_replay(prepared.execution_id()).is_err());
    let stage = StageAudit {
        version: 1,
        execution_id: prepared.execution_id(),
        stage: 12,
        binding_digest: digest(&request.binding),
        decision: Decision::Allow,
        reason: LifecycleReason::Authorization(ReasonCode::Authorized),
    };
    assert!(alice.append_stage(&stage).is_err());
    let terminal = TerminalOutcome {
        version: 1,
        state: OutcomeState::Succeeded,
        result_digest: Some(Digest::from_bytes([23; 32])),
        reason: LifecycleReason::Authorization(ReasonCode::Authorized),
    };
    assert!(alice.finish(&prepared, &terminal).is_err());
    assert_eq!(
        alice
            .store
            .lookup(prepared.execution_id())
            .unwrap()
            .unwrap()
            .state,
        OutcomeState::Prepared
    );
    // Initial audit has no durable owner yet and remains a trusted host boundary.
    let initial = StageAudit {
        execution_id: Digest::from_bytes([24; 32]),
        stage: 0,
        ..stage
    };
    alice.append_stage(&initial).unwrap();
}
#[test]
fn corrupt_owned_execution_preview_errors_instead_of_looking_absent() {
    let mut alice = authority(std::path::Path::new(":memory:"), "alice");
    let request = approved_request(&alice);
    let PrepareOutcome::Prepared(prepared) = alice.prepare(&request).unwrap() else {
        panic!("Alice intent not prepared");
    };
    // Simulate on-disk corruption after the normal immutable write boundary.
    alice.store.conn.lock().unwrap().execute_batch(
        "DROP TRIGGER local_governed_approvals_update_guard; UPDATE local_governed_approvals SET preview_json='{}';"
    ).unwrap();
    assert!(alice.lookup(prepared.execution_id()).is_err());
    assert!(alice.refuse_replay(prepared.execution_id()).is_err());
}
#[test]
fn local_compensation_cannot_reserve_another_principals_same_scope_origin() {
    let scratch = Scratch::new();
    let path = scratch.path().join("compensation-owners.sqlite");
    let mut alice = authority(&path, "alice");
    let mut bob = authority(&path, "bob");
    let original = approved_request(&alice);
    let PrepareOutcome::Prepared(prepared) = alice.prepare(&original).unwrap() else {
        panic!("Alice intent not prepared");
    };
    alice
        .finish(
            &prepared,
            &TerminalOutcome {
                version: 1,
                state: OutcomeState::Failed,
                result_digest: Some(Digest::from_bytes([25; 32])),
                reason: LifecycleReason::AdapterFailure,
            },
        )
        .unwrap();
    let mut p = preview();
    p.user_id = "bob".into();
    p.request.principal = Principal::new("bob", PrincipalKind::HumanSubject).unwrap();
    p.request.call_id = "bob-inverse-call".into();
    p.request.attempt_id = "bob-inverse-attempt".into();
    p.request.request_digest = p.request.computed_digest();
    p.binding.request_digest = p.request.request_digest;
    p.binding.principal_digest = digest(&p.request.principal);
    p.binding.compensation_origin = Some(CompensationBinding::from_prepared(&prepared));
    assert_eq!(p.binding.scope_digest, original.binding.scope_digest);
    bob.request_local_approval(&p).unwrap();
    bob.decide_local_approval(
        &p.request.call_id,
        p.digest(),
        "bob-inverse-decision",
        true,
        1100,
    )
    .unwrap();
    let request = PrepareRequest {
        call_id: p.request.call_id.clone(),
        tool_id: p.tool_id.clone(),
        execution_id: Digest::from_bytes([26; 32]),
        binding: p.binding.clone(),
        approval: bob
            .exact_local_approval(&p.request.call_id, p.digest(), 1200)
            .unwrap(),
        now_ms: 1200,
    };
    let before = lifecycle_counts(&alice);
    assert!(matches!(
        bob.prepare(&request).unwrap(),
        PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient)
    ));
    assert_eq!(lifecycle_counts(&alice), before);
    assert!(
        bob.exact_local_approval(&p.request.call_id, p.digest(), 1200)
            .unwrap()
            .is_some(),
        "foreign origin must not consume Bob's approval"
    );
    let reservations: i64 = alice
        .store
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM governed_tool_intents WHERE original_execution_id=?1",
            [prepared.execution_id().to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reservations, 0);
    assert!(alice.store.lookup(request.execution_id).unwrap().is_none());
}
#[test]
fn locally_owned_failed_origin_still_allows_one_compensation() {
    let mut alice = authority(std::path::Path::new(":memory:"), "alice");
    let original = approved_request(&alice);
    let PrepareOutcome::Prepared(prepared) = alice.prepare(&original).unwrap() else {
        panic!("Alice intent not prepared");
    };
    alice
        .finish(
            &prepared,
            &TerminalOutcome {
                version: 1,
                state: OutcomeState::Failed,
                result_digest: Some(Digest::from_bytes([27; 32])),
                reason: LifecycleReason::AdapterFailure,
            },
        )
        .unwrap();
    let mut p = preview();
    p.request.call_id = "alice-inverse-call".into();
    p.request.attempt_id = "alice-inverse-attempt".into();
    p.request.request_digest = p.request.computed_digest();
    p.binding.request_digest = p.request.request_digest;
    p.binding.compensation_origin = Some(CompensationBinding::from_prepared(&prepared));
    alice.request_local_approval(&p).unwrap();
    alice
        .decide_local_approval(
            &p.request.call_id,
            p.digest(),
            "alice-inverse-decision",
            true,
            1100,
        )
        .unwrap();
    let request = PrepareRequest {
        call_id: p.request.call_id.clone(),
        tool_id: p.tool_id.clone(),
        execution_id: Digest::from_bytes([28; 32]),
        binding: p.binding.clone(),
        approval: alice
            .exact_local_approval(&p.request.call_id, p.digest(), 1200)
            .unwrap(),
        now_ms: 1200,
    };
    assert!(matches!(
        alice.prepare(&request).unwrap(),
        PrepareOutcome::Prepared(_)
    ));
    assert!(matches!(
        alice.prepare(&request).unwrap(),
        PrepareOutcome::Uncertain
    ));
}
