//! MCP configuration/provider views and ACP peer launch helpers.
//!
//! Abbey is not an MCP or ACP **client/host** runtime: it does not connect out
//! to other providers' MCP servers or route model work through them. It *does*
//! serve its own read-only tools — see [`crate::mcp_host`] and `abbey mcp
//! serve`, which is a separate code path that never prints to stdout.
//!
//! Local MCP inventory is available under every Abbey model backend; mutations
//! are routed only after the caller names a concrete external provider.

pub mod mcp;

use crate::agent::{AgentConfig, which_bin};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub use mcp::{McpProvider, load_mcp_inventory, load_mcp_servers, mcp_config_sources};

#[derive(Debug, Clone)]
pub struct AcpPeer {
    pub name: &'static str,
    pub bin: &'static str,
    /// Args that start ACP stdio mode.
    pub acp_args: &'static [&'static str],
    pub path: Option<PathBuf>,
    pub note: &'static str,
}

pub fn print_mcp_status(cwd: &Path) -> Result<i32> {
    println!(
        "abbey mcp — configured inventory of other agents' MCP servers\n\
         (Abbey does not connect to these; for Abbey's own read-only MCP server \
         run `abbey mcp serve`)"
    );
    println!("cwd: {}", cwd.display());
    let inventory = load_mcp_inventory(cwd);
    if inventory.groups.is_empty() && inventory.diagnostics.is_empty() {
        println!("(no MCP configuration files found)");
        println!("tip: abbey mcp paths");
        return Ok(0);
    }

    let mut total = 0usize;
    for group in &inventory.groups {
        println!(
            "\n{}  [{}; {} server(s)]",
            group.source.path.display(),
            group
                .source
                .providers
                .iter()
                .map(|provider| provider.label())
                .collect::<Vec<_>>()
                .join("+"),
            group.servers.len()
        );
        if group.servers.is_empty() {
            println!("  (configured file has no MCP servers)");
        }
        for server in &group.servers {
            total += 1;
            let state = if server.disabled {
                "configured, disabled"
            } else {
                "configured, enabled"
            };
            let arguments = if server.args.is_empty() {
                ""
            } else {
                " (arguments redacted)"
            };
            println!(
                "  {:<24} {:<20} {:<10} {}{} [{}; {}]",
                server.name,
                state,
                server.transport.label(),
                server.safe_target(),
                arguments,
                server.provider.label(),
                server.source.display()
            );
        }
    }
    for diagnostic in &inventory.diagnostics {
        eprintln!(
            "warning: skipped malformed MCP config {}: {}",
            diagnostic.path.display(),
            diagnostic.message
        );
    }
    println!("\ntotal configured entries: {total}");
    println!("management: abbey mcp <cursor|codex|claude> <list|enable|disable|login …>");
    println!("runtime: provider-owned; Abbey is an inventory/router, not an MCP host");
    Ok(0)
}

pub fn print_mcp_paths(cwd: &Path) -> Result<i32> {
    for source in mcp_config_sources(cwd) {
        let mark = if source.path.is_file() {
            "ok "
        } else {
            "—  "
        };
        println!(
            "{mark} {:<14} {}",
            source
                .providers
                .iter()
                .map(|provider| provider.label())
                .collect::<Vec<_>>()
                .join("+"),
            source.path.display()
        );
    }
    Ok(0)
}

/// Known Agent Client Protocol peers Abbey can discover / launch.
pub fn acp_peers() -> Vec<AcpPeer> {
    vec![
        AcpPeer {
            name: "gemini",
            bin: "gemini",
            acp_args: &["--acp"],
            path: which_bin("gemini"),
            note: "Google Gemini CLI ACP stdio server",
        },
        AcpPeer {
            name: "opencode",
            bin: "opencode",
            acp_args: &["acp"],
            path: which_bin("opencode"),
            note: "OpenCode `acp` subcommand",
        },
    ]
}

pub fn print_acp_status() -> Result<i32> {
    println!("abbey acp — Agent Client Protocol peer inventory");
    println!("note: Abbey does not host ACP sessions; it discovers/launches peer servers.\n");
    println!("{:<12} {:<10} detail", "peer", "status");
    for peer in acp_peers() {
        match &peer.path {
            Some(path) => {
                println!(
                    "{:<12} {:<10} {} — {} {}",
                    peer.name,
                    "present",
                    path.display(),
                    peer.bin,
                    peer.acp_args.join(" ")
                );
                println!("{:<12} {:<10} {}", "", "", peer.note);
            }
            None => println!("{:<12} {:<10} {}", peer.name, "missing", peer.note),
        }
    }
    println!(
        "\nlaunch: abbey acp run gemini|opencode\n\
         note: abi-mcp is MCP, not an ACP peer, and is intentionally absent here"
    );
    Ok(0)
}

/// Run an ACP peer in stdio mode (foreground — for IDE/host attachment).
pub fn run_acp_peer(name: &str) -> Result<i32> {
    let peer = acp_peers()
        .into_iter()
        .find(|peer| peer.name.eq_ignore_ascii_case(name))
        .with_context(|| format!("unknown ACP peer `{name}` (try: abbey acp list)"))?;
    let binary = peer
        .path
        .clone()
        .with_context(|| format!("{} not on PATH", peer.bin))?;
    eprintln!(
        "abbey: starting ACP stdio server: {} {}\n\
         (attach an ACP host to this process; Ctrl+C to stop)",
        peer.bin,
        peer.acp_args.join(" ")
    );
    let status = Command::new(binary)
        .args(peer.acp_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(status.code().unwrap_or(1))
}

/// Compatibility entry point used by current command dispatch.
///
/// `cfg` intentionally does not gate local status/paths: those are filesystem
/// reads and remain valid under `fm`, `abi`, and `claude`. Provider management is selected
/// from the arguments, never inferred from the active generation backend.
pub fn dispatch_mcp(_cfg: &AgentConfig, cwd: &Path, args: &[String]) -> Result<i32> {
    if args.is_empty() {
        return print_mcp_status(cwd);
    }
    match args[0].as_str() {
        "status" | "inventory" | "show" => print_mcp_status(cwd),
        "paths" => print_mcp_paths(cwd),
        // Abbey's own read-only MCP *server*. Nothing but JSON-RPC frames may
        // reach stdout past this point, so it shares no code with the printing
        // inventory surfaces above.
        "serve" | "server" | "stdio" => crate::mcp_host::dispatch(&args[1..]),
        "tools" => crate::mcp_host::print_registry(),
        "host" | "refuse" => crate::claims::refuse("mcp-host"),
        "provider" | "view" => {
            let provider = args
                .get(1)
                .and_then(|value| McpProvider::parse(value))
                .ok_or_else(|| anyhow::anyhow!("usage: abbey mcp view <cursor|codex|claude>"))?;
            print_provider_view(cwd, provider)
        }
        "help" | "-h" | "--help" => {
            println!(
                "abbey mcp — Abbey's own read-only MCP server + local config inventory\n\n\
                 Abbey as MCP server (read-only tools):\n\
                   abbey mcp serve            # JSON-RPC 2.0 over stdin/stdout\n\
                   abbey mcp serve http       # Streamable HTTP POST /mcp, loopback only\n\
                                              # [--host <loopback>] [--port <n>]\n\
                                              # unauthenticated: no TLS, no OAuth 2.1/PKCE;\n\
                                              # a non-loopback --host is a hard error\n\
                   abbey mcp tools            # describe the safe registry + limits\n\n\
                 Local inventory (works under cursor/grok/fm/abi/claude backends):\n\
                   abbey mcp status\n\
                   abbey mcp paths\n\
                   abbey mcp view codex\n\n\
                 Provider management:\n\
                   abbey mcp cursor list\n\
                   abbey mcp codex list\n\
                   abbey mcp claude list\n\
                   abbey mcp <provider> enable|disable|login <id>\n\n\
                 Abbey serves its own read-only tools; it is not a client/host for other\n\
                 providers' MCP servers, and their management support is provider-owned."
            );
            Ok(0)
        }
        provider if McpProvider::parse(provider).is_some() => {
            let provider = McpProvider::parse(provider).expect("guarded above");
            run_mcp_for_provider(provider, &args[1..])
        }
        other => {
            eprintln!(
                "abbey: MCP management command `{other}` has no provider.\n\
                 Use: abbey mcp <cursor|codex|claude> {other} …"
            );
            Ok(2)
        }
    }
}

fn print_provider_view(cwd: &Path, provider: McpProvider) -> Result<i32> {
    if provider == McpProvider::Codex {
        let view = mcp::provider_mcp_view_with(
            provider,
            &crate::inventory::SystemPluginRunner,
            std::time::Duration::from_secs(8),
        );
        if !view.groups.is_empty() || !view.diagnostics.is_empty() {
            for group in view.groups {
                for server in group.servers {
                    let state = if server.disabled {
                        "disabled"
                    } else {
                        "enabled"
                    };
                    println!(
                        "{:<24} {:<8} {:<10} {}",
                        server.name,
                        state,
                        server.transport.label(),
                        server.safe_target()
                    );
                }
            }
            if let Some(error) = view.diagnostics.first() {
                eprintln!("codex MCP view: {}", error.message);
                return Ok(2);
            }
            return Ok(0);
        }
    }
    let inventory = load_mcp_inventory(cwd);
    for group in inventory.groups {
        if !group.source.providers.contains(&provider)
            && !group.source.providers.contains(&McpProvider::Shared)
        {
            continue;
        }
        for server in group.servers {
            let state = if server.disabled {
                "configured, disabled"
            } else {
                "configured, enabled"
            };
            println!(
                "{:<24} {:<20} {:<10} {}",
                server.name,
                state,
                server.transport.label(),
                server.safe_target()
            );
        }
    }
    Ok(0)
}

pub fn run_mcp_for_provider(provider: McpProvider, args: &[String]) -> Result<i32> {
    if !provider.supports_management() {
        bail!("{} does not expose MCP management", provider.label());
    }
    let binary = which_bin(provider.binary())
        .with_context(|| format!("{} is not on PATH", provider.binary()))?;
    let management_args = if args.is_empty() {
        &["list".into()][..]
    } else {
        args
    };
    let status = Command::new(binary)
        .arg("mcp")
        .args(management_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(status.code().unwrap_or(1))
}

pub fn dispatch_acp(args: &[String]) -> Result<i32> {
    if args.is_empty() {
        return print_acp_status();
    }
    match args[0].as_str() {
        "status" | "list" | "ls" => print_acp_status(),
        "run" | "serve" => {
            let name = args
                .get(1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("usage: abbey acp run <gemini|opencode>"))?;
            run_acp_peer(name)
        }
        "host" | "refuse" => crate::claims::refuse("acp-host"),
        "help" | "-h" | "--help" => {
            println!(
                "abbey acp — Agent Client Protocol peer inventory\n\n\
                 abbey acp list\n\
                 abbey acp run gemini\n\
                 abbey acp run opencode\n\n\
                 Abbey is not an ACP host; abi-mcp is MCP, not ACP."
            );
            Ok(0)
        }
        other => bail!(
            "unknown acp subcommand `{other}`\n\
             usage: abbey acp [list|run <peer>|help]"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_peer_table_has_only_actual_acp_peers() {
        let names: Vec<_> = acp_peers().iter().map(|peer| peer.name).collect();
        assert!(names.contains(&"gemini"));
        assert!(names.contains(&"opencode"));
        assert!(!names.contains(&"abi-mcp"));
    }
}
