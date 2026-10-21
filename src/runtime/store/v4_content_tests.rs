//! Durable visible-content regression coverage.
use super::*;

#[test]
fn v4_admission_has_no_fabricated_assistant_text() {
    let (_d, store) = scratch_store("no-admission-text");
    let id = prepare(&store, "Admission");
    let sub = store
        .v4_admit_turn(&submit(&id, "admit", "hello"), NOW)
        .unwrap();
    let page = store
        .v4_content(&V4ContentQuery {
            conversation_id: id,
            run_id: sub.run_id,
            after: 0,
            limit: 50,
        })
        .unwrap();
    assert!(
        page.events
            .iter()
            .all(|e| !matches!(e.payload, V4ContentPayload::TextDelta { .. }))
    );
}

#[test]
fn v4_truncated_preview_preserves_total_bytes_after_reopen() {
    let (dir, store) = scratch_store("preview-total");
    let id = prepare(&store, "Total");
    let sub = store
        .v4_admit_turn(&submit(&id, "total", "hello"), NOW)
        .unwrap();
    for text in ["a".repeat(16_383), "🙂".into(), "z".repeat(100)] {
        store
            .v4_append_content(&sub.run_id, V4ContentPayload::TextDelta { text }, NOW)
            .unwrap();
    }
    store.v4_fail_unadopted_turn(&sub.run_id).unwrap();
    drop(store);
    let store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(dir.path())).unwrap();
    let page = store
        .v4_messages(&V4MessageQuery {
            conversation_id: id,
            after: None,
            limit: 50,
        })
        .unwrap();
    let msg = page
        .messages
        .iter()
        .find(|m| m.role == crate::app_core::V4MessageRole::Assistant)
        .unwrap();
    assert_eq!(msg.text, "a".repeat(16_383));
    assert_eq!(msg.text_bytes, 16_487);
    assert!(!msg.text_complete);
}

#[test]
fn v4_preview_failure_rolls_back_content_event() {
    let (_dir, store) = scratch_store("preview-rollback");
    let id = prepare(&store, "Atomic");
    let sub = store
        .v4_admit_turn(&submit(&id, "atomic", "hello"), NOW)
        .unwrap();
    store.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_preview BEFORE INSERT ON assistant_messages WHEN NEW.role='assistant' BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
    let before: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM assistant_content_events", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(
        store
            .v4_append_content(
                &sub.run_id,
                V4ContentPayload::TextDelta {
                    text: "answer".into()
                },
                NOW
            )
            .is_err()
    );
    let after: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM assistant_content_events", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn v4_turn_enforces_cumulative_text_budget() {
    let (_dir, store) = scratch_store("turn-budget");
    let id = prepare(&store, "Budget");
    let sub = store
        .v4_admit_turn(&submit(&id, "budget", "hello"), NOW)
        .unwrap();
    for _ in 0..256 {
        store
            .v4_append_content(
                &sub.run_id,
                V4ContentPayload::TextDelta {
                    text: "x".repeat(16_384),
                },
                NOW,
            )
            .unwrap();
    }
    assert!(
        store
            .v4_append_content(
                &sub.run_id,
                V4ContentPayload::TextDelta { text: "x".into() },
                NOW
            )
            .is_err()
    );
    store.v4_fail_unadopted_turn(&sub.run_id).unwrap();
}

#[test]
fn v4_content_pages_fit_wire_bound_and_reconstruct_escaped_text() {
    let (_dir, store) = scratch_store("wire-page");
    let id = prepare(&store, "Wire");
    let sub = store
        .v4_admit_turn(&submit(&id, "wire", "hello"), NOW)
        .unwrap();
    let chunk = "\u{1}".repeat(16_384);
    for _ in 0..12 {
        store
            .v4_append_content(
                &sub.run_id,
                V4ContentPayload::TextDelta {
                    text: chunk.clone(),
                },
                NOW,
            )
            .unwrap();
    }
    store.v4_fail_unadopted_turn(&sub.run_id).unwrap();
    let mut after = 0;
    let mut text = String::new();
    loop {
        let page = store
            .v4_content(&V4ContentQuery {
                conversation_id: id.clone(),
                run_id: sub.run_id.clone(),
                after,
                limit: 50,
            })
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= crate::app_core::MAX_V4_PAGE_BYTES);
        for e in page.events {
            if let V4ContentPayload::TextDelta { text: delta } = e.payload {
                text.push_str(&delta);
            }
        }
        match page.next_after {
            Some(next) => {
                assert!(next > after);
                after = next;
            }
            None => break,
        }
    }
    assert_eq!(text, chunk.repeat(12));
}

#[test]
fn v4_corrupt_content_refuses_read_instead_of_inventing_notice() {
    let (_dir, store) = scratch_store("corrupt-content");
    let id = prepare(&store, "Corrupt");
    let sub = store
        .v4_admit_turn(&submit(&id, "corrupt", "hello"), NOW)
        .unwrap();
    store
        .v4_append_content(
            &sub.run_id,
            V4ContentPayload::TextDelta {
                text: "answer".into(),
            },
            NOW,
        )
        .unwrap();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE assistant_content_payloads SET payload_json='{}'",
            [],
        )
        .unwrap();
    assert!(
        store
            .v4_content(&V4ContentQuery {
                conversation_id: id,
                run_id: sub.run_id,
                after: 0,
                limit: 50
            })
            .is_err()
    );
}

#[test]
fn v4_finalize_cannot_rewrite_terminal_outcome() {
    let (_dir, store) = scratch_store("immutable-terminal");
    let id = prepare(&store, "Terminal");
    let sub = store
        .v4_admit_turn(&submit(&id, "immutable", "hello"), NOW)
        .unwrap();
    store
        .v4_append_content(
            &sub.run_id,
            V4ContentPayload::TextDelta {
                text: "partial".into(),
            },
            NOW,
        )
        .unwrap();
    store.v4_fail_unadopted_turn(&sub.run_id).unwrap();
    assert!(
        store
            .v4_finalize_turn(&sub.run_id, RunState::Succeeded, NOW)
            .is_err()
    );
    let page = store
        .v4_messages(&V4MessageQuery {
            conversation_id: id,
            after: None,
            limit: 50,
        })
        .unwrap();
    assert!(page.messages.iter().all(|m| m.state == RunState::Failed));
}

#[test]
fn v4_content_cursor_is_bound_to_the_selected_run() {
    let (_dir, store) = scratch_store("cursor-binding");
    let id = prepare(&store, "Cursor");
    let a = store
        .v4_admit_turn(&submit(&id, "cursor-a", "hello"), NOW)
        .unwrap();
    let event = store
        .v4_append_content(
            &a.run_id,
            V4ContentPayload::TextDelta {
                text: "first".into(),
            },
            NOW,
        )
        .unwrap();
    store.v4_fail_unadopted_turn(&a.run_id).unwrap();
    let b = store
        .v4_admit_turn(&submit(&id, "cursor-b", "hello"), NOW)
        .unwrap();
    assert!(
        store
            .v4_content(&V4ContentQuery {
                conversation_id: id.clone(),
                run_id: b.run_id.clone(),
                after: event.sequence,
                limit: 50
            })
            .is_err()
    );
    assert!(
        store
            .v4_content(&V4ContentQuery {
                conversation_id: id,
                run_id: b.run_id,
                after: 65_536,
                limit: 50
            })
            .is_err()
    );
}

#[test]
fn v4_exhausted_conversation_refuses_admission_atomically() {
    let (_dir, store) = scratch_store("exhausted-admission");
    let id = prepare(&store, "Full");
    let first = store
        .v4_admit_turn(&submit(&id, "full-first", "hello"), NOW)
        .unwrap();
    store.v4_fail_unadopted_turn(&first.run_id).unwrap();
    store.conn.lock().unwrap().execute(
        "INSERT INTO assistant_content_events(conversation_id, sequence, request_id, run_id, kind, byte_count, timestamp_ms) VALUES (?1,65536,?2,?2,'completed',0,?3)",
        rusqlite::params![id.as_str(), first.run_id.as_str(), NOW as i64],
    ).unwrap();
    let before = store.v4_conversation(&id).unwrap().message_count;
    assert!(matches!(
        store.v4_admit_turn(&submit(&id, "full-next", "next"), NOW),
        Err(StoreError::AssistantConflict)
    ));
    assert_eq!(store.v4_conversation(&id).unwrap().message_count, before);
    assert!(
        run_by_idempotency_key(&store.conn.lock().unwrap(), "full-next")
            .unwrap()
            .is_none()
    );
}

#[test]
fn v4_lookup_turn_recovers_exact_accepted_idempotency_key_without_admission() {
    let (_dir, store) = scratch_store("lookup-accepted");
    let cid = prepare(&store, "Lookup");
    let request = submit(&cid, "lookup-key", "original input");
    let accepted = store.v4_admit_turn(&request, NOW).unwrap();
    let found = store
        .v4_lookup_turn(&cid, &request.idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(found.run_id, accepted.run_id);
    assert_eq!(found.idempotency_key, request.idempotency_key);
    assert_eq!(found.state, RunState::Queued);
    assert!(
        store
            .v4_lookup_turn(&cid, &"not-admitted".parse().unwrap())
            .unwrap()
            .is_none()
    );
    let other = prepare(&store, "Other");
    assert!(
        store
            .v4_lookup_turn(&other, &request.idempotency_key)
            .is_err()
    );
    store.v4_fail_unadopted_turn(&accepted.run_id).unwrap();
    assert_eq!(
        store
            .v4_lookup_turn(&cid, &request.idempotency_key)
            .unwrap()
            .unwrap()
            .state,
        RunState::Failed
    );
    assert_eq!(store.v4_conversation(&cid).unwrap().message_count, 1);
}

#[test]
fn v4_conversation_cursor_must_exist_in_the_exact_title_and_archive_query() {
    let (_dir, store) = scratch_store("conversation-cursor");
    let one = prepare(&store, "Alpha");
    let two = prepare(&store, "Beta");
    let query = |after, search| V4ConversationQuery {
        search,
        after,
        limit: 1,
        archived: false,
    };
    assert!(
        store
            .v4_list_conversations(&query(Some(ConversationId::new()), None))
            .is_err()
    );
    assert!(
        store
            .v4_list_conversations(&query(Some(one.clone()), Some("Beta".into())))
            .is_err()
    );
    let first = store.v4_list_conversations(&query(None, None)).unwrap();
    assert_eq!(first.next_after, Some(one.clone()));
    let second = store
        .v4_list_conversations(&query(first.next_after, None))
        .unwrap();
    assert_eq!(second.conversations[0].conversation_id, two);
    store.v4_archive_conversation(&one, NOW).unwrap();
    assert!(
        store
            .v4_list_conversations(&query(Some(one), None))
            .is_err()
    );
}
