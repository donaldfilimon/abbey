//! Abbey's **loopback-only** Streamable HTTP transport for the read-only MCP
//! server.
//!
//! ## Same dispatch, different pipe
//!
//! This module owns a socket and an HTTP framing layer, and nothing else. Every
//! request body is handed to the *same* [`Server::handle_frame`] the stdio
//! transport uses, and every **JSON-RPC** response body is produced by the same
//! [`Server::encode_frame`] — so protocol routing, the tool registry, the JSON
//! depth/size limits, the call timeout, and outbound secret redaction are shared
//! code, not a parallel implementation. `tests/mcp_http.rs` asserts the two
//! transports advertise a byte-identical `tools/list` payload.
//!
//! The exception, stated precisely because "every response" would be an
//! overclaim: transport-level refusals (403/404/405/413/415/429/431/501/503) are
//! built by [`HttpResponse::rpc_error`] *before* dispatch, from static text plus
//! at most an echo of the client's own request line. They never reach
//! `encode_frame`, and so carry neither the redaction pass nor
//! [`MAX_RESPONSE_BYTES`] — which is sound only because no tool output, no
//! environment value, and no server state can appear in them.
//!
//! ## Loopback is a bind restriction, not an authentication story
//!
//! The listener refuses to bind anything but `127.0.0.0/8`, `::1`, or
//! `localhost` — a non-loopback address is a hard error at startup, not a
//! warning. Because the socket is unreachable off-host, OAuth 2.1/PKCE is not
//! required for this transport and is deliberately **not faked**: there is no
//! token, no PKCE exchange, no resource indicator, and no audience binding
//! anywhere in this module.
//!
//! Be precise about what that buys. Loopback-only means *remote* callers cannot
//! reach it. It does **not** mean the endpoint is authenticated: any process or
//! user on this machine can call it while it is running, and the
//! `Mcp-Session-Id` is a routing key, not a credential. Non-loopback binding,
//! HTTPS/TLS, OAuth 2.1/PKCE, resource indicators, and token-audience binding
//! all remain Proposed and unimplemented.
//!
//! ## What a browser cannot do to it
//!
//! Loopback servers are attacked through the user's own browser via DNS
//! rebinding. [`origin`] closes that with parsed `Host` and `Origin` checks;
//! see that module for the reasoning and the hostile cases covered.
//!
//! ## Not implemented here
//!
//! No SSE/`GET` stream, no resumability, no pipelining or keep-alive (one
//! request per connection, `Connection: close`), no chunked request bodies, and
//! no `resources`/`prompts`/`tasks` capabilities.

mod gate;
mod origin;
mod session;
mod wire;

#[cfg(test)]
mod tests;

use std::io::BufReader;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
#[cfg(test)]
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::Server;
use super::jsonrpc::{self, INVALID_REQUEST, REQUEST_TOO_LARGE};
use super::limits::{
    DEFAULT_HTTP_PORT, HTTP_READ_TIMEOUT, HTTP_SESSION_IDLE_TIMEOUT, HTTP_WRITE_TIMEOUT,
    MAX_CONCURRENT_REQUESTS, MAX_HTTP_SESSIONS, MAX_REQUEST_BYTES, MAX_REQUESTS_PER_WINDOW,
    RATE_LIMIT_WINDOW,
};
use gate::{ConcurrencyGate, RateLimiter};
use session::SessionTable;
use wire::{HttpError, HttpRequest, ReadOutcome};

/// The single endpoint path, plus `/` as an alias for convenience.
pub const ENDPOINT_PATH: &str = "/mcp";

/// Header carrying the MCP session identifier, per the Streamable HTTP spec.
pub const SESSION_HEADER: &str = "Mcp-Session-Id";

/// Tunable bounds for one listener.
///
/// Defaults are the documented constants in [`super::limits`]. They are fields
/// rather than direct constant reads for one reason: a test must be able to
/// build a listener with a cap of 2 and a one-second timeout, or the boundary
/// tests would have to open 8 sockets and wait 10 seconds to prove anything.
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub max_concurrent: usize,
    pub rate_budget: u32,
    pub rate_window: Duration,
    pub max_sessions: usize,
    pub session_idle: Duration,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            read_timeout: HTTP_READ_TIMEOUT,
            write_timeout: HTTP_WRITE_TIMEOUT,
            max_concurrent: MAX_CONCURRENT_REQUESTS,
            rate_budget: MAX_REQUESTS_PER_WINDOW,
            rate_window: RATE_LIMIT_WINDOW,
            max_sessions: MAX_HTTP_SESSIONS,
            session_idle: HTTP_SESSION_IDLE_TIMEOUT,
        }
    }
}

/// Resolve a requested bind address, refusing anything that is not loopback.
///
/// This is the hard error the whole transport rests on. Names are **not**
/// resolved: `localhost` is special-cased because it is reserved to loopback,
/// and any other hostname is rejected outright rather than looked up, so a name
/// that happens to resolve to `127.0.0.1` today cannot smuggle in a binding this
/// server would have to re-validate tomorrow.
pub fn loopback_bind_addr(host: &str, port: u16) -> Result<SocketAddr> {
    let raw = host.trim();
    let candidate = raw
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(raw);
    if candidate.eq_ignore_ascii_case("localhost") {
        return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }
    let Ok(address) = candidate.parse::<IpAddr>() else {
        bail!(
            "refusing to bind `{raw}`: the MCP HTTP transport binds loopback only \
             (127.0.0.1, ::1, or localhost). Hostnames are not resolved. \
             Non-loopback binding is Proposed and unimplemented — it would require TLS \
             and OAuth 2.1, neither of which exists here."
        );
    };
    if !address.is_loopback() {
        bail!(
            "refusing to bind `{raw}`: the MCP HTTP transport binds loopback only \
             (127.0.0.1, ::1, or localhost). Exposing this unauthenticated read-only \
             server off-host is Proposed and unimplemented — it would require TLS and \
             OAuth 2.1, neither of which exists here."
        );
    }
    Ok(SocketAddr::new(address, port))
}

/// A bound-but-not-yet-running loopback MCP listener.
pub struct HttpServer {
    listener: TcpListener,
    config: HttpConfig,
    gate: Arc<ConcurrencyGate>,
    limiter: Arc<RateLimiter>,
    sessions: Arc<SessionTable>,
    stopping: Arc<AtomicBool>,
}

impl HttpServer {
    /// Bind with the shipped limits.
    pub fn bind(host: &str, port: u16) -> Result<Self> {
        Self::bind_with(host, port, HttpConfig::default())
    }

    /// Bind with explicit limits (used by the boundary tests).
    pub fn bind_with(host: &str, port: u16, config: HttpConfig) -> Result<Self> {
        let addr = loopback_bind_addr(host, port)?;
        let listener = TcpListener::bind(addr)
            .with_context(|| format!("cannot bind the MCP HTTP transport on {addr}"))?;
        Ok(Self {
            listener,
            gate: Arc::new(ConcurrencyGate::new(config.max_concurrent)),
            limiter: Arc::new(RateLimiter::new(config.rate_budget, config.rate_window)),
            sessions: Arc::new(SessionTable::new(config.max_sessions, config.session_idle)),
            stopping: Arc::new(AtomicBool::new(false)),
            config,
        })
    }

    /// The address actually bound — meaningful when `port` was 0.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// Accept until the process is stopped. Never returns in normal operation.
    pub fn run(self) -> Result<()> {
        self.accept_loop();
        Ok(())
    }

    /// Run on a background thread and hand back a controllable handle.
    ///
    /// Test-only, exactly like [`Server::with_tools`]: the shipped path is
    /// [`Self::run`], which owns the process until it is stopped.
    #[cfg(test)]
    pub fn spawn(self) -> Result<Running> {
        let addr = self.local_addr()?;
        let stopping = Arc::clone(&self.stopping);
        let gate = Arc::clone(&self.gate);
        let sessions = Arc::clone(&self.sessions);
        let join = std::thread::Builder::new()
            .name("abbey-mcp-http-accept".into())
            .spawn(move || self.accept_loop())?;
        Ok(Running {
            addr,
            stopping,
            gate,
            sessions,
            join: Some(join),
        })
    }

    fn accept_loop(self) {
        loop {
            let stream = match self.listener.accept() {
                Ok((stream, _peer)) => stream,
                Err(_) => {
                    if self.stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    continue;
                }
            };
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(self.config.read_timeout));
            let _ = stream.set_write_timeout(Some(self.config.write_timeout));

            // Admission control happens on the accept thread, before any worker
            // exists, so a refused peer costs one inline write and no thread.
            if !self.limiter.try_admit() {
                self.refuse(
                    stream,
                    429,
                    format!(
                        "rate limit exceeded: at most {} requests per {} s",
                        self.limiter.budget(),
                        self.limiter.window().as_secs()
                    ),
                );
                continue;
            }
            let Some(permit) = self.gate.try_acquire() else {
                self.refuse(
                    stream,
                    503,
                    format!(
                        "server is at its concurrency cap of {} in-flight requests",
                        self.gate.capacity()
                    ),
                );
                continue;
            };

            let sessions = Arc::clone(&self.sessions);
            let config = self.config.clone();
            let spawned = std::thread::Builder::new()
                .name("abbey-mcp-http".into())
                .spawn(move || {
                    // Held for the whole connection: read, dispatch, and write.
                    let _permit = permit;
                    serve_connection(stream, &sessions, &config);
                });
            if let Err(error) = spawned {
                eprintln!("abbey mcp http: cannot start a connection worker: {error}");
            }
        }
    }

    /// Answer an admission-control refusal inline and close.
    fn refuse(&self, mut stream: TcpStream, status: u16, message: String) {
        let response = HttpResponse::rpc_error(status, INVALID_REQUEST, &message);
        let _ = wire::write_all_deadline(&mut stream, &response.bytes(), self.config.write_timeout);
    }
}

/// Handle to a listener running on its own thread (test-only).
#[cfg(test)]
pub struct Running {
    addr: SocketAddr,
    stopping: Arc<AtomicBool>,
    gate: Arc<ConcurrencyGate>,
    sessions: Arc<SessionTable>,
    join: Option<JoinHandle<()>>,
}

#[cfg(test)]
impl Running {
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.gate.in_flight()
    }

    #[must_use]
    pub fn sessions(&self) -> usize {
        self.sessions.len()
    }

    /// Stop accepting and join the accept thread.
    ///
    /// `accept()` is blocking, so the flag alone would not wake it — a throwaway
    /// loopback connection does.
    pub fn shutdown(mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(test)]
impl Drop for Running {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
    }
}

/// One connection: read a single request, answer it, close.
fn serve_connection(stream: TcpStream, sessions: &SessionTable, config: &HttpConfig) {
    let deadline = Instant::now() + config.read_timeout;
    let mut reader = BufReader::new(&stream);
    let outcome = wire::read_request(&mut reader, deadline);
    drop(reader);

    let response = match outcome {
        ReadOutcome::Disconnected => return,
        ReadOutcome::Rejected(error) => HttpResponse::from_http_error(&error),
        ReadOutcome::Request(request) => respond(&request, sessions),
    };
    let mut writer = &stream;
    let _ = wire::write_all_deadline(&mut writer, &response.bytes(), config.write_timeout);
}

/// A fully-determined HTTP response.
struct HttpResponse {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn rpc_error(status: u16, code: i64, message: &str) -> Self {
        let body = serde_json::to_vec(&jsonrpc::error(None, code, message))
            .unwrap_or_else(|_| b"{}".to_vec());
        Self {
            status,
            headers: Vec::new(),
            body,
        }
    }

    fn from_http_error(error: &HttpError) -> Self {
        let code = if error.status == 413 {
            REQUEST_TOO_LARGE
        } else {
            INVALID_REQUEST
        };
        Self::rpc_error(error.status, code, &error.message)
    }

    fn bytes(&self) -> Vec<u8> {
        wire::response_bytes(self.status, &self.headers, &self.body)
    }

    #[cfg(test)]
    fn body_str(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Just enough of a JSON-RPC frame to route it before dispatch.
#[derive(serde::Deserialize)]
struct MethodPeek {
    #[serde(default)]
    method: Option<String>,
}

/// The `method` of a well-formed single JSON-RPC request, if there is one.
fn peek_method(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<MethodPeek>(body)
        .ok()
        .and_then(|peek| peek.method)
}

/// Turn one parsed request into a response. Pure apart from the session table,
/// which is what makes the transport testable without a socket.
fn respond(request: &HttpRequest, sessions: &SessionTable) -> HttpResponse {
    if let Err(rejection) = origin::check(request.header("host"), request.header("origin")) {
        return HttpResponse::rpc_error(403, INVALID_REQUEST, rejection.message());
    }
    if request.path != ENDPOINT_PATH && request.path != "/" {
        return HttpResponse::rpc_error(
            404,
            INVALID_REQUEST,
            &format!(
                "unknown path `{}` — the MCP endpoint is {ENDPOINT_PATH}",
                request.path
            ),
        );
    }
    match request.method.as_str() {
        "POST" => post(request, sessions),
        "DELETE" => match request.header("mcp-session-id") {
            Some(id) if sessions.remove(id) => HttpResponse {
                status: 204,
                headers: Vec::new(),
                body: Vec::new(),
            },
            _ => HttpResponse::rpc_error(
                404,
                INVALID_REQUEST,
                "unknown or already-released Mcp-Session-Id",
            ),
        },
        "GET" => HttpResponse::rpc_error(
            405,
            INVALID_REQUEST,
            "GET is not supported: this transport implements POST only. Server-Sent Events \
             streaming and resumability are not implemented.",
        ),
        other => HttpResponse::rpc_error(
            405,
            INVALID_REQUEST,
            &format!("method `{other}` is not supported — use POST {ENDPOINT_PATH}"),
        ),
    }
}

fn post(request: &HttpRequest, sessions: &SessionTable) -> HttpResponse {
    let content_type = request.header("content-type").unwrap_or_default();
    if !content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("application/json")
    {
        return HttpResponse::rpc_error(
            415,
            INVALID_REQUEST,
            "Content-Type must be application/json",
        );
    }
    if request.body.is_empty() {
        return HttpResponse::rpc_error(400, INVALID_REQUEST, "empty request body");
    }
    // Defence in depth: `wire::read_request` already refuses to read this many
    // bytes, so reaching here would mean the framing layer regressed.
    if request.body.len() > MAX_REQUEST_BYTES {
        return HttpResponse::rpc_error(
            413,
            REQUEST_TOO_LARGE,
            &format!("request body exceeds {MAX_REQUEST_BYTES} bytes"),
        );
    }

    let method = peek_method(&request.body);
    let supplied = request.header("mcp-session-id");
    let (server, minted) = match method.as_deref() {
        Some("initialize") => match sessions.create() {
            Some((id, server)) => (server, Some(id)),
            None => {
                return HttpResponse::rpc_error(
                    503,
                    INVALID_REQUEST,
                    &format!(
                        "no free MCP session slots (cap {}); release one with DELETE {ENDPOINT_PATH}",
                        sessions.capacity()
                    ),
                );
            }
        },
        _ => match supplied {
            Some(id) => match sessions.get(id) {
                Some(server) => (server, None),
                None => {
                    return HttpResponse::rpc_error(
                        404,
                        INVALID_REQUEST,
                        "unknown or expired Mcp-Session-Id — send `initialize` again",
                    );
                }
            },
            // A frame we could not even read a method out of is a protocol
            // error, not a missing-header error: answer it with the JSON-RPC
            // diagnostic the shared dispatch produces rather than a transport
            // complaint the client cannot act on.
            None if method.is_none() => (Arc::new(Mutex::new(Server::new())), None),
            None => {
                return HttpResponse::rpc_error(
                    400,
                    INVALID_REQUEST,
                    &format!(
                        "missing {SESSION_HEADER} — POST `initialize` first and echo the \
                         {SESSION_HEADER} it returns"
                    ),
                );
            }
        },
    };

    let mut guard = server.lock().unwrap_or_else(PoisonError::into_inner);
    let dispatched = guard.handle_frame(&request.body);
    let mut headers: Vec<(&'static str, String)> = Vec::new();
    if let Some(id) = minted {
        headers.push((SESSION_HEADER, id));
    }
    match dispatched {
        // A notification produces no JSON-RPC response; Streamable HTTP says to
        // acknowledge it with 202 and an empty body.
        None => HttpResponse {
            status: 202,
            headers,
            body: Vec::new(),
        },
        Some(response) => HttpResponse {
            status: 200,
            headers,
            body: guard.encode_frame(&response).into_bytes(),
        },
    }
}

/// `abbey mcp serve http [--host H] [--port P]`.
pub fn serve_http(args: &[String]) -> Result<i32> {
    let mut host = "127.0.0.1".to_owned();
    let mut port = DEFAULT_HTTP_PORT;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--host" | "--bind" => {
                index += 1;
                host = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("`--host` needs a value"))?;
            }
            "--port" => {
                index += 1;
                port = args
                    .get(index)
                    .ok_or_else(|| anyhow::anyhow!("`--port` needs a value"))?
                    .parse::<u16>()
                    .context("`--port` must be a number in 0..=65535")?;
            }
            other => bail!(
                "unknown `abbey mcp serve http` argument `{other}` — try: --host <loopback> --port <n>"
            ),
        }
        index += 1;
    }

    let server = HttpServer::bind(&host, port)?;
    let addr = server.local_addr()?;
    // stderr, never stdout: keeping the two transports' diagnostics on the same
    // channel means no surface has to remember which one it is on.
    eprintln!("abbey mcp http: listening on http://{addr}{ENDPOINT_PATH}");
    eprintln!(
        "abbey mcp http: loopback only · unauthenticated (no TLS, no OAuth 2.1/PKCE) · \
         read-only tools: {}",
        super::tool_names().join(", ")
    );
    server.run()?;
    Ok(0)
}
