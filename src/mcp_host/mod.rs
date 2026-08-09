//! Abbey's own read-only **MCP server** over stdio.
//!
//! ## What this is, precisely
//!
//! `abbey mcp serve` speaks JSON-RPC 2.0 over newline-delimited stdin/stdout and
//! exposes a fixed, capability-scoped registry of Abbey's own read-only tools.
//! In MCP vocabulary that makes Abbey a **server**: the thing that *offers*
//! tools. It does **not** make Abbey a host/client that consumes external MCP
//! providers — that remains the Proposed `runtime-provider-neutral-owned`
//! claim, and `abbey mcp status` remains a configuration inventory of other
//! agents' MCP setups.
//!
//! ## Hard security invariant
//!
//! No shell execution, no filesystem mutation, and no arbitrary-command tool is
//! registered or discoverable. This is enforced structurally, not by policy
//! text: [`tools::EffectClass`] has one variant, `ReadOnly`, so a mutating or
//! executing tool cannot be *described*, and `tests/mcp_server.rs` asserts that
//! the advertised `tools/list` contains no execution-capable entry.
//!
//! ## What this slice does not implement
//!
//! Streamable HTTP, TLS, OAuth 2.1/PKCE, resource indicators, token-audience
//! binding, rate limiting, and the stateless MCP lifecycle revision are all
//! absent. They are deliberately left out rather than stubbed; see
//! `tasks/todo.md` Phase 9 and the `mcp-server-stdio-readonly` claim.

mod jsonrpc;
mod limits;
mod redact;
mod serve;
mod tools;

#[cfg(test)]
mod tests;

pub use limits::{
    MAX_BATCH_SIZE, MAX_JSON_DEPTH, MAX_REQUEST_BYTES, MAX_REQUEST_TIMEOUT, MAX_RESPONSE_BYTES,
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
/// never reached from the serve path).
pub fn print_registry() -> Result<i32> {
    println!("abbey mcp serve — read-only stdio MCP server (Abbey as MCP *server*, not host)\n");
    println!(
        "protocol:  {} (also negotiates {})",
        LATEST_PROTOCOL_VERSION,
        SUPPORTED_PROTOCOL_VERSIONS.join(", ")
    );
    println!("transport: stdio, newline-delimited JSON-RPC 2.0 (no HTTP in this build)");
    println!(
        "limits:    request {MAX_REQUEST_BYTES} B · response {MAX_RESPONSE_BYTES} B · depth \
         {MAX_JSON_DEPTH} · batch {MAX_BATCH_SIZE} · timeout {} ms",
        MAX_REQUEST_TIMEOUT.as_millis()
    );
    println!("registry:  {}", tool_names().join(", "));
    println!("\nsafe tools (all read-only; no shell, no writes, no process spawn):");
    for tool in SAFE_TOOLS {
        println!("  {:<18} {}", tool.name, tool.title);
        println!("  {:<18} {}", "", tool.description);
    }
    println!(
        "\nnot implemented here: Streamable HTTP, TLS, OAuth 2.1/PKCE, resource indicators,\n\
         token-audience binding, rate limiting, stateless-lifecycle revision \
         (`abbey claims proposed`)."
    );
    Ok(0)
}

/// `abbey mcp serve …` dispatch.
pub fn dispatch(args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str) {
        None | Some("stdio") => serve_stdio(),
        Some("tools") | Some("list") | Some("info") => print_registry(),
        Some("-h") | Some("--help") | Some("help") => print_registry(),
        Some(other) => {
            anyhow::bail!("unknown `abbey mcp serve` argument `{other}` — try: stdio|tools|--help")
        }
    }
}
