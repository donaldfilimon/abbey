#[test]
fn protocol_v4_socket_keeps_its_envelope_for_denial_and_unsupported_authority() {
    let harness = Harness::start();
    let denied = v4_frame(&harness, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(denied.version, 4);
    match denied.payload {
        crate::daemon::V4ResponsePayload::Error { error } => {
            assert_eq!(error.code, crate::app_core::V4ErrorCode::Unauthorized);
        }
        other => panic!("expected a v4 authentication denial, got {other:?}"),
    }

    let unsupported = v4_frame(&harness, TEST_BEARER);
    assert_eq!(unsupported.version, 4);
    assert_eq!(unsupported.schema_version, crate::app_core::APP_SCHEMA_V4);
    match unsupported.payload {
        crate::daemon::V4ResponsePayload::Error { error } => {
            assert_eq!(error.code, crate::app_core::V4ErrorCode::UnsupportedVersion);
        }
        other => panic!("expected v4 to stay unsupported on this handler, got {other:?}"),
    }
    harness.stop();
}

fn v4_frame(harness: &Harness, bearer: &str) -> crate::daemon::V4ResponseEnvelope {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "version": 4,
        "schema_version": crate::app_core::APP_SCHEMA_V4,
        "request_id": "v4-socket",
        "bearer": bearer,
        "grants": {"capabilities": []},
        "command": {
            "command": "negotiate",
            "data": {
                "supported_versions": [4],
                "requested": {"capabilities": []}
            }
        }
    }))
    .unwrap();
    let mut stream = UnixStream::connect(&harness.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    stream
        .write_all(&(u32::try_from(bytes.len()).unwrap()).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
    let mut prefix = [0_u8; 4];
    stream.read_exact(&mut prefix).unwrap();
    let mut response = vec![0_u8; u32::from_be_bytes(prefix) as usize];
    stream.read_exact(&mut response).unwrap();
    assert!(
        serde_json::from_slice::<ResponseEnvelope>(&response).is_err(),
        "a v4 frame must not decode as a legacy envelope"
    );
    serde_json::from_slice(&response).unwrap()
}
