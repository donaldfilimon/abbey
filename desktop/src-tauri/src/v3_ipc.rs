//! Serde-identical protocol-v3 memory/grant/claim types for desktop codegen.
#![allow(dead_code)] // projected by codegen; runtime commands use `abbey::app_core` types

//!
//! Runtime Tauri commands still take `abbey::app_core` types. This module
//! exists so `desktop/codegen` can project the wire without ingesting the full
//! `V3Command` enum (which pulls tool/model types this client does not invoke).
//! Keep field names, tags, and `V3Capability` declaration order in lockstep
//! with `src/app_core/v3.rs`.

use abbey::app_core::ClaimStatus;
use serde::{Deserialize, Serialize};

/// Individually negotiated protocol-v3 authority. Declaration order is the
/// canonical serialized order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3Capability {
    ListTools,
    InvokeTools,
    DecideToolApprovals,
    CancelTools,
    ReadMemory,
    ReadModels,
    DownloadModels,
    ManageModels,
    ReadTraining,
    ManageTraining,
    ReadWorkers,
    CancelJobs,
    ReadClaimsById,
    PollEvents,
    InferModels,
}

/// Ordered grant set. Wire shape matches `abbey::app_core::V3CapabilitySet`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3CapabilitySet {
    capabilities: Vec<V3Capability>,
}

/// Fixed-watermark page request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct V3PageQuery {
    pub after: u64,
    pub through: Option<u64>,
    pub limit: u16,
}

impl Default for V3PageQuery {
    fn default() -> Self {
        Self {
            after: 0,
            through: None,
            limit: 32,
        }
    }
}

/// Read-only query for one opaque resource identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3ResourceQuery {
    pub resource_id: String,
}

/// Bounded search within one explicitly selected memory space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3SearchRequest {
    pub space_id: String,
    pub query: String,
    pub page: V3PageQuery,
}

/// Sanitized state for a bounded operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum V3OperationState {
    Available,
    Queued,
    Running,
    InputRequired,
    Succeeded,
    Failed,
    Denied,
    Cancelled,
    NotDownloaded,
}

/// Sanitized record used by inventory and metadata pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3EntityRecord {
    pub id: String,
    pub label: String,
    pub state: V3OperationState,
}

/// Fixed-watermark inventory page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3EntityPage {
    pub after: u64,
    pub through: u64,
    pub records: Vec<V3EntityRecord>,
}

/// Exact-identifier claim lookup result.
///
/// Distinct from the protocol-v1 `ClaimsSnapshot`/`ClaimsQuery` pair that the
/// Claims view already renders: this is a single canonical record fetched by
/// stable ID through `ReadClaimsById`, not a filtered ledger page. `status`
/// reuses the `ClaimStatus` already projected from `src/app_core/contracts.rs`
/// rather than redeclaring it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3StableClaim {
    pub id: String,
    pub name: String,
    pub status: ClaimStatus,
    pub note: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_wire_matches_app_core_serde() {
        let core_search = abbey::app_core::V3SearchRequest {
            space_id: "memory-v1-summary".to_owned(),
            query: "fixture".to_owned(),
            page: abbey::app_core::V3PageQuery {
                after: 0,
                through: None,
                limit: 32,
            },
        };
        let json = serde_json::to_string(&core_search).expect("serialize core search");
        let round: V3SearchRequest = serde_json::from_str(&json).expect("desktop search");
        assert_eq!(round.space_id, "memory-v1-summary");
        assert_eq!(round.query, "fixture");
        assert_eq!(round.page.limit, 32);

        let core_grants = abbey::app_core::V3CapabilitySet::from_sorted(vec![
            abbey::app_core::V3Capability::ReadMemory,
        ])
        .expect("grant set");
        let grants_json = serde_json::to_string(&core_grants).expect("serialize grants");
        let desktop_grants: V3CapabilitySet =
            serde_json::from_str(&grants_json).expect("desktop grants");
        let back: abbey::app_core::V3CapabilitySet =
            serde_json::from_str(&serde_json::to_string(&desktop_grants).expect("re-ser"))
                .expect("core grants");
        assert_eq!(core_grants, back);
    }

    #[test]
    fn page_query_wire_matches_app_core_serde() {
        let core_page = abbey::app_core::V3PageQuery {
            after: 4,
            through: Some(9),
            limit: 16,
        };
        let json = serde_json::to_string(&core_page).expect("serialize core page");
        let mirror: V3PageQuery = serde_json::from_str(&json).expect("desktop page");
        assert_eq!(mirror.after, 4);
        assert_eq!(mirror.through, Some(9));
        assert_eq!(mirror.limit, 16);

        let back: abbey::app_core::V3PageQuery =
            serde_json::from_str(&serde_json::to_string(&mirror).expect("re-ser page"))
                .expect("core page");
        assert_eq!(core_page, back);
    }

    #[test]
    fn resource_query_wire_matches_app_core_serde() {
        let core_query = abbey::app_core::V3ResourceQuery {
            resource_id: "resource-fixture".to_owned(),
        };
        let json = serde_json::to_string(&core_query).expect("serialize core query");
        let mirror: V3ResourceQuery = serde_json::from_str(&json).expect("desktop query");
        assert_eq!(mirror.resource_id, "resource-fixture");

        let back: abbey::app_core::V3ResourceQuery =
            serde_json::from_str(&serde_json::to_string(&mirror).expect("re-ser query"))
                .expect("core query");
        assert_eq!(core_query, back);
    }

    #[test]
    fn operation_state_wire_matches_app_core_serde() {
        let variants = [
            abbey::app_core::V3OperationState::Available,
            abbey::app_core::V3OperationState::Queued,
            abbey::app_core::V3OperationState::Running,
            abbey::app_core::V3OperationState::InputRequired,
            abbey::app_core::V3OperationState::Succeeded,
            abbey::app_core::V3OperationState::Failed,
            abbey::app_core::V3OperationState::Denied,
            abbey::app_core::V3OperationState::Cancelled,
            abbey::app_core::V3OperationState::NotDownloaded,
        ];
        for core_state in variants {
            let json = serde_json::to_string(&core_state).expect("serialize core state");
            let mirror: V3OperationState = serde_json::from_str(&json).expect("desktop state");
            let back: abbey::app_core::V3OperationState =
                serde_json::from_str(&serde_json::to_string(&mirror).expect("re-ser state"))
                    .expect("core state");
            assert_eq!(core_state, back);
        }
    }

    #[test]
    fn entity_record_wire_matches_app_core_serde() {
        let core_record = abbey::app_core::V3EntityRecord {
            id: "entity-fixture".to_owned(),
            label: "Entity Fixture".to_owned(),
            state: abbey::app_core::V3OperationState::Running,
        };
        let json = serde_json::to_string(&core_record).expect("serialize core record");
        let mirror: V3EntityRecord = serde_json::from_str(&json).expect("desktop record");
        assert_eq!(mirror.id, "entity-fixture");
        assert_eq!(mirror.label, "Entity Fixture");
        assert_eq!(mirror.state, V3OperationState::Running);

        let back: abbey::app_core::V3EntityRecord =
            serde_json::from_str(&serde_json::to_string(&mirror).expect("re-ser record"))
                .expect("core record");
        assert_eq!(core_record, back);
    }

    #[test]
    fn entity_page_wire_matches_app_core_serde() {
        let core_page = abbey::app_core::V3EntityPage {
            after: 0,
            through: 2,
            records: vec![
                abbey::app_core::V3EntityRecord {
                    id: "entity-1".to_owned(),
                    label: "Entity One".to_owned(),
                    state: abbey::app_core::V3OperationState::Available,
                },
                abbey::app_core::V3EntityRecord {
                    id: "entity-2".to_owned(),
                    label: "Entity Two".to_owned(),
                    state: abbey::app_core::V3OperationState::NotDownloaded,
                },
            ],
        };
        let json = serde_json::to_string(&core_page).expect("serialize core page");
        let mirror: V3EntityPage = serde_json::from_str(&json).expect("desktop page");
        assert_eq!(mirror.after, 0);
        assert_eq!(mirror.through, 2);
        assert_eq!(mirror.records.len(), 2);
        assert_eq!(mirror.records[0].id, "entity-1");
        assert_eq!(mirror.records[1].state, V3OperationState::NotDownloaded);

        let back: abbey::app_core::V3EntityPage =
            serde_json::from_str(&serde_json::to_string(&mirror).expect("re-ser page"))
                .expect("core page");
        assert_eq!(core_page, back);
    }

    #[test]
    fn stable_claim_wire_matches_app_core_serde() {
        // `status` is `ClaimStatus`, imported directly via
        // `use abbey::app_core::ClaimStatus;` above rather than redeclared
        // in this mirror, so no drift is possible on that field
        // specifically — this round-trip still pins `id`/`name`/`note`.
        let core_claim = abbey::app_core::V3StableClaim {
            id: "claim-fixture".to_owned(),
            name: "Claim Fixture".to_owned(),
            status: ClaimStatus::Current,
            note: "fixture note".to_owned(),
        };
        let json = serde_json::to_string(&core_claim).expect("serialize core claim");
        let mirror: V3StableClaim = serde_json::from_str(&json).expect("desktop claim");
        assert_eq!(mirror.id, "claim-fixture");
        assert_eq!(mirror.name, "Claim Fixture");
        assert_eq!(mirror.status, ClaimStatus::Current);
        assert_eq!(mirror.note, "fixture note");

        let back: abbey::app_core::V3StableClaim =
            serde_json::from_str(&serde_json::to_string(&mirror).expect("re-ser claim"))
                .expect("core claim");
        assert_eq!(core_claim, back);
    }
}
