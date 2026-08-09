//! Conformance, limit, and safety tests for the read-only stdio MCP server.

use std::io::Cursor;
use std::time::Duration;

use serde_json::{Value, json};

use super::jsonrpc::{
    INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR,
    REQUEST_TIMEOUT, REQUEST_TOO_LARGE,
};
use super::limits::{
    MAX_JSON_DEPTH, MAX_PROTOCOL_VERSION_BYTES, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES,
    MAX_TOOL_NAME_BYTES,
};
use super::redact::{REDACTION_PLACEHOLDER, name_is_sensitive, redact_with};
use super::serve::{LATEST_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS, Server};
use super::tools::{EffectClass, SAFE_TOOLS, SafeTool, ToolError, tool_names};

/// Substrings that would mark a tool *name* as execution-capable. A name is
/// the client-visible handle, so the bar here is deliberately blunt.
const EXECUTION_NAME_MARKERS: &[&str] = &[
    "shell",
    "exec",
    "command",
    "bash",
    "spawn",
    "subprocess",
    "eval",
    "run",
    "write",
    "delete",
    "remove",
    "os_control",
    "process",
    "file",
];

/// Phrases that would describe an execution or mutation *capability*.
///
/// Kept separate from the name markers on purpose: a description that
/// *disclaims* execution ("excluded because it spawns vendor binaries") is the
/// behaviour we want, and a blunt substring scan over prose would punish it.
const EXECUTION_CAPABILITY_PHRASES: &[&str] = &[
    "run a command",
    "run any command",
    "execute a",
    "execute any",
    "shell command",
    "arbitrary command",
    "write a file",
    "modify the filesystem",
    "delete a",
];

fn frame(server: &mut Server, message: Value) -> Option<Value> {
    server.handle_frame(message.to_string().as_bytes())
}

fn initialized() -> Server {
    let mut server = Server::new();
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": LATEST_PROTOCOL_VERSION,
                          "clientInfo": {"name": "test", "version": "0"}}}),
    )
    .expect("initialize is a request and must be answered");
    assert!(
        response.get("error").is_none(),
        "initialize failed: {response}"
    );
    server
}

fn error_code(response: &Value) -> i64 {
    response
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("expected a JSON-RPC error, got {response}"))
}

// ---------------------------------------------------------------- conformance

#[test]
fn initialize_then_list_then_call_is_the_happy_path() {
    let mut server = Server::new();

    let init = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": "a", "method": "initialize",
               "params": {"protocolVersion": LATEST_PROTOCOL_VERSION}}),
    )
    .unwrap();
    assert_eq!(init["jsonrpc"], "2.0");
    assert_eq!(init["id"], "a");
    assert_eq!(init["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    assert_eq!(
        init["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "abbey");

    let listed = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .unwrap();
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), SAFE_TOOLS.len());
    for tool in tools {
        assert_eq!(
            tool["inputSchema"]["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "every descriptor must be JSON Schema 2020-12"
        );
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
    }

    let called = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
               "params": {"name": "abbey_claims",
                          "arguments": {"status": "blocked", "contains": "linux"}}}),
    )
    .unwrap();
    let result = &called["result"];
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["matched"], 1);
    assert_eq!(
        result["structuredContent"]["claims"][0]["status"], "blocked",
        "the tool must read the canonical ledger, not a copy"
    );
    assert_eq!(result["content"][0]["type"], "text");
}

#[test]
fn protocol_version_is_negotiated_not_assumed() {
    // An older revision this server supports is echoed back verbatim.
    let mut server = Server::new();
    let older = SUPPORTED_PROTOCOL_VERSIONS.last().unwrap();
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": older}}),
    )
    .unwrap();
    assert_eq!(response["result"]["protocolVersion"], *older);

    // An unknown revision falls back to the newest one this server implements.
    let mut server = Server::new();
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": "1999-01-01"}}),
    )
    .unwrap();
    assert_eq!(
        response["result"]["protocolVersion"],
        LATEST_PROTOCOL_VERSION
    );
}

#[test]
fn tool_traffic_before_initialize_is_refused() {
    let mut server = Server::new();
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .unwrap();
    assert_eq!(error_code(&response), INVALID_REQUEST);
}

#[test]
fn notifications_never_receive_a_response() {
    let mut server = initialized();
    for notification in [
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
               "params": {"requestId": 3, "reason": "user cancelled"}}),
        // Even a malformed notification stays silent.
        json!({"jsonrpc": "1.0", "method": "notifications/initialized"}),
    ] {
        assert_eq!(
            frame(&mut server, notification.clone()),
            None,
            "responded to notification {notification}"
        );
    }
}

#[test]
fn unknown_method_is_method_not_found() {
    let mut server = initialized();
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 9, "method": "resources/list"}),
    )
    .unwrap();
    assert_eq!(error_code(&response), METHOD_NOT_FOUND);
    assert_eq!(response["id"], 9);
}

#[test]
fn unknown_tool_is_rejected_without_touching_a_handler() {
    let mut server = initialized();
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": "shell.exec", "arguments": {"cmd": "rm -rf /"}}}),
    )
    .unwrap();
    assert_eq!(error_code(&response), INVALID_PARAMS);
}

// ------------------------------------------------------------ malformed input

#[test]
fn malformed_frames_produce_errors_not_panics() {
    let mut server = initialized();

    let cases: &[(&str, i64)] = &[
        ("{not json at all", PARSE_ERROR),
        ("[]", INVALID_REQUEST),
        ("\"a bare string\"", INVALID_REQUEST),
        ("42", INVALID_REQUEST),
        (r#"{"id": 1, "method": "ping"}"#, INVALID_REQUEST),
        (r#"{"jsonrpc": "2.0", "id": 1}"#, INVALID_REQUEST),
        (
            r#"{"jsonrpc": "2.0", "id": 1, "method": 5}"#,
            INVALID_REQUEST,
        ),
        (
            r#"{"jsonrpc": "2.0", "id": {"bad": true}, "method": "ping"}"#,
            INVALID_REQUEST,
        ),
        (
            r#"{"jsonrpc": "2.0", "id": 1, "method": "ping", "params": []}"#,
            INVALID_PARAMS,
        ),
    ];
    for (raw, expected) in cases {
        let response = server
            .handle_frame(raw.as_bytes())
            .unwrap_or_else(|| panic!("frame {raw} produced no response"));
        assert_eq!(response["jsonrpc"], "2.0", "frame {raw}");
        assert_eq!(error_code(&response), *expected, "frame {raw}");
    }
}

#[test]
fn batches_are_rejected_at_the_documented_ceiling() {
    let mut server = initialized();
    let response = server
        .handle_frame(br#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#)
        .unwrap();
    assert_eq!(error_code(&response), INVALID_REQUEST);
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("batching")
    );
}

#[test]
fn nesting_past_the_depth_limit_is_rejected_before_any_tool_runs() {
    let mut server = initialized();
    let mut nested = json!({});
    for _ in 0..(MAX_JSON_DEPTH + 4) {
        nested = json!({"n": nested});
    }
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": "abbey_status", "arguments": nested}}),
    )
    .unwrap();
    assert_eq!(error_code(&response), INVALID_REQUEST);
}

#[test]
fn oversized_frames_are_rejected_and_the_session_survives() {
    // One frame past the ceiling, then a well-formed one. The second must be
    // answered normally, which only happens if the oversized line was drained
    // rather than partially re-parsed.
    let mut input = String::with_capacity(MAX_REQUEST_BYTES * 2);
    input.push_str(r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"pad":""#);
    input.push_str(&"A".repeat(MAX_REQUEST_BYTES + 64));
    input.push_str("\"}}\n");
    input.push_str("{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n");

    let mut output = Vec::new();
    Server::new()
        .serve(Cursor::new(input.into_bytes()), &mut output)
        .expect("serve returns cleanly at EOF");

    let lines: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("every emitted line is JSON"))
        .collect();
    assert_eq!(lines.len(), 2, "expected one error and one success");
    assert_eq!(error_code(&lines[0]), REQUEST_TOO_LARGE);
    assert_eq!(lines[0]["id"], Value::Null);
    assert_eq!(lines[1]["id"], 2);
    assert!(lines[1].get("result").is_some());
}

#[test]
fn a_frame_exactly_one_byte_past_the_ceiling_does_not_swallow_the_next_one() {
    // The awkward boundary: the bounded read stops after MAX_REQUEST_BYTES + 1
    // bytes, and at exactly that length the terminating newline is already
    // consumed. Draining unconditionally here would discard the *next* request,
    // so this case must still produce two responses.
    let prefix = r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"pad":""#;
    let suffix = "\"}}\n";
    let pad = MAX_REQUEST_BYTES + 1 - prefix.len() - suffix.len();
    let mut input = String::with_capacity(MAX_REQUEST_BYTES * 2);
    input.push_str(prefix);
    input.push_str(&"A".repeat(pad));
    input.push_str(suffix);
    assert_eq!(
        input.len(),
        MAX_REQUEST_BYTES + 1,
        "the first line must land exactly on the boundary, newline included"
    );
    input.push_str("{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n");

    let mut output = Vec::new();
    Server::new()
        .serve(Cursor::new(input.into_bytes()), &mut output)
        .expect("serve returns cleanly at EOF");

    let lines: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("every emitted line is JSON"))
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "the frame after the boundary case was swallowed"
    );
    assert_eq!(error_code(&lines[0]), REQUEST_TOO_LARGE);
    assert_eq!(lines[1]["id"], 2);
    assert!(lines[1].get("result").is_some());
}

#[test]
fn eof_ends_the_session_cleanly() {
    let mut output = Vec::new();
    Server::new()
        .serve(Cursor::new(Vec::new()), &mut output)
        .expect("empty stdin is a clean shutdown, not an error");
    assert!(output.is_empty());
}

// --------------------------------------------------------- timeout / slow tool

fn slow_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {},
        "required": [],
        "additionalProperties": false
    })
}

fn slow_handler(_arguments: &Value) -> Result<Value, ToolError> {
    std::thread::sleep(Duration::from_secs(30));
    Ok(json!({"unreachable": true}))
}

fn leak_handler(_arguments: &Value) -> Result<Value, ToolError> {
    Ok(json!({"echo": CANARY}))
}

fn huge_handler(_arguments: &Value) -> Result<Value, ToolError> {
    Ok(json!({"blob": "Z".repeat(MAX_RESPONSE_BYTES + 1024)}))
}

const CANARY: &str = "abbey-canary-9f2c1d4e-do-not-log";

/// Test-only registry. Never reachable from the shipped [`SAFE_TOOLS`].
const HARNESS_TOOLS: &[SafeTool] = &[
    SafeTool {
        name: "test_slow",
        title: "slow",
        description: "test-only handler that outlives its deadline",
        effect: EffectClass::ReadOnly,
        schema: slow_schema,
        handler: slow_handler,
    },
    SafeTool {
        name: "test_leak",
        title: "leak",
        description: "test-only handler that returns a canary secret",
        effect: EffectClass::ReadOnly,
        schema: slow_schema,
        handler: leak_handler,
    },
    SafeTool {
        name: "test_huge",
        title: "huge",
        description: "test-only handler that returns more than the response ceiling",
        effect: EffectClass::ReadOnly,
        schema: slow_schema,
        handler: huge_handler,
    },
];

fn harness(timeout: Duration) -> Server {
    let mut server = Server::with_tools(HARNESS_TOOLS, timeout);
    frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}}),
    )
    .unwrap();
    server
}

#[test]
fn a_tool_that_outlives_its_deadline_times_out_instead_of_wedging_the_reader() {
    let mut server = harness(Duration::from_millis(60));
    let started = std::time::Instant::now();
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": "test_slow"}}),
    )
    .unwrap();
    assert_eq!(error_code(&response), REQUEST_TIMEOUT);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the reader must not wait for the abandoned handler"
    );

    // The session is still usable after a timeout.
    let after = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
    )
    .unwrap();
    assert!(after.get("result").is_some());
}

#[test]
fn a_response_past_the_size_ceiling_is_replaced_rather_than_written() {
    let mut server = Server::with_tools(HARNESS_TOOLS, Duration::from_secs(30));
    let input = format!(
        "{}\n{}\n{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": "test_huge"}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "ping"})
    );
    let mut output = Vec::new();
    server
        .serve(Cursor::new(input.into_bytes()), &mut output)
        .unwrap();
    let lines: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| {
            assert!(
                line.len() <= MAX_RESPONSE_BYTES,
                "an oversized frame reached the wire ({} bytes)",
                line.len()
            );
            serde_json::from_str(line).expect("every emitted line is JSON")
        })
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(error_code(&lines[1]), INTERNAL_ERROR);
    assert_eq!(
        lines[1]["id"], 2,
        "the caller can still correlate the failure"
    );
    assert!(lines[2].get("result").is_some(), "session survives");
}

#[test]
fn over_long_tool_names_and_protocol_versions_are_rejected() {
    let mut server = initialized();
    let long_name = "a".repeat(MAX_TOOL_NAME_BYTES + 1);
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": long_name}}),
    )
    .unwrap();
    assert_eq!(error_code(&response), INVALID_PARAMS);

    let mut fresh = Server::new();
    let long_version = "9".repeat(MAX_PROTOCOL_VERSION_BYTES + 1);
    let response = frame(
        &mut fresh,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": long_version}}),
    )
    .unwrap();
    assert_eq!(error_code(&response), INVALID_PARAMS);
    // A rejected handshake must not leave the server initialized.
    let after = frame(
        &mut fresh,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .unwrap();
    assert_eq!(error_code(&after), INVALID_REQUEST);
}

// ------------------------------------------------------------------ redaction

#[test]
fn credential_shaped_variable_names_are_recognized() {
    for name in [
        "OPENAI_API_KEY",
        "ABBEYD_BEARER_TOKEN",
        "aws_secret_access_key",
        "GH_TOKEN",
        "DB_PASSWORD",
    ] {
        assert!(name_is_sensitive(name), "{name} should be sensitive");
    }
    for name in ["PATH", "HOME", "LANG", "TERM", "ABBEY_STATE_DIR"] {
        assert!(!name_is_sensitive(name), "{name} should not be sensitive");
    }
}

#[test]
fn redaction_masks_secrets_but_leaves_short_values_alone() {
    let secrets = vec!["sk-live-0123456789".to_owned(), "en".to_owned()];
    let text = "token=sk-live-0123456789 lang=en";
    let masked = redact_with(text, &secrets);
    assert!(!masked.contains("sk-live-0123456789"));
    assert!(masked.contains(REDACTION_PLACEHOLDER));
    assert!(
        masked.contains("lang=en"),
        "a two-byte value must not be substring-replaced: {masked}"
    );
}

#[test]
fn a_secret_never_reaches_the_wire_even_if_a_handler_returns_it() {
    let mut server = Server::with_tools(HARNESS_TOOLS, Duration::from_secs(5));
    server.set_secrets(vec![CANARY.to_owned()]);
    let input = format!(
        "{}\n{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": "test_leak"}})
    );
    let mut output = Vec::new();
    server
        .serve(Cursor::new(input.into_bytes()), &mut output)
        .unwrap();
    let transcript = String::from_utf8(output).unwrap();
    assert!(
        !transcript.contains(CANARY),
        "canary escaped redaction: {transcript}"
    );
    assert!(transcript.contains(REDACTION_PLACEHOLDER));
}

// ------------------------------------------------------- registry containment

#[test]
fn no_advertised_tool_is_execution_capable() {
    let mut server = initialized();
    let listed = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .unwrap();
    let advertised = listed["result"]["tools"].as_array().unwrap();
    assert!(!advertised.is_empty(), "the registry must advertise tools");

    for tool in advertised {
        let name = tool["name"].as_str().unwrap().to_ascii_lowercase();
        for marker in EXECUTION_NAME_MARKERS {
            assert!(
                !name.contains(marker),
                "advertised tool `{name}` contains `{marker}` — an execution-capable tool must \
                 never be discoverable over MCP"
            );
        }
        let description = tool["description"].as_str().unwrap().to_ascii_lowercase();
        for phrase in EXECUTION_CAPABILITY_PHRASES {
            assert!(
                !description.contains(phrase),
                "advertised tool `{name}` describes the capability `{phrase}`"
            );
        }
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "{name}");
        assert_eq!(tool["annotations"]["destructiveHint"], false, "{name}");
        assert_eq!(tool["annotations"]["openWorldHint"], false, "{name}");
    }

    for tool in SAFE_TOOLS {
        assert_eq!(
            tool.effect,
            EffectClass::ReadOnly,
            "{} must be read-only",
            tool.name
        );
    }
    assert_eq!(
        tool_names(),
        vec!["abbey_status", "abbey_claims", "abbey_platform"],
        "the safe registry changed — re-justify every entry as non-mutating"
    );
}

#[test]
fn tool_arguments_reject_unknown_and_ill_typed_fields() {
    let mut server = initialized();
    for arguments in [
        json!({"unexpected": 1}),
        json!({"status": 5}),
        json!({"status": "not-a-status"}),
    ] {
        let response = frame(
            &mut server,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": "abbey_claims", "arguments": arguments}}),
        )
        .unwrap();
        assert_eq!(
            response["result"]["isError"], true,
            "arguments {arguments} should have failed"
        );
    }
}

#[test]
fn platform_tool_reports_host_facts_without_spawning_a_process() {
    let mut server = initialized();
    let response = frame(
        &mut server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": "abbey_platform"}}),
    )
    .unwrap();
    let structured = &response["result"]["structuredContent"];
    assert_eq!(structured["os"], std::env::consts::OS);
    assert!(structured["threads"].as_u64().unwrap() >= 1);
    assert!(structured["surfaces"].as_array().unwrap().len() > 3);
    assert!(
        structured["accelerator_detection"]
            .as_str()
            .unwrap()
            .contains("spawns vendor binaries"),
        "the omission must be stated in the payload, not just in a doc comment"
    );
}
