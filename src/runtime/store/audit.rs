//! Bounded audit DTOs and secret-aware metadata sanitization.

use super::{StoreError, parse_run_id, to_sql_error};
use crate::app_core::RunId;
use serde_json::{Map, Value};

const MAX_METADATA_BYTES: usize = 4 * 1024;
const MAX_STRING_BYTES: usize = 512;
const MAX_COLLECTION_ITEMS: usize = 32;
const MAX_DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct NewAuditEvent {
    pub run_id: Option<RunId>,
    pub action: String,
    pub outcome: String,
    pub metadata: AuditMetadata,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEvent {
    pub id: i64,
    pub run_id: Option<RunId>,
    pub action: String,
    pub outcome: String,
    pub metadata: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditMetadata(pub(super) Value);

impl AuditMetadata {
    pub fn new(mut value: Value) -> Result<Self, StoreError> {
        if !value.is_object() {
            return Err(StoreError::InvalidAuditMetadata(
                "metadata must be a JSON object",
            ));
        }
        sanitize_value(&mut value, 0)?;
        let encoded = serde_json::to_vec(&value)?;
        if encoded.len() > MAX_METADATA_BYTES {
            return Err(StoreError::InvalidAuditMetadata(
                "metadata exceeds 4096 bytes",
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.0
    }
}

pub(super) fn validate_audit_label(value: &str, error: &'static str) -> Result<(), StoreError> {
    if value.is_empty() || value.len() > 64 || value.chars().any(char::is_control) {
        return Err(StoreError::InvalidInput(error));
    }
    Ok(())
}

pub(super) fn row_to_audit(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditEvent> {
    let raw_run_id: Option<String> = row.get(1)?;
    let metadata_json: String = row.get(4)?;
    Ok(AuditEvent {
        id: row.get(0)?,
        run_id: raw_run_id
            .map(|value| parse_run_id(&value).map_err(to_sql_error))
            .transpose()?,
        action: row.get(2)?,
        outcome: row.get(3)?,
        metadata: serde_json::from_str(&metadata_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        created_at: row.get(5)?,
    })
}

fn sanitize_value(value: &mut Value, depth: usize) -> Result<(), StoreError> {
    if depth > MAX_DEPTH {
        return Err(StoreError::InvalidAuditMetadata(
            "metadata nesting exceeds four levels",
        ));
    }
    match value {
        Value::Object(object) => sanitize_object(object, depth),
        Value::Array(items) => {
            if items.len() > MAX_COLLECTION_ITEMS {
                return Err(StoreError::InvalidAuditMetadata(
                    "metadata array exceeds 32 items",
                ));
            }
            for item in items {
                sanitize_value(item, depth + 1)?;
            }
            Ok(())
        }
        Value::String(text) => {
            if text.len() > MAX_STRING_BYTES {
                return Err(StoreError::InvalidAuditMetadata(
                    "metadata string exceeds 512 bytes",
                ));
            }
            if looks_secret(text) {
                *text = "[REDACTED]".into();
            }
            Ok(())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
    }
}

fn sanitize_object(object: &mut Map<String, Value>, depth: usize) -> Result<(), StoreError> {
    if object.len() > MAX_COLLECTION_ITEMS {
        return Err(StoreError::InvalidAuditMetadata(
            "metadata object exceeds 32 fields",
        ));
    }
    for (key, value) in object {
        if key.is_empty() || key.len() > 64 || key.chars().any(char::is_control) {
            return Err(StoreError::InvalidAuditMetadata("metadata key is invalid"));
        }
        if is_sensitive_key(key) {
            *value = Value::String("[REDACTED]".into());
        } else {
            sanitize_value(value, depth + 1)?;
        }
    }
    Ok(())
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace(['-', ' '], "_");
    [
        "prompt",
        "credential",
        "password",
        "secret",
        "token",
        "authorization",
        "cookie",
        "api_key",
    ]
    .iter()
    .any(|marker| key.contains(marker))
}

fn looks_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("bearer ")
        || lower.starts_with("sk-")
        || lower.contains("-----begin private key-----")
}
