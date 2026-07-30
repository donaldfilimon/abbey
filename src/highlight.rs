//! Auto syntax highlighting for fenced code (and files).
//!
//! When stdout is a TTY (and `NO_COLOR` / `ABBEY_HIGHLIGHT=0` are unset), Abbey
//! colourises markdown ``` fences in captured agent output via syntect.
//! Interactive inherited stdio is unchanged — cursor-agent paints that stream.
//!
//! Explicit: `abbey highlight [file|-] [--lang LANG]`.

use crate::output;
use anyhow::{Context, Result, bail};
use std::io::{self, IsTerminal, Read};
use std::path::Path;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::{LinesWithEndings, as_24_bit_terminal_escaped};

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static THEMES: OnceLock<ThemeSet> = OnceLock::new();

fn syntaxes() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn themes() -> &'static ThemeSet {
    THEMES.get_or_init(ThemeSet::load_defaults)
}

fn theme() -> &'static Theme {
    let name =
        std::env::var("ABBEY_HIGHLIGHT_THEME").unwrap_or_else(|_| "base16-ocean.dark".into());
    themes()
        .themes
        .get(name.as_str())
        .or_else(|| themes().themes.get("base16-ocean.dark"))
        .or_else(|| themes().themes.values().next())
        .expect("syntect ships at least one theme")
}

/// True when Abbey should inject ANSI for highlighted output.
pub fn enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match std::env::var("ABBEY_HIGHLIGHT") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("off") => {
            return false;
        }
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on") => {
            return true;
        }
        Ok(v) if v.eq_ignore_ascii_case("always") => return true,
        Ok(v) if v.eq_ignore_ascii_case("never") => return false,
        _ => {}
    }
    io::stdout().is_terminal()
}

fn find_syntax<'a>(
    ps: &'a SyntaxSet,
    lang: Option<&str>,
    path: Option<&Path>,
) -> &'a SyntaxReference {
    if let Some(lang) = lang.map(str::trim).filter(|s| !s.is_empty()) {
        let key = normalize_lang(lang);
        if let Some(s) = ps.find_syntax_by_token(key) {
            return s;
        }
        if let Some(s) = ps.find_syntax_by_extension(key) {
            return s;
        }
        if let Some(s) = ps.find_syntax_by_name(lang) {
            return s;
        }
    }
    if let Some(path) = path {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if let Some(s) = ps.find_syntax_by_extension(ext) {
                return s;
            }
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(s) = ps.find_syntax_by_extension(name) {
                return s;
            }
        }
    }
    ps.find_syntax_plain_text()
}

fn normalize_lang(lang: &str) -> &str {
    match lang.to_ascii_lowercase().as_str() {
        "rs" | "rust" => "Rust",
        "ts" | "typescript" | "tsx" => "TypeScript",
        "js" | "javascript" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" | "python" => "Python",
        "sh" | "bash" | "zsh" | "shell" => "Shell-Unix-Generic",
        "toml" => "TOML",
        "json" | "jsonc" => "JSON",
        "yml" | "yaml" => "YAML",
        "md" | "markdown" => "Markdown",
        "diff" | "patch" => "Diff",
        "go" => "Go",
        "swift" => "Swift",
        "zig" => "Zig",
        "c" => "C",
        "cpp" | "cc" | "cxx" | "c++" | "hpp" => "C++",
        "java" => "Java",
        "kt" | "kotlin" => "Kotlin",
        "rb" | "ruby" => "Ruby",
        "sql" => "SQL",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "xml" => "XML",
        "txt" | "text" | "plain" => "Plain Text",
        _ => lang,
    }
}

/// Highlight a bare code blob (no markdown wrapper).
pub fn colorize_code(code: &str, lang: Option<&str>, path: Option<&Path>) -> String {
    let ps = syntaxes();
    let syntax = find_syntax(ps, lang, path);
    let mut h = HighlightLines::new(syntax, theme());
    let mut out = String::with_capacity(code.len().saturating_mul(2));
    for line in LinesWithEndings::from(code) {
        match h.highlight_line(line, ps) {
            Ok(ranges) => out.push_str(&as_24_bit_terminal_escaped(&ranges, false)),
            Err(_) => out.push_str(line),
        }
    }
    // Reset terminal attributes after a highlighted block.
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\x1b[0m");
    out
}

/// Colourise markdown ``` / ~~~ fenced blocks; leave prose alone.
pub fn colorize_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let open = fence_open(trimmed);
        if open.is_none() {
            out.push_str(line);
            continue;
        }
        let (marker, lang) = open.unwrap();
        // Keep the fence line (dim) so structure stays readable.
        out.push_str("\x1b[2m");
        out.push_str(trimmed);
        out.push_str("\x1b[0m");
        if line.ends_with('\n') {
            out.push('\n');
        }
        let mut body = String::new();
        let mut closed = false;
        for body_line in lines.by_ref() {
            let t = body_line.trim_end_matches(['\r', '\n']);
            if fence_close(t, marker) {
                if !body.is_empty() {
                    out.push_str(&colorize_code(&body, lang.as_deref(), None));
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                out.push_str("\x1b[2m");
                out.push_str(t);
                out.push_str("\x1b[0m");
                if body_line.ends_with('\n') {
                    out.push('\n');
                }
                closed = true;
                break;
            }
            body.push_str(body_line);
        }
        if !closed {
            // Unterminated fence — emit body highlighted anyway.
            if !body.is_empty() {
                out.push_str(&colorize_code(&body, lang.as_deref(), None));
            }
        }
    }
    out
}

fn fence_open(line: &str) -> Option<(&'static str, Option<String>)> {
    let t = line.trim_start();
    let marker = if t.starts_with("```") {
        "```"
    } else if t.starts_with("~~~") {
        "~~~"
    } else {
        return None;
    };
    let after = t[marker.len()..].trim();
    let lang = after
        .split_whitespace()
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_matches(|c| c == '{' || c == '}').to_string());
    Some((marker, lang))
}

fn fence_close(line: &str, marker: &str) -> bool {
    let t = line.trim();
    if !t.starts_with(marker) {
        return false;
    }
    let ch = marker.chars().next().unwrap_or('`');
    t.chars().all(|c| c == ch)
}

/// Auto-highlight markdown fences when enabled; otherwise return text unchanged.
pub fn auto_markdown(text: &str) -> String {
    if !enabled() || text.is_empty() {
        return text.to_string();
    }
    // Skip JSON / already-coloured blobs.
    if looks_like_json(text) || text.contains("\x1b[") {
        return text.to_string();
    }
    if !text.contains("```") && !text.contains("~~~") {
        return text.to_string();
    }
    colorize_markdown(text)
}

fn looks_like_json(text: &str) -> bool {
    let t = text.trim_start();
    (t.starts_with('{') || t.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(t).is_ok()
}

/// Print captured agent stdout with auto fence highlighting (broken-pipe safe).
pub fn emit_agent_stdout(text: impl AsRef<str>) {
    let rendered = auto_markdown(text.as_ref());
    let _ = output::print(rendered);
}

/// CLI / slash: `highlight [path|-] [--lang LANG] [--force]`.
pub fn dispatch(args: &[String]) -> Result<i32> {
    let mut lang: Option<String> = None;
    let mut force = false;
    let mut markdown = false;
    let mut path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--lang" | "-l" => {
                i += 1;
                let Some(v) = args.get(i) else {
                    bail!("usage: abbey highlight [--lang LANG] [--force] [--markdown] [file|-]");
                };
                lang = Some(v.clone());
            }
            "--force" | "-f" | "--always" => force = true,
            "--markdown" | "--md" => markdown = true,
            "-h" | "--help" => {
                print_help();
                return Ok(0);
            }
            s if s.starts_with('-') && s != "-" => {
                bail!("unknown highlight flag: {s}");
            }
            s => {
                if path.is_some() {
                    bail!("usage: abbey highlight [--lang LANG] [--force] [--markdown] [file|-]");
                }
                path = Some(s.to_string());
            }
        }
        i += 1;
    }

    let text = match path.as_deref() {
        None | Some("-") => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("reading stdin")?;
            buf
        }
        Some(p) => std::fs::read_to_string(p).with_context(|| format!("read {p}"))?,
    };

    let colour = force || enabled();
    let rendered = if !colour {
        text
    } else if markdown
        || path.as_deref().is_some_and(|p| {
            p != "-" && Path::new(p).extension().and_then(|e| e.to_str()) == Some("md")
        })
        || (lang.is_none() && (text.contains("```") || text.contains("~~~")))
    {
        colorize_markdown(&text)
    } else {
        colorize_code(&text, lang.as_deref(), path.as_deref().map(Path::new))
    };
    let _ = output::print(&rendered);
    if !rendered.ends_with('\n') {
        let _ = output::println("");
    }
    Ok(0)
}

fn print_help() {
    println!(
        "abbey highlight — syntax-colour code (syntect)\n\
         \n\
         usage: abbey highlight [--lang LANG] [--force] [--markdown] [file|-]\n\
         \n\
         Auto (capture prints): colourises ``` fences when stdout is a TTY.\n\
         Disable: NO_COLOR=1 or ABBEY_HIGHLIGHT=0\n\
         Force:   ABBEY_HIGHLIGHT=always  or  --force\n\
         Theme:   ABBEY_HIGHLIGHT_THEME (default base16-ocean.dark)"
    );
}

pub fn status_line() -> String {
    let on = if enabled() { "auto/on" } else { "off" };
    format!("highlight: {on} (fenced code in -p/print/commit; `abbey highlight`)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_open_parses_lang() {
        let (m, lang) = fence_open("```rust").unwrap();
        assert_eq!(m, "```");
        assert_eq!(lang.as_deref(), Some("rust"));
        let (m2, lang2) = fence_open("~~~python").unwrap();
        assert_eq!(m2, "~~~");
        assert_eq!(lang2.as_deref(), Some("python"));
        assert!(fence_open("not a fence").is_none());
    }

    #[test]
    fn colorize_markdown_keeps_prose_and_marks_fence() {
        let src = "hello\n\n```rs\nfn main() {}\n```\n\nbye\n";
        let out = colorize_markdown(src);
        assert!(out.contains("hello"));
        assert!(out.contains("bye"));
        // Tokens may be split by ANSI SGR; strip escapes before checking source text.
        let plain: String = {
            let mut s = String::new();
            let mut chars = out.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\x1b' {
                    if chars.next() == Some('[') {
                        for d in chars.by_ref() {
                            if d.is_ascii_alphabetic() {
                                break;
                            }
                        }
                    }
                } else {
                    s.push(c);
                }
            }
            s
        };
        assert!(plain.contains("fn main"));
        assert!(out.contains("\x1b["));
    }

    #[test]
    fn auto_skips_json() {
        let json = r#"{"ok":true,"n":1}"#;
        // Force-enabled path still skips JSON via looks_like_json when calling auto…
        // Exercise looks_like_json + colorize path separately.
        assert!(looks_like_json(json));
        let md = "```json\n{\"a\":1}\n```\n";
        let coloured = colorize_markdown(md);
        assert!(coloured.contains("\x1b["));
    }

    #[test]
    fn colorize_code_rust_emits_ansi() {
        let out = colorize_code("fn main() { let x = 1; }\n", Some("rust"), None);
        assert!(out.contains("\x1b["));
        assert!(out.contains("main"));
    }
}
