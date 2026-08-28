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
}
