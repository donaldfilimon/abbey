//! Actual authenticated socket + RuntimeHandler + private SQLite compatibility.
use super::*;
use crate::app_core::{V4Capability, V4TaskCreate, V4TaskExecutionKind};
use crate::daemon::framing::{self, FrameKind};
use crate::daemon::{
    BearerSecret, DaemonConfig, DaemonServer, RuntimeDaemonConfig, RuntimeHandler, Shutdown,
};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

#[test]
fn schema2_and_schema3_share_real_conversations_without_admin_authority() {
    let root = std::path::Path::new("/tmp").join(format!("abbey-schema3-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let handler = RuntimeHandler::start(RuntimeDaemonConfig::new(&root, &root)).unwrap();
    let socket = root.join("daemon.sock");
    let config = DaemonConfig::local(
        socket.clone(),
        BearerSecret::parse("schema3-inert-fixture-bearer-000001").unwrap(),
    );
    let client = DaemonClient::new(config.clone());
    let shutdown = Shutdown::default();
    let done = shutdown.clone();
    let server = std::thread::spawn(move || DaemonServer::new(config, handler).serve(done));
    let deadline = Instant::now() + Duration::from_secs(5);
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let grants = V4CapabilitySet::from_sorted(vec![
        V4Capability::ReadConversations,
        V4Capability::WriteConversations,
    ])
    .unwrap();
    let old = client.negotiate_v4_schema2(grants.clone()).unwrap();
    let new = client.negotiate_v4(grants).unwrap();
    assert_eq!(old.schema_version, 2);
    assert_eq!(new.schema_version, 3);
    let first = old.create_conversation("Schema two").unwrap();
    let same = new
        .read_conversation(first.conversation_id.clone())
        .unwrap();
    assert_eq!(first, same);
    let second = new.create_conversation("Schema three").unwrap();
    assert_eq!(
        old.read_conversation(second.conversation_id.clone())
            .unwrap(),
        second
    );
    let request = V4Command::CreateTask(V4TaskCreate {
        operation_id: "create:1".parse().unwrap(),
        conversation_id: first.conversation_id,
        goal: "inert fixture".into(),
        workspace_id: "workspace.one".into(),
        route_id: "abi.local".into(),
        permission_profile_id: "profile.one".into(),
        profile_revision: 1,
        budget_tokens: 100,
        execution_kind: V4TaskExecutionKind::ReadOnly,
    });
    let requested =
        V4CapabilitySet::from_sorted(vec![V4Capability::ReadTasks, V4Capability::WriteTasks])
            .unwrap();
    assert!(client.negotiate_v4_schema2(requested.clone()).is_err());
    let admin = client.negotiate_v4(requested).unwrap();
    assert!(admin.grants().as_slice().is_empty());
    assert_eq!(admin.request(request), Err(V4ClientError::CapabilityDenied));
    shutdown.request();
    server.join().unwrap().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn framing_schema_mismatch_and_malformed_errors_keep_the_selected_identity() {
    struct Inert;
    impl crate::daemon::DaemonHandler for Inert {
        fn supports_version(&self, _: u16) -> bool {
            false
        }
        fn handle_versioned(
            &self,
            _: u16,
            _: crate::app_core::AppCommand,
        ) -> Result<crate::app_core::AppEvent, crate::daemon::HandlerFailure> {
            unreachable!()
        }
        fn supports_v4(&self) -> bool {
            true
        }
        fn handle_v4(&self, _: V4Command) -> Result<V4Event, crate::daemon::HandlerFailure> {
            panic!("mismatches never reach handler")
        }
    }
    let root =
        std::path::Path::new("/tmp").join(format!("abbey-schema3-frame-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = root.join("daemon.sock");
    let config = DaemonConfig::local(
        socket.clone(),
        BearerSecret::parse("schema3-inert-fixture-bearer-000001").unwrap(),
    );
    let stop = Shutdown::default();
    let done = stop.clone();
    let server = std::thread::spawn(move || DaemonServer::new(config, Inert).serve(done));
    let deadline = Instant::now() + Duration::from_secs(5);
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    for (kind, schema) in [(FrameKind::Legacy, 3), (FrameKind::Schema3, 2)] {
        let raw=serde_json::to_vec(&serde_json::json!({"version":4,"schema_version":schema,"request_id":"mismatch","bearer":"schema3-inert-fixture-bearer-000001","grants":{"capabilities":[]},"command":{"command":"negotiate","data":{"supported_versions":[4],"requested":{"capabilities":[]}}}})).unwrap();
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        framing::write_frame(&mut stream, kind, &raw, 1024 * 1024).unwrap();
        let (actual, body) = framing::read_frame(&mut stream, 1024 * 1024).unwrap();
        assert_eq!(actual, kind);
        let response: V4ResponseEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(response.schema_version, kind.schema());
        assert!(matches!(response.payload, V4ResponsePayload::Error { .. }));
    }
    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    framing::write_frame(&mut stream, FrameKind::Schema3, b"{", 1024 * 1024).unwrap();
    let (kind, body) = framing::read_frame(&mut stream, 1024 * 1024).unwrap();
    assert_eq!(kind, FrameKind::Schema3);
    assert_eq!(
        serde_json::from_slice::<V4ResponseEnvelope>(&body)
            .unwrap()
            .schema_version,
        3
    );
    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    use std::io::Write as _;
    stream.write_all(b"ABS3\0\x01\0\x01").unwrap();
    let (kind, body) = framing::read_frame(&mut stream, 1024 * 1024).unwrap();
    assert_eq!(kind, FrameKind::Schema3);
    assert!(
        matches!(serde_json::from_slice::<V4ResponseEnvelope>(&body).unwrap().payload,V4ResponsePayload::Error{error} if error.code==V4ErrorCode::LimitExceeded)
    );
    stop.request();
    server.join().unwrap().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_schema2_selection_reaches_an_old_only_peer_and_current_selection_never_retries() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    for compatibility in [true, false] {
        let root = std::path::Path::new("/tmp")
            .join(format!("abbey-schema2-only-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.join("daemon.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut prefix = [0; 4];
            stream.read_exact(&mut prefix).unwrap();
            let bytes = if compatibility {
                let length = u32::from_be_bytes(prefix) as usize;
                assert!((1..=1024 * 1024).contains(&length));
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                let request: V4RequestEnvelope = serde_json::from_slice(&body).unwrap();
                assert_eq!(request.schema_version, 2);
                let mut response = V4ResponseEnvelope::ok(
                    request.request_id,
                    V4Event::Negotiated(crate::app_core::V4GrantNegotiation {
                        protocol_version: 4,
                        schema_version: 2,
                        granted: V4CapabilitySet::from_sorted(vec![
                            V4Capability::ReadConversations,
                        ])
                        .unwrap(),
                    }),
                );
                response.schema_version = 2;
                serde_json::to_vec(&response).unwrap()
            } else {
                assert_eq!(prefix, *b"ABS3");
                serde_json::to_vec(&crate::daemon::ResponseEnvelope::error(
                    "",
                    "frame_too_large",
                    "frame exceeds configured limit",
                ))
                .unwrap()
            };
            stream
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&bytes).unwrap();
            drop(stream);
            listener.set_nonblocking(true).unwrap();
            std::thread::sleep(Duration::from_millis(80));
            assert!(
                matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock),
                "selection never falls back or reconnects"
            );
        });
        let client = DaemonClient::new(DaemonConfig::local(
            &socket,
            BearerSecret::parse("schema2-only-inert-bearer-0000001").unwrap(),
        ));
        let grants = V4CapabilitySet::from_sorted(vec![V4Capability::ReadConversations]).unwrap();
        if compatibility {
            assert_eq!(
                client
                    .negotiate_v4_schema2(grants.clone())
                    .unwrap()
                    .grants(),
                &grants
            );
        } else {
            assert!(client.negotiate_v4(grants).is_err());
        }
        peer.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn negotiated_grants_must_be_a_subset_for_each_explicit_schema() {
    use std::os::unix::net::UnixListener;
    for schema in [2, 3] {
        for mode in 0..3 {
            let requested = V4CapabilitySet::from_sorted(if mode == 1 {
                vec![
                    V4Capability::ReadConversations,
                    V4Capability::WriteConversations,
                ]
            } else {
                vec![V4Capability::ReadConversations]
            })
            .unwrap();
            let returned = V4CapabilitySet::from_sorted(if mode == 2 {
                vec![
                    V4Capability::ReadConversations,
                    V4Capability::WriteConversations,
                ]
            } else {
                vec![V4Capability::ReadConversations]
            })
            .unwrap();
            let root = std::path::Path::new("/tmp")
                .join(format!("abbey-grant-subset-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            let socket = root.join("daemon.sock");
            let listener = UnixListener::bind(&socket).unwrap();
            let response_grants = returned.clone();
            let request_grants = requested.clone();
            let peer = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let (kind, body) = framing::read_frame(&mut stream, 1024 * 1024).unwrap();
                assert_eq!(kind.schema(), schema);
                let request: V4RequestEnvelope = serde_json::from_slice(&body).unwrap();
                assert!(
                    matches!(&request.command,V4Command::Negotiate(r) if r.requested==request_grants)
                );
                let mut response = V4ResponseEnvelope::ok(
                    request.request_id,
                    V4Event::Negotiated(crate::app_core::V4GrantNegotiation {
                        protocol_version: 4,
                        schema_version: schema,
                        granted: response_grants,
                    }),
                );
                response.schema_version = schema;
                framing::write_frame(
                    &mut stream,
                    kind,
                    &serde_json::to_vec(&response).unwrap(),
                    1024 * 1024,
                )
                .unwrap();
                drop(stream);
                listener.set_nonblocking(true).unwrap();
                std::thread::sleep(Duration::from_millis(80));
                assert!(
                    matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock)
                );
            });
            let client = DaemonClient::new(DaemonConfig::local(
                &socket,
                BearerSecret::parse("grant-subset-inert-bearer-00000001").unwrap(),
            ));
            let result = if schema == 2 {
                client.negotiate_v4_schema2(requested)
            } else {
                client.negotiate_v4(requested)
            };
            if mode == 2 {
                assert!(matches!(result, Err(V4ClientError::InvalidResponse)));
            } else {
                assert_eq!(result.unwrap().grants(), &returned);
            }
            peer.join().unwrap();
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
