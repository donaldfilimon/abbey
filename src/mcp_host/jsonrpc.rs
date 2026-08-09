//! Minimal, strict JSON-RPC 2.0 framing types for the stdio MCP server.
//!
//! Deliberately hand-rolled rather than derived: a malformed frame must produce
//! a well-formed JSON-RPC error object, so parsing has to distinguish "not
//! JSON", "not a JSON-RPC request", and "request whose id we can still echo".

use serde_json::{Value, json};

use super::limits::MAX_JSON_DEPTH;

/// Standard JSON-RPC 2.0 error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// Implementation-defined codes, inside the reserved `-32000..=-32099` range.
pub const REQUEST_TOO_LARGE: i64 = -32001;
pub const REQUEST_TIMEOUT: i64 = -32002;

/// A parsed JSON-RPC message. `id == None` means a notification, which by
/// specification must never receive a response.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

/// Why a frame could not be turned into a [`Message`].
#[derive(Debug, Clone, PartialEq)]
pub struct ParseFailure {
    pub code: i64,
    pub message: String,
    /// Echoed back when the frame was structured enough to carry a usable id.
    pub id: Option<Value>,
}

impl ParseFailure {
    fn new(code: i64, message: impl Into<String>, id: Option<Value>) -> Self {
        Self {
            code,
            message: message.into(),
            id,
        }
    }
}

/// True when `value` is a legal JSON-RPC id (string, number, or explicit null).
fn is_valid_id(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Number(_) | Value::Null)
}

/// Parse one already-decoded JSON document into a JSON-RPC message.
///
/// Batches are rejected here rather than in the transport so the rule lives
/// next to the framing types it constrains.
pub fn parse_message(document: &Value) -> Result<Message, ParseFailure> {
    if document.is_array() {
        return Err(ParseFailure::new(
            INVALID_REQUEST,
            format!(
                "JSON-RPC batching is not accepted (max batch size {})",
                super::limits::MAX_BATCH_SIZE
            ),
            None,
        ));
    }
    let Some(object) = document.as_object() else {
        return Err(ParseFailure::new(
            INVALID_REQUEST,
            "request must be a JSON object",
            None,
        ));
    };

    // Extract the id first so later failures can still be correlated.
    //
    // An explicit `"id": null` is folded into the notification case (`None`)
    // rather than treated as a request with a null id. MCP forbids null request
    // ids, and a response carrying `"id": null` cannot be correlated by the
    // client anyway, so silence is the only useful answer.
    let id = match object.get("id") {
        None => None,
        Some(Value::Null) => None,
        Some(value) if is_valid_id(value) => Some(value.clone()),
        Some(_) => {
            return Err(ParseFailure::new(
                INVALID_REQUEST,
                "id must be a string, number, or null",
                None,
            ));
        }
    };

    match object.get("jsonrpc") {
        Some(Value::String(version)) if version == "2.0" => {}
        _ => {
            return Err(ParseFailure::new(
                INVALID_REQUEST,
                "missing or unsupported `jsonrpc` version (expected \"2.0\")",
                id,
            ));
        }
    }

    let Some(Value::String(method)) = object.get("method") else {
        return Err(ParseFailure::new(
            INVALID_REQUEST,
            "missing or non-string `method`",
            id,
        ));
    };

    let params = match object.get("params") {
        None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
        Some(value @ Value::Object(_)) => value.clone(),
        Some(_) => {
            return Err(ParseFailure::new(
                INVALID_PARAMS,
                "`params` must be an object when present",
                id,
            ));
        }
    };

    Ok(Message {
        id,
        method: method.clone(),
        params,
    })
}

/// Structural nesting depth of a JSON document; scalars are depth 1.
///
/// Iterative rather than recursive, because the whole point of measuring depth
/// is to survive input designed to exhaust the stack.
pub fn json_depth(value: &Value) -> usize {
    let mut deepest = 0usize;
    let mut stack = vec![(value, 1usize)];
    while let Some((node, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        // Once past the enforced ceiling the exact figure is irrelevant, and
        // walking further would be the very work the limit exists to avoid.
        if deepest > MAX_JSON_DEPTH {
            return deepest;
        }
        match node {
            Value::Array(items) => stack.extend(items.iter().map(|item| (item, depth + 1))),
            Value::Object(entries) => {
                stack.extend(entries.values().map(|entry| (entry, depth + 1)));
            }
            _ => {}
        }
    }
    deepest
}

/// Build a JSON-RPC success response.
pub fn success(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// Build a JSON-RPC error response. A `None` id serializes as JSON null, which
/// is what the specification requires when the id could not be determined.
pub fn error(id: Option<Value>, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {"code": code, "message": message.into()}
    })
}
