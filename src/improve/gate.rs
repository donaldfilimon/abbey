//! Production gate runner — spawn `./check.sh` (or `ABBEY_CHECK_CMD`).
//!
//! Abbey does not reimplement the gate; it captures exit + output, clamps the
//! excerpt for agent prompts, and classifies fail kinds for diagnose framing.

use crate::agent::truncate_utf8_bytes;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// Soft ceiling for gate excerpt injected into agent prompts (please-fix sized).
const MAX_GATE_EXCERPT_BYTES: usize = 24 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    Fmt,
    Clippy,
    Test,
    FileSize,
    Toolchain,
    Other,
}

impl FailKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::Clippy => "clippy",
            Self::Test => "test",
            Self::FileSize => "file-size",
            Self::Toolchain => "toolchain",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateReport {
    pub ok: bool,
    pub exit: i32,
    /// Clamped, signal-first view of stdout+stderr. The full text is only ever
    /// consumed locally by `classify_failures` + `clamp_excerpt`.
    pub excerpt: String,
    pub kinds: Vec<FailKind>,
    pub elapsed_ms: u128,
    pub cmd: String,
}

impl GateReport {
    pub fn kinds_csv(&self) -> String {
        if self.kinds.is_empty() {
            if self.ok {
                "ok".into()
            } else {
                "unknown".into()
            }
        } else {
            self.kinds
                .iter()
                .map(|k| k.label())
                .collect::<Vec<_>>()
                .join(",")
        }
    }
}

pub fn resolve_check_cmd(root: &Path) -> Result<(String, Vec<String>)> {
    if let Ok(raw) = std::env::var("ABBEY_CHECK_CMD") {
        let mut parts = shell_split(&raw);
        if parts.is_empty() {
            bail!("ABBEY_CHECK_CMD is empty");
        }
        let prog = parts.remove(0);
        return Ok((prog, parts));
    }
    let script = root.join("check.sh");
    if !script.exists() {
        bail!(
            "improve: no check.sh at {} (set ABBEY_CHECK_CMD to override)",
            script.display()
        );
    }
    Ok((script.display().to_string(), Vec::new()))
}

/// Naive whitespace split — enough for `ABBEY_CHECK_CMD="cargo test"` style overrides.
fn shell_split(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

pub fn run_gate(root: &Path) -> Result<GateReport> {
    let (prog, args) = resolve_check_cmd(root)?;
    let cmd_display = if args.is_empty() {
        prog.clone()
    } else {
        format!("{prog} {}", args.join(" "))
    };
    let started = Instant::now();
    let out = Command::new(&prog)
        .args(&args)
        .current_dir(root)
        .output()
        .with_context(|| format!("spawn gate `{cmd_display}`"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut combined = String::new();
    if !stdout.is_empty() {
        combined.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }
    let exit = out.status.code().unwrap_or(1);
    let ok = out.status.success();
    let kinds = if ok {
        Vec::new()
    } else {
        classify_failures(&combined)
    };
    let excerpt = clamp_excerpt(&combined);
    Ok(GateReport {
        ok,
        exit,
        excerpt,
        kinds,
        elapsed_ms: started.elapsed().as_millis(),
        cmd: cmd_display,
    })
}

pub fn clamp_excerpt(body: &str) -> String {
    let summarized = prefer_signal_tail(body);
    truncate_utf8_bytes(&summarized, MAX_GATE_EXCERPT_BYTES)
}

/// Keep failure-shaped lines and a short head/tail so agents see the signal.
fn prefer_signal_tail(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    if lines.len() <= 120 {
        return body.to_string();
    }
    let mut signal: Vec<&str> = Vec::new();
    for line in &lines {
        let lower = line.to_ascii_lowercase();
        if lower.contains("error")
            || lower.contains("failed")
            || lower.contains("FAIL ")
            || lower.contains("warning:")
            || lower.contains("error[")
            || lower.contains("clippy")
            || lower.starts_with("== ")
        {
            signal.push(line);
        }
    }
    if signal.is_empty() {
        let head = lines.iter().take(40);
        let tail = lines.iter().rev().take(80).collect::<Vec<_>>();
        let mut out = String::new();
        for l in head {
            out.push_str(l);
            out.push('\n');
        }
        out.push_str("\n…\n\n");
        for l in tail.into_iter().rev() {
            out.push_str(l);
            out.push('\n');
        }
        return out;
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = String::from("## Gate failure signals\n");
    for line in signal.into_iter().take(200) {
        let key = line.trim();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub fn classify_failures(output: &str) -> Vec<FailKind> {
    let lower = output.to_ascii_lowercase();
    let mut kinds = Vec::new();
    let push = |kinds: &mut Vec<FailKind>, k: FailKind| {
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    };
    if lower.contains("is not supported by the following packages")
        || lower.contains("check.sh: the active toolchain")
        || (lower.contains("== toolchain ==") && lower.contains("failed"))
    {
        push(&mut kinds, FailKind::Toolchain);
    }
    if lower.contains("== rustfmt ==")
        || lower.contains("cargo fmt")
        || lower.contains("diff in") && (lower.contains("rustfmt") || lower.contains("cargo fmt"))
    {
        push(&mut kinds, FailKind::Fmt);
    }
    if lower.contains("== clippy") || lower.contains("cargo clippy") || lower.contains("clippy::") {
        push(&mut kinds, FailKind::Clippy);
    }
    if lower.contains("== test")
        || lower.contains("test result: failed")
        || lower.contains("failures:")
        || lower.contains("failed (") && lower.contains("test")
    {
        push(&mut kinds, FailKind::Test);
    }
    if lower.contains("file size guard")
        || lower.contains("fail src/") && lower.contains("lines")
        || lower.contains("hard max 1000")
        || lower.contains("max 200")
    {
        push(&mut kinds, FailKind::FileSize);
    }
    if kinds.is_empty() && !output.trim().is_empty() {
        push(&mut kinds, FailKind::Other);
    }
    kinds
}

pub fn wants_security_lane(kinds: &[FailKind]) -> bool {
    // File-size / fmt rarely need security; clippy+test red might.
    kinds
        .iter()
        .any(|k| matches!(k, FailKind::Clippy | FailKind::Test | FailKind::Other))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_clippy_and_test() {
        let out = r#"
== rustfmt ==
ok
== clippy ==
error: unused import
== test ==
test foo ... FAILED
failures:
    foo
"#;
        let k = classify_failures(out);
        assert!(k.contains(&FailKind::Clippy));
        assert!(k.contains(&FailKind::Test));
    }

    #[test]
    fn classify_toolchain() {
        let out = "error: rustc 1.97.1 is not supported by the following packages\ncheck.sh: the active toolchain is too old\n";
        let k = classify_failures(out);
        assert!(k.contains(&FailKind::Toolchain));
    }

    #[test]
    fn classify_filesize() {
        let out = "== file size guard ==\nFAIL src/foo.rs: 1001 lines (hard max 1000)\n";
        let k = classify_failures(out);
        assert!(k.contains(&FailKind::FileSize));
    }

    #[test]
    fn clamp_excerpt_caps_bytes() {
        let huge = "error: boom\n".repeat(5000);
        let ex = clamp_excerpt(&huge);
        assert!(ex.len() <= MAX_GATE_EXCERPT_BYTES);
        assert!(ex.contains("error") || ex.contains("Gate failure"));
    }

    #[test]
    fn shell_split_basic() {
        assert_eq!(
            shell_split("cargo test --quiet"),
            vec!["cargo", "test", "--quiet"]
        );
    }
}
