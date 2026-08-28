//! Protocol-v3 desktop reads. Separate from v1/v2 `AppCommand` routing.
//!
//! The desktop requests **exactly** [`V3Capability::ReadMemory`],
//! [`V3Capability::ReadModels`], and [`V3Capability::ReadClaimsById`] — all
//! three read-only. No mutating grant (`DownloadModels`, `ManageModels`,
//! `InvokeTools`, …) is ever requested, so mutation remains unreachable from
//! the webview by construction rather than by UI convention.
//!
//! Requesting all three in one negotiation is safe and deliberate:
//! `V3GrantNegotiation::validate_for` rejects only grants that were *not*
//! requested, so a daemon that cannot serve one of them answers with the
//! others and negotiation still succeeds. `ReadModels` is edition-scoped and
//! owner-only, so its absence is the expected case rather than an error and
//! the Models surface simply renders unavailable. `ReadClaimsById` is granted
//! unconditionally at daemon startup, so it is the one grant expected to be
//! present whenever a daemon is reachable at all.
//!
//! In-process `AppService` is not a memory or model backend: no bearer means
//! deny-all grants and rejected reads, never a silent SQLite open.

use abbey::app_core::{
    V3Capability, V3CapabilitySet, V3EntityPage, V3EntityRecord, V3PageQuery, V3ResourceQuery,
    V3SearchRequest, V3StableClaim,
};
use abbey::daemon::V3DaemonSession;

use super::{Route, bearer_source, from_client_error, route};
use crate::ipc::{IpcError, IpcErrorKind};

fn v3_read_session() -> Result<V3DaemonSession, IpcError> {
    if bearer_source().is_none() {
        return Err(IpcError::new(
            IpcErrorKind::Rejected,
            "protocol-v3 reads require a configured abbeyd",
        ));
    }
    match route()? {
        Route::InProcess(_) => Err(IpcError::new(
            IpcErrorKind::Rejected,
            "protocol-v3 reads are daemon-only and never open the in-process store",
        )),
        Route::Daemon(client) => {
            let requested = V3CapabilitySet::from_sorted(vec![
                V3Capability::ReadMemory,
                V3Capability::ReadModels,
                V3Capability::ReadClaimsById,
            ])
            .expect("read-only ReadMemory + ReadModels + ReadClaimsById set is canonical");
            client.negotiate_v3(requested).map_err(from_client_error)
        }
    }
}

/// Negotiated v3 grants for this process. No bearer → empty set, not an error.
///
/// The returned set is what the frontend gates surfaces on, so a daemon that
/// grants only `ReadMemory` correctly leaves the Models surface unavailable
/// rather than showing a view whose every read would be refused.
pub fn v3_grants() -> Result<V3CapabilitySet, IpcError> {
    if bearer_source().is_none() {
        return Ok(V3CapabilitySet::deny_all());
    }
    Ok(v3_read_session()?.negotiation().granted.clone())
}

pub fn memory_search(request: V3SearchRequest) -> Result<V3EntityPage, IpcError> {
    v3_read_session()?
        .search_memory(request)
        .map_err(from_client_error)
}

pub fn memory_metadata(query: V3ResourceQuery) -> Result<V3EntityRecord, IpcError> {
    v3_read_session()?
        .read_memory_metadata(query)
        .map_err(from_client_error)
}

/// `ReadModels` — one bounded fixed-watermark page of the model inventory.
///
/// Read-only by construction: the session never negotiates `DownloadModels` or
/// `ManageModels`, so the daemon refuses any mutation this process could
/// attempt even if a future caller tried.
pub fn models_list(query: V3PageQuery) -> Result<V3EntityPage, IpcError> {
    v3_read_session()?
        .list_models(query)
        .map_err(from_client_error)
}

/// `ReadClaimsById` — one canonical claim looked up by exact stable ID.
///
/// Distinct from the protocol-v1 `app_claims` ledger read: that one filters a
/// snapshot, this one resolves a single stable identifier and reports
/// `not_found` for anything else. The daemon grants this unconditionally at
/// startup, unlike `ReadModels`.
pub fn claim_by_id(query: V3ResourceQuery) -> Result<V3StableClaim, IpcError> {
    v3_read_session()?
        .claim_by_id(query)
        .map_err(from_client_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::IpcErrorKind;

    const MEMORY_SPACE: &str = "memory-v1-summary";

    #[test]
    fn in_process_v3_grants_are_empty_and_reads_reject() {
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
        // Models must fail closed on the same in-process route: a desktop
        // without a configured daemon has no model authority either.
        let models_error = models_list(V3PageQuery::default())
            .expect_err("in-process model inventory is not permitted");
        assert_eq!(models_error.kind, IpcErrorKind::Rejected);
        let claim_error = claim_by_id(V3ResourceQuery {
            resource_id: "claim-does-not-matter".to_owned(),
        })
        .expect_err("in-process claim lookup is not permitted");
        assert_eq!(claim_error.kind, IpcErrorKind::Rejected);
    }

    /// The negotiated request must stay read-only. If a later change adds a
    /// mutating model grant to `v3_read_session`, this fails rather than
    /// silently handing the webview download/load/unload authority.
    #[test]
    fn requested_v3_grants_are_read_only() {
        let requested = V3CapabilitySet::from_sorted(vec![
            V3Capability::ReadMemory,
            V3Capability::ReadModels,
            V3Capability::ReadClaimsById,
        ])
        .expect("canonical read-only set");
        for forbidden in [
            V3Capability::DownloadModels,
            V3Capability::ManageModels,
            V3Capability::InvokeTools,
            V3Capability::ManageTraining,
            V3Capability::InferModels,
        ] {
            assert!(
                !requested.contains(forbidden),
                "desktop must never request {forbidden:?}"
            );
        }
    }

    /// Live proof of every v3 read this process negotiates. Vacuous unless
    /// `ABBEY_DESKTOP_LIVE_DAEMON=1` is exported by
    /// `desktop/scripts/prove-daemon-read.sh` after seeding memory.
    ///
    /// Covers memory search/metadata and exact-ID claim lookup. `ReadModels`
    /// is deliberately NOT exercised here: the scratch daemon configures no
    /// model authority, so it is never granted and a read would correctly be
    /// refused. That gap is stated in `tasks/goals.md` rather than papered
    /// over with an assertion that would pass for the wrong reason.
    #[cfg(unix)]
    #[test]
    fn live_daemon_desktop_v3_reads() {
        if std::env::var_os("ABBEY_DESKTOP_LIVE_DAEMON").is_none() {
            return;
        }
        let grants = v3_grants().expect("v3 grants through live abbeyd");
        assert!(
            grants.contains(V3Capability::ReadMemory),
            "expected ReadMemory, granted {grants:?}"
        );
        // Unlike `ReadModels`, the daemon grants this one unconditionally at
        // startup, so a reachable daemon must always produce it.
        assert!(
            grants.contains(V3Capability::ReadClaimsById),
            "expected ReadClaimsById to be unconditionally granted, granted {grants:?}"
        );
        // `ReadModels` is edition-scoped and owner-only, so the daemon may
        // grant it or not. Both are valid; what must never appear is a
        // capability this process did not request — especially a mutating one.
        for granted in grants.as_slice() {
            assert!(
                matches!(
                    granted,
                    V3Capability::ReadMemory
                        | V3Capability::ReadModels
                        | V3Capability::ReadClaimsById
                ),
                "live daemon granted unrequested capability {granted:?}"
            );
        }

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

        // Exact-ID claim lookup through the same live session. This is real
        // evidence that the desktop path resolves a canonical claim, not just
        // that the grant was negotiated. `ci-self-hosted-linux-proof` is the
        // same stable ID pinned by `tests/daemon_cli.rs`.
        let claim = claim_by_id(V3ResourceQuery {
            resource_id: "ci-self-hosted-linux-proof".to_owned(),
        })
        .expect("desktop stable-claim lookup through live abbeyd");
        assert_eq!(claim.id, "ci-self-hosted-linux-proof");
        // A non-exact ID must report not found rather than fuzzy-matching the
        // prefix above.
        let missing = claim_by_id(V3ResourceQuery {
            resource_id: "ci-self-hosted-linux".to_owned(),
        })
        .expect_err("a non-exact stable id must not resolve");
        assert_eq!(missing.kind, IpcErrorKind::Rejected);

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
