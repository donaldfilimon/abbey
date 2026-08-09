//! Abbey's own read-only **MCP server** over stdio and loopback HTTP.
//!
//! ## What this is, precisely
//!
//! `abbey mcp serve` speaks JSON-RPC 2.0 over newline-delimited stdin/stdout and
//! exposes a fixed, capability-scoped registry of Abbey's own read-only tools.
//! `abbey mcp serve http` offers the *same* registry over a **loopback-only**
//! Streamable HTTP POST endpoint. In MCP vocabulary that makes Abbey a
//! **server**: the thing that *offers* tools. It does **not** make Abbey a
//! host/client that consumes external MCP providers — that remains the Proposed
//! `runtime-provider-neutral-owned` claim, and `abbey mcp status` remains a
//! configuration inventory of other agents' MCP setups.
//!
//! ## One dispatch, two pipes
//!
//! [`Server::handle_frame`] routes every request and [`Server::encode_frame`]
//! produces every response body, for both transports. The HTTP module owns a
//! socket and an HTTP framing layer and nothing else, so the tool registry, the
//! byte/depth/timeout limits, and outbound secret redaction are shared code
//! rather than a parallel implementation that could drift.
//!
//! ## Hard security invariant
//!
//! No shell execution, no filesystem mutation, and no arbitrary-command tool is
//! registered or discoverable — over either transport. This is enforced
//! structurally, not by policy text: [`tools::EffectClass`] has one variant,
//! `ReadOnly`, so a mutating or executing tool cannot be *described*.
//! `tests/mcp_server.rs` asserts that the advertised `tools/list` contains no
//! execution-capable entry, and `tests/mcp_http.rs` asserts the HTTP-advertised
//! list is byte-identical to the stdio one.
//!
//! ## What this slice does not implement
//!
//! HTTPS/TLS, non-loopback binding, OAuth 2.1/PKCE, resource indicators,
//! token-audience binding, SSE/`GET` streaming with resumability, and the
//! stateless MCP lifecycle revision are all absent. They are deliberately left
//! out rather than stubbed; see `tasks/todo.md` Phase 9 and the
//! `mcp-server-http-loopback-readonly` claim. The HTTP transport is
//! **unauthenticated**: loopback-only bounds *who can reach it* (no off-host
//! caller), not *who may call it* (any local process may).

mod http;
mod jsonrpc;
mod limits;
mod redact;
mod serve;
mod tools;

#[cfg(test)]
mod tests;

pub use http::{ENDPOINT_PATH, SESSION_HEADER};
pub use limits::{
    DEFAULT_HTTP_PORT, HTTP_READ_TIMEOUT, HTTP_SESSION_IDLE_TIMEOUT, HTTP_WRITE_TIMEOUT,
    MAX_BATCH_SIZE, MAX_CONCURRENT_REQUESTS, MAX_HTTP_HEAD_BYTES, MAX_HTTP_HEADERS,
    MAX_HTTP_SESSIONS, MAX_JSON_DEPTH, MAX_REQUEST_BYTES, MAX_REQUEST_TIMEOUT,
    MAX_REQUESTS_PER_WINDOW, MAX_RESPONSE_BYTES, RATE_LIMIT_WINDOW,
};
pub use serve::{LATEST_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS, Server};
pub use tools::{SAFE_TOOLS, tool_names};

use anyhow::Result;

/// Run one stdio MCP session against the real stdin/stdout.
///
/// Nothing but JSON-RPC frames may reach stdout from here on.
pub fn serve_stdio() -> Result<i32> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut server = Server::new();
    server.serve(stdin.lock(), stdout.lock())?;
    Ok(0)
}

/// Human-readable description of the advertised registry (a normal CLI print —
/// never reached from either serve path).
pub fn print_registry() -> Result<i32> {
    println!("abbey mcp serve — read-only MCP server (Abbey as MCP *server*, not host)\n");
    println!(
        "protocol:  {} (also negotiates {})",
        LATEST_PROTOCOL_VERSION,
        SUPPORTED_PROTOCOL_VERSIONS.join(", ")
    );
    println!("transports:");
    println!("  stdio    newline-delimited JSON-RPC 2.0 — `abbey mcp serve`");
    println!(
        "  http     Streamable HTTP POST {ENDPOINT_PATH}, loopback only — \
         `abbey mcp serve http --port {DEFAULT_HTTP_PORT}`"
    );
    println!(
        "shared limits: request {MAX_REQUEST_BYTES} B · response {MAX_RESPONSE_BYTES} B · depth \
         {MAX_JSON_DEPTH} · batch {MAX_BATCH_SIZE} · timeout {} ms",
        MAX_REQUEST_TIMEOUT.as_millis()
    );
    println!(
        "http limits:   concurrency {MAX_CONCURRENT_REQUESTS} · {MAX_REQUESTS_PER_WINDOW} req / \
         {} s · read {} s · write {} s · head {MAX_HTTP_HEAD_BYTES} B · headers \
         {MAX_HTTP_HEADERS} · sessions {MAX_HTTP_SESSIONS} (idle {} s)",
        RATE_LIMIT_WINDOW.as_secs(),
        HTTP_READ_TIMEOUT.as_secs(),
        HTTP_WRITE_TIMEOUT.as_secs(),
        HTTP_SESSION_IDLE_TIMEOUT.as_secs()
    );
    println!("registry:  {}", tool_names().join(", "));
    println!("\nsafe tools (all read-only; no shell, no writes, no process spawn):");
    for tool in SAFE_TOOLS {
        println!("  {:<18} {}", tool.name, tool.title);
        println!("  {:<18} {}", "", tool.description);
    }
    println!(
        "\nStreamable HTTP is implemented for loopback only, and it is unauthenticated: any\n\
         local process may call it while it runs, and {SESSION_HEADER} is a routing key, not a\n\
         credential. A non-loopback bind is a hard error, not a warning. HTTPS/TLS, OAuth\n\
         2.1/PKCE, resource indicators, token-audience binding, SSE streaming/resumability,\n\
         and the stateless-lifecycle revision are NOT implemented (`abbey claims proposed`)."
    );
    Ok(0)
}

/// `abbey mcp serve …` dispatch.
pub fn dispatch(args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str) {
        None | Some("stdio") => serve_stdio(),
        Some("http") => http::serve_http(&args[1..]),
        Some("tools") | Some("list") | Some("info") => print_registry(),
        Some("-h") | Some("--help") | Some("help") => print_registry(),
        Some(other) => {
            anyhow::bail!(
                "unknown `abbey mcp serve` argument `{other}` — try: stdio|http|tools|--help"
            )
        }
    }
}
