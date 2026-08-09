//! Process-level conformance for `abbey mcp serve`.
//!
//! The unit tests in `src/mcp_host/tests.rs` drive the `Server` type directly.
//! These drive the *binary*, because three guarantees only exist once the
//! process runs: stdout carries JSON-RPC and nothing else, EOF on stdin is a
//! clean exit rather than a hang, and no credential from the real environment
//! reaches the wire.
//!
//! Every test uses a throwaway `ABBEY_STATE_DIR`, and nothing here spawns
//! cursor-agent.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_abbey");

/// A canary planted in the child's environment under a credential-shaped name.
/// It must never appear in any response frame.
const CANARY_NAME: &str = "ABBEY_MCP_TEST_BEARER_TOKEN";
const CANARY_VALUE: &str = "canary-8b31e0d4-must-not-be-emitted";

/// A per-test state directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "abbey-mcp-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch state dir");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Feed `frames` to `abbey mcp serve` and collect (exit code, stdout, stderr).
fn serve(scratch: &Scratch, frames: &[&str]) -> (i32, String, String) {
    let mut child = Command::new(BIN)
        .args(["mcp", "serve"])
        .env("ABBEY_STATE_DIR", &scratch.0)
        .env(CANARY_NAME, CANARY_VALUE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn abbey mcp serve");
    {
        let stdin = child.stdin.as_mut().expect("child stdin");
        for line in frames {
            stdin.write_all(line.as_bytes()).expect("write frame");
            stdin.write_all(b"\n").expect("write newline");
        }
        stdin.flush().expect("flush");
    }
    // Dropping stdin sends EOF, which must terminate the session.
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("await abbey mcp serve");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Parse stdout as one JSON document per line, failing if anything else leaked.
fn frames(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!("stdout carried a non-JSON line ({error}): {line:?}")
            })
        })
        .collect()
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"abbey-test","version":"1"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

#[test]
fn a_real_client_exchange_completes_over_stdio() {
    let scratch = Scratch::new("exchange");
    let (code, stdout, stderr) = serve(
        &scratch,
        &[
            INITIALIZE,
            INITIALIZED,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"abbey_status"}}"#,
        ],
    );

    assert_eq!(code, 0, "EOF must be a clean exit; stderr: {stderr}");
    let responses = frames(&stdout);
    assert_eq!(
        responses.len(),
        3,
        "expected exactly three responses (the notification gets none): {stdout}"
    );

    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "abbey");

    assert_eq!(responses[1]["id"], 2);
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    assert!(!tools.is_empty());

    assert_eq!(responses[2]["id"], 3);
    let status = &responses[2]["result"]["structuredContent"];
    // The served status must name the edition this binary was compiled as, so
    // the assertion tracks the build rather than pinning one edition's label.
    let expected_edition = if cfg!(feature = "personal-edition") {
        "personal"
    } else {
        "standard"
    };
    assert_eq!(status["edition"], expected_edition);
    assert_eq!(status["state"], "ready");
    assert!(status["capabilities"]["capabilities"].is_array());
}

#[test]
fn stdout_is_a_protocol_channel_only() {
    let scratch = Scratch::new("stdout-purity");
    let (_, stdout, _) = serve(&scratch, &[INITIALIZE, INITIALIZED]);
    // `frames` panics on any line that is not a JSON document, which is the
    // assertion: no banner, no log line, no stray println reaches stdout.
    let responses = frames(&stdout);
    assert_eq!(responses.len(), 1);
    for response in &responses {
        assert_eq!(response["jsonrpc"], "2.0");
    }
}

#[test]
fn the_advertised_registry_contains_no_execution_capable_tool() {
    let scratch = Scratch::new("no-exec");
    let (_, stdout, _) = serve(
        &scratch,
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        ],
    );
    let responses = frames(&stdout);
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();

    assert_eq!(names, ["abbey_status", "abbey_claims", "abbey_platform"]);
    for forbidden in [
        "shell.exec",
        "shell",
        "exec",
        "run_command",
        "bash",
        "write_file",
        "os.execute",
    ] {
        assert!(
            !names.contains(&forbidden),
            "`{forbidden}` must never be discoverable over MCP"
        );
    }
    for tool in tools {
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(
            tool["inputSchema"]["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
    }

    // A client that asks for one anyway is refused, not served.
    let (_, stdout, _) = serve(
        &scratch,
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"shell.exec","arguments":{"command":"id"}}}"#,
        ],
    );
    let responses = frames(&stdout);
    assert_eq!(responses[1]["error"]["code"], -32602);
}

#[test]
fn malformed_and_unknown_traffic_produces_errors_not_a_crash() {
    let scratch = Scratch::new("malformed");
    let (code, stdout, stderr) = serve(
        &scratch,
        &[
            INITIALIZE,
            "{ this is not json",
            r#"{"jsonrpc":"2.0","id":3}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"file:///etc/passwd"}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"abbey_platform"}}"#,
        ],
    );

    assert_eq!(code, 0, "the server must survive malformed input: {stderr}");
    let responses = frames(&stdout);
    assert_eq!(responses.len(), 5);
    assert_eq!(responses[1]["error"]["code"], -32700, "parse error");
    assert_eq!(responses[2]["error"]["code"], -32600, "invalid request");
    assert_eq!(responses[3]["error"]["code"], -32601, "method not found");
    // The session is still healthy afterwards.
    assert_eq!(responses[4]["result"]["isError"], false);
}

#[test]
fn an_oversized_frame_is_rejected_and_the_session_continues() {
    let scratch = Scratch::new("oversized");
    let mut huge = String::from(r#"{"jsonrpc":"2.0","id":2,"method":"ping","params":{"pad":""#);
    huge.push_str(&"A".repeat(300 * 1024));
    huge.push_str(r#""}}"#);

    let (code, stdout, stderr) = serve(
        &scratch,
        &[
            INITIALIZE,
            &huge,
            r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#,
        ],
    );

    assert_eq!(code, 0, "stderr: {stderr}");
    let responses = frames(&stdout);
    assert_eq!(responses.len(), 3);
    assert_eq!(
        responses[1]["error"]["code"], -32001,
        "oversized frames are rejected at the documented limit"
    );
    assert_eq!(
        responses[2]["id"], 3,
        "the frame after an oversized one must still parse — the giant line is drained"
    );
    assert!(responses[2].get("result").is_some());
}

#[test]
fn no_environment_credential_reaches_any_response() {
    let scratch = Scratch::new("redaction");
    // The last frame is the sharp one: an invalid `status` argument makes the
    // handler echo the caller's string back in its error message. Feeding it
    // the canary means the value genuinely travels through a response path, so
    // the assertion below tests the redaction pass rather than testing that
    // nothing happened to mention it.
    let echo = format!(
        r#"{{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{{"name":"abbey_claims","arguments":{{"status":"{CANARY_VALUE}"}}}}}}"#
    );
    let (_, stdout, stderr) = serve(
        &scratch,
        &[
            INITIALIZE,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"abbey_status"}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"abbey_platform"}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"abbey_claims","arguments":{"contains":"memory"}}}"#,
            &echo,
        ],
    );
    assert!(
        !stdout.contains(CANARY_VALUE),
        "a credential-shaped environment value reached the wire: {stdout}"
    );
    assert!(!stderr.contains(CANARY_VALUE));

    let responses = frames(&stdout);
    assert_eq!(responses.len(), 6, "the exchange must actually have run");
    let echoed = responses[5]["result"]["content"][0]["text"]
        .as_str()
        .expect("the echoing handler returns a text error result");
    assert!(
        echoed.contains("[REDACTED]"),
        "the value reached the response and should have been masked: {echoed}"
    );
}

#[test]
fn the_registry_description_command_is_not_the_serve_path() {
    // `abbey mcp tools` is an ordinary human-readable CLI print. It must state
    // the limits and what is deliberately unimplemented.
    let scratch = Scratch::new("registry");
    let out = Command::new(BIN)
        .args(["mcp", "tools"])
        .env("ABBEY_STATE_DIR", &scratch.0)
        .output()
        .expect("spawn abbey mcp tools");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("abbey_status"));
    assert!(stdout.contains("Streamable HTTP"), "{stdout}");
    assert!(stdout.contains("OAuth"), "{stdout}");
}
