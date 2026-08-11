//! Typed-client correlation tests for sanitized protocol-v3 memory reads.

use super::*;
use crate::app_core::{V3EntityRecord, V3OperationState, V3ResourceQuery, V3SearchRequest};

const SPACE_ID: &str = "memory-v1-summary";
const RECORD_ID: &str = "memory-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn v3_memory_reads_echo_exact_grants_and_correlate_each_response() {
    let root = scratch_dir("v3-memory");
    let config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let thread = thread::spawn(move || {
        let (mut negotiation_stream, _) = listener.accept().unwrap();
        let negotiation_request = read_v3_request(&mut negotiation_stream);
        let V3Command::Negotiate(requested) = negotiation_request.command else {
            panic!("expected negotiation request");
        };
        assert_eq!(requested.requested.as_slice(), &[V3Capability::ReadMemory]);
        let granted = V3CapabilitySet::from_sorted(vec![V3Capability::ReadMemory]).unwrap();
        negotiation_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                negotiation_request.request_id,
                V3Event::Negotiated(V3GrantNegotiation {
                    protocol_version: crate::app_core::APP_PROTOCOL_V3,
                    schema_version: crate::app_core::APP_SCHEMA_V3,
                    granted: granted.clone(),
                }),
            )))
            .unwrap();

        let (mut spaces_stream, _) = listener.accept().unwrap();
        let spaces_request = read_v3_request(&mut spaces_stream);
        assert_eq!(spaces_request.grants, granted);
        let V3Command::ListMemorySpaces(page) = spaces_request.command else {
            panic!("expected memory-space request");
        };
        assert_eq!(page.after, 0);
        spaces_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                spaces_request.request_id,
                V3Event::MemorySpaces(V3EntityPage {
                    after: 0,
                    through: 1,
                    records: vec![V3EntityRecord {
                        id: SPACE_ID.to_owned(),
                        label: "Sanitized memory summaries".to_owned(),
                        state: V3OperationState::Available,
                    }],
                }),
            )))
            .unwrap();

        let (mut search_stream, _) = listener.accept().unwrap();
        let search_request = read_v3_request(&mut search_stream);
        assert_eq!(search_request.grants, granted);
        let V3Command::SearchMemory(request) = search_request.command else {
            panic!("expected memory-search request");
        };
        assert_eq!(request.space_id, SPACE_ID);
        assert_eq!(request.query, "needle");
        search_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                search_request.request_id,
                V3Event::MemorySearchResults(V3EntityPage {
                    after: 0,
                    through: 1_u64 << 63,
                    records: vec![record()],
                }),
            )))
            .unwrap();

        let (mut metadata_stream, _) = listener.accept().unwrap();
        let metadata_request = read_v3_request(&mut metadata_stream);
        assert_eq!(metadata_request.grants, granted);
        let V3Command::ReadMemoryMetadata(query) = metadata_request.command else {
            panic!("expected memory-metadata request");
        };
        assert_eq!(query.resource_id, RECORD_ID);
        metadata_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                metadata_request.request_id,
                V3Event::MemoryMetadata(record()),
            )))
            .unwrap();
    });

    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadMemory]).unwrap();
    let session = DaemonClient::new(config).negotiate_v3(requested).unwrap();
    assert_eq!(
        session
            .list_memory_spaces(V3PageQuery::default())
            .unwrap()
            .records
            .len(),
        1
    );
    let results = session
        .search_memory(V3SearchRequest {
            space_id: SPACE_ID.to_owned(),
            query: "needle".to_owned(),
            page: V3PageQuery::default(),
        })
        .unwrap();
    assert_eq!(results.records[0].id, RECORD_ID);
    assert_eq!(
        session
            .read_memory_metadata(V3ResourceQuery {
                resource_id: RECORD_ID.to_owned(),
            })
            .unwrap(),
        record()
    );
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v3_memory_read_stops_locally_when_the_grant_is_denied() {
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadMemory]).unwrap();
    let (config, thread, root) = fake_v3_server(|request| {
        encoded_v3_response(V3ResponseEnvelope::ok(
            request.request_id,
            V3Event::Negotiated(V3GrantNegotiation {
                protocol_version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3,
                granted: V3CapabilitySet::deny_all(),
            }),
        ))
    });
    let session = DaemonClient::new(config).negotiate_v3(requested).unwrap();
    assert!(matches!(
        session.list_memory_spaces(V3PageQuery::default()),
        Err(ClientError::V3CapabilityNotGranted {
            capability: V3Capability::ReadMemory
        })
    ));
    assert!(matches!(
        session.search_memory(V3SearchRequest {
            space_id: SPACE_ID.to_owned(),
            query: "needle".to_owned(),
            page: V3PageQuery::default(),
        }),
        Err(ClientError::V3CapabilityNotGranted {
            capability: V3Capability::ReadMemory
        })
    ));
    assert!(matches!(
        session.read_memory_metadata(V3ResourceQuery {
            resource_id: RECORD_ID.to_owned(),
        }),
        Err(ClientError::V3CapabilityNotGranted {
            capability: V3Capability::ReadMemory
        })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

fn record() -> V3EntityRecord {
    V3EntityRecord {
        id: RECORD_ID.to_owned(),
        label: "bounded summary".to_owned(),
        state: V3OperationState::Available,
    }
}
