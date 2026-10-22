//! Typed protocol-v3 canonical claim reads over the shared bounded daemon
//! transport.
//!
//! The claims domain of the negotiated protocol-v3 session lives here so the
//! session surface stays split by domain. Every method below is the exact
//! body it had in the parent module; only the file it lives in changed.

use crate::app_core::{V3Capability, V3Command, V3Event, V3ResourceQuery, V3StableClaim};

use super::super::{ClientError, V3DaemonSession};

impl V3DaemonSession {
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
