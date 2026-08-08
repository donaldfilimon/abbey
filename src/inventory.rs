//! Skills, plugins, and peer agentic-tool inventory.
//!
//! Inventory is deliberately provider-explicit. A marketplace visible to Cursor
//! is not an installed Codex plugin, and a mirrored skill is not silently made
//! authoritative merely because its directory happened to be scanned first.

pub mod plugins;
pub mod skills;

use anyhow::{Result, bail};
use std::path::PathBuf;

pub use plugins::{SystemPluginRunner, inventory_plugins};
pub use skills::list_skills;

#[derive(Debug, Clone)]
pub struct AgentTool {
    pub name: String,
    pub path: Option<PathBuf>,
    pub kind: &'static str,
}

pub fn print_skills() -> Result<()> {
    print_skills_bounded(Some((200, 50)))
}

/// Print the complete provenance-preserving inventory without display caps.
pub fn print_skills_all() -> Result<()> {
    print_skills_bounded(None)
}

fn print_skills_bounded(limits: Option<(usize, usize)>) -> Result<()> {
    let inventory = skills::skill_inventory()?;
    if inventory.entries.is_empty() {
        println!("(no SKILL.md manifests found in configured skill roots)");
        return Ok(());
    }
    let entry_limit = limits.map_or(inventory.entries.len(), |value| value.0);
    for skill in inventory.entries.iter().take(entry_limit) {
        let description: String = skill
            .description
            .replace('\n', " ")
            .chars()
            .take(180)
            .collect();
        let manifest_count = skill
            .provenance
            .iter()
            .filter(|source| source.manifest.file_name().is_some())
            .count();
        let mirror_origins = skill
            .provenance
            .iter()
            .map(|source| source.origin.label())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("+");
        let mirrors = if manifest_count > 1 {
            format!(" ({manifest_count} exact sources: {mirror_origins})")
        } else {
            String::new()
        };
        if description.is_empty() {
            println!(
                "{:<32} [{}] {}{}",
                skill.name,
                skill.origin.label(),
                skill.root.display(),
                mirrors
            );
        } else {
            println!(
                "{:<32} [{}] {}{}",
                skill.name,
                skill.origin.label(),
                description,
                mirrors
            );
        }
    }
    let diagnostic_limit = limits.map_or(inventory.diagnostics.len(), |value| value.1);
    for diagnostic in inventory.diagnostics.iter().take(diagnostic_limit) {
        eprintln!(
            "warning: {} (skill {}, {} source path(s))",
            diagnostic.message,
            diagnostic.name,
            diagnostic.paths.len()
        );
    }
    if entry_limit < inventory.entries.len() || diagnostic_limit < inventory.diagnostics.len() {
        eprintln!(
            "skills: showing {} of {} entries and {} of {} diagnostics; use `abbey skills --all` for the complete provenance audit",
            entry_limit.min(inventory.entries.len()),
            inventory.entries.len(),
            diagnostic_limit.min(inventory.diagnostics.len()),
            inventory.diagnostics.len()
        );
    }
    Ok(())
}

pub fn list_agent_tools() -> Vec<AgentTool> {
    const PEERS: &[(&str, &str)] = &[
        ("cursor-agent", "Cursor agent CLI"),
        ("agent", "Cursor/Grok agent alias"),
        ("abi", "ABI CLI (WDBX, personas, OS, plugins)"),
        ("gemini", "Gemini CLI (supports --acp)"),
        ("grok", "Grok Build CLI"),
        ("codex", "OpenAI Codex CLI"),
        ("claude", "Claude Code CLI"),
        ("opencode", "OpenCode CLI (supports `acp`)"),
    ];
    PEERS
        .iter()
        .map(|(name, kind)| AgentTool {
            name: (*name).into(),
            path: crate::agent::which_bin(name),
            kind,
        })
        .collect()
}

pub fn print_agent_tools() {
    println!("Peer agentic tools (PATH discovery):");
    for tool in list_agent_tools() {
        match &tool.path {
            Some(path) => println!("  {:<14} {}  ({})", tool.name, path.display(), tool.kind),
            None => println!("  {:<14} (not found)  ({})", tool.name, tool.kind),
        }
    }
}

pub fn print_plugins() -> Result<()> {
    print_plugins_bounded(Some((200, 50)))
}

fn print_plugins_all() -> Result<()> {
    print_plugins_bounded(None)
}

fn print_plugins_bounded(limits: Option<(usize, usize)>) -> Result<()> {
    let inventory = inventory_plugins();
    if inventory.entries.is_empty() {
        println!("(no provider plugin inventory returned)");
    }
    let entry_limit = limits.map_or(inventory.entries.len(), |value| value.0);
    for entry in inventory.entries.iter().take(entry_limit) {
        println!(
            "{:<42} [{:<6} {:<11} {}] {}",
            entry.name,
            entry.provider.label(),
            entry.kind.label(),
            entry.state.label(),
            entry.source
        );
    }
    let diagnostic_limit = limits.map_or(inventory.diagnostics.len(), |value| value.1);
    for diagnostic in inventory.diagnostics.iter().take(diagnostic_limit) {
        eprintln!(
            "{} plugin inventory: {}",
            diagnostic.provider.label(),
            diagnostic.message
        );
    }
    if entry_limit < inventory.entries.len() || diagnostic_limit < inventory.diagnostics.len() {
        eprintln!(
            "plugins: showing {} of {} entries and {} of {} diagnostics; use `abbey plugins --all` for the complete provider audit",
            entry_limit.min(inventory.entries.len()),
            inventory.entries.len(),
            diagnostic_limit.min(inventory.diagnostics.len()),
            inventory.diagnostics.len()
        );
    }
    Ok(())
}

/// Backward-compatible CLI entry point. Inventory with no arguments is local
/// and covers every provider. Management requires an explicit provider token:
/// `abbey plugin codex list`, `abbey plugin claude enable …`, etc.
pub fn run_plugin_passthrough(args: &[String]) -> Result<i32> {
    if args.is_empty() {
        print_plugins()?;
        return Ok(0);
    }
    if args == ["--all"] {
        print_plugins_all()?;
        return Ok(0);
    }
    let (provider, rest) = if args.first().is_some_and(|arg| arg == "--provider") {
        let provider = args
            .get(1)
            .and_then(|value| plugins::parse_plugin_provider(value))
            .ok_or_else(|| anyhow::anyhow!("--provider requires cursor|codex|claude|abi"))?;
        (provider, &args[2..])
    } else if let Some(provider) = plugins::parse_plugin_provider(&args[0]) {
        (provider, &args[1..])
    } else {
        bail!(
            "plugin management requires an explicit provider\n\
             usage: abbey plugin <cursor|codex|claude|abi> <command>"
        );
    };
    plugins::run_plugin_for_provider(provider, rest)
}
