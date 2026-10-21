use super::*;
use crate::app_core::{RunState, V4Capability};
use crate::daemon::{BearerSecret, DaemonConfig};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::time::Duration;

fn fixture(
    reply: impl FnOnce(&V4RequestEnvelope) -> Option<V4Event> + Send + 'static,
) -> (
    V4DaemonSession,
    std::thread::JoinHandle<()>,
    std::path::PathBuf,
) {
    let root = std::path::Path::new("/tmp")
        .join(format!("abbey-v4-lookup-client-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let socket = root.join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut prefix = [0; 4];
        stream.read_exact(&mut prefix).unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(prefix) as usize];
        stream.read_exact(&mut bytes).unwrap();
        let request: V4RequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert!(matches!(request.command, V4Command::LookupTurn(_, _)));
        if let Some(event) = reply(&request) {
            let response = V4ResponseEnvelope::ok(request.request_id, event);
            let bytes = serde_json::to_vec(&response).unwrap();
            stream
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&bytes).unwrap();
        }
        drop(stream);
        listener.set_nonblocking(true).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "lookup must never submit or retry"
        );
    });
    let client = DaemonClient::new(DaemonConfig::local(
        socket,
        BearerSecret::parse("cancel-client-fixture-bearer-00001").unwrap(),
    ));
    (
        V4DaemonSession {
            client,
            granted: V4CapabilitySet::from_sorted(vec![V4Capability::ReadTurns]).unwrap(),
        },
        handle,
        root,
    )
}

#[test]
fn v4_lookup_client_correlates_missing_and_found_receipts_and_sends_once() {
    for mode in 0..4 {
        let conversation = ConversationId::new();
        let key: IdempotencyKey = "lookup-key".parse().unwrap();
        let c = conversation.clone();
        let k = key.clone();
        let (session, server, root) = fixture(move |request| {
            assert!(
                matches!(&request.command,V4Command::LookupTurn(cid,key) if cid==&c && key==&k)
            );
            Some(match mode {
                0 => V4Event::TurnLookup(c, k, None),
                1 => V4Event::TurnLookup(
                    c.clone(),
                    k.clone(),
                    Some(V4TurnSubmission {
                        conversation_id: c,
                        run_id: RunId::new(),
                        idempotency_key: k,
                        user_message_sequence: 1,
                        state: RunState::Running,
                        duplicate: false,
                    }),
                ),
                2 => V4Event::TurnLookup(ConversationId::new(), k, None),
                _ => V4Event::TurnLookup(c, "wrong-key".parse().unwrap(), None),
            })
        });
        let result = session.lookup_turn(conversation, key);
        match mode {
            0 => assert_eq!(result, Ok(None)),
            1 => assert!(result.unwrap().is_some()),
            _ => assert_eq!(result, Err(V4ClientError::InvalidResponse)),
        }
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn v4_lookup_client_lost_read_response_never_resubmits() {
    let (session, server, root) = fixture(|_| None);
    assert_eq!(
        session.lookup_turn(ConversationId::new(), "lookup-key".parse().unwrap()),
        Err(V4ClientError::Transport)
    );
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v4_lookup_without_read_turns_grant_never_connects() {
    let session = V4DaemonSession {
        client: DaemonClient::new(DaemonConfig::local(
            "/missing-v4-lookup-fixture",
            BearerSecret::parse("lookup-no-grant-fixture-bearer-00001").unwrap(),
        )),
        granted: V4CapabilitySet::deny_all(),
    };
    assert_eq!(
        session.lookup_turn(ConversationId::new(), "lookup-key".parse().unwrap()),
        Err(V4ClientError::CapabilityDenied)
    );
}
