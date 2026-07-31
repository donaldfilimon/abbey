//! In-session slash commands (Claude Code / Grok / Codex parity surface).

use std::fmt::Write as _;

/// Catalog entry for help / TUI.
#[derive(Debug, Clone, Copy)]
pub struct SlashCmd {
    pub name: &'static str,
    pub help: &'static str,
    pub kind: SlashKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashKind {
    Local,
    Agent,
}

pub const SLASH_CATALOG: &[SlashCmd] = &[
    SlashCmd {
        name: "help",
        help: "List slash commands",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "clear",
        help: "Drop active chat id (fresh session next run)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "compact",
        help: "Trim local history log to last N entries",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "doctor",
        help: "Paths, model, chat, knobs",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "claims",
        help: "Current/Proposed/OOS gate: /claims [proposed|oos|refuse …]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "platform",
        help: "Host targets + threads/GPU/NPU/TPU detect: /platform [compute]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "vision",
        help: "Vision honesty: path attach + agent gen (local weights OOS)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "cot",
        help: "CoT transcript viewer: /cot [show|run <task>]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "runtime",
        help: "Tool responsibility matrix (Abbey is not a tool host)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "oos",
        help: "OOS honesty index: /oos [lora|weights|accel|shell|host]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "lora",
        help: "LoRA honesty (learn curation Current; runners OOS)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "weights",
        help: "Local weights honesty (bindings/fm Current; bundled OOS)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "accel",
        help: "NPU/TPU honesty: /accel [status|detect|refuse]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "shell",
        help: "Unrestricted-OS honesty (allowlist Current)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "host",
        help: "MCP/ACP host honesty (inventory Current; host OOS)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "model",
        help: "Show or set default model alias/id",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "status",
        help: "Cursor auth status",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "models",
        help: "List account models",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "diff",
        help: "Show git diff (working tree; highlighted on TTY)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "highlight",
        help: "Syntax-colour file/stdin: /highlight [--lang LANG] [file|-]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "commit",
        help: "Draft conventional commit from staged diff",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "review",
        help: "Review git diff with agent",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "security-review",
        help: "Security-focused review of git diff",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "pr",
        help: "Draft PR title + body from branch commits/diff",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "init",
        help: "Scan project → AGENTS.md (/init [--force|--print|--agent])",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "plan",
        help: "Plan mode (read-only planning prompt)",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "ask",
        help: "Ask mode (read-only Q&A)",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "new",
        help: "Force new chat session",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "continue",
        help: "Resume current chat",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "fork",
        help: "New chat with note to continue prior context",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "persona",
        help: "Show/set persona abbey|aviva|abi",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "role",
        help: "Show/set worker role max|gemma|auto",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "max",
        help: "Force Max technical role for following prompt",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "gemma",
        help: "Force Gemma visual/conversational role for following prompt",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "image",
        help: "Attach image path + prompt (workspace read; no local vision)",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "video",
        help: "Attach video path + prompt (workspace read; no local vision)",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "think",
        help: "Set thinking model: /think low|medium|high|xhigh|max",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "imagine",
        help: "Generate image via agent tools: /imagine [--out=p] <desc>",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "gen-video",
        help: "Generate video via agent tools (best-effort): /gen-video <desc>",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "reason",
        help: "Structured reasoning with thinking model: /reason <task>",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "speak",
        help: "High-quality TTS: /speak <text> (Premium/Enhanced when installed)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "listen",
        help: "On-device STT: /listen [seconds]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "voice",
        help: "Voice I/O: /voice status|voices|speak|listen|ask",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "memory",
        help: "Chat history, or: /memory search <q>",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "routes",
        help: "Recent routes (conf · stage · alt · fb)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "skills",
        help: "List multi-CLI skills with descriptions",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "plugins",
        help: "Discover / pass through plugins",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "agents",
        help: "Peer agentic tools on PATH",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "os",
        help: "OS control: /os dry-run|execute --confirm|allowlist",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "learn",
        help: "Self-learn: correction|preference|routes|digest|review|stats",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "learn-review",
        help: "Alias: train_candidate review (provenance curation)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "learn-stats",
        help: "Alias: train_candidate curation counts",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "allowlist",
        help: "OS-control allowlist / policy (alias of /os allowlist)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "parallel",
        help: "Fan-out Max/Gemma/Aviva (alias of /subagents)",
        kind: SlashKind::Agent,
    },
    SlashCmd {
        name: "subagents",
        help: "Multi-subagent + local peers: /subagents [list|run --lanes …]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "improve",
        help: "Smart improve: /improve status|plan|run [--confirm] (ledger + check.sh)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "permissions",
        help: "Show trust/force/sandbox knobs",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "config",
        help: "Show Abbey env config",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "cost",
        help: "Cost tracking (not available via cursor-agent)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "tasks",
        help: "Recent session history",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "rewind",
        help: "Clear chat id (session rewind)",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "mcp",
        help: "MCP inventory + cursor-agent mcp: /mcp [status|list|…]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "acp",
        help: "ACP peers: /acp [list|run gemini|opencode]",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "debug",
        help: "Local diagnostics",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "branch",
        help: "Create git branch: /branch <name>",
        kind: SlashKind::Local,
    },
    SlashCmd {
        name: "please-fix",
        help: "Fix last failed command",
        kind: SlashKind::Agent,
    },
];

pub fn help_text() -> String {
    let mut out = String::from("Abbey slash commands (Grok Build / Codex / Claude Code parity):\n");
    for c in SLASH_CATALOG {
        let _ = writeln!(
            out,
            "  /{:<16} {} [{}]",
            c.name,
            c.help,
            match c.kind {
                SlashKind::Local => "local",
                SlashKind::Agent => "agent",
            }
        );
    }
    out.push_str(
        "\nTip: bare prompts launch the agent; flags like -m/--force/--worktree still apply.\n",
    );
    out
}

/// Parse `/cmd rest…` → (cmd, rest). Returns None if not a slash command.
pub fn parse_slash(input: &str) -> Option<(&str, &str)> {
    let t = input.trim();
    if !t.starts_with('/') {
        return None;
    }
    let body = t.trim_start_matches('/');
    if body.is_empty() {
        return Some(("help", ""));
    }
    let (cmd, rest) = match body.split_once(char::is_whitespace) {
        Some((c, r)) => (c, r.trim()),
        None => (body, ""),
    };
    Some((cmd, rest))
}

pub fn lookup(name: &str) -> Option<&'static SlashCmd> {
    let lower = name.to_ascii_lowercase();
    SLASH_CATALOG.iter().find(|c| c.name == lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_slash_basic() {
        assert_eq!(parse_slash("/init --force"), Some(("init", "--force")));
        assert_eq!(parse_slash("/help"), Some(("help", "")));
        assert_eq!(parse_slash("nope"), None);
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert!(lookup("INIT").is_some());
        assert_eq!(lookup("INIT").unwrap().name, "init");
    }
}
