//! Encoded schema3 budgets must preserve whole-item continuation through public readers.
use super::*;
use crate::app_core::{V4Capability, V4ContentEvent, V4ContentPayload, V4GrantNegotiation};
use crate::daemon::{
    BearerSecret, DaemonConfig, DaemonHandler, DaemonServer, HandlerFailure, Shutdown,
};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct CompletedRun {
    conversation: ConversationId,
    run: RunId,
    events: Arc<Vec<V4ContentEvent>>,
}
impl DaemonHandler for CompletedRun {
    fn supports_version(&self, _: u16) -> bool {
        false
    }
    fn handle_versioned(
        &self,
        _: u16,
        _: crate::app_core::AppCommand,
    ) -> Result<crate::app_core::AppEvent, HandlerFailure> {
        Err(HandlerFailure::new("invalid_command", "fixture protocol"))
    }
    fn supports_v4(&self) -> bool {
        true
    }
    fn authorizes_v4(&self, grants: &V4CapabilitySet, command: &V4Command) -> bool {
        grants.as_slice() == [V4Capability::ReadTurns] && grants.permits(command)
    }
    fn handle_v4(&self, command: V4Command) -> Result<V4Event, HandlerFailure> {
        match command {
            V4Command::Negotiate(r) => Ok(V4Event::Negotiated(V4GrantNegotiation {
                protocol_version: 4,
                schema_version: 3,
                granted: r.requested,
            })),
            V4Command::ReadContent(q)
                if q.conversation_id == self.conversation && q.run_id == self.run =>
            {
                let events: Vec<_> = self
                    .events
                    .iter()
                    .filter(|e| e.sequence > q.after)
                    .take(usize::from(q.limit))
                    .cloned()
                    .collect();
                let last = events.last().map(|e| e.sequence);
                let next_after = last.filter(|s| *s < self.events.last().unwrap().sequence);
                let page = V4ContentPage {
                    conversation_id: self.conversation.clone(),
                    run_id: self.run.clone(),
                    after: q.after,
                    events,
                    next_after,
                };
                page.validate().unwrap();
                Ok(V4Event::Content(page))
            }
            _ => Err(HandlerFailure::new("invalid_command", "fixture scope")),
        }
    }
}

#[test]
fn schema3_encoded_pages_reconstruct_one_completed_run_losslessly() {
    let conversation = ConversationId::new();
    let run = RunId::new();
    let mut source: Vec<_> = (1..=32)
        .map(|sequence| V4ContentEvent {
            conversation_id: conversation.clone(),
            run_id: run.clone(),
            sequence,
            timestamp_ms: sequence,
            payload: V4ContentPayload::TextDelta {
                text: format!("item-{sequence}:{}", "abc🙂\n".repeat(1500)),
            },
        })
        .collect();
    source.push(V4ContentEvent {
        conversation_id: conversation.clone(),
        run_id: run.clone(),
        sequence: 33,
        timestamp_ms: 33,
        payload: V4ContentPayload::Terminal {
            state: RunState::Succeeded,
        },
    });
    let source = Arc::new(source);
    let root =
        std::path::Path::new("/tmp").join(format!("abbey-v4-pages-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = root.join("daemon.sock");
    let config = DaemonConfig::local(
        &socket,
        BearerSecret::parse("content-page-inert-bearer-00000001").unwrap(),
    );
    let client = DaemonClient::new(config.clone());
    let done = Shutdown::default();
    let stop = done.clone();
    let owned = source.clone();
    let c = conversation.clone();
    let r = run.clone();
    let server = std::thread::spawn(move || {
        DaemonServer::new(
            config,
            CompletedRun {
                conversation: c,
                run: r,
                events: owned,
            },
        )
        .serve(stop)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    for schema in [2, 3] {
        let grants = V4CapabilitySet::from_sorted(vec![V4Capability::ReadTurns]).unwrap();
        let session = if schema == 2 {
            client.negotiate_v4_schema2(grants)
        } else {
            client.negotiate_v4(grants)
        }
        .unwrap();
        let mut after = 0;
        let mut pages = 0;
        let mut restored = Vec::new();
        loop {
            pages += 1;
            assert!(pages <= 33);
            let page = session
                .read_content(V4ContentQuery {
                    conversation_id: conversation.clone(),
                    run_id: run.clone(),
                    after,
                    limit: 50,
                })
                .unwrap();
            assert_eq!(page.after, after);
            assert!(!page.events.is_empty());
            let last = page.events.last().unwrap().sequence;
            restored.extend(page.events);
            match page.next_after {
                Some(next) => {
                    assert_eq!(next, last);
                    assert!(next > after && next < 33);
                    after = next
                }
                None => break,
            }
        }
        assert_eq!(restored, *source);
        assert_eq!(restored.last().unwrap().sequence, 33);
        if schema == 2 {
            assert_eq!(pages, 1);
        } else {
            assert!(pages > 1);
        }
    }
    done.request();
    server.join().unwrap().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn legal_indivisible_large_item_refuses_schema3_and_remains_complete_on_schema2() {
    let conversation = ConversationId::new();
    let run = RunId::new();
    let text = "\u{1}".repeat(16 * 1024);
    let source = Arc::new(vec![
        V4ContentEvent {
            conversation_id: conversation.clone(),
            run_id: run.clone(),
            sequence: 1,
            timestamp_ms: 1,
            payload: V4ContentPayload::TextDelta { text },
        },
        V4ContentEvent {
            conversation_id: conversation.clone(),
            run_id: run.clone(),
            sequence: 2,
            timestamp_ms: 2,
            payload: V4ContentPayload::Terminal {
                state: RunState::Succeeded,
            },
        },
    ]);
    source[0].validate().unwrap();
    assert!(serde_json::to_vec(&source[0]).unwrap().len() > 64 * 1024);
    let root =
        std::path::Path::new("/tmp").join(format!("abbey-v4-large-item-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = root.join("daemon.sock");
    let config = DaemonConfig::local(
        &socket,
        BearerSecret::parse("large-item-inert-bearer-0000000001").unwrap(),
    );
    let client = DaemonClient::new(config.clone());
    let done = Shutdown::default();
    let stop = done.clone();
    let owned = source.clone();
    let c = conversation.clone();
    let r = run.clone();
    let server = std::thread::spawn(move || {
        DaemonServer::new(
            config,
            CompletedRun {
                conversation: c,
                run: r,
                events: owned,
            },
        )
        .serve(stop)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let grants = V4CapabilitySet::from_sorted(vec![V4Capability::ReadTurns]).unwrap();
    let old = client.negotiate_v4_schema2(grants.clone()).unwrap();
    let new = client.negotiate_v4(grants).unwrap();
    let query = || V4ContentQuery {
        conversation_id: conversation.clone(),
        run_id: run.clone(),
        after: 0,
        limit: 50,
    };
    assert_eq!(old.read_content(query()).unwrap().events, *source);
    assert!(matches!(
        new.read_content(query()),
        Err(V4ClientError::Rejected(V4ErrorCode::ResponseTooLarge))
    ));
    assert_eq!(
        old.read_content(query()).unwrap().events,
        *source,
        "budgeting cannot modify stored content or pretend completion"
    );
    done.request();
    server.join().unwrap().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
