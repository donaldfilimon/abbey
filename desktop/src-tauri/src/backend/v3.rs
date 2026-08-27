//! Protocol-v3 desktop reads. Separate from v1/v2 `AppCommand` routing.
//!
//! The desktop requests **exactly** [`V3Capability::ReadMemory`]. Extra grants
//! are never asked for. In-process `AppService` is not a memory backend: no
//! bearer means deny-all grants and rejected search/metadata, never a silent
//! SQLite open.

use abbey::app_core::{
    V3Capability, V3CapabilitySet, V3EntityPage, V3EntityRecord, V3ResourceQuery, V3SearchRequest,
};
use abbey::daemon::V3DaemonSession;

use super::{Route, bearer_source, from_client_error, route};
use crate::ipc::{IpcError, IpcErrorKind};

fn v3_memory_session() -> Result<V3DaemonSession, IpcError> {
    if bearer_source().is_none() {
        return Err(IpcError::new(
            IpcErrorKind::Rejected,
            "protocol-v3 memory reads require a configured abbeyd",
        ));
    }
    match route()? {
        Route::InProcess(_) => Err(IpcError::new(
            IpcErrorKind::Rejected,
            "protocol-v3 memory reads are daemon-only and never open the in-process store",
        )),
        Route::Daemon(client) => {
            let requested = V3CapabilitySet::from_sorted(vec![V3Capability::ReadMemory])
                .expect("single ReadMemory grant is canonical");
            client.negotiate_v3(requested).map_err(from_client_error)
        }
    }
}

/// Negotiated v3 grants for this process. No bearer → empty set, not an error.
pub fn v3_grants() -> Result<V3CapabilitySet, IpcError> {
    if bearer_source().is_none() {
        return Ok(V3CapabilitySet::deny_all());
    }
    Ok(v3_memory_session()?.negotiation().granted.clone())
}

pub fn memory_search(request: V3SearchRequest) -> Result<V3EntityPage, IpcError> {
    v3_memory_session()?
        .search_memory(request)
        .map_err(from_client_error)
}

pub fn memory_metadata(query: V3ResourceQuery) -> Result<V3EntityRecord, IpcError> {
    v3_memory_session()?
        .read_memory_metadata(query)
        .map_err(from_client_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::IpcErrorKind;

    const MEMORY_SPACE: &str = "memory-v1-summary";

    #[test]
    fn in_process_v3_grants_are_empty_and_memory_reads_reject() {
        if crate::backend::bearer_source().is_some() {
            return;
        }
        let grants = v3_grants().expect("no-bearer grants");
        assert!(grants.as_slice().is_empty());
        let error = memory_search(V3SearchRequest {
            space_id: MEMORY_SPACE.to_owned(),
            query: "desktop-memory-proof".to_owned(),
            page: abbey::app_core::V3PageQuery::default(),
        })
        .expect_err("in-process memory search is not permitted");
        assert_eq!(error.kind, IpcErrorKind::Rejected);
        let meta_error = memory_metadata(V3ResourceQuery {
            resource_id: "memory-00000000000000000000000000000000".to_owned(),
        })
        .expect_err("in-process memory metadata is not permitted");
        assert_eq!(meta_error.kind, IpcErrorKind::Rejected);
    }

    /// Live proof. Vacuous unless `ABBEY_DESKTOP_LIVE_DAEMON=1` is exported by
    /// `desktop/scripts/prove-daemon-read.sh` after seeding memory.
    #[cfg(unix)]
    #[test]
    fn live_daemon_desktop_memory_reads() {
        if std::env::var_os("ABBEY_DESKTOP_LIVE_DAEMON").is_none() {
            return;
        }
        let grants = v3_grants().expect("v3 grants through live abbeyd");
        assert!(
            grants.contains(V3Capability::ReadMemory),
            "expected ReadMemory only, granted {grants:?}"
        );
        assert_eq!(grants.as_slice(), &[V3Capability::ReadMemory]);

        let page = memory_search(V3SearchRequest {
            space_id: MEMORY_SPACE.to_owned(),
            query: "desktop-memory-proof".to_owned(),
            page: abbey::app_core::V3PageQuery::default(),
        })
        .expect("desktop memory search through live abbeyd");
        assert!(
            !page.records.is_empty(),
            "seeded summary was not searchable"
        );
        let record = &page.records[0];
        assert!(record.id.starts_with("memory-"), "{}", record.id);
        assert!(
            record.label.contains("desktop-memory-proof"),
            "{}",
            record.label
        );

        let metadata = memory_metadata(V3ResourceQuery {
            resource_id: record.id.clone(),
        })
        .expect("desktop memory metadata through live abbeyd");
        assert_eq!(metadata, *record);

        let rendered = serde_json::to_string(&(page, metadata)).expect("serialize memory page");
        for private in [
            "RAW_PAYLOAD_CANARY",
            "RAW_PROVENANCE_CANARY",
            "/private/source/canary",
        ] {
            assert!(
                !rendered.contains(private),
                "desktop memory IPC leaked {private}: {rendered}"
            );
        }
    }
}
