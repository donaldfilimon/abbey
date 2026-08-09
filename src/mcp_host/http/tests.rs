//! Boundary tests for the loopback HTTP transport.
//!
//! Two layers, deliberately:
//!
//! * **Pure** tests drive [`respond`] with a hand-built [`HttpRequest`]. No
//!   socket, no thread, no timing — so the routing, authority, and session rules
//!   are asserted deterministically.
//! * **Socket** tests run a real listener on an ephemeral port. They exist only
//!   for the properties a socket has and a function call does not: the accept
//!   thread's admission control, and a peer that stops talking mid-request.
//!
//! Every socket test builds its listener with a deliberately tiny [`HttpConfig`]
//! rather than the shipped constants. Proving the cap works with a cap of 1 is
//! the same proof as with a cap of 8, and it does not cost the suite eight
//! sockets and ten seconds of waiting.

use std::io::{Read, Write};

use serde_json::{Value, json};

use super::*;
use crate::mcp_host::limits::{MAX_HTTP_HEAD_BYTES, MAX_HTTP_HEADERS};
use crate::mcp_host::tests::{EXECUTION_CAPABILITY_PHRASES, EXECUTION_NAME_MARKERS};
use crate::mcp_host::tools::{self, EffectClass, SAFE_TOOLS};

// ---------------------------------------------------------------- pure layer

/// Build a request the way [`wire::read_request`] would have parsed one.
fn request(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> HttpRequest {
    HttpRequest {
        method: method.to_ascii_uppercase(),
        path: path.to_owned(),
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_ascii_lowercase(), (*value).to_owned()))
            .collect(),
        body: body.to_vec(),
    }
}

/// A `POST /mcp` from a well-behaved loopback client carrying `frame`.
fn post_frame(frame: &Value) -> HttpRequest {
    request(
        "POST",
        ENDPOINT_PATH,
        &[
            ("host", "127.0.0.1:8787"),
            ("content-type", "application/json"),
        ],
        serde_json::to_vec(frame)
            .expect("frame serializes")
            .as_ref(),
    )
}

fn initialize_frame() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {}}
    })
}

/// Value of a response header, matched case-insensitively.
fn header<'a>(response: &'a HttpResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Run `initialize` against a fresh table and hand back the minted session id.
fn open_session(sessions: &SessionTable) -> String {
    let response = respond(&post_frame(&initialize_frame()), sessions);
    assert_eq!(response.status, 200, "{}", response.body_str());
    header(&response, SESSION_HEADER)
        .expect("initialize mints a session id")
        .to_owned()
}

fn body_json(response: &HttpResponse) -> Value {
    serde_json::from_str(&response.body_str()).expect("response body is JSON")
}

#[test]
fn a_non_loopback_bind_is_a_hard_error_not_a_warning() {
    for host in [
        "127.0.0.1",
        "127.0.0.53",
        "::1",
        "[::1]",
        "localhost",
        "LOCALHOST",
    ] {
        let addr = loopback_bind_addr(host, 0).unwrap_or_else(|error| panic!("`{host}`: {error}"));
        assert!(addr.ip().is_loopback(), "`{host}` resolved to {addr}");
    }
    // A hostname is refused outright rather than resolved, so a name that
    // happens to point at 127.0.0.1 today cannot smuggle a binding through.
    for host in [
        "0.0.0.0",
        "::",
        "192.168.1.5",
        "10.0.0.1",
        "evil.com",
        "localhost.evil.com",
        "127.0.0.1.evil.com",
        "",
    ] {
        let error = loopback_bind_addr(host, 0)
            .expect_err("`{host}` must be refused")
            .to_string();
        assert!(
            error.contains("loopback only"),
            "`{host}` was refused for the wrong reason: {error}"
        );
    }
}

#[test]
fn a_hostile_origin_is_rejected_before_any_dispatch() {
    let sessions = SessionTable::new(4, Duration::from_secs(300));
    let body = serde_json::to_vec(&initialize_frame()).expect("frame serializes");

    for origin in [
        "http://evil.com",
        "http://127.0.0.1.evil.com",
        "http://localhost.evil.com",
        "https://127.0.0.1:8787",
        "null",
    ] {
        let hostile = request(
            "POST",
            ENDPOINT_PATH,
            &[
                ("host", "127.0.0.1:8787"),
                ("content-type", "application/json"),
                ("origin", origin),
            ],
            &body,
        );
        let response = respond(&hostile, &sessions);
        assert_eq!(response.status, 403, "origin `{origin}` must be refused");
        assert!(response.body_str().contains("DNS-rebinding defense"));
    }

    // A rebound request carries the attacker's name in `Host` and no `Origin`
    // at all. The `Host` layer is what catches that one.
    let rebound = request(
        "POST",
        ENDPOINT_PATH,
        &[
            ("host", "attacker.example"),
            ("content-type", "application/json"),
        ],
        &body,
    );
    assert_eq!(respond(&rebound, &sessions).status, 403);

    // No hostile request may have opened a session on the way to its refusal.
    assert_eq!(sessions.len(), 0);
}

#[test]
fn the_http_advertised_tool_list_is_identical_to_the_stdio_one() {
    // stdio: drive a bare Server exactly as `serve()` would.
    let mut stdio = Server::new();
    stdio
        .handle_frame(&serde_json::to_vec(&initialize_frame()).expect("frame serializes"))
        .expect("initialize answers");
    let listed = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let over_stdio = stdio
        .handle_frame(&serde_json::to_vec(&listed).expect("frame serializes"))
        .expect("tools/list answers");

    // http: the same exchange through the transport.
    let sessions = SessionTable::new(4, Duration::from_secs(300));
    let id = open_session(&sessions);
    let mut listing = post_frame(&listed);
    listing
        .headers
        .push(("mcp-session-id".to_owned(), id.clone()));
    let response = respond(&listing, &sessions);
    assert_eq!(response.status, 200, "{}", response.body_str());
    let over_http = body_json(&response);

    assert_eq!(
        over_http["result"], over_stdio["result"],
        "the two transports must advertise the same registry"
    );
    assert_eq!(
        over_http["result"],
        tools::list_payload(SAFE_TOOLS),
        "and it must be the shipped safe registry, not a transport-local copy"
    );

    // Nothing execution-capable is reachable over HTTP. Judged by the *same*
    // markers the stdio test uses — the point is one bar for both transports,
    // not a second opinion.
    let advertised = over_http["result"]["tools"]
        .as_array()
        .expect("tools is an array");
    assert!(!advertised.is_empty(), "the registry must advertise tools");
    for tool in advertised {
        let name = tool["name"]
            .as_str()
            .expect("every tool has a name")
            .to_ascii_lowercase();
        for marker in EXECUTION_NAME_MARKERS {
            assert!(
                !name.contains(marker),
                "HTTP advertised `{name}`, which contains the execution marker `{marker}`"
            );
        }
        let description = tool["description"]
            .as_str()
            .expect("every tool has a description")
            .to_ascii_lowercase();
        for phrase in EXECUTION_CAPABILITY_PHRASES {
            assert!(
                !description.contains(phrase),
                "HTTP advertised `{name}`, which describes the capability `{phrase}`"
            );
        }
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "{name}");
        assert_eq!(tool["annotations"]["destructiveHint"], false, "{name}");
        assert_eq!(tool["annotations"]["openWorldHint"], false, "{name}");
    }

    // The structural invariant underneath all of it: the registry's effect type
    // still admits read-only tools only, so no HTTP-reachable tool can execute.
    for tool in SAFE_TOOLS {
        assert_eq!(tool.effect, EffectClass::ReadOnly, "{}", tool.name);
    }
}

#[test]
fn the_http_body_is_produced_by_the_shared_encode_frame() {
    // Not a style preference: if the transport ever serialized its own body it
    // would silently lose outbound redaction and MAX_RESPONSE_BYTES. Pin the
    // byte-level identity so that regression cannot pass.
    let sessions = SessionTable::new(4, Duration::from_secs(300));
    let id = open_session(&sessions);
    let ping = json!({"jsonrpc": "2.0", "id": 7, "method": "ping"});
    let mut over_http = post_frame(&ping);
    over_http
        .headers
        .push(("mcp-session-id".to_owned(), id.clone()));
    let response = respond(&over_http, &sessions);

    let mut reference = Server::new();
    reference
        .handle_frame(&serde_json::to_vec(&initialize_frame()).expect("frame serializes"))
        .expect("initialize answers");
    let answered = reference
        .handle_frame(&serde_json::to_vec(&ping).expect("frame serializes"))
        .expect("ping answers");
    assert_eq!(response.body_str(), reference.encode_frame(&answered));
}

#[test]
fn only_post_on_the_mcp_endpoint_reaches_the_dispatch() {
    let sessions = SessionTable::new(4, Duration::from_secs(300));
    let loopback = [("host", "127.0.0.1:8787")];

    // No SSE stream: GET is refused rather than half-implemented.
    let get = respond(&request("GET", ENDPOINT_PATH, &loopback, b""), &sessions);
    assert_eq!(get.status, 405);
    assert!(get.body_str().contains("Server-Sent Events"));

    let elsewhere = respond(&request("POST", "/admin", &loopback, b"{}"), &sessions);
    assert_eq!(elsewhere.status, 404);

    let untyped = respond(&request("POST", ENDPOINT_PATH, &loopback, b"{}"), &sessions);
    assert_eq!(untyped.status, 415, "a missing Content-Type is refused");
}

#[test]
fn a_request_without_a_session_is_refused_rather_than_silently_reinitialized() {
    let sessions = SessionTable::new(4, Duration::from_secs(300));
    let orphan = post_frame(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}));
    let response = respond(&orphan, &sessions);
    assert_eq!(response.status, 400);
    assert!(response.body_str().contains(SESSION_HEADER));

    // A stale id is a 404, not a fresh session: skipping the handshake is MCP's
    // stateless-lifecycle revision, which this server does not claim.
    let mut stale = post_frame(&json!({"jsonrpc": "2.0", "id": 4, "method": "ping"}));
    stale
        .headers
        .push(("mcp-session-id".to_owned(), "not-a-session".to_owned()));
    assert_eq!(respond(&stale, &sessions).status, 404);
}

#[test]
fn a_session_can_be_released_and_is_then_gone() {
    let sessions = SessionTable::new(4, Duration::from_secs(300));
    let id = open_session(&sessions);
    assert_eq!(sessions.len(), 1);

    let release = request(
        "DELETE",
        ENDPOINT_PATH,
        &[("host", "127.0.0.1:8787"), ("mcp-session-id", &id)],
        b"",
    );
    assert_eq!(respond(&release, &sessions).status, 204);
    assert_eq!(sessions.len(), 0);
    assert_eq!(respond(&release, &sessions).status, 404);
}

#[test]
fn a_body_past_the_shared_request_ceiling_is_rejected_before_it_is_read() {
    // Defence in depth at the `respond` layer; `wire` refuses it earlier still,
    // which the socket test below covers.
    let sessions = SessionTable::new(4, Duration::from_secs(300));
    let oversized = request(
        "POST",
        ENDPOINT_PATH,
        &[
            ("host", "127.0.0.1:8787"),
            ("content-type", "application/json"),
        ],
        &vec![b'x'; MAX_REQUEST_BYTES + 1],
    );
    let response = respond(&oversized, &sessions);
    assert_eq!(response.status, 413);
    assert!(response.body_str().contains(&MAX_REQUEST_BYTES.to_string()));
}

// -------------------------------------------------------------- socket layer

/// Config for a socket test: tiny caps, short timeouts, generous elsewhere.
fn tight(max_concurrent: usize, rate_budget: u32, read_timeout: Duration) -> HttpConfig {
    HttpConfig {
        read_timeout,
        write_timeout: Duration::from_secs(2),
        max_concurrent,
        rate_budget,
        rate_window: Duration::from_secs(60),
        max_sessions: 4,
        session_idle: Duration::from_secs(300),
    }
}

fn listener(config: HttpConfig) -> Running {
    HttpServer::bind_with("127.0.0.1", 0, config)
        .expect("loopback bind on an ephemeral port")
        .spawn()
        .expect("accept thread starts")
}

/// Send raw bytes, read until the server closes, return the whole response.
fn raw(addr: SocketAddr, bytes: &[u8]) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect to the test listener");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set a read timeout");
    stream.write_all(bytes).expect("write the request");
    stream.flush().expect("flush the request");
    let mut response = Vec::new();
    // The server always answers `Connection: close`, so EOF delimits the body.
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).into_owned()
}

/// A well-formed `POST /mcp` carrying `frame`.
fn post(addr: SocketAddr, frame: &str) -> String {
    raw(
        addr,
        format!(
            "POST {ENDPOINT_PATH} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: \
             application/json\r\nContent-Length: {}\r\n\r\n{frame}",
            frame.len()
        )
        .as_bytes(),
    )
}

fn status_line(response: &str) -> &str {
    response.lines().next().unwrap_or_default()
}

/// Spin until `probe` holds, so a test never races the accept thread.
fn wait_until(mut probe: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if probe() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn connections_past_the_concurrency_cap_are_refused_and_the_server_recovers() {
    let server = listener(tight(1, 1000, Duration::from_millis(400)));
    let addr = server.addr();

    // Hold the only permit: connect and say nothing. The worker sits in
    // `read_request` until its read deadline expires.
    let stalled = TcpStream::connect(addr).expect("occupy the single permit");
    wait_until(
        || server.in_flight() == 1,
        "the first connection to be admitted",
    );

    let refused = raw(addr, b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(
        status_line(&refused).contains("503"),
        "past the cap the server must refuse, got: {refused}"
    );
    assert!(refused.contains("concurrency cap of 1"), "{refused}");

    // The stalled peer times out, its permit drops, and service resumes.
    drop(stalled);
    wait_until(|| server.in_flight() == 0, "the permit to be returned");
    let accepted = post(addr, r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
    assert!(status_line(&accepted).contains("200"), "{accepted}");

    server.shutdown();
}

#[test]
fn requests_past_the_rate_budget_are_answered_429() {
    let server = listener(tight(4, 2, Duration::from_secs(2)));
    let addr = server.addr();
    let frame = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;

    assert!(
        status_line(&post(addr, frame)).contains("200"),
        "1st is inside the budget"
    );
    assert!(
        status_line(&post(addr, frame)).contains("200"),
        "2nd is exactly at the budget"
    );
    let refused = post(addr, frame);
    assert!(
        status_line(&refused).contains("429"),
        "3rd must be rate limited, got: {refused}"
    );
    assert!(refused.contains("rate limit exceeded"), "{refused}");

    server.shutdown();
}

#[test]
fn a_stalled_client_is_dropped_and_the_server_still_answers() {
    let server = listener(tight(4, 1000, Duration::from_millis(300)));
    let addr = server.addr();

    // Announce a body and never send it. Without an overall deadline this peer
    // would hold its permit forever.
    let stalled = raw(
        addr,
        b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
          Content-Length: 4096\r\n\r\n{",
    );
    assert!(
        status_line(&stalled).contains("408"),
        "a stalled body must time out, got: {stalled}"
    );

    let accepted = post(addr, r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
    assert!(status_line(&accepted).contains("200"), "{accepted}");

    server.shutdown();
}

#[test]
fn an_over_long_request_head_is_rejected() {
    let server = listener(tight(4, 1000, Duration::from_secs(2)));
    let addr = server.addr();
    let padding = "a".repeat(MAX_HTTP_HEAD_BYTES + 64);
    let response = raw(
        addr,
        format!("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Pad: {padding}\r\n\r\n").as_bytes(),
    );
    assert!(status_line(&response).contains("431"), "{response}");
    server.shutdown();
}

#[test]
fn too_many_header_lines_are_rejected() {
    let server = listener(tight(4, 1000, Duration::from_secs(2)));
    let addr = server.addr();
    let mut head = String::from("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\n");
    for index in 0..=MAX_HTTP_HEADERS {
        head.push_str(&format!("X-{index}: v\r\n"));
    }
    head.push_str("\r\n");
    let response = raw(addr, head.as_bytes());
    assert!(status_line(&response).contains("431"), "{response}");
    server.shutdown();
}

#[test]
fn a_declared_body_past_the_shared_ceiling_is_refused_without_reading_it() {
    let server = listener(tight(4, 1000, Duration::from_secs(2)));
    let addr = server.addr();
    // Only the head is sent. A 413 here proves the ceiling is enforced from
    // `Content-Length` alone, before a single body byte is allocated.
    let response = raw(
        addr,
        format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n",
            MAX_REQUEST_BYTES + 1
        )
        .as_bytes(),
    );
    assert!(status_line(&response).contains("413"), "{response}");
    server.shutdown();
}

#[test]
fn a_chunked_body_is_refused_rather_than_mis_parsed() {
    let server = listener(tight(4, 1000, Duration::from_secs(2)));
    let addr = server.addr();
    let response = raw(
        addr,
        b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
          Transfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
    );
    assert!(status_line(&response).contains("501"), "{response}");
    server.shutdown();
}

#[test]
fn a_hostile_origin_is_refused_over_a_real_socket() {
    let server = listener(tight(4, 1000, Duration::from_secs(2)));
    let addr = server.addr();
    let frame = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
    let response = raw(
        addr,
        format!(
            "POST {ENDPOINT_PATH} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: \
             http://127.0.0.1.evil.com\r\nContent-Type: application/json\r\nContent-Length: \
             {}\r\n\r\n{frame}",
            frame.len()
        )
        .as_bytes(),
    );
    assert!(status_line(&response).contains("403"), "{response}");
    assert!(response.contains("DNS-rebinding defense"), "{response}");
    server.shutdown();
}

#[test]
fn a_full_exchange_completes_over_the_socket() {
    let server = listener(tight(4, 1000, Duration::from_secs(2)));
    let addr = server.addr();

    let opened = post(
        addr,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    );
    assert!(status_line(&opened).contains("200"), "{opened}");
    let id = opened
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case(SESSION_HEADER))
                .map(|(_, value)| value.trim().to_owned())
        })
        .expect("initialize returns a session id");
    assert_eq!(server.sessions(), 1);

    let frame = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
    let listed = raw(
        addr,
        format!(
            "POST {ENDPOINT_PATH} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: \
             application/json\r\n{SESSION_HEADER}: {id}\r\nContent-Length: {}\r\n\r\n{frame}",
            frame.len()
        )
        .as_bytes(),
    );
    assert!(status_line(&listed).contains("200"), "{listed}");
    assert!(listed.contains("abbey_status"), "{listed}");

    server.shutdown();
}
