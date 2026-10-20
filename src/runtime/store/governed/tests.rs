use super::*;
use crate::runtime::{NewToolApproval, ToolApprovalDecision};
use abi_capability::{Approval, ApprovalLevel, ApprovalState, Principal, PrincipalKind};
fn request(call: &str, id: u8) -> PrepareRequest {
    let d = Digest::from_bytes([id; 32]);
    PrepareRequest {
        call_id: call.into(),
        tool_id: "recording".into(),
        execution_id: d,
        now_ms: 1200,
        approval: None,
        binding: AuthorizationBinding {
            request_digest: d,
            principal_digest: d,
            scope_digest: d,
            package_digest: d,
            grant_digest: d,
            policy_digest: d,
            schema_digest: d,
            precondition_digest: d,
            required_approval: ApprovalLevel::A0None,
            compensation_origin: None,
        },
    }
}
fn approved(store: &RuntimeStore, call: &str, id: u8) -> PrepareRequest {
    approve_request(store, request(call, id))
}
fn approve_request(store: &RuntimeStore, mut r: PrepareRequest) -> PrepareRequest {
    let call = r.call_id.clone();
    let call = call.as_str();
    r.binding.required_approval = ApprovalLevel::A1Actor;
    store
        .create_governed_tool_approval(
            NewToolApproval {
                call_id: call.into(),
                tool_id: r.tool_id.clone(),
                call_digest: hex(r.binding.request_digest),
                created_at_ms: 1000,
                expires_at_ms: 5000,
            },
            &r.binding,
        )
        .unwrap();
    store
        .decide_tool_approval(
            call,
            &hex(r.binding.request_digest),
            &format!("decision-{call}"),
            ToolApprovalDecision::Approve,
            1100,
        )
        .unwrap();
    r.approval = Some(ExactApproval {
        binding: r.binding.clone(),
        issued_at_ms: 1000,
        approval: Approval {
            decision_id: format!("decision-{call}"),
            call_id: call.into(),
            call_digest: r.binding.request_digest,
            package_digest: r.binding.package_digest,
            grant_id: "grant".into(),
            approver: Principal {
                kind: PrincipalKind::HumanSubject,
                id: "approver".into(),
            },
            coapprover: None,
            level: ApprovalLevel::A1Actor,
            expires_at_ms: 5000,
            state: ApprovalState::Approved,
        },
    });
    r
}
fn prepared(store: &mut RuntimeStore, r: &PrepareRequest) -> PreparedExecution {
    match store.prepare(r).unwrap() {
        PrepareOutcome::Prepared(p) => p,
        other => panic!("{other:?}"),
    }
}
#[test]
fn a0_has_no_fabricated_approval_and_restart_never_replays() {
    let dir = Scratch::new();
    let path = dir.path().join("runtime.sqlite");
    let mut s = RuntimeStore::open(&path).unwrap();
    let r = request("a0", 1);
    let p = prepared(&mut s, &r);
    assert!(s.tool_approval("a0", 1200).unwrap().is_none());
    assert!(matches!(s.prepare(&r).unwrap(), PrepareOutcome::Uncertain));
    drop(s);
    let mut s = RuntimeStore::open(&path).unwrap();
    assert_eq!(
        s.lookup(p.execution_id()).unwrap().unwrap().state,
        OutcomeState::Uncertain
    );
    assert!(matches!(s.prepare(&r).unwrap(), PrepareOutcome::Uncertain));
    prepared(&mut s, &request("fresh", 2));
}
#[test]
fn human_approval_consumption_and_stage11_are_atomic() {
    let mut s = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
    let r = approved(&s, "approved", 3);
    // A failing stage audit write rolls back approval and both execution intents.
    s.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_stage BEFORE INSERT ON governed_stage_events BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(s.prepare(&r).is_err());
    assert_eq!(
        s.tool_approval(&r.call_id, 1200).unwrap().unwrap().state,
        ToolApprovalState::Approved
    );
    assert!(s.tool_execution(&r.call_id).unwrap().is_none());
    s.conn
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_stage")
        .unwrap();
    let p = prepared(&mut s, &r);
    assert_eq!(
        s.tool_approval(&r.call_id, 1200).unwrap().unwrap().state,
        ToolApprovalState::Consumed
    );
    let terminal = TerminalOutcome {
        version: 1,
        state: OutcomeState::Succeeded,
        result_digest: Some(Digest::from_bytes([9; 32])),
        reason: LifecycleReason::Authorization(ReasonCode::Authorized),
    };
    s.finish(&p, &terminal).unwrap();
    assert_eq!(s.lookup(p.execution_id()).unwrap(), Some(terminal.clone()));
    assert!(s.finish(&p, &terminal).is_err());
}
#[test]
fn independent_connections_compete_for_one_exact_approval() {
    let dir = Scratch::new();
    let path = dir.path().join("runtime.sqlite");
    let s = RuntimeStore::open(&path).unwrap();
    let r = approved(&s, "race", 4);
    drop(s);
    let mut a = RuntimeStore::open(&path).unwrap();
    let mut b = RuntimeStore::open(&path).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let gate = barrier.clone();
    let other = r.clone();
    let worker = std::thread::spawn(move || {
        gate.wait();
        a.prepare(&other).unwrap()
    });
    barrier.wait();
    let second = b.prepare(&r).unwrap();
    let first = worker.join().unwrap();
    assert_eq!(
        usize::from(matches!(first, PrepareOutcome::Prepared(_)))
            + usize::from(matches!(second, PrepareOutcome::Prepared(_))),
        1
    );
}
#[test]
fn corrupted_binding_and_missing_audit_fail_closed() {
    let dir = Scratch::new();
    let path = dir.path().join("runtime.sqlite");
    let mut s = RuntimeStore::open(&path).unwrap();
    prepared(&mut s, &request("corrupt", 5));
    drop(s);
    let conn = Connection::open(&path).unwrap();
    // Simulate damaged protections to independently exercise startup validation.
    conn.execute_batch("DROP TRIGGER governed_intents_update_guard")
        .unwrap();
    conn.execute("UPDATE governed_tool_intents SET binding_json='{}'", [])
        .unwrap();
    drop(conn);
    assert!(RuntimeStore::open(&path).is_err());
}
#[test]
fn unknown_cannot_claim_success_or_fabricate_digest() {
    let mut s = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
    let r = approved(&s, "unknown", 6);
    let p = prepared(&mut s, &r);
    assert!(
        s.finish(
            &p,
            &outcome(
                OutcomeState::Succeeded,
                LifecycleReason::Authorization(ReasonCode::Authorized)
            )
        )
        .is_err()
    );
    s.finish(
        &p,
        &outcome(OutcomeState::Uncertain, LifecycleReason::OutcomeUnknown),
    )
    .unwrap();
    assert_eq!(
        s.tool_execution(&r.call_id).unwrap().unwrap().state,
        super::super::ToolExecutionState::Interrupted
    );
    assert!(matches!(s.prepare(&r).unwrap(), PrepareOutcome::Uncertain));
}
#[test]
fn child_process_crash_fixture() {
    let Ok(path) = std::env::var("ABBEY_GOVERNED_CRASH_FIXTURE") else {
        return;
    };
    let mut s = RuntimeStore::open(std::path::Path::new(&path)).unwrap();
    let r = approved(&s, "crash", 7);
    prepared(&mut s, &r);
    if std::env::var_os("ABBEY_GOVERNED_AFTER_EFFECT").is_some() {
        std::fs::write(format!("{path}.effect"), b"recorded").unwrap();
    }
    std::process::exit(73);
}
#[test]
fn process_death_before_and_after_recording_effect_requires_fresh_call() {
    for after in [false, true] {
        let dir = Scratch::new();
        let path = dir.path().join("runtime.sqlite");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "runtime::store::governed::tests::child_process_crash_fixture",
            "--nocapture",
        ])
        .env("ABBEY_GOVERNED_CRASH_FIXTURE", &path)
        .stdin(std::process::Stdio::null());
        if after {
            cmd.env("ABBEY_GOVERNED_AFTER_EFFECT", "1");
        } else {
            cmd.env_remove("ABBEY_GOVERNED_AFTER_EFFECT");
        }
        assert_eq!(cmd.status().unwrap().code(), Some(73));
        let mut s = RuntimeStore::open(&path).unwrap();
        assert_eq!(
            s.lookup(Digest::from_bytes([7; 32]))
                .unwrap()
                .unwrap()
                .state,
            OutcomeState::Uncertain
        );
        let mut replay = request("crash", 7);
        replay.binding.required_approval = ApprovalLevel::A1Actor;
        // Reconstruct from the admitted immutable envelope, never an execution permit.
        let text: String = s
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT approval_json FROM governed_tool_intents", [], |r| {
                r.get(0)
            })
            .unwrap();
        replay.approval = Some(serde_json::from_str(&text).unwrap());
        assert!(matches!(
            s.prepare(&replay).unwrap(),
            PrepareOutcome::Uncertain
        ));
        assert_eq!(path.with_extension("sqlite.effect").exists(), after);
        let fresh = approved(&s, "fresh", 8);
        prepared(&mut s, &fresh);
    }
}

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "abbey-governed-{}-{}-{}",
            std::process::id(),
            super::super::epoch_ms().unwrap(),
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
fn exact_approval_identity_ttl_and_cancelled_state_are_enforced() {
    let mut s = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
    let r = approved(&s, "identity", 20);
    for tamper in 0..4 {
        let mut changed = r.clone();
        match tamper {
            0 => changed.approval.as_mut().unwrap().approval.decision_id = "other".into(),
            1 => changed.tool_id = "other".into(),
            2 => changed.approval.as_mut().unwrap().issued_at_ms = 999,
            _ => changed.approval.as_mut().unwrap().approval.expires_at_ms = 5001,
        }
        assert!(!matches!(
            s.prepare(&changed),
            Ok(PrepareOutcome::Prepared(_))
        ));
    }
    let mut expired = r.clone();
    expired.now_ms = 5000;
    assert!(s.prepare(&expired).is_err());
    s.cancel_tool_approval(&r.call_id, "cancel-identity", 1300)
        .unwrap();
    assert!(matches!(s.prepare(&r).unwrap(), PrepareOutcome::Refused(_)));
    assert!(s.tool_execution(&r.call_id).unwrap().is_none());
}
#[test]
fn legacy_methods_cannot_override_governed_reservations() {
    let mut s = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
    let r = request("a0-reserved", 21);
    prepared(&mut s, &r);
    assert!(
        s.create_tool_approval(NewToolApproval {
            call_id: r.call_id.clone(),
            tool_id: r.tool_id.clone(),
            call_digest: hex(r.binding.request_digest),
            created_at_ms: 1000,
            expires_at_ms: 5000
        })
        .is_err()
    );
    let r = approved(&s, "human-reserved", 22);
    prepared(&mut s, &r);
    assert!(
        s.complete_tool_execution(
            &r.call_id,
            &r.execution_id.to_string(),
            super::super::ToolExecutionOutcome::Succeeded,
            &hex(r.execution_id),
            1300
        )
        .is_err()
    );
}
#[test]
fn malformed_database_missing_audit_and_future_schema_refuse_open() {
    for fault in 0..4 {
        let dir = Scratch::new();
        let path = dir.path().join("runtime.sqlite");
        let mut s = RuntimeStore::open(&path).unwrap();
        prepared(&mut s, &request("bad", 23));
        drop(s);
        if fault == 0 {
            std::fs::write(&path, b"not a database").unwrap();
        } else {
            let conn = Connection::open(&path).unwrap();
            match fault {
                1=>conn.execute_batch("DROP TRIGGER governed_events_no_delete; DELETE FROM governed_stage_events;").unwrap(),
                2=>{conn.execute("INSERT INTO schema_migrations VALUES(99,'future')",[]).unwrap();},
                _=>{conn.execute("DELETE FROM schema_migrations WHERE version=3",[]).unwrap();},
            }
        }
        assert!(RuntimeStore::open(&path).is_err());
    }
}

#[test]
fn every_preview_commitment_is_immutable_before_decision_and_consumption() {
    let mut s = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
    let original = approved(&s, "preview", 24);
    for field in 0..8 {
        let mut r = original.clone();
        let changed = Digest::from_bytes([99; 32]);
        match field {
            0 => r.binding.principal_digest = changed,
            1 => r.binding.scope_digest = changed,
            2 => r.binding.package_digest = changed,
            3 => r.binding.grant_digest = changed,
            4 => r.binding.policy_digest = changed,
            5 => r.binding.schema_digest = changed,
            6 => r.binding.precondition_digest = changed,
            _ => r.binding.required_approval = ApprovalLevel::A0None,
        }
        let exact = r.approval.as_mut().unwrap();
        exact.binding = r.binding.clone();
        exact.approval.package_digest = r.binding.package_digest;
        assert!(matches!(
            s.prepare(&r).unwrap(),
            PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient)
        ));
    }
    assert!(
        s.prepare_tool_execution(
            &original.call_id,
            &hex(original.binding.request_digest),
            "legacy-bypass",
            1200
        )
        .is_err()
    );
    prepared(&mut s, &original);
    let mut legacy = request("legacy", 25);
    legacy.binding.required_approval = ApprovalLevel::A1Actor;
    s.create_tool_approval(NewToolApproval {
        call_id: legacy.call_id.clone(),
        tool_id: legacy.tool_id.clone(),
        call_digest: hex(legacy.binding.request_digest),
        created_at_ms: 1000,
        expires_at_ms: 5000,
    })
    .unwrap();
    s.decide_tool_approval(
        &legacy.call_id,
        &hex(legacy.binding.request_digest),
        "legacy-decision",
        ToolApprovalDecision::Approve,
        1100,
    )
    .unwrap();
    let mut exact = original.approval.unwrap();
    exact.binding = legacy.binding.clone();
    exact.approval.call_id = legacy.call_id.clone();
    exact.approval.call_digest = legacy.binding.request_digest;
    exact.approval.package_digest = legacy.binding.package_digest;
    exact.approval.decision_id = "legacy-decision".into();
    legacy.approval = Some(exact);
    assert!(matches!(
        s.prepare(&legacy).unwrap(),
        PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient)
    ));
    assert!(
        s.prepare_tool_execution(
            &legacy.call_id,
            &hex(legacy.binding.request_digest),
            "legacy-execution",
            1200
        )
        .is_ok()
    );
}

#[test]
fn governed_and_legacy_execution_names_share_replay_reservations() {
    let mut s = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
    let r = request("a0-id", 26);
    prepared(&mut s, &r);
    s.create_tool_approval(NewToolApproval {
        call_id: "legacy-id".into(),
        tool_id: r.tool_id.clone(),
        call_digest: hex(r.binding.request_digest),
        created_at_ms: 1000,
        expires_at_ms: 5000,
    })
    .unwrap();
    assert!(
        s.decide_tool_approval(
            "legacy-id",
            &hex(r.binding.request_digest),
            &r.execution_id.to_string(),
            ToolApprovalDecision::Approve,
            1100
        )
        .is_err()
    );
    s.decide_tool_approval(
        "legacy-id",
        &hex(r.binding.request_digest),
        "legacy-id-decision",
        ToolApprovalDecision::Approve,
        1100,
    )
    .unwrap();
    let collision = Digest::from_bytes([27; 32]);
    s.prepare_tool_execution(
        "legacy-id",
        &hex(r.binding.request_digest),
        &collision.to_string(),
        1200,
    )
    .unwrap();
    assert!(matches!(
        s.prepare(&request("new-a0", 27)).unwrap(),
        PrepareOutcome::Uncertain
    ));
}

#[test]
fn sql_cannot_rewrite_intent_identity_or_reopen_terminal_reservations() {
    let dir = Scratch::new();
    let path = dir.path().join("runtime.sqlite");
    let mut store = RuntimeStore::open(&path).unwrap();
    let original = request("immutable-call", 30);
    let permit = prepared(&mut store, &original);
    let conn = Connection::open(&path).unwrap();
    for assignment in [
        "call_id='renamed-call'",
        "tool_id='renamed-tool'",
        "execution_id='sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'",
        "approval_call_id='other-call'",
        "legacy_execution_call_id='other-call'",
        "original_execution_id='other'",
        "original_binding_digest='other'",
        "binding_json='{}'",
        "approval_json='{}'",
        "started_at_ms=999",
        "outcome_json='{}'",
    ] {
        let error = conn
            .execute(
                &format!("UPDATE governed_tool_intents SET {assignment}"),
                [],
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("immutable intent"),
            "{assignment}: {error}"
        );
    }
    store
        .finish(
            &permit,
            &TerminalOutcome {
                version: 1,
                state: OutcomeState::Succeeded,
                result_digest: Some(Digest::from_bytes([31; 32])),
                reason: LifecycleReason::Authorization(ReasonCode::Authorized),
            },
        )
        .unwrap();
    for assignment in [
        "state='prepared'",
        "state='failed'",
        "outcome_json='{}'",
        "call_id='renamed-call'",
    ] {
        assert!(
            conn.execute(
                &format!("UPDATE governed_tool_intents SET {assignment}"),
                []
            )
            .is_err()
        );
    }
    drop(conn);
    drop(store);
    let mut reopened = RuntimeStore::open(&path).unwrap();
    assert_eq!(
        reopened
            .lookup(original.execution_id)
            .unwrap()
            .unwrap()
            .state,
        OutcomeState::Succeeded
    );
    let mut replay = original.clone();
    replay.execution_id = Digest::from_bytes([32; 32]);
    assert!(matches!(
        reopened.prepare(&replay).unwrap(),
        PrepareOutcome::Uncertain
    ));
    // The same guard permits the intended recovery transition for another intent.
    let unfinished = request("immutable-recovery", 33);
    prepared(&mut reopened, &unfinished);
    drop(reopened);
    let mut recovered = RuntimeStore::open(&path).unwrap();
    assert_eq!(
        recovered
            .lookup(unfinished.execution_id)
            .unwrap()
            .unwrap()
            .state,
        OutcomeState::Uncertain
    );
    assert!(matches!(
        recovered.prepare(&unfinished).unwrap(),
        PrepareOutcome::Uncertain
    ));
}

fn failed_original(store: &mut RuntimeStore, call: &str, id: u8) -> PreparedExecution {
    let original = request(call, id);
    let permit = prepared(store, &original);
    store
        .finish(
            &permit,
            &TerminalOutcome {
                version: 1,
                state: OutcomeState::Failed,
                result_digest: Some(Digest::from_bytes([90; 32])),
                reason: LifecycleReason::AdapterFailure,
            },
        )
        .unwrap();
    permit
}
fn inverse(original: &PreparedExecution, call: &str, id: u8) -> PrepareRequest {
    let mut r = request(call, id);
    r.binding.scope_digest = original.binding().scope_digest;
    r.binding.compensation_origin = Some(CompensationBinding::from_prepared(original));
    r
}
#[test]
fn compensation_origin_is_exact_single_use_and_survives_restart() {
    let dir = Scratch::new();
    let path = dir.path().join("runtime.sqlite");
    let mut store = RuntimeStore::open(&path).unwrap();
    let original = failed_original(&mut store, "original", 40);
    let request = inverse(&original, "inverse-one", 41);
    let mut wrong = request.clone();
    wrong.binding.scope_digest = Digest::from_bytes([99; 32]);
    assert!(matches!(
        store.prepare(&wrong).unwrap(),
        PrepareOutcome::Refused(_)
    ));
    let mut wrong = request.clone();
    wrong
        .binding
        .compensation_origin
        .as_mut()
        .unwrap()
        .original_binding_digest = Digest::from_bytes([99; 32]);
    assert!(matches!(
        store.prepare(&wrong).unwrap(),
        PrepareOutcome::Refused(_)
    ));
    let mut wrong = request.clone();
    wrong
        .binding
        .compensation_origin
        .as_mut()
        .unwrap()
        .original_execution_id = Digest::from_bytes([99; 32]);
    assert!(matches!(
        store.prepare(&wrong).unwrap(),
        PrepareOutcome::Refused(_)
    ));
    prepared(&mut store, &request);
    assert!(matches!(
        store
            .prepare(&inverse(&original, "inverse-two", 42))
            .unwrap(),
        PrepareOutcome::Uncertain
    ));
    drop(store);
    let mut store = RuntimeStore::open(&path).unwrap();
    assert_eq!(
        store.lookup(request.execution_id).unwrap().unwrap().state,
        OutcomeState::Uncertain
    );
    assert!(matches!(
        store
            .prepare(&inverse(&original, "inverse-three", 43))
            .unwrap(),
        PrepareOutcome::Uncertain
    ));
}
#[test]
fn compensation_rejects_every_non_failed_origin() {
    for (offset, state, reason) in [
        (
            0,
            OutcomeState::Prepared,
            LifecycleReason::Authorization(ReasonCode::Authorized),
        ),
        (
            1,
            OutcomeState::Succeeded,
            LifecycleReason::Authorization(ReasonCode::Authorized),
        ),
        (2, OutcomeState::Uncertain, LifecycleReason::OutcomeUnknown),
        (
            3,
            OutcomeState::Unverifiable,
            LifecycleReason::PostconditionUnverifiable,
        ),
        (
            4,
            OutcomeState::CompensationFailed,
            LifecycleReason::CompensationFailed,
        ),
    ] {
        let mut store = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
        let original = prepared(&mut store, &request("nonfailed", 50 + offset));
        if state != OutcomeState::Prepared {
            store
                .finish(
                    &original,
                    &TerminalOutcome {
                        version: 1,
                        state,
                        reason,
                        result_digest: if state == OutcomeState::Uncertain {
                            None
                        } else {
                            Some(Digest::from_bytes([90; 32]))
                        },
                    },
                )
                .unwrap();
        }
        assert!(matches!(
            store
                .prepare(&inverse(&original, "inverse", 60 + offset))
                .unwrap(),
            PrepareOutcome::Refused(_)
        ));
    }
}
#[test]
fn independent_connections_reserve_failed_origin_once() {
    let dir = Scratch::new();
    let path = dir.path().join("runtime.sqlite");
    let mut store = RuntimeStore::open(&path).unwrap();
    let original = failed_original(&mut store, "race-origin", 70);
    drop(store);
    let mut a = RuntimeStore::open(&path).unwrap();
    let mut b = RuntimeStore::open(&path).unwrap();
    let first = inverse(&original, "inverse-race-a", 71);
    let second = inverse(&original, "inverse-race-b", 72);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let gate = barrier.clone();
    let thread = std::thread::spawn(move || {
        gate.wait();
        a.prepare(&first).unwrap()
    });
    barrier.wait();
    let second = b.prepare(&second).unwrap();
    let first = thread.join().unwrap();
    assert_eq!(
        usize::from(matches!(first, PrepareOutcome::Prepared(_)))
            + usize::from(matches!(second, PrepareOutcome::Prepared(_))),
        1
    );
}

#[test]
fn compensation_reservation_and_human_consumption_rollback_together() {
    let mut store = RuntimeStore::open(std::path::Path::new(":memory:")).unwrap();
    let original = failed_original(&mut store, "atomic-origin", 80);
    let first = approve_request(&store, inverse(&original, "atomic-inverse-one", 81));
    let second = approve_request(&store, inverse(&original, "atomic-inverse-two", 82));
    store.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_inverse BEFORE INSERT ON governed_stage_events BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(store.prepare(&first).is_err());
    assert_eq!(
        store
            .tool_approval(&first.call_id, 1200)
            .unwrap()
            .unwrap()
            .state,
        ToolApprovalState::Approved
    );
    store
        .conn
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_inverse")
        .unwrap();
    prepared(&mut store, &second);
    assert!(matches!(
        store.prepare(&first).unwrap(),
        PrepareOutcome::Uncertain
    ));
    assert_eq!(
        store
            .tool_approval(&first.call_id, 1200)
            .unwrap()
            .unwrap()
            .state,
        ToolApprovalState::Approved
    );
}

#[test]
fn refusal_history_is_append_only_and_post_admission_binding_is_checked_on_reopen() {
    let dir = Scratch::new();
    let path = dir.path().join("runtime.sqlite");
    let mut store = RuntimeStore::open(&path).unwrap();
    let r = request("audit-boundary", 31);
    let denial = StageAudit {
        version: 1,
        execution_id: r.execution_id,
        stage: 11,
        binding_digest: binding_digest(&r.binding),
        decision: Decision::Deny,
        reason: LifecycleReason::CancelledBeforeDispatch,
    };
    store.append_stage(&denial).unwrap();
    let proof = prepared(&mut store, &r);
    let mut post = StageAudit {
        stage: 12,
        decision: Decision::Allow,
        reason: LifecycleReason::Authorization(ReasonCode::Authorized),
        ..denial
    };
    store.append_stage(&post).unwrap();
    post.binding_digest = Digest::from_bytes([99; 32]);
    assert_eq!(store.append_stage(&post), Err(LifecycleError::InvalidState));
    drop(store);
    let mut reopened = RuntimeStore::open(&path).unwrap();
    assert!(reopened.lookup(proof.execution_id()).unwrap().is_some());
    drop(reopened);
    let conn = Connection::open(&path).unwrap();
    assert!(
        conn.execute("DELETE FROM governed_admission_refusals", [])
            .is_err()
    );
    assert!(
        conn.execute("UPDATE governed_admission_refusals SET stage=10", [])
            .is_err()
    );
    conn.execute_batch("DROP TRIGGER governed_events_no_update")
        .unwrap();
    conn.execute(
        "UPDATE governed_stage_events SET record_json=?1 WHERE stage=12",
        [serde_json::to_string(&post).unwrap()],
    )
    .unwrap();
    drop(conn);
    assert!(RuntimeStore::open(&path).is_err());
}

#[test]
fn replay_denial_preserves_terminal_authority_and_rolls_back_on_insert_failure() {
    for human in [false, true] {
        for state in [
            OutcomeState::Succeeded,
            OutcomeState::NotDispatched,
            OutcomeState::Uncertain,
        ] {
            let scratch = Scratch::new();
            let path = scratch.path().join("replay.sqlite");
            let mut store = RuntimeStore::open(&path).unwrap();
            let r = if human {
                approved(&store, "replay", 71)
            } else {
                request("replay", 71)
            };
            let proof = prepared(&mut store, &r);
            let terminal = TerminalOutcome {
                version: 1,
                state,
                result_digest: (state == OutcomeState::Succeeded)
                    .then_some(Digest::from_bytes([8; 32])),
                reason: if state == OutcomeState::NotDispatched {
                    LifecycleReason::CancelledBeforeDispatch
                } else if state == OutcomeState::Uncertain {
                    LifecycleReason::OutcomeUnknown
                } else {
                    LifecycleReason::Authorization(ReasonCode::Authorized)
                },
            };
            store.finish(&proof, &terminal).unwrap();
            drop(store);
            let mut store = RuntimeStore::open(&path).unwrap();
            let original = store.lookup(r.execution_id).unwrap();
            let approval = store.tool_approval("replay", 1200).unwrap();
            assert!(!store.refuse_replay(Digest::from_bytes([72; 32])).unwrap());
            store.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_replay BEFORE INSERT ON governed_admission_refusals BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
            assert_eq!(
                store.refuse_replay(r.execution_id),
                Err(LifecycleError::Unavailable)
            );
            let count: i64 = store
                .conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM governed_admission_refusals",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0);
            store
                .conn
                .lock()
                .unwrap()
                .execute_batch("DROP TRIGGER fail_replay")
                .unwrap();
            assert!(store.refuse_replay(r.execution_id).unwrap());
            assert_eq!(store.lookup(r.execution_id).unwrap(), original);
            assert_eq!(store.tool_approval("replay", 1200).unwrap(), approval);
            drop(store);
            let mut store = RuntimeStore::open(&path).unwrap();
            assert_eq!(store.lookup(r.execution_id).unwrap(), original);
            let text: String = store
                .conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT record_json FROM governed_admission_refusals",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let denial: StageAudit = serde_json::from_str(&text).unwrap();
            assert_eq!(denial.binding_digest, binding_digest(&r.binding));
            assert_eq!(
                denial.reason,
                LifecycleReason::Authorization(ReasonCode::ApprovalReplayed)
            );
        }
    }
}
