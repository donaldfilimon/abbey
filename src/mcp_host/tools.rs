//! The capability-scoped **safe** tool registry exposed over MCP.
//!
//! ## Admission rule
//!
//! A tool may live here only if answering it requires no process spawn, no
//! filesystem write, no network call, and no mutation of Abbey state. The type
//! system carries that rule: [`EffectClass`] has exactly one variant, so there
//! is no way to *describe* a mutating tool, let alone register one. Adding a
//! second variant is the reviewable event.
//!
//! ## Why `abbey_platform` omits accelerator detection
//!
//! [`crate::platform::detect_accelerators`] shells out to `nvidia-smi`,
//! `rocm-smi`, and friends. That is read-only in intent but it is still process
//! execution, and this registry's whole promise is that a remote peer cannot
//! cause Abbey to run a program. So the MCP tool exposes only the parts of
//! `platform` that are pure computation over constants and
//! `available_parallelism` — the accelerator inventory stays a local CLI
//! surface (`abbey platform compute`).

use serde_json::{Value, json};

use crate::app_core::{AppCommand, AppEvent, AppService, ClaimStatus, ClaimsQuery};

/// The only effect class this registry can express.
///
/// There is deliberately no `Mutating`, `Execute`, or `Network` variant. A
/// shell/exec tool is not "absent by configuration" here — it is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectClass {
    ReadOnly,
}

/// A failure inside a tool handler, reported as an MCP tool error result rather
/// than a JSON-RPC protocol error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError(pub String);

impl ToolError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// One registered read-only tool.
#[derive(Debug, Clone, Copy)]
pub struct SafeTool {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub effect: EffectClass,
    /// JSON Schema 2020-12 document describing the accepted arguments.
    pub schema: fn() -> Value,
    pub handler: fn(&Value) -> Result<Value, ToolError>,
}

impl SafeTool {
    /// MCP `tools/list` descriptor.
    #[must_use]
    pub fn descriptor(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": (self.schema)(),
            "annotations": {
                // Derived from the effect class rather than hardcoded, so the
                // hint can never drift from what the registry can express.
                "readOnlyHint": matches!(self.effect, EffectClass::ReadOnly),
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        })
    }
}

/// The canonical registry advertised to every MCP client.
pub const SAFE_TOOLS: &[SafeTool] = &[
    SafeTool {
        name: "abbey_status",
        title: "Abbey runtime status",
        description: "Read Abbey's versioned runtime status: protocol/schema versions, edition, \
                      build stamp, and the read-only capability set. Pure in-memory read.",
        effect: EffectClass::ReadOnly,
        schema: status_schema,
        handler: status_handler,
    },
    SafeTool {
        name: "abbey_claims",
        title: "Abbey capability claims",
        description: "Query Abbey's canonical honesty ledger (Current / Partial / Proposed / \
                      Blocked / Out of scope) by status and substring. Reads a compiled-in \
                      table; touches no files.",
        effect: EffectClass::ReadOnly,
        schema: claims_schema,
        handler: claims_handler,
    },
    SafeTool {
        name: "abbey_platform",
        title: "Abbey host surface matrix",
        description: "Report host OS/arch/family, thread budget, default subagent jobs, argv \
                      clamp, and which Abbey surfaces are portable to this host. Accelerator \
                      detection is deliberately excluded because it spawns vendor binaries.",
        effect: EffectClass::ReadOnly,
        schema: platform_schema,
        handler: platform_handler,
    },
];

/// Every tool name advertised by this server.
#[must_use]
pub fn tool_names() -> Vec<&'static str> {
    SAFE_TOOLS.iter().map(|tool| tool.name).collect()
}

/// The `tools/list` payload.
#[must_use]
pub fn list_payload(tools: &[SafeTool]) -> Value {
    json!({"tools": tools.iter().map(SafeTool::descriptor).collect::<Vec<_>>()})
}

/// Wrap a handler outcome in MCP's tool-result shape.
#[must_use]
pub fn call_result(outcome: Result<Value, ToolError>) -> Value {
    match outcome {
        Ok(structured) => {
            let rendered = serde_json::to_string_pretty(&structured)
                .unwrap_or_else(|_| structured.to_string());
            json!({
                "content": [{"type": "text", "text": rendered}],
                "structuredContent": structured,
                "isError": false
            })
        }
        Err(ToolError(message)) => json!({
            "content": [{"type": "text", "text": message}],
            "isError": true
        }),
    }
}

/// Shared JSON Schema 2020-12 preamble.
fn object_schema(properties: Value, required: Value) -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn status_schema() -> Value {
    object_schema(json!({}), json!([]))
}

fn claims_schema() -> Value {
    object_schema(
        json!({
            "status": {
                "type": "string",
                "description": "Restrict to one ledger status.",
                "enum": ["current", "partial", "proposed", "blocked", "out_of_scope"]
            },
            "contains": {
                "type": "string",
                "description": "Case-insensitive substring over claim name, note, and substitute.",
                "minLength": 1,
                "maxLength": 256
            }
        }),
        json!([]),
    )
}

fn platform_schema() -> Value {
    object_schema(json!({}), json!([]))
}

/// Reject any argument key the schema does not declare, so a client cannot
/// smuggle an unexpected field past validation.
fn reject_unknown_keys(arguments: &Value, allowed: &[&str]) -> Result<(), ToolError> {
    let Some(object) = arguments.as_object() else {
        return Err(ToolError::new("arguments must be a JSON object"));
    };
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(ToolError::new(format!("unknown argument `{key}`")));
        }
    }
    Ok(())
}

/// Run one read-only application command and unwrap its payload.
fn app_payload(command: AppCommand) -> Result<Value, ToolError> {
    let event = AppService::default()
        .handle(command)
        .map_err(|error| ToolError::new(error.to_string()))?;
    let payload = match event {
        AppEvent::Status(status) => serde_json::to_value(status),
        AppEvent::Claims(snapshot) => serde_json::to_value(snapshot),
        AppEvent::ApprovalRequested(request) => serde_json::to_value(request),
        AppEvent::RunSubmitted(_)
        | AppEvent::RunStatus(_)
        | AppEvent::CancellationAcknowledged(_)
        | AppEvent::RunEvents(_) => {
            return Err(ToolError::new(
                "application service returned an execution event to a read-only MCP tool",
            ));
        }
    };
    payload.map_err(|error| ToolError::new(format!("cannot serialize application event: {error}")))
}

fn status_handler(arguments: &Value) -> Result<Value, ToolError> {
    reject_unknown_keys(arguments, &[])?;
    app_payload(AppCommand::Status)
}

fn claims_handler(arguments: &Value) -> Result<Value, ToolError> {
    reject_unknown_keys(arguments, &["status", "contains"])?;
    let status = match arguments.get("status") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) => Some(parse_claim_status(raw)?),
        Some(_) => return Err(ToolError::new("`status` must be a string")),
    };
    let contains = match arguments.get("contains") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) => Some(raw.clone()),
        Some(_) => return Err(ToolError::new("`contains` must be a string")),
    };
    app_payload(AppCommand::Claims(ClaimsQuery { status, contains }))
}

fn parse_claim_status(raw: &str) -> Result<ClaimStatus, ToolError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "current" => Ok(ClaimStatus::Current),
        "partial" => Ok(ClaimStatus::Partial),
        "proposed" => Ok(ClaimStatus::Proposed),
        "blocked" => Ok(ClaimStatus::Blocked),
        "out_of_scope" | "oos" => Ok(ClaimStatus::OutOfScope),
        other => Err(ToolError::new(format!(
            "unknown claim status `{other}` — expected current|partial|proposed|blocked|out_of_scope"
        ))),
    }
}

fn platform_handler(arguments: &Value) -> Result<Value, ToolError> {
    reject_unknown_keys(arguments, &[])?;
    let surfaces = crate::platform::surface_matrix()
        .iter()
        .map(|surface| {
            json!({
                "name": surface.name,
                "linux": surface.linux,
                "macos": surface.macos,
                "windows": surface.windows,
                "supported_here": crate::platform::surface_on_this_host(surface),
                "note": surface.note
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "threads": crate::platform::thread_budget(),
        "default_subagent_jobs": crate::platform::default_subagent_jobs(),
        "argv_prompt_clamp_bytes": crate::host::max_prompt_argv_bytes(),
        "surfaces": surfaces,
        "accelerator_detection": "excluded from this tool: it spawns vendor binaries \
                                  (nvidia-smi/rocm-smi). Use the local CLI `abbey platform compute`."
    }))
}
