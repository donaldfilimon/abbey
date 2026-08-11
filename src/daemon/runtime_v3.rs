//! Minimal daemon-owned authority for the first served protocol-v3 slice.
//!
//! Only startup-bound ABI-local model inventory and exact stable-ID reads from
//! Abbey's canonical claim registry are representable here. Tool, memory,
//! training, worker, cancellation, and polling grants remain absent.

use crate::app_core::{
    APP_PROTOCOL_V3, APP_SCHEMA_V3, BackendSelection, ClaimStatus, V3Capability, V3CapabilitySet,
    V3Command, V3EntityPage, V3EntityRecord, V3Event, V3GrantNegotiation, V3OperationState,
    V3StableClaim,
};
use crate::runtime::ProviderRoute;

use super::server::HandlerFailure;

/// Frozen protocol-v3 grants and presentation-safe model inventory.
pub(super) struct V3RuntimeAuthority {
    grants: V3CapabilitySet,
    models: Vec<V3EntityRecord>,
}

impl V3RuntimeAuthority {
    /// Derive v3 authority from the same startup-owned provider objects used
    /// by execution. Foundation Models is deliberately not granted yet.
    pub(super) fn from_provider_routes<'a>(
        routes: impl IntoIterator<Item = &'a ProviderRoute>,
    ) -> Result<Self, HandlerFailure> {
        let models = routes
            .into_iter()
            .filter(|provider| provider.backend() == BackendSelection::Abi)
            .map(|provider| V3EntityRecord {
                id: format!("abi-local:{}", provider.model()),
                label: format!("ABI local model {}", provider.model()),
                state: V3OperationState::Available,
            })
            .collect::<Vec<_>>();
        let mut available = Vec::with_capacity(2);
        if !models.is_empty() {
            available.push(V3Capability::ReadModels);
        }
        available.push(V3Capability::ReadClaimsById);
        let grants = V3CapabilitySet::from_sorted(available).map_err(|_| internal_failure())?;
        Ok(Self { grants, models })
    }

    /// Dispatch negotiation or the one granted read-only model command.
    pub(super) fn handle(&self, command: V3Command) -> Result<V3Event, HandlerFailure> {
        if let V3Command::Negotiate(request) = command {
            let granted = self
                .grants
                .as_slice()
                .iter()
                .copied()
                .filter(|grant| request.requested.contains(*grant))
                .collect();
            let granted = V3CapabilitySet::from_sorted(granted).map_err(|_| internal_failure())?;
            return Ok(V3Event::Negotiated(V3GrantNegotiation {
                protocol_version: APP_PROTOCOL_V3,
                schema_version: APP_SCHEMA_V3,
                granted,
            }));
        }
        if !self.grants.permits(&command) {
            return Err(capability_denied_failure());
        }
        match command {
            V3Command::ListModels(page) => {
                let snapshot = u64::try_from(self.models.len()).map_err(|_| internal_failure())?;
                let through = page.through.unwrap_or(snapshot);
                if through > snapshot || page.after > through {
                    return Err(invalid_command_failure());
                }
                let available = through.saturating_sub(page.after);
                let take = available.min(u64::from(page.limit));
                let skip = usize::try_from(page.after).map_err(|_| invalid_command_failure())?;
                let take = usize::try_from(take).map_err(|_| invalid_command_failure())?;
                Ok(V3Event::Models(V3EntityPage {
                    after: page.after,
                    through,
                    records: self.models.iter().skip(skip).take(take).cloned().collect(),
                }))
            }
            V3Command::ClaimById(query) => {
                let claim = crate::claims::CLAIMS
                    .iter()
                    .find(|claim| claim.id == query.resource_id)
                    .ok_or_else(not_found_failure)?;
                let claim = V3StableClaim {
                    id: claim.id.to_owned(),
                    name: claim.name.to_owned(),
                    status: claim_status(claim.status),
                    note: claim.note.to_owned(),
                };
                claim.validate().map_err(|_| internal_failure())?;
                Ok(V3Event::Claim(claim))
            }
            _ => Err(capability_denied_failure()),
        }
    }

    /// Validate every echoed grant against startup authority and the command.
    pub(super) fn authorizes(&self, grants: &V3CapabilitySet, command: &V3Command) -> bool {
        grants
            .as_slice()
            .iter()
            .all(|grant| self.grants.contains(*grant))
            && grants.permits(command)
    }
}

const fn invalid_command_failure() -> HandlerFailure {
    HandlerFailure::new("invalid_command", "command payload is invalid")
}

const fn capability_denied_failure() -> HandlerFailure {
    HandlerFailure::new(
        "capability_denied",
        "protocol-v3 capability was not granted",
    )
}

const fn not_found_failure() -> HandlerFailure {
    HandlerFailure::new("not_found", "claim id was not found")
}

const fn internal_failure() -> HandlerFailure {
    HandlerFailure::new("runtime_unavailable", "runtime operation is unavailable")
}

const fn claim_status(status: crate::claims::Status) -> ClaimStatus {
    match status {
        crate::claims::Status::Current => ClaimStatus::Current,
        crate::claims::Status::Partial => ClaimStatus::Partial,
        crate::claims::Status::Proposed => ClaimStatus::Proposed,
        crate::claims::Status::Blocked => ClaimStatus::Blocked,
        crate::claims::Status::OutOfScope => ClaimStatus::OutOfScope,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use abi_agent_runtime::{EchoProvider, RunBudget};

    use super::*;
    use crate::app_core::{V3GrantRequest, V3PageQuery, V3ResourceQuery};

    fn abi_route() -> ProviderRoute {
        ProviderRoute::new(
            BackendSelection::Abi,
            "local",
            Arc::new(EchoProvider::new()),
            RunBudget::unlimited()
                .with_max_events(8)
                .with_max_output_tokens(8)
                .with_max_duration(Duration::from_secs(1)),
        )
        .unwrap()
    }

    #[test]
    fn negotiates_only_startup_bound_abi_model_reads() {
        let route = abi_route();
        let authority = V3RuntimeAuthority::from_provider_routes([&route]).unwrap();
        let requested =
            V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels, V3Capability::PollEvents])
                .unwrap();
        let V3Event::Negotiated(negotiated) = authority
            .handle(V3Command::Negotiate(V3GrantRequest {
                supported_versions: vec![1, 2, 3],
                requested,
            }))
            .unwrap()
        else {
            panic!("expected v3 negotiation");
        };
        assert_eq!(negotiated.granted.as_slice(), &[V3Capability::ReadModels]);

        let V3Event::Models(models) = authority
            .handle(V3Command::ListModels(V3PageQuery::default()))
            .unwrap()
        else {
            panic!("expected model inventory");
        };
        assert_eq!(models.after, 0);
        assert_eq!(models.through, 1);
        assert_eq!(models.records.len(), 1);
        assert_eq!(models.records[0].id, "abi-local:local");
        assert_eq!(
            authority
                .handle(V3Command::ListModels(V3PageQuery {
                    after: 0,
                    through: Some(2),
                    limit: 1,
                }))
                .unwrap_err()
                .code(),
            "invalid_command"
        );
        assert_eq!(
            authority
                .handle(V3Command::PollEvents(V3PageQuery::default()))
                .unwrap_err()
                .code(),
            "capability_denied"
        );
    }

    #[test]
    fn no_abi_route_still_negotiates_canonical_claim_reads() {
        let authority = V3RuntimeAuthority::from_provider_routes([]).unwrap();
        let V3Event::Negotiated(negotiated) = authority
            .handle(V3Command::Negotiate(V3GrantRequest {
                supported_versions: vec![3],
                requested: V3CapabilitySet::from_sorted(vec![
                    V3Capability::ReadModels,
                    V3Capability::ReadClaimsById,
                ])
                .unwrap(),
            }))
            .unwrap()
        else {
            panic!("expected v3 negotiation");
        };
        assert_eq!(
            negotiated.granted.as_slice(),
            &[V3Capability::ReadClaimsById]
        );
        let V3Event::Claim(claim) = authority
            .handle(V3Command::ClaimById(V3ResourceQuery {
                resource_id: "backend-cursor-agent".into(),
            }))
            .unwrap()
        else {
            panic!("expected stable claim");
        };
        assert_eq!(claim.id, "backend-cursor-agent");
        assert_eq!(claim.status, ClaimStatus::Current);
        assert_eq!(
            authority
                .handle(V3Command::ListModels(V3PageQuery::default()))
                .unwrap_err()
                .code(),
            "capability_denied"
        );
        assert_eq!(
            authority
                .handle(V3Command::ClaimById(V3ResourceQuery {
                    resource_id: "backend-cursor".into(),
                }))
                .unwrap_err()
                .code(),
            "not_found"
        );
    }
}
