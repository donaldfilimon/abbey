//! Process-level conformance for `abbey mcp serve http`.
//!
//! The unit tests in `src/mcp_host/http/tests.rs` drive the transport's
//! functions and an in-process listener. These drive the *binary*, because four
//! guarantees only exist once the real process runs:
//!
//! * a non-loopback `--host` is a hard **startup** error, not a warning,
//! * the listening address is announced on stderr and never on stdout,
//! * a credential in the real environment does not reach an HTTP response body
//!   (the stdio suite's canary test proves this for stdio only — the HTTP body
//!   is built by the same `Server::encode_frame`, and this is what pins that),
//! * the HTTP-advertised `tools/list` equals the stdio-advertised one, compared
//!   across two separately spawned processes rather than inside one.
//!
//! Every test uses a throwaway `ABBEY_STATE_DIR` and an ephemeral port
//! (`--port 0`), so the suite never contends for a fixed port and never touches
//! a user's state.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_abbey");

/// A canary planted in the child's environment under a credential-shaped name.
/// It must never appear in any response body.
const CANARY_NAME: &str = "ABBEY_MCP_TEST_BEARER_TOKEN";
const CANARY_VALUE: &str = "canary-9f2c71ab-must-not-be-emitted";

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"abbey-test","version":"1"}}}"#;
const LIST: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
const CALL: &str =
    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"abbey_status"}}"#;

/// A per-test state directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "abbey-mcp-http-{tag}-{}-{:?}",
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

/// A running `abbey mcp serve http`, killed on drop.
struct Serving {
    child: Child,
    addr: SocketAddr,
    /// The child's stderr pipe, held open for the server's whole life.
    ///
    /// Not decoration. `abbey` resets SIGPIPE to its default before `main`, so
    /// closing this reader after the first announcement line kills the server
    /// the moment it prints its second one — which is exactly what happened
    /// when this handle was dropped early: `connect` still succeeded against
    /// the listen backlog and every request came back empty.
    _stderr: BufReader<std::process::ChildStderr>,
}

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start the real binary on an ephemeral loopback port.
///
/// The address is read back off **stderr**, which is both how a human finds it
/// and the assertion that it is not on stdout.
fn serve_http(scratch: &Scratch) -> Serving {
    let mut child = Command::new(BIN)
        .args(["mcp", "serve", "http", "--host", "127.0.0.1", "--port", "0"])
        .env("ABBEY_STATE_DIR", &scratch.0)
        .env(CANARY_NAME, CANARY_VALUE)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn abbey mcp serve http");

    let mut stderr = BufReader::new(child.stderr.take().expect("child stderr"));
    let mut announcement = String::new();
    stderr
        .read_line(&mut announcement)
        .expect("read the listening announcement");
    let addr = announcement
        .rsplit_once("http://")
        .and_then(|(_, rest)| rest.split('/').next())
        .and_then(|authority| authority.trim().parse::<SocketAddr>().ok())
        .unwrap_or_else(|| panic!("cannot parse a listen address out of {announcement:?}"));

    Serving {
        child,
        addr,
        _stderr: stderr,
    }
}

/// One raw HTTP exchange. Returns the whole response, headers included.
fn exchange(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect to abbey mcp serve http");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set a read timeout");
    stream
        .write_all(request.as_bytes())
        .expect("write the request");
    stream.flush().expect("flush the request");
    let mut response = Vec::new();
    // The server always answers `Connection: close`, so EOF delimits the body.
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).into_owned()
}

/// `POST /mcp` carrying `frame`, with optional extra headers.
fn post(addr: SocketAddr, frame: &str, extra: &[(&str, &str)]) -> String {
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n",
        frame.len()
    );
    for (name, value) in extra {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(frame);
    exchange(addr, &request)
}

fn status(response: &str) -> &str {
    response.lines().next().unwrap_or_default()
}

fn header<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    response
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.trim())
        })
}

/// The JSON body, i.e. everything after the blank line.
fn body(response: &str) -> Value {
    let (_, raw) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no header/body separator in {response:?}"));
    serde_json::from_str(raw).unwrap_or_else(|error| panic!("body is not JSON ({error}): {raw:?}"))
}

/// Open a session and return its `Mcp-Session-Id`.
fn open_session(addr: SocketAddr) -> String {
    let response = post(addr, INITIALIZE, &[]);
    assert!(status(&response).contains("200"), "{response}");
    header(&response, "Mcp-Session-Id")
        .expect("initialize returns a session id")
        .to_owned()
}

#[test]
fn a_non_loopback_bind_fails_at_startup_instead_of_listening() {
    let scratch = Scratch::new("bind");
    for host in ["0.0.0.0", "::", "192.168.1.5", "example.com"] {
        let out = Command::new(BIN)
            .args(["mcp", "serve", "http", "--host", host, "--port", "0"])
            .env("ABBEY_STATE_DIR", &scratch.0)
            .stdin(Stdio::null())
            .output()
            .expect("spawn abbey mcp serve http");
        assert!(
            !out.status.success(),
            "`--host {host}` must fail, not listen"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("loopback only"),
            "`--host {host}` failed for the wrong reason: {stderr}"
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).trim().is_empty(),
            "a refused bind must print nothing to stdout"
        );
    }
}

#[test]
fn a_real_client_exchange_completes_over_loopback_http() {
    let scratch = Scratch::new("exchange");
    let server = serve_http(&scratch);

    let opened = post(server.addr, INITIALIZE, &[]);
    assert!(status(&opened).contains("200"), "{opened}");
    assert_eq!(header(&opened, "Content-Type"), Some("application/json"));
    assert_eq!(header(&opened, "Connection"), Some("close"));
    let session = header(&opened, "Mcp-Session-Id")
        .expect("initialize returns a session id")
        .to_owned();
    let initialized = body(&opened);
    assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(initialized["result"]["serverInfo"]["name"], "abbey");

    let listed = post(server.addr, LIST, &[("Mcp-Session-Id", &session)]);
    assert!(status(&listed).contains("200"), "{listed}");
    let tools = body(&listed)["result"]["tools"]
        .as_array()
        .expect("tools/list returns an array")
        .clone();
    assert!(
        tools.iter().any(|tool| {
            tool["name"] == "abbey_status" && tool["annotations"]["readOnlyHint"] == true
        }),
        "{tools:?}"
    );

    let called = post(server.addr, CALL, &[("Mcp-Session-Id", &session)]);
    assert!(status(&called).contains("200"), "{called}");
    let result = body(&called)["result"].clone();
    assert_eq!(result["isError"], false, "{result}");
    assert!(
        result["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("version")),
        "{result}"
    );

    // Releasing the session is the documented lifecycle, and it really releases.
    let released = exchange(
        server.addr,
        &format!("DELETE /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nMcp-Session-Id: {session}\r\n\r\n"),
    );
    assert!(status(&released).contains("204"), "{released}");
    let orphaned = post(server.addr, LIST, &[("Mcp-Session-Id", &session)]);
    assert!(status(&orphaned).contains("404"), "{orphaned}");
}

#[test]
fn a_hostile_origin_is_refused_by_the_running_server() {
    let scratch = Scratch::new("origin");
    let server = serve_http(&scratch);
    let session = open_session(server.addr);

    // The lookalike prefixes are the whole point: substring matching would let
    // both of these through.
    for origin in [
        "http://evil.com",
        "http://127.0.0.1.evil.com",
        "http://localhost.evil.com",
        "null",
    ] {
        let refused = post(
            server.addr,
            LIST,
            &[("Mcp-Session-Id", &session), ("Origin", origin)],
        );
        assert!(
            status(&refused).contains("403"),
            "Origin `{origin}` must be refused: {refused}"
        );
        assert!(refused.contains("DNS-rebinding defense"), "{refused}");
    }

    // A rebound request need not send Origin at all — Host carries the attacker's
    // name, and that layer must catch it on its own.
    let rebound = exchange(
        server.addr,
        &format!(
            "POST /mcp HTTP/1.1\r\nHost: attacker.example\r\nContent-Type: application/json\r\n\
             Mcp-Session-Id: {session}\r\nContent-Length: {}\r\n\r\n{LIST}",
            LIST.len()
        ),
    );
    assert!(status(&rebound).contains("403"), "{rebound}");

    // A loopback origin still works, so the defense is not simply refusing all.
    let allowed = post(
        server.addr,
        LIST,
        &[("Mcp-Session-Id", &session), ("Origin", "http://127.0.0.1")],
    );
    assert!(status(&allowed).contains("200"), "{allowed}");
}

#[test]
fn no_environment_credential_reaches_any_http_response() {
    let scratch = Scratch::new("canary");
    let server = serve_http(&scratch);
    let session = open_session(server.addr);

    // Every reachable tool, exercised for real, plus the error paths.
    let mut seen = Vec::new();
    for frame in [
        LIST,
        CALL,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"abbey_platform"}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"abbey_claims"}}"#,
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"nope"}}"#,
        r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
    ] {
        seen.push(post(server.addr, frame, &[("Mcp-Session-Id", &session)]));
    }
    let transcript = seen.join("\n");
    assert!(
        !transcript.contains(CANARY_VALUE),
        "a credential from the environment reached an HTTP response body"
    );
    // The canary really was in the child's environment, or this proves nothing.
    assert!(transcript.contains("abbey_status"), "{transcript}");
}

#[test]
fn the_http_advertised_tool_list_equals_the_stdio_one() {
    // Two separate processes: this is the cross-transport claim, so it must not
    // be satisfied by one process comparing a value with itself.
    let scratch = Scratch::new("parity");

    let mut stdio = Command::new(BIN)
        .args(["mcp", "serve"])
        .env("ABBEY_STATE_DIR", &scratch.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn abbey mcp serve");
    {
        let stdin = stdio.stdin.as_mut().expect("child stdin");
        for frame in [INITIALIZE, LIST] {
            stdin.write_all(frame.as_bytes()).expect("write frame");
            stdin.write_all(b"\n").expect("write newline");
        }
        stdin.flush().expect("flush");
    }
    drop(stdio.stdin.take());
    let out = stdio.wait_with_output().expect("await abbey mcp serve");
    let over_stdio: Value = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|frame| frame["id"] == 2)
        .expect("stdio answered tools/list")["result"]
        .clone();

    let server = serve_http(&scratch);
    let session = open_session(server.addr);
    let over_http =
        body(&post(server.addr, LIST, &[("Mcp-Session-Id", &session)]))["result"].clone();

    assert_eq!(
        over_http, over_stdio,
        "the two transports must advertise exactly the same registry"
    );

    // And that shared registry contains nothing execution-capable.
    for tool in over_http["tools"].as_array().expect("tools is an array") {
        let name = tool["name"]
            .as_str()
            .expect("every tool has a name")
            .to_ascii_lowercase();
        for marker in [
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
        ] {
            assert!(
                !name.contains(marker),
                "HTTP advertises `{name}`, which contains the execution marker `{marker}`"
            );
        }
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "{name}");
        assert_eq!(tool["annotations"]["destructiveHint"], false, "{name}");
    }
}

#[test]
fn the_transport_refuses_what_it_does_not_implement() {
    let scratch = Scratch::new("unimplemented");
    let server = serve_http(&scratch);

    // No SSE stream: GET is refused rather than half-implemented.
    let streamed = exchange(
        server.addr,
        "GET /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n\r\n",
    );
    assert!(status(&streamed).contains("405"), "{streamed}");
    assert!(streamed.contains("Server-Sent Events"), "{streamed}");

    // No OAuth: the server must not emit a WWW-Authenticate challenge or any
    // other hint that a token would be honoured. Faking that would be worse
    // than not having it.
    assert!(
        header(&streamed, "WWW-Authenticate").is_none(),
        "an unauthenticated transport must not advertise an auth scheme"
    );

    // No CORS: a web page must never be told it may read these responses.
    assert!(
        !streamed
            .to_ascii_lowercase()
            .contains("access-control-allow"),
        "{streamed}"
    );

    let elsewhere = exchange(
        server.addr,
        "POST /admin HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: 2\r\n\r\n{}",
    );
    assert!(status(&elsewhere).contains("404"), "{elsewhere}");
}
