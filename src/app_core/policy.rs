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
    use crate::app_core::{BackendSelection, ClaimsQuery, IdempotencyKey, RunMode, RunRequest};

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
}
