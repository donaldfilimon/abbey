//! Chat identity publication follows successful turn notice admission.
// Register as a child of src/agent/owned_budget_tests.rs; it uses that actual
// private Fixture and public current run_resilient/AbbeyState/StreamTap APIs.

use super::*;

#[test]
fn fresh_chat_control_admits_notice_then_publishes_actual_successful_identity() {
    let fixture = Fixture::new("identity-control", 128);
    assert_eq!(
        run_resilient(
            &fixture.cfg,
            &fixture.state,
            true,
            &["synthetic fresh".into()]
        )
        .unwrap(),
        0
    );
    let identity = fixture
        .state
        .resolve_chat_for(AgentBackend::Abi)
        .unwrap()
        .unwrap();
    assert_ne!(identity, CHAT);
    assert!(!fixture.tap.cancel.is_cancelled());
    assert!(
        fixture
            .events
            .try_iter()
            .any(|event| matches!(event, StreamEvent::Notice(text) if text.contains("new chat")))
    );
    assert!(
        fs::read_to_string(fixture.cfg.transcript_path(&identity).unwrap())
            .unwrap()
            .contains(&fixture.body)
    );
}

fn rejected_identity_notice(fresh: bool, empty: bool) {
    let fixture = Fixture::new(
        if empty {
            "identity-empty"
        } else {
            "identity-fresh"
        },
        128,
    );
    if empty {
        fixture.state.clear_chat(false).unwrap();
    }
    let prior = fixture.state.resolve_chat_for(AgentBackend::Abi).unwrap();
    let history_before = fs::read(&fixture.state.history_file).ok();
    let mirror_before = fs::read(fixture.state.active_chat_file()).ok();
    assert_eq!(prior.as_deref(), if empty { None } else { Some(CHAT) });
    // Exactly the decoded ceiling is accepted, with the raw budget untouched.
    // The next new/created-chat diagnostic must reject before canonical save.
    assert!(
        fixture
            .tap
            .emit(StreamEvent::Notice("n".repeat(MAX_CAPTURE_BYTES)))
    );
    assert!(!fixture.tap.exceeded());
    assert!(!fixture.tap.cancel.is_cancelled());
    assert_eq!(fixture.tap.raw_remaining(), MAX_CAPTURE_BYTES);
    let outcome = run_resilient(
        &fixture.cfg,
        &fixture.state,
        fresh,
        &["synthetic rejected identity".into()],
    );
    let after = fixture.state.resolve_chat_for(AgentBackend::Abi).unwrap();
    let history_unchanged = fs::read(&fixture.state.history_file).ok() == history_before;
    let mirror_unchanged = fs::read(fixture.state.active_chat_file()).ok() == mirror_before;
    let failed = fixture
        .events
        .try_iter()
        .any(|event| matches!(event, StreamEvent::Failed(_)));
    assert!(
        !matches!(outcome, Ok(0))
            && fixture.tap.cancel.is_cancelled()
            && failed
            && after == prior
            && history_unchanged
            && mirror_unchanged,
        "rejected identity notice published canonical state: fresh={fresh}, empty={empty}, outcome={outcome:?}, before={prior:?}, after={after:?}, history_unchanged={history_unchanged}, mirror_unchanged={mirror_unchanged}"
    );
    assert_eq!(fs::read(&fixture.transcript).unwrap(), TRANSCRIPT_SEED);
    assert_eq!(fs::read(&fixture.cot).unwrap(), COT_SEED);
}

#[test]
fn fresh_chat_notice_budget_rejection_preserves_existing_canonical_identity() {
    rejected_identity_notice(true, false);
}

#[test]
fn first_chat_notice_budget_rejection_preserves_absent_canonical_identity() {
    rejected_identity_notice(false, true);
}
