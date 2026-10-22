use super::*;

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
