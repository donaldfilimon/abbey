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
        .join(format!("abbey-v4-cancel-client-{}", uuid::Uuid::new_v4()));
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
        assert!(matches!(request.command, V4Command::CancelTurn(_, _)));
        if let Some(event) = reply(&request) {
            let mut response = V4ResponseEnvelope::ok(request.request_id, event);
            response.schema_version = crate::app_core::APP_SCHEMA_V4_CONVERSATIONS;
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
            "cancellation must never retry"
        );
    });
    let client = DaemonClient::new(DaemonConfig::local(
        socket,
        BearerSecret::parse("cancel-client-fixture-bearer-00001").unwrap(),
    ));
    (
        V4DaemonSession {
            schema_version: crate::app_core::APP_SCHEMA_V4_CONVERSATIONS,
            client,
            granted: V4CapabilitySet::from_sorted(vec![V4Capability::CancelTurns]).unwrap(),
        },
        handle,
        root,
    )
}

#[test]
fn v4_cancel_client_rejects_wrong_pair_or_event_and_sends_once() {
    for mismatch in 0..3 {
        let c = ConversationId::new();
        let r = RunId::new();
        let expected_c = c.clone();
        let expected_r = r.clone();
        let (session, server, root) = fixture(move |request| {
            assert!(
                matches!(&request.command, V4Command::CancelTurn(c,r) if c == &expected_c && r == &expected_r)
            );
            Some(match mismatch {
                0 => V4Event::TurnCancelled(ConversationId::new(), expected_r, RunState::Cancelled),
                1 => V4Event::TurnCancelled(expected_c, RunId::new(), RunState::Cancelled),
                _ => V4Event::Routes(vec![]),
            })
        });
        assert_eq!(
            session.cancel_turn(c, r),
            Err(V4ClientError::InvalidResponse)
        );
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn v4_cancel_client_lost_response_sends_once_and_terminal_race_is_truthful() {
    let (session, server, root) = fixture(|_| None);
    assert_eq!(
        session.cancel_turn(ConversationId::new(), RunId::new()),
        Err(V4ClientError::Transport)
    );
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    let (session, server, root) = fixture(|request| {
        let V4Command::CancelTurn(c, r) = &request.command else {
            unreachable!()
        };
        Some(V4Event::TurnCancelled(
            c.clone(),
            r.clone(),
            RunState::Succeeded,
        ))
    });
    assert_eq!(
        session.cancel_turn(ConversationId::new(), RunId::new()),
        Ok(RunState::Succeeded)
    );
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v4_cancel_client_missing_grant_never_connects() {
    let client = DaemonClient::new(DaemonConfig::local(
        "/missing-v4-cancel-fixture",
        BearerSecret::parse("cancel-client-fixture-bearer-00001").unwrap(),
    ));
    let session = V4DaemonSession {
        schema_version: crate::app_core::APP_SCHEMA_V4_CONVERSATIONS,
        client,
        granted: V4CapabilitySet::deny_all(),
    };
    assert_eq!(
        session.cancel_turn(ConversationId::new(), RunId::new()),
        Err(V4ClientError::CapabilityDenied)
    );
}
