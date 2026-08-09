//! Standard-edition policy for the initial read-only application surface.

use super::{AppCapability, AppCommand, CapabilitySet};

#[derive(Debug, Clone, Copy, Default)]
pub struct StandardPolicy;

impl StandardPolicy {
    #[must_use]
    pub fn permits(self, command: &AppCommand, capabilities: &CapabilitySet) -> bool {
        let required = match command {
            AppCommand::Status => AppCapability::ReadStatus,
            AppCommand::Claims(_) => AppCapability::ReadClaims,
            AppCommand::ReadRoutes(_) => AppCapability::ReadRoutes,
            AppCommand::SubmitRun(_) => AppCapability::SubmitRun,
            AppCommand::GetRun(_) => AppCapability::ReadRun,
            AppCommand::CancelRun(_) => AppCapability::CancelRun,
            AppCommand::RunEvents(_) => AppCapability::ReadRunEvents,
        };
        capabilities.contains(required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_core::{
        BackendSelection, ClaimsQuery, IdempotencyKey, RouteAuditQuery, RunMode, RunRequest,
    };

    #[test]
    fn policy_only_recognizes_declared_read_operations() {
        let policy = StandardPolicy;
        let capabilities = CapabilitySet::standard();
        assert!(policy.permits(&AppCommand::Status, &capabilities));
        assert!(policy.permits(&AppCommand::Claims(ClaimsQuery::default()), &capabilities));

        let request = RunRequest {
            idempotency_key: "policy-request".parse::<IdempotencyKey>().unwrap(),
            conversation_id: None,
            mode: RunMode::Background,
            backend: BackendSelection::Abi,
            input: "bounded input".into(),
            labels: Vec::new(),
        };
        assert!(!policy.permits(&AppCommand::SubmitRun(request.clone()), &capabilities));
        assert!(policy.permits(
            &AppCommand::SubmitRun(request),
            &CapabilitySet::runtime_v2()
        ));
    }

    /// A capability set that does not grant `ReadRoutes` must deny the audit
    /// read. Built by deserialization because `CapabilitySet`'s field is
    /// private — which is also the shape an older daemon would send.
    #[test]
    fn a_capability_set_without_read_routes_denies_the_route_audit() {
        let policy = StandardPolicy;
        let command = AppCommand::ReadRoutes(RouteAuditQuery::default());

        let without = serde_json::from_value::<CapabilitySet>(serde_json::json!({
            "capabilities": ["read_status", "read_claims"]
        }))
        .expect("a legacy read-only capability set");
        without.validate().expect("legacy set is still well formed");
        assert!(!without.contains(AppCapability::ReadRoutes));
        assert!(
            !policy.permits(&command, &without),
            "the audit read must not be permitted without ReadRoutes"
        );

        // Both shipped sets grant it, so the command is reachable everywhere
        // the desktop and daemon actually run.
        for granted in [CapabilitySet::standard(), CapabilitySet::runtime_v2()] {
            assert!(policy.permits(&command, &granted));
        }
        // And granting only ReadRoutes does not smuggle in anything else.
        let only_routes = serde_json::from_value::<CapabilitySet>(serde_json::json!({
            "capabilities": ["read_routes"]
        }))
        .expect("a route-audit-only capability set");
        assert!(policy.permits(&command, &only_routes));
        assert!(!policy.permits(&AppCommand::Status, &only_routes));
        assert!(!policy.permits(&AppCommand::Claims(ClaimsQuery::default()), &only_routes));
    }
}
