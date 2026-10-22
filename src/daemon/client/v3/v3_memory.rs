//! Typed protocol-v3 memory reads over the shared bounded daemon transport.
//!
//! The memory domain of the negotiated protocol-v3 session lives here so the
//! session surface stays split by domain. Every method below is the exact
//! body it had in the parent module; only the file it lives in changed.

use crate::app_core::{
    V3Capability, V3Command, V3EntityPage, V3Event, V3PageQuery, V3ResourceQuery, V3SearchRequest,
};

use super::super::{ClientError, V3DaemonSession};

impl V3DaemonSession {
    /// Read one bounded page of daemon-owned sanitized memory spaces.
    #[cfg(unix)]
    pub fn list_memory_spaces(&self, query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        self.require(V3Capability::ReadMemory)?;
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ListMemorySpaces(query),
        )?;
        let V3Event::MemorySpaces(page) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "memory spaces",
                received: super::v3_event_name(&event),
            });
        };
        validate_entity_page(&page, query)?;
        Ok(page)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn list_memory_spaces(&self, _query: V3PageQuery) -> Result<V3EntityPage, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Search only the explicitly selected sanitized memory-summary space.
    #[cfg(unix)]
    pub fn search_memory(&self, request: V3SearchRequest) -> Result<V3EntityPage, ClientError> {
        request
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        self.require(V3Capability::ReadMemory)?;
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::SearchMemory(request.clone()),
        )?;
        let V3Event::MemorySearchResults(page) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "memory search results",
                received: super::v3_event_name(&event),
            });
        };
        validate_entity_page(&page, request.page)?;
        Ok(page)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn search_memory(&self, _request: V3SearchRequest) -> Result<V3EntityPage, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Read sanitized metadata for one opaque memory-record identifier.
    #[cfg(unix)]
    pub fn read_memory_metadata(
        &self,
        query: V3ResourceQuery,
    ) -> Result<crate::app_core::V3EntityRecord, ClientError> {
        query
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        self.require(V3Capability::ReadMemory)?;
        let event = super::unix::request_v3(
            &self.client.config,
            self.negotiation.granted.clone(),
            V3Command::ReadMemoryMetadata(query.clone()),
        )?;
        let V3Event::MemoryMetadata(record) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "memory metadata",
                received: super::v3_event_name(&event),
            });
        };
        record
            .validate()
            .map_err(|_| ClientError::InvalidV3Response)?;
        if record.id != query.resource_id {
            return Err(ClientError::InvalidV3Response);
        }
        Ok(record)
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn read_memory_metadata(
        &self,
        _query: V3ResourceQuery,
    ) -> Result<crate::app_core::V3EntityRecord, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }
}

fn validate_entity_page(page: &V3EntityPage, query: V3PageQuery) -> Result<(), ClientError> {
    page.validate()
        .map_err(|_| ClientError::InvalidV3Response)?;
    if page.after != query.after
        || query.through.is_some_and(|through| page.through != through)
        || page.records.len() > usize::from(query.limit)
    {
        return Err(ClientError::InvalidV3Response);
    }
    Ok(())
}
