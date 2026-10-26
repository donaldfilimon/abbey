//! Schema-aware envelope decoders; serde cannot turn schema-2 admin data into commands.
use super::*;

use serde::Deserialize;
impl<'de> Deserialize<'de> for V4RequestEnvelope {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            version: u16,
            schema_version: u16,
            request_id: String,
            bearer: String,
            grants: V4CapabilitySet,
            command: V4Command,
        }
        let w =
            Wire::deserialize(d).map_err(|_| serde::de::Error::custom("invalid v4 envelope"))?;
        if w.version != APP_PROTOCOL_V4
            || route(&w.request_id).is_err()
            || w.bearer.len() > 4096
            || w.bearer.chars().any(char::is_control)
            || w.command.validate_for_schema(w.schema_version).is_err()
            || (w.schema_version == APP_SCHEMA_V4_CONVERSATIONS
                && w.grants.as_slice().iter().any(|c| !c.is_conversation()))
        {
            return Err(serde::de::Error::custom("invalid v4 envelope"));
        }
        Ok(Self {
            version: w.version,
            schema_version: w.schema_version,
            request_id: w.request_id,
            bearer: w.bearer,
            grants: w.grants,
            command: w.command,
        })
    }
}
impl<'de> Deserialize<'de> for V4ResponseEnvelope {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            version: u16,
            schema_version: u16,
            request_id: String,
            payload: V4ResponsePayload,
        }
        let w =
            Wire::deserialize(d).map_err(|_| serde::de::Error::custom("invalid v4 envelope"))?;
        if w.version != APP_PROTOCOL_V4
            || !matches!(
                w.schema_version,
                APP_SCHEMA_V4 | APP_SCHEMA_V4_CONVERSATIONS
            )
        {
            return Err(serde::de::Error::custom("invalid v4 envelope"));
        }
        if (!w.request_id.is_empty() && route(&w.request_id).is_err())
            || (w.request_id.is_empty() && matches!(&w.payload, V4ResponsePayload::Ok { .. }))
        {
            return Err(serde::de::Error::custom("invalid v4 envelope"));
        }
        match &w.payload {
            V4ResponsePayload::Ok { event } => event.validate_for_schema(w.schema_version),
            V4ResponsePayload::Error { error } => error.validate(),
        }
        .map_err(|_| serde::de::Error::custom("invalid v4 envelope"))?;
        Ok(Self {
            version: w.version,
            schema_version: w.schema_version,
            request_id: w.request_id,
            payload: w.payload,
        })
    }
}
