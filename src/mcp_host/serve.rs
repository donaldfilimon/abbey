//! Newline-delimited JSON-RPC transport over stdio.
//!
//! ## stdout is a protocol channel
//!
//! Once `abbey mcp serve` starts, **stdout carries JSON-RPC frames and nothing
//! else**. Every diagnostic goes to stderr. This module therefore shares no code
//! with the printing surfaces in `crate::protocols`, and EOF on stdin ends the
//! session with a clean exit rather than a hang.
//!
//! ## Bounded reads
//!
//! Frames are read through `Read::take(MAX_REQUEST_BYTES + 1)`, so a peer that
//! never sends a newline cannot drive an unbounded allocation. An oversized
//! frame is answered with an error *and* drained to the next newline, so the
//! tail of a giant line is never mistaken for the next request.

use std::io::{BufRead, Read, Write};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use super::jsonrpc::{
    self, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, Message, PARSE_ERROR,
    REQUEST_TIMEOUT, REQUEST_TOO_LARGE,
};
use super::limits::{
    MAX_JSON_DEPTH, MAX_PROTOCOL_VERSION_BYTES, MAX_REQUEST_BYTES, MAX_REQUEST_TIMEOUT,
    MAX_RESPONSE_BYTES, MAX_TOOL_NAME_BYTES,
};
use super::redact;
use super::tools::{self, SAFE_TOOLS, SafeTool};

/// MCP revision this server prefers when the client asks for something it does
/// not implement.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// Revisions this server will negotiate, newest first.
///
/// The lifecycle implemented here is the classic `initialize` handshake. The
/// later stateless-lifecycle revision is **not** claimed.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// One stdio MCP session.
pub struct Server {
    tools: &'static [SafeTool],
    call_timeout: Duration,
    initialized: bool,
    secrets: Vec<String>,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: SAFE_TOOLS,
            call_timeout: MAX_REQUEST_TIMEOUT,
            initialized: false,
            // Snapshot once: the environment cannot change mid-session, and a
            // per-response scan of `environ` would be wasted work.
            secrets: redact::sensitive_env_values(),
        }
    }

    /// Test-only constructor used to exercise the timeout machinery with a
    /// deliberately slow handler. The shipped registry is always [`SAFE_TOOLS`].
    #[cfg(test)]
    pub(super) fn with_tools(tools: &'static [SafeTool], call_timeout: Duration) -> Self {
        Self {
            tools,
            call_timeout,
            initialized: false,
            secrets: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn set_secrets(&mut self, secrets: Vec<String>) {
        self.secrets = secrets;
    }

    /// Read frames until EOF, writing one response line per request.
    pub fn serve(
        &mut self,
        mut reader: impl BufRead,
        mut writer: impl Write,
    ) -> std::io::Result<()> {
        loop {
            let mut frame = Vec::new();
            let read = (&mut reader)
                .take((MAX_REQUEST_BYTES + 1) as u64)
                .read_until(b'\n', &mut frame)?;
            if read == 0 {
                return Ok(());
            }
            if frame.len() > MAX_REQUEST_BYTES {
                // Drain only when the bounded read stopped mid-line. A frame of
                // exactly `MAX_REQUEST_BYTES + 1` bytes can already include its
                // terminating newline, and draining then would swallow the
                // *next* request.
                if frame.last() != Some(&b'\n') {
                    drain_line(&mut reader)?;
                }
                self.write_frame(
                    &mut writer,
                    &jsonrpc::error(
                        None,
                        REQUEST_TOO_LARGE,
                        format!("request frame exceeds {MAX_REQUEST_BYTES} bytes"),
                    ),
                )?;
                continue;
            }
            while frame
                .last()
                .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
            {
                frame.pop();
            }
            if frame.is_empty() {
                continue;
            }
            if let Some(response) = self.handle_frame(&frame) {
                self.write_frame(&mut writer, &response)?;
            }
        }
    }

    /// Handle one raw frame. `None` means "notification — say nothing".
    pub fn handle_frame(&mut self, frame: &[u8]) -> Option<Value> {
        let document: Value = match serde_json::from_slice(frame) {
            Ok(document) => document,
            Err(error) => {
                return Some(jsonrpc::error(
                    None,
                    PARSE_ERROR,
                    format!("frame is not valid JSON: {error}"),
                ));
            }
        };
        // A frame with a method but no id is a notification, and a notification
        // never receives a response — not even when it is malformed.
        let is_notification = document.as_object().is_some_and(|object| {
            !object.contains_key("id") && object.get("method").is_some_and(Value::is_string)
        });
        if jsonrpc::json_depth(&document) > MAX_JSON_DEPTH {
            if is_notification {
                return None;
            }
            return Some(jsonrpc::error(
                None,
                INVALID_REQUEST,
                format!("request nesting exceeds the maximum depth of {MAX_JSON_DEPTH}"),
            ));
        }
        let message = match jsonrpc::parse_message(&document) {
            Ok(message) => message,
            Err(failure) => {
                if is_notification {
                    return None;
                }
                return Some(jsonrpc::error(failure.id, failure.code, failure.message));
            }
        };
        self.handle_message(message)
    }

    /// Route one parsed message. `None` means the message was a notification.
    pub fn handle_message(&mut self, message: Message) -> Option<Value> {
        let Some(id) = message.id.clone() else {
            // Notifications are acknowledged by silence, including
            // `notifications/initialized` and `notifications/cancelled`.
            return None;
        };
        let outcome = match message.method.as_str() {
            "initialize" => self.initialize(&message.params),
            "ping" => Ok(json!({})),
            "tools/list" => self
                .require_initialized()
                .map(|()| tools::list_payload(self.tools)),
            "tools/call" => self
                .require_initialized()
                .and_then(|()| self.call_tool(&message.params)),
            other => Err(Failure::new(
                METHOD_NOT_FOUND,
                format!("unknown method `{other}`"),
            )),
        };
        Some(match outcome {
            Ok(result) => jsonrpc::success(id, result),
            Err(failure) => jsonrpc::error(Some(id), failure.code, failure.message),
        })
    }

    fn require_initialized(&self) -> Result<(), Failure> {
        if self.initialized {
            Ok(())
        } else {
            Err(Failure::new(
                INVALID_REQUEST,
                "server is not initialized — send `initialize` first",
            ))
        }
    }

    fn initialize(&mut self, params: &Value) -> Result<Value, Failure> {
        let requested = match params.get("protocolVersion") {
            None | Some(Value::Null) => None,
            Some(Value::String(version)) if version.len() <= MAX_PROTOCOL_VERSION_BYTES => {
                Some(version.clone())
            }
            Some(Value::String(_)) => {
                return Err(Failure::new(
                    INVALID_PARAMS,
                    format!("`protocolVersion` exceeds {MAX_PROTOCOL_VERSION_BYTES} bytes"),
                ));
            }
            Some(_) => {
                return Err(Failure::new(
                    INVALID_PARAMS,
                    "`protocolVersion` must be a string",
                ));
            }
        };
        let negotiated = requested
            .filter(|version| SUPPORTED_PROTOCOL_VERSIONS.contains(&version.as_str()))
            .unwrap_or_else(|| LATEST_PROTOCOL_VERSION.to_owned());
        self.initialized = true;
        Ok(json!({
            "protocolVersion": negotiated,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {
                "name": "abbey",
                "title": "Abbey read-only tools",
                "version": crate::build_info::VERSION
            },
            "instructions": "Read-only Abbey introspection over stdio. Every tool is \
                             non-mutating: there is no shell, filesystem, or command-execution \
                             tool in this registry, and none can be added without changing the \
                             registry's effect type."
        }))
    }

    fn call_tool(&self, params: &Value) -> Result<Value, Failure> {
        let Some(Value::String(name)) = params.get("name") else {
            return Err(Failure::new(
                INVALID_PARAMS,
                "`name` is required and must be a string",
            ));
        };
        if name.len() > MAX_TOOL_NAME_BYTES {
            return Err(Failure::new(
                INVALID_PARAMS,
                format!("tool name exceeds {MAX_TOOL_NAME_BYTES} bytes"),
            ));
        }
        let Some(tool) = self.tools.iter().find(|tool| tool.name == name) else {
            return Err(Failure::new(
                INVALID_PARAMS,
                format!("unknown tool `{name}`"),
            ));
        };
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
            Some(value @ Value::Object(_)) => value.clone(),
            Some(_) => {
                return Err(Failure::new(
                    INVALID_PARAMS,
                    "`arguments` must be an object when present",
                ));
            }
        };
        Ok(tools::call_result(self.run_with_timeout(tool, arguments)?))
    }

    /// Run a handler on a worker thread so a slow tool cannot wedge the reader.
    ///
    /// A handler that outlives its deadline is abandoned, not killed: the send
    /// on the dropped channel simply fails. Cooperative mid-flight cancellation
    /// is not implemented, and no registered tool is long-running.
    fn run_with_timeout(
        &self,
        tool: &'static SafeTool,
        arguments: Value,
    ) -> Result<Result<Value, tools::ToolError>, Failure> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let handler = tool.handler;
        std::thread::Builder::new()
            .name("abbey-mcp-tool".into())
            .spawn(move || {
                let _ = sender.send(handler(&arguments));
            })
            .map_err(|error| {
                Failure::new(INTERNAL_ERROR, format!("cannot start tool worker: {error}"))
            })?;
        receiver.recv_timeout(self.call_timeout).map_err(|_| {
            Failure::new(
                REQUEST_TIMEOUT,
                format!(
                    "tool `{}` exceeded the {} ms request deadline",
                    tool.name,
                    self.call_timeout.as_millis()
                ),
            )
        })
    }

    /// Serialize, redact, size-check, and write exactly one line.
    fn write_frame(&self, writer: &mut impl Write, response: &Value) -> std::io::Result<()> {
        let mut line = serde_json::to_string(response)
            .unwrap_or_else(|_| r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"response is not serializable"}}"#.to_owned());
        line = redact::redact_with(&line, &self.secrets);
        if line.len() > MAX_RESPONSE_BYTES {
            let id = response.get("id").cloned().unwrap_or(Value::Null);
            line = serde_json::to_string(&jsonrpc::error(
                Some(id),
                INTERNAL_ERROR,
                format!("response exceeds {MAX_RESPONSE_BYTES} bytes"),
            ))
            .expect("static error envelope serializes");
        }
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()
    }
}

/// A JSON-RPC-level failure produced while routing a request.
struct Failure {
    code: i64,
    message: String,
}

impl Failure {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Discard bytes up to and including the next newline.
fn drain_line(reader: &mut impl BufRead) -> std::io::Result<()> {
    let mut discard = Vec::new();
    loop {
        discard.clear();
        let read = (&mut *reader)
            .take(MAX_REQUEST_BYTES as u64)
            .read_until(b'\n', &mut discard)?;
        if read == 0 || discard.last() == Some(&b'\n') {
            return Ok(());
        }
    }
}
