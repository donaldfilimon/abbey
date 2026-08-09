use abbey::app_core::{
    AppCommand, AppEvent, BackendSelection, IdempotencyKey, RunEventPage, RunEventRecord,
    RunEventsQuery, RunId, RunLifecycleEvent, RunMode, RunRequest, RunSnapshot, RunState,
    RunSubmission, RunSubmissionDisposition,
};
use abbey::run_control::{
    RunControlCliCommand, RunLifecycleReducer, parse_slash_run_args, render_human,
};

const RUN_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const IDEMPOTENCY_KEY: &str = "frontend-fixture";

#[test]
fn shared_parser_builds_the_exact_bounded_commands() {
    let submit = parse(&[
        "submit",
        "--backend",
        "abi",
        "--run-mode",
        "one-shot",
        "--idempotency-key",
        IDEMPOTENCY_KEY,
        "--label",
        "fixture",
        "--json",
        "private",
        "prompt",
    ]);
    let AppCommand::SubmitRun(request) = submit.into_app_command().unwrap() else {
        panic!("submit must build SubmitRun");
    };
    assert_eq!(request.backend, BackendSelection::Abi);
    assert_eq!(request.mode, RunMode::OneShot);
    assert_eq!(request.idempotency_key.as_str(), IDEMPOTENCY_KEY);
    assert_eq!(request.labels, ["fixture"]);
    assert_eq!(request.input, "private prompt");
    let debug = format!(
        "{:?}",
        parse(&["submit", "--backend", "abi", "private-debug-prompt"])
    );
    assert!(!debug.contains("private-debug-prompt"));
    assert!(debug.contains("[REDACTED]"));

    let run_id = run_id();
    let status = parse(&["status", RUN_ID, "--json"])
        .into_app_command()
        .unwrap();
    let cancel = parse(&["cancel", RUN_ID, "--json"])
        .into_app_command()
        .unwrap();
    let events = parse(&[
        "events",
        RUN_ID,
        "--after-sequence",
        "2",
        "--through-sequence",
        "4",
        "--limit",
        "2",
        "--json",
    ])
    .into_app_command()
    .unwrap();

    assert_eq!(
        serde_json::to_value([status, cancel, events]).unwrap(),
        serde_json::json!([
            {"type":"get_run","payload":{"run_id":run_id}},
            {"type":"cancel_run","payload":{"run_id":run_id}},
            {
                "type":"run_events",
                "payload":{
                    "run_id":run_id,
                    "after_sequence":2,
                    "through_sequence":4,
                    "limit":2
                }
            }
        ])
    );
}

#[test]
fn reducer_has_a_stable_sanitized_fixed_watermark_fixture() {
    let run_id = run_id();
    let submit = AppCommand::SubmitRun(RunRequest {
        idempotency_key: IDEMPOTENCY_KEY.parse::<IdempotencyKey>().unwrap(),
        conversation_id: None,
        mode: RunMode::Background,
        backend: BackendSelection::Abi,
        input: "private prompt must not survive projection".into(),
        labels: vec!["fixture".into()],
    });
    let snapshot = snapshot(run_id.clone(), RunState::Queued, 1);
    let mut reducer = RunLifecycleReducer::default();
    reducer
        .apply(
            &submit,
            AppEvent::RunSubmitted(RunSubmission {
                disposition: RunSubmissionDisposition::Enqueued,
                run: snapshot.clone(),
            }),
        )
        .unwrap();

    let first_query = AppCommand::RunEvents(RunEventsQuery {
        run_id: run_id.clone(),
        after_sequence: 0,
        through_sequence: None,
        limit: 4,
    });
    reducer
        .apply(
            &first_query,
            AppEvent::RunEvents(page(
                run_id.clone(),
                0,
                4,
                true,
                &[
                    (1, RunLifecycleEvent::Queued),
                    (2, RunLifecycleEvent::Starting),
                ],
            )),
        )
        .unwrap();

    let next = reducer.next_events_command(2).unwrap().unwrap();
    assert_eq!(
        next,
        AppCommand::RunEvents(RunEventsQuery {
            run_id: run_id.clone(),
            after_sequence: 2,
            through_sequence: Some(4),
            limit: 2,
        })
    );
    reducer
        .apply(
            &next,
            AppEvent::RunEvents(page(
                run_id,
                2,
                4,
                false,
                &[
                    (3, RunLifecycleEvent::Running),
                    (4, RunLifecycleEvent::Succeeded),
                ],
            )),
        )
        .unwrap();
    assert!(reducer.next_events_command(2).unwrap().is_none());

    let encoded = serde_json::to_value(reducer.view()).unwrap();
    assert_eq!(
        encoded,
        serde_json::json!({
            "snapshot": {
                "run_id": RUN_ID,
                "conversation_id": null,
                "idempotency_key": IDEMPOTENCY_KEY,
                "state": "queued",
                "created_at": "2026-08-08T12:00:00Z",
                "updated_at": "2026-08-08T12:00:00Z",
                "failure": null,
                "event_count": 1
            },
            "submission": "enqueued",
            "event_pages": [
                {
                    "run_id": RUN_ID,
                    "events": [
                        {
                            "run_id": RUN_ID,
                            "sequence": 1,
                            "recorded_at": "2026-08-08T12:00:01Z",
                            "event": {"type": "queued"}
                        },
                        {
                            "run_id": RUN_ID,
                            "sequence": 2,
                            "recorded_at": "2026-08-08T12:00:02Z",
                            "event": {"type": "starting"}
                        }
                    ],
                    "after_sequence": 0,
                    "next_after_sequence": 2,
                    "through_sequence": 4,
                    "has_more": true
                },
                {
                    "run_id": RUN_ID,
                    "events": [
                        {
                            "run_id": RUN_ID,
                            "sequence": 3,
                            "recorded_at": "2026-08-08T12:00:03Z",
                            "event": {"type": "running"}
                        },
                        {
                            "run_id": RUN_ID,
                            "sequence": 4,
                            "recorded_at": "2026-08-08T12:00:04Z",
                            "event": {"type": "succeeded"}
                        }
                    ],
                    "after_sequence": 2,
                    "next_after_sequence": 4,
                    "through_sequence": 4,
                    "has_more": false
                }
            ]
        })
    );
    let wire = serde_json::to_string(&encoded).unwrap();
    assert!(!wire.contains("private prompt"));
    assert!(!wire.contains("provider"));
    assert!(render_human(reducer.view()).contains("has_more=false"));
}

#[test]
fn reducer_rejects_changed_watermarks_run_ids_and_lifecycle_regressions() {
    let run_id = run_id();
    let query = AppCommand::RunEvents(RunEventsQuery {
        run_id: run_id.clone(),
        after_sequence: 0,
        through_sequence: None,
        limit: 3,
    });
    let mut reducer = RunLifecycleReducer::default();
    reducer
        .apply(
            &query,
            AppEvent::RunEvents(page(
                run_id.clone(),
                0,
                4,
                true,
                &[
                    (1, RunLifecycleEvent::Queued),
                    (2, RunLifecycleEvent::Starting),
                    (3, RunLifecycleEvent::Running),
                ],
            )),
        )
        .unwrap();

    let changed_watermark = AppCommand::RunEvents(RunEventsQuery {
        run_id: run_id.clone(),
        after_sequence: 3,
        through_sequence: Some(5),
        limit: 1,
    });
    assert!(
        reducer
            .apply(
                &changed_watermark,
                AppEvent::RunEvents(page(
                    run_id.clone(),
                    3,
                    5,
                    true,
                    &[(4, RunLifecycleEvent::Succeeded)],
                )),
            )
            .is_err()
    );

    let regressing = AppCommand::RunEvents(RunEventsQuery {
        run_id: run_id.clone(),
        after_sequence: 3,
        through_sequence: Some(4),
        limit: 1,
    });
    assert!(
        reducer
            .apply(
                &regressing,
                AppEvent::RunEvents(page(
                    run_id,
                    3,
                    4,
                    false,
                    &[(4, RunLifecycleEvent::Starting)],
                )),
            )
            .is_err()
    );
}

#[test]
fn reducer_rejects_snapshot_identity_mutation_and_state_without_new_event() {
    let run_id = run_id();
    let command = AppCommand::GetRun(abbey::app_core::RunQuery {
        run_id: run_id.clone(),
    });
    let mut reducer = RunLifecycleReducer::default();
    reducer
        .apply(
            &command,
            AppEvent::RunStatus(snapshot(run_id.clone(), RunState::Queued, 1)),
        )
        .unwrap();

    let mut changed_identity = snapshot(run_id.clone(), RunState::Queued, 1);
    changed_identity.idempotency_key = "different-request".parse().unwrap();
    assert!(
        reducer
            .apply(&command, AppEvent::RunStatus(changed_identity))
            .is_err()
    );
    assert!(
        reducer
            .apply(
                &command,
                AppEvent::RunStatus(snapshot(run_id, RunState::Running, 1)),
            )
            .is_err()
    );
}

#[test]
fn reducer_requires_queued_first_and_rejects_events_after_terminal_state() {
    let run_id = run_id();
    let query = AppCommand::RunEvents(RunEventsQuery {
        run_id: run_id.clone(),
        after_sequence: 0,
        through_sequence: None,
        limit: 4,
    });
    let mut reducer = RunLifecycleReducer::default();
    assert!(
        reducer
            .apply(
                &query,
                AppEvent::RunEvents(page(
                    run_id.clone(),
                    0,
                    1,
                    false,
                    &[(1, RunLifecycleEvent::Running)],
                )),
            )
            .is_err()
    );

    let first = AppCommand::RunEvents(RunEventsQuery {
        run_id: run_id.clone(),
        after_sequence: 0,
        through_sequence: None,
        limit: 4,
    });
    reducer
        .apply(
            &first,
            AppEvent::RunEvents(page(
                run_id.clone(),
                0,
                5,
                true,
                &[
                    (1, RunLifecycleEvent::Queued),
                    (2, RunLifecycleEvent::Starting),
                    (3, RunLifecycleEvent::Running),
                    (4, RunLifecycleEvent::Succeeded),
                ],
            )),
        )
        .unwrap();
    let after_terminal = AppCommand::RunEvents(RunEventsQuery {
        run_id: run_id.clone(),
        after_sequence: 4,
        through_sequence: Some(5),
        limit: 1,
    });
    assert!(
        reducer
            .apply(
                &after_terminal,
                AppEvent::RunEvents(page(
                    run_id,
                    4,
                    5,
                    false,
                    &[(5, RunLifecycleEvent::Running)],
                )),
            )
            .is_err()
    );
}

fn parse(args: &[&str]) -> RunControlCliCommand {
    parse_slash_run_args(&args.iter().map(|value| (*value).into()).collect::<Vec<_>>()).unwrap()
}

fn run_id() -> RunId {
    RUN_ID.parse().unwrap()
}

fn snapshot(run_id: RunId, state: RunState, event_count: u64) -> RunSnapshot {
    RunSnapshot {
        run_id,
        conversation_id: None,
        idempotency_key: IDEMPOTENCY_KEY.parse().unwrap(),
        state,
        created_at: "2026-08-08T12:00:00Z".into(),
        updated_at: "2026-08-08T12:00:00Z".into(),
        failure: None,
        event_count,
    }
}

fn page(
    run_id: RunId,
    after_sequence: u64,
    through_sequence: u64,
    has_more: bool,
    events: &[(u64, RunLifecycleEvent)],
) -> RunEventPage {
    RunEventPage {
        run_id: run_id.clone(),
        events: events
            .iter()
            .map(|(sequence, event)| RunEventRecord {
                run_id: run_id.clone(),
                sequence: *sequence,
                recorded_at: format!("2026-08-08T12:00:{sequence:02}Z"),
                event: event.clone(),
            })
            .collect(),
        after_sequence,
        next_after_sequence: events
            .last()
            .map_or(after_sequence, |(sequence, _)| *sequence),
        through_sequence,
        has_more,
    }
}
