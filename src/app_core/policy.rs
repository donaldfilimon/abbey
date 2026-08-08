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
        };
        capabilities.contains(required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_core::ClaimsQuery;

    #[test]
    fn policy_only_recognizes_declared_read_operations() {
        let policy = StandardPolicy;
        let capabilities = CapabilitySet::standard();
        assert!(policy.permits(&AppCommand::Status, &capabilities));
        assert!(policy.permits(&AppCommand::Claims(ClaimsQuery::default()), &capabilities));
    }
}
