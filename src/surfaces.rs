//! Honesty surfaces for vision weights, CoT viewing, and tool runtime.
//!
//! | Ask | Current | Deferred status |
//! |-----|---------|-----------------|
//! | Vision/video | path attach + agent/MCP generation | local neural media is Proposed |
//! | Reasoning | Cursor thinking + structured wrapper + transcript viewer | Abbey-owned hidden CoT engine/UI is OOS |
//! | Tools | inventory + delegated tools during a turn | Abbey-owned runtime/MCP host is Proposed |

use crate::claims;
use crate::output;
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const COT_REL: &str = "cot/latest.md";

pub fn cot_path(state: &AbbeyState) -> PathBuf {
    state.state_dir.join(COT_REL)
}

pub fn save_cot(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::File::create(path)?;
    writeln!(
        file,
        "# Abbey CoT transcript\n\
         \n\
         > Display of structured agent reasoning — **not** an Abbey-owned CoT engine.\n\
         > Model/runtime: cursor-agent thinking id (see `abbey reason`).\n"
    )?;
    file.write_all(body.trim_end().as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Pretty-print a saved (or raw) reason transcript with section emphasis.
pub fn render_cot(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);

    for line in text.lines() {
        let trimmed = line.trim_start();
        let lowercase = trimmed.to_ascii_lowercase();

        let is_numbered_section = trimmed
            .strip_prefix('#')
            .map(|heading| heading.starts_with(|ch: char| ch.is_ascii_digit()))
            .unwrap_or(false);
        let is_numbered_step = trimmed
            .split_once('.')
            .map(|(number, _)| !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()))
            .unwrap_or(false);
        let is_named_section = lowercase == "restatement"
            || lowercase.starts_with("conclusion")
            || lowercase.starts_with("assumption")
            || lowercase.starts_with("step-by-step");
        let is_heading =
            trimmed.starts_with('#') || is_numbered_section || is_numbered_step || is_named_section;

        if is_heading {
            out.push_str("\x1b[1m");
            out.push_str(line);
            out.push_str("\x1b[0m\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    out
}

pub fn print_vision_status() -> Result<i32> {
    println!("abbey vision — media surfaces (honest)\n");
    println!("Current:");
    println!("  · path attach     abbey --image|--video|/image  (paths → --add-dir + note)");
    println!("  · agent generate  abbey imagine|generate video  (cursor-agent / MCP tools)");
    println!("  · no pixel encode Abbey never loads weights or embeds frames");
    println!();
    println!("Proposed (unavailable):");
    println!("  · local neural image/video models or on-device VLM in Abbey");
    println!();
    println!("instead: abbey --image ./shot.png \"…\" · abbey imagine \"…\"");
    println!("refuse:  abbey vision refuse · abbey claims refuse vision");
    Ok(0)
}

pub fn print_runtime_matrix() -> Result<i32> {
    println!("abbey runtime — who executes what (Abbey is NOT a tool runtime)\n");
    println!("{:<28} {:<16} detail", "capability", "executor");

    let rows = [
        ("prompt / chat", "cursor-agent", "or fm/grok backends"),
        (
            "MCP tool calls",
            "cursor-agent",
            "--approve-mcps; Abbey inventories only",
        ),
        (
            "ACP peer server",
            "peer CLI",
            "abbey acp run gemini|opencode",
        ),
        ("OS allowlist", "Abbey / abi", "dry-run; execute --confirm"),
        (
            "memory put/search/map",
            "Abbey",
            "sqlite or --features wdbx",
        ),
        (
            "subagent lanes",
            "Abbey→agents",
            "cursor-agent print + PATH peers",
        ),
        (
            "image/video generate",
            "cursor-agent",
            "tools/MCP — not Abbey weights",
        ),
        ("voice TTS/STT", "Abbey→OS", "macOS say + Speech only"),
        ("local neural media", "—", "Proposed (unavailable)"),
        ("Abbey CoT engine", "—", "Out of scope (viewer is Current)"),
        (
            "Abbey MCP server",
            "Abbey",
            "Current: read-only stdio + unauthenticated loopback HTTP",
        ),
        (
            "Abbey MCP client-host",
            "—",
            "Proposed (does not consume external servers)",
        ),
    ];

    for (capability, executor, detail) in rows {
        println!("{capability:<28} {executor:<16} {detail}");
    }

    println!();
    println!(
        "rule: tools during a turn run inside cursor-agent. Abbey does not connect out\n\
         to other providers' MCP/ACP servers or dispatch arbitrary tool schemas; it does\n\
         serve its own read-only tools over stdio or loopback-only HTTP (`abbey mcp serve`).\n\
         refuse: abbey runtime host · abbey claims refuse mcp-host"
    );
    Ok(0)
}

pub fn print_cot_status(state: &AbbeyState) -> Result<i32> {
    let path = cot_path(state);

    println!("abbey cot — reasoning transcript viewer (not an Abbey CoT UI/engine)\n");
    println!("Current:");
    println!("  · abbey reason / --thinking / /think  → Cursor *-thinking-* + structured wrap");
    println!("  · abbey cot show                      → last saved transcript");
    println!("  · abbey cot run <task…>               → reason + save + view");
    println!();
    println!("Out of scope:");
    println!("  ✗ Abbey-owned chain-of-thought engine / interactive CoT UI");
    println!();

    if path.is_file() {
        println!("latest: {}", path.display());
    } else {
        println!("latest: (none yet — run `abbey cot run …` or `abbey reason …`)");
    }

    Ok(0)
}

pub fn show_cot(state: &AbbeyState) -> Result<i32> {
    let path = cot_path(state);

    if !path.is_file() {
        eprintln!(
            "abbey: no CoT transcript yet.\n\
             run: abbey cot run <task…>   or   abbey reason <task…>"
        );
        return Ok(1);
    }

    let text = fs::read_to_string(&path)?;
    let colour = crate::highlight::enabled();

    if colour {
        let _ = output::print(render_cot(&text));
    } else {
        let _ = output::print(text);
    }

    Ok(0)
}

pub fn dispatch_vision(args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str) {
        Some("-h") | Some("--help") => {
            println!(
                "abbey vision — media honesty\n\
                 usage: abbey vision [status|refuse]\n\
                 Current: --image/--video + imagine via agent tools\n\
                 Proposed: local neural image/video models"
            );
            Ok(0)
        }
        None | Some("status") | Some("show") => print_vision_status(),
        Some("refuse") | Some("weights") | Some("local") => claims::refuse("vision"),
        Some(other) => bail!("unknown vision subcommand `{other}` — try: status|refuse"),
    }
}

pub fn dispatch_cot(args: &[String], state: &AbbeyState) -> Result<i32> {
    match args.first().map(String::as_str) {
        None | Some("status") => print_cot_status(state),
        Some("show") | Some("latest") | Some("view") => show_cot(state),
        Some("run") => {
            // The command router handles `run` by calling generate::run_reason.
            bail!("internal: cot run handled by commands")
        }
        Some("refuse") | Some("engine") | Some("ui") => claims::refuse("cot"),
        Some("-h") | Some("--help") => {
            println!(
                "abbey cot — structured reasoning transcript viewer\n\
                 \n\
                 usage:\n\
                   abbey cot                 # status\n\
                   abbey cot show            # last transcript\n\
                   abbey cot run <task…>     # reason + save + view\n\
                   abbey cot refuse          # OOS: Abbey CoT engine\n\
                 \n\
                 note: reasoning runs on Cursor thinking models; Abbey only wraps and displays."
            );
            Ok(0)
        }
        Some(other) => bail!("unknown cot subcommand `{other}` — try: status|show|run|refuse"),
    }
}

pub fn dispatch_runtime(args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str) {
        None | Some("status") | Some("matrix") | Some("show") => print_runtime_matrix(),
        Some("host") | Some("refuse") | Some("tools") => claims::refuse("mcp-host"),
        Some("-h") | Some("--help") => {
            println!(
                "abbey runtime — tool responsibility matrix\n\
                 usage: abbey runtime [matrix|refuse]\n\
                 Abbey does not yet own a tool runtime; the Proposed host remains unavailable."
            );
            Ok(0)
        }
        Some(other) => bail!("unknown runtime subcommand `{other}` — try: matrix|refuse"),
    }
}

pub fn status_lines() -> Vec<String> {
    vec![
        "vision:     path attach + agent gen — local neural models Proposed (`abbey vision`)"
            .into(),
        "cot:        transcript viewer for reason — Abbey CoT engine OOS (`abbey cot`)".into(),
        "runtime:    read-only stdio + loopback HTTP server; no external-server client-host (`abbey runtime`)".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_cot_bolds_numbered_sections() {
        let raw = "1. Restate\nbody\n5. Conclusion\n";
        let out = render_cot(raw);

        assert!(out.contains("\x1b[1m"));
        assert!(out.contains("1. Restate"));
        assert!(out.contains("body"));
    }

    #[test]
    fn render_cot_bolds_markdown_and_named_sections() {
        let raw = "## Assumptions\ntext\nConclusion\n";
        let out = render_cot(raw);

        assert!(out.contains("\x1b[1m## Assumptions\x1b[0m"));
        assert!(out.contains("\x1b[1mConclusion\x1b[0m"));
    }

    #[test]
    fn render_cot_does_not_bold_regular_text() {
        let out = render_cot("This is ordinary text.\n");

        assert_eq!(out, "This is ordinary text.\n");
    }

    #[test]
    fn save_cot_writes_header() {
        let dir = std::env::temp_dir().join("abbey-cot-test-hdr");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let path = dir.join("latest.md");
        save_cot(&path, "1. Restate\nok\n").unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("Abbey-owned CoT engine"));
        assert!(text.contains("1. Restate"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_lines_distinguish_proposed_from_oos() {
        let lines = status_lines().join("\n");

        assert!(lines.contains("local neural models Proposed"));
        assert!(lines.contains("CoT engine OOS"));
    }
}
