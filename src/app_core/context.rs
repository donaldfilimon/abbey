//! Immutable context shared by presentation adapters.

use super::{
    APP_PROTOCOL_V1, APP_PROTOCOL_VERSION, APP_SCHEMA_V1, APP_SCHEMA_VERSION, CapabilitySet,
    Edition, RunRouteCapability, RuntimeState, RuntimeStatus, ValidationError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppContext {
    status: RuntimeStatus,
}

impl AppContext {
    /// Construct the context for the compiled edition.
    ///
    /// The capability set is deliberately edition-independent: the personal
    /// edition is a separate identity, not a larger permission set.
    #[must_use]
    pub fn active() -> Self {
        Self {
            status: RuntimeStatus {
                protocol_version: APP_PROTOCOL_V1,
                schema_version: APP_SCHEMA_V1,
                edition: match crate::edition::ACTIVE {
                    crate::edition::Edition::Safe => Edition::Standard,
                    crate::edition::Edition::Personal => Edition::Personal,
                },
                state: RuntimeState::Ready,
                version: crate::build_info::VERSION.to_owned(),
                build_git: crate::build_info::BUILD_GIT.to_owned(),
                build_target: crate::build_info::BUILD_TARGET.to_owned(),
                capabilities: CapabilitySet::standard(),
                run_routes: Vec::new(),
            },
        }
    }

    /// Construct the current runtime-control context from startup-bound routes.
    pub fn runtime_v2(routes: Vec<RunRouteCapability>) -> Result<Self, ValidationError> {
        let has_routes = !routes.is_empty();
        let context = Self {
            status: RuntimeStatus {
                protocol_version: APP_PROTOCOL_VERSION,
                schema_version: APP_SCHEMA_VERSION,
                edition: match crate::edition::ACTIVE {
                    crate::edition::Edition::Safe => Edition::Standard,
                    crate::edition::Edition::Personal => Edition::Personal,
                },
                state: RuntimeState::Ready,
                version: crate::build_info::VERSION.to_owned(),
                build_git: crate::build_info::BUILD_GIT.to_owned(),
                build_target: crate::build_info::BUILD_TARGET.to_owned(),
                capabilities: CapabilitySet::runtime_v2_for_routes(has_routes),
                run_routes: routes,
            },
        };
        context.status.validate()?;
        Ok(context)
    }

    #[must_use]
    pub fn status(&self) -> &RuntimeStatus {
        &self.status
    }
}

impl Default for AppContext {
    fn default() -> Self {
        Self::active()
    }
}
