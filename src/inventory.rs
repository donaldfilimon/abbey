//! Skills + plugins + peer agentic-tool inventory (multi-CLI aware).

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub root: PathBuf,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct PluginHint {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct AgentTool {
    pub name: String,
    pub path: Option<PathBuf>,
    pub kind: &'static str,
}

fn skill_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for p in [
            home.join(".grok/skills"),
            home.join(".agents/skills"),
            home.join(".claude/skills"),
            home.join(".codex/skills"),
            home.join(".cursor/skills"),
            home.join("plugins/abi-mega/skills"),
            home.join("abi/.agents/skills"),
            home.join("abi/.claude/skills"),
        ] {
            if p.is_dir() {
                roots.push(p);
            }
        }
    }
    roots
}

fn read_skill_description(dir: &Path) -> String {
    let skill = dir.join("SKILL.md");
    let Ok(text) = fs::read_to_string(skill) else {
        return String::new();
    };
    // YAML frontmatter description: or first non-heading paragraph
    let mut in_fm = false;
    for line in text.lines() {
        let t = line.trim();
        if t == "---" {
            in_fm = !in_fm;
            continue;
        }
        if in_fm {
            if let Some(rest) = t.strip_prefix("description:") {
                let d = rest.trim().trim_matches('"').trim_matches('\'');
                if !d.is_empty() {
                    return d.chars().take(160).collect();
                }
            }
        }
    }
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"))
        .map(|l| l.chars().take(160).collect())
        .unwrap_or_default()
}

pub fn list_skills() -> Result<Vec<SkillEntry>> {
    let mut out = Vec::new();
    for root in skill_roots() {
        let Ok(rd) = fs::read_dir(&root) else {
            continue;
        };
        for e in rd.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let description = read_skill_description(&e.path());
            out.push(SkillEntry {
                name,
                root: e.path(),
                description,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    Ok(out)
}

pub fn print_skills() -> Result<()> {
    let skills = list_skills()?;
    if skills.is_empty() {
        println!(
            "(no skills directories found under ~/.grok|~/.agents|~/.claude|~/.cursor|abi-mega)"
        );
        return Ok(());
    }
    for s in skills {
        if s.description.is_empty() {
            println!("{:<28} {}", s.name, s.root.display());
        } else {
            println!("{:<28} {}", s.name, s.description);
        }
    }
    Ok(())
}

pub fn list_agent_tools() -> Vec<AgentTool> {
    const PEERS: &[(&str, &str)] = &[
        ("cursor-agent", "primary LLM executor"),
        ("agent", "cursor/grok agent alias"),
        ("abi", "ABI CLI (WDBX, personas, os, plugins)"),
        ("abi-mcp", "ABI MCP stdio server"),
        ("grok", "Grok Build CLI"),
        ("codex", "OpenAI Codex CLI"),
        ("claude", "Claude Code CLI"),
        ("opencode", "OpenCode CLI"),
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
    for t in list_agent_tools() {
        match &t.path {
            Some(p) => println!("  {:<14} {}  ({})", t.name, p.display(), t.kind),
            None => println!("  {:<14} (not found)  ({})", t.name, t.kind),
        }
    }
}

pub fn list_plugins_hints() -> Result<Vec<PluginHint>> {
    let mut out = Vec::new();
    // cursor-agent plugin list (best-effort)
    if let Some(agent) = crate::agent::which_bin("cursor-agent") {
        if let Ok(o) = std::process::Command::new(agent)
            .args(["plugin", "list"])
            .output()
        {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let line = line.trim();
                if !line.is_empty() {
                    out.push(PluginHint {
                        name: line.to_string(),
                        source: "cursor-agent".into(),
                    });
                }
            }
            for line in String::from_utf8_lossy(&o.stderr).lines() {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with("error") {
                    out.push(PluginHint {
                        name: line.to_string(),
                        source: "cursor-agent".into(),
                    });
                }
            }
        }
    }
    if let Some(abi) = crate::agent::which_bin("abi") {
        if let Ok(o) = std::process::Command::new(abi)
            .args(["plugin", "list"])
            .output()
        {
            // abi plugin list goes to stderr historically
            let text = if o.stderr.is_empty() {
                String::from_utf8_lossy(&o.stdout).into_owned()
            } else {
                String::from_utf8_lossy(&o.stderr).into_owned()
            };
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with("usage:") {
                    continue;
                }
                out.push(PluginHint {
                    name: line.to_string(),
                    source: "abi".into(),
                });
            }
        }
    }
    // Cursor plugin cache (local)
    if let Some(home) = dirs::home_dir() {
        let cache = home.join(".cursor/plugins/cache");
        if cache.is_dir() {
            if let Ok(rd) = fs::read_dir(cache) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        out.push(PluginHint {
                            name: e.file_name().to_string_lossy().into_owned(),
                            source: "cursor-cache".into(),
                        });
                    }
                }
            }
        }
    }
    Ok(out)
}

pub fn print_plugins() -> Result<()> {
    let plugs = list_plugins_hints()?;
    if plugs.is_empty() {
        println!("(no plugins discovered — try: abbey plugin … | abi plugin list)");
        return Ok(());
    }
    for p in plugs {
        println!("{:<40} [{}]", p.name, p.source);
    }
    Ok(())
}

pub fn run_plugin_passthrough(args: &[String]) -> Result<i32> {
    if args.is_empty() {
        print_plugins()?;
        return Ok(0);
    }
    // Prefer cursor-agent plugin, then abi
    if let Some(agent) = crate::agent::which_bin("cursor-agent") {
        let mut a = vec!["plugin".into()];
        a.extend(args.iter().cloned());
        let st = std::process::Command::new(agent).args(&a).status()?;
        return Ok(st.code().unwrap_or(1));
    }
    if let Some(abi) = crate::agent::which_bin("abi") {
        let mut a = vec!["plugin".into()];
        a.extend(args.iter().cloned());
        let st = std::process::Command::new(abi).args(&a).status()?;
        return Ok(st.code().unwrap_or(1));
    }
    anyhow::bail!("no cursor-agent or abi for plugin passthrough");
}
