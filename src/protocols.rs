//! MCP + ACP inventory and management helpers.
//!
//! Abbey is **not** an MCP host or ACP host runtime. This module:
//! - reads configured MCP servers from standard config files
//! - discovers ACP-capable peer agents on PATH
//! - forwards `abbey mcp …` management verbs to cursor-agent when needed
//!
//! Runtime tool use during a generation still happens inside cursor-agent
//! (see `--approve-mcps`).

use crate::agent::{AgentConfig, which_bin};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct McpServerEntry {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub source: PathBuf,
    pub disabled: bool,
}

#[derive(Debug, Clone)]
pub struct AcpPeer {
    pub name: &'static str,
    pub bin: &'static str,
    /// Args that start ACP stdio mode.
    pub acp_args: &'static [&'static str],
    pub path: Option<PathBuf>,
    pub note: &'static str,
}

/// Config files cursor-agent / common hosts consult.
pub fn mcp_config_candidates(cwd: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    out.push(cwd.join(".cursor/mcp.json"));
    out.push(cwd.join(".mcp.json"));
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".cursor/mcp.json"));
        out.push(home.join(".claude/mcp.json"));
        out.push(home.join(".claude.json"));
        out.push(home.join("Library/Application Support/Claude/claude_desktop_config.json"));
        out.push(home.join(".config/claude/mcp.json"));
    }
    out
}

fn extract_servers(value: &Value, source: &Path) -> Vec<McpServerEntry> {
    let Some(map) = value
        .get("mcpServers")
        .or_else(|| value.get("mcp_servers"))
        .and_then(|v| v.as_object())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, cfg) in map {
        let disabled = cfg
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let command = cfg
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("(missing command)")
            .to_string();
        let args = cfg
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push(McpServerEntry {
            name: name.clone(),
            command,
            args,
            source: source.to_path_buf(),
            disabled,
        });
    }
    out
}

pub fn load_mcp_servers(cwd: &Path) -> Result<Vec<(PathBuf, Vec<McpServerEntry>)>> {
    let mut groups = Vec::new();
    for path in mcp_config_candidates(cwd) {
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let value: Value =
            serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        let servers = extract_servers(&value, &path);
        groups.push((path, servers));
    }
    Ok(groups)
}

pub fn print_mcp_status(cwd: &Path) -> Result<i32> {
    println!("abbey mcp — config inventory (not an MCP host runtime)");
    println!("cwd: {}", cwd.display());
    let groups = load_mcp_servers(cwd)?;
    if groups.is_empty() {
        println!("(no mcp.json files found)");
        println!(
            "tip: create ~/.cursor/mcp.json with an `mcpServers` object,\n\
             then: abbey mcp list   # cursor-agent view"
        );
        return Ok(0);
    }
    let mut total = 0usize;
    for (path, servers) in &groups {
        println!("\n{}  ({} server(s))", path.display(), servers.len());
        if servers.is_empty() {
            println!("  (empty mcpServers object)");
            continue;
        }
        for s in servers {
            total += 1;
            let mark = if s.disabled { "off" } else { "on " };
            let args = if s.args.is_empty() {
                String::new()
            } else {
                format!(" {}", s.args.join(" "))
            };
            println!(
                "  [{mark}] {:<20} {}{args}  [{}]",
                s.name,
                s.command,
                s.source.file_name().and_then(|n| n.to_str()).unwrap_or("?")
            );
        }
    }
    println!("\ntotal configured: {total}");
    println!("management: abbey mcp list|enable|disable|list-tools <id>  → cursor-agent");
    println!("runtime:    abbey --approve-mcps \"…\"  (tools run inside cursor-agent)");
    Ok(0)
}

pub fn print_mcp_paths(cwd: &Path) -> Result<i32> {
    for path in mcp_config_candidates(cwd) {
        let mark = if path.is_file() { "ok " } else { "—  " };
        println!("{mark} {}", path.display());
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
        AcpPeer {
            name: "abi-mcp",
            bin: "abi-mcp",
            acp_args: &[],
            path: which_bin("abi-mcp"),
            note: "ABI MCP stdio server (MCP, not ACP — listed for adjacency)",
        },
    ]
}

pub fn print_acp_status() -> Result<i32> {
    println!("abbey acp — Agent Client Protocol peer inventory");
    println!("note: Abbey does not host ACP sessions; it discovers/launches peer servers.\n");
    println!("{:<12} {:<10} detail", "peer", "status");
    for p in acp_peers() {
        match &p.path {
            Some(path) => {
                let mode = if p.acp_args.is_empty() {
                    "stdio".into()
                } else {
                    format!("{} {}", p.bin, p.acp_args.join(" "))
                };
                println!(
                    "{:<12} {:<10} {} — {}",
                    p.name,
                    "present",
                    path.display(),
                    mode
                );
                println!("{:<12} {:<10} {}", "", "", p.note);
            }
            None => {
                println!("{:<12} {:<10} {}", p.name, "missing", p.note);
            }
        }
    }
    println!(
        "\nlaunch:  abbey acp run gemini|opencode   # stdio ACP server (for hosts that speak ACP)\n\
         bridge:  use an MCP↔ACP bridge (e.g. mcacp) if your host only speaks MCP"
    );
    Ok(0)
}

/// Run an ACP peer in stdio mode (foreground — for IDE/host attachment).
pub fn run_acp_peer(name: &str) -> Result<i32> {
    let peer = acp_peers()
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name) && !p.acp_args.is_empty())
        .with_context(|| format!("unknown ACP peer `{name}` (try: abbey acp list)"))?;
    let bin = peer
        .path
        .clone()
        .with_context(|| format!("{} not on PATH", peer.bin))?;
    eprintln!(
        "abbey: starting ACP stdio server: {} {}\n\
         (attach an ACP host to this process; Ctrl+C to stop)",
        peer.bin,
        peer.acp_args.join(" ")
    );
    let st = Command::new(bin)
        .args(peer.acp_args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;
    Ok(st.code().unwrap_or(1))
}

pub fn dispatch_mcp(cfg: &AgentConfig, cwd: &Path, args: &[String]) -> Result<i32> {
    if !cfg.backend.supports_account_surface() {
        eprintln!(
            "abbey: `mcp` is not applicable under the on-device backend (ABBEY_BACKEND=fm).\n\
             Unset ABBEY_BACKEND to use cursor-agent MCP management."
        );
        return Ok(2);
    }
    if args.is_empty() {
        return print_mcp_status(cwd);
    }
    match args[0].as_str() {
        "status" | "inventory" | "show" => print_mcp_status(cwd),
        "paths" => print_mcp_paths(cwd),
        "help" | "-h" | "--help" => {
            println!(
                "abbey mcp — MCP config inventory + cursor-agent management\n\n\
                 Local:\n\
                   abbey mcp              status / inventory\n\
                   abbey mcp status\n\
                   abbey mcp paths\n\n\
                 Via cursor-agent:\n\
                   abbey mcp list\n\
                   abbey mcp list-tools <id>\n\
                   abbey mcp enable|disable <id>\n\
                   abbey mcp login <id>\n\n\
                 Runtime tools: abbey --approve-mcps \"…\""
            );
            Ok(0)
        }
        // Everything else → cursor-agent mcp …
        _ => {
            let mut a = vec!["mcp".into()];
            a.extend(args.iter().cloned());
            let st = cfg.passthrough(&a)?;
            Ok(st.code().unwrap_or(1))
        }
    }
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
                .map(|s| s.as_str())
                .ok_or_else(|| anyhow::anyhow!("usage: abbey acp run <gemini|opencode>"))?;
            run_acp_peer(name)
        }
        "help" | "-h" | "--help" => {
            println!(
                "abbey acp — Agent Client Protocol peer inventory\n\n\
                 abbey acp              list peers\n\
                 abbey acp list\n\
                 abbey acp run gemini   start gemini --acp (stdio)\n\
                 abbey acp run opencode start opencode acp (stdio)\n\n\
                 Abbey is not an ACP host; pair with Zed/Claude/an MCP↔ACP bridge."
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
    fn extract_servers_from_mcp_json() {
        let v: Value = serde_json::from_str(
            r#"{
              "mcpServers": {
                "docs": { "command": "npx", "args": ["-y", "ctx7"] },
                "off": { "command": "true", "disabled": true }
              }
            }"#,
        )
        .unwrap();
        let got = extract_servers(&v, Path::new("/tmp/mcp.json"));
        assert_eq!(got.len(), 2);
        let docs = got.iter().find(|s| s.name == "docs").unwrap();
        assert_eq!(docs.command, "npx");
        assert_eq!(docs.args, vec!["-y", "ctx7"]);
        assert!(!docs.disabled);
        assert!(got.iter().find(|s| s.name == "off").unwrap().disabled);
    }

    #[test]
    fn acp_peer_table_includes_gemini_and_opencode() {
        let names: Vec<_> = acp_peers().iter().map(|p| p.name).collect();
        assert!(names.contains(&"gemini"));
        assert!(names.contains(&"opencode"));
    }
}
