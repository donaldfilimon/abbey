//! Typed protocol-v3 session reads over the shared bounded daemon transport.

use crate::app_core::{
    V3Capability, V3Command, V3EntityPage, V3Event, V3PageQuery, V3ResourceQuery, V3StableClaim,
    V3ToolPage,
};

use super::{ClientError, V3DaemonSession};

impl V3DaemonSession {
    /// Return the validated negotiation result for presentation or inspection.
    #[must_use]
    pub const fn negotiation(&self) -> &crate::app_core::V3GrantNegotiation {
        &self.negotiation
    }

    /// Read one bounded fixed-watermark page from the canonical safe registry.
    ///
    /// Descriptors carry no invocation authority. The request echoes the
    /// private negotiated grant set and is sent exactly once without retry or
    /// downgrade.
    #[cfg(unix)]
    pub fn list_tools(&self, query: V3PageQuery) -> Result<V3ToolPage, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::ListTools) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ListTools,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ListTools(query),
        )?;
        let V3Event::Tools(page) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "tool inventory",
                received: super::v3_event_name(&event),
            });
        };
        page.validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        let expected = usize::try_from(
            page.through
                .saturating_sub(page.after)
                .min(u64::from(query.limit)),
        )
        .map_err(|_| ClientError::InvalidV3Response)?;
        if page.after != query.after
            || query.through.is_some_and(|through| page.through != through)
            || page.tools.len() != expected
        {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(page)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn list_tools(&self, _query: V3PageQuery) -> Result<V3ToolPage, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Read one bounded fixed-watermark model inventory page.
    ///
    /// The request echoes the exact negotiated grant set and is sent once. It
    /// is never retried or replayed, even though this first command is a read,
    /// so future v3 mutations cannot accidentally inherit retry behavior.
    #[cfg(unix)]
    pub fn list_models(&self, query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self.negotiation.granted.contains(V3Capability::ReadModels) {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ReadModels,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ListModels(query),
        )?;
        let V3Event::Models(page) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "model inventory",
                received: super::v3_event_name(&event),
            });
        };
        page.validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        if page.after != query.after
            || query.through.is_some_and(|through| page.through != through)
            || page.records.len() > usize::from(query.limit)
        {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(page)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn list_models(&self, _query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Read one canonical Abbey claim by its exact stable identifier.
    ///
    /// This echoes the private negotiated grant set and sends exactly one v3
    /// request. It never performs fuzzy matching, downgrade, retry, or replay.
    #[cfg(unix)]
    pub fn claim_by_id(&self, query: V3ResourceQuery) -> Result<V3StableClaim, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !self
            .negotiation
            .granted
            .contains(V3Capability::ReadClaimsById)
        {
            return Err(ClientError::V3CapabilityNotGranted {
                capability: V3Capability::ReadClaimsById,
            });
        }
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ClaimById(query.clone()),
        )?;
        let V3Event::Claim(claim) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "claim",
                received: super::v3_event_name(&event),
            });
        };
        claim
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        if claim.id != query.resource_id {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(claim)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn claim_by_id(&self, _query: V3ResourceQuery) -> Result<V3StableClaim, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }
}
