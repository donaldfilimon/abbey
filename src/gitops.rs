//! Local git helpers shared by CLI and TUI slash commands.

use anyhow::{Result, bail};
use std::process::Command;

pub fn run_git(args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("git {} failed: {}", args.join(" "), err.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn is_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn diff_text(staged: bool) -> Result<String> {
    if !is_repo() {
        bail!("not a git repository");
    }
    let diff = if staged {
        run_git(&["diff", "--cached"])?
    } else {
        let d = run_git(&["diff", "HEAD"]).unwrap_or_default();
        if d.trim().is_empty() {
            run_git(&["diff"])?
        } else {
            d
        }
    };
    if diff.trim().is_empty() {
        bail!("no git diff (working tree clean / nothing staged)");
    }
    Ok(truncate_diff(diff, 4000))
}

pub fn truncate_diff(diff: String, max_lines: usize) -> String {
    let lines = diff.lines().count();
    if lines <= max_lines {
        return diff;
    }
    let mut out = diff.lines().take(max_lines).collect::<Vec<_>>().join("\n");
    out.push_str(&format!(
        "\n... [truncated at {max_lines} lines; {lines} total]\n"
    ));
    out
}

pub fn create_branch(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        bail!("usage: /branch <name>");
    }
    if !is_repo() {
        bail!("not a git repository");
    }
    // Prefer switch -c (modern) then checkout -b
    if run_git(&["switch", "-c", name]).is_ok() {
        return Ok(format!("created and switched to branch {name}"));
    }
    run_git(&["checkout", "-b", name])?;
    Ok(format!("created and switched to branch {name}"))
}

pub fn pr_context() -> Result<String> {
    if !is_repo() {
        bail!("not a git repository");
    }
    let branch = run_git(&["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string();
    let log = run_git(&["log", "--oneline", "-20"]).unwrap_or_default();
    let diff = run_git(&["diff", "main...HEAD"])
        .or_else(|_| run_git(&["diff", "master...HEAD"]))
        .or_else(|_| run_git(&["diff", "HEAD~10..HEAD"]))
        .unwrap_or_else(|_| run_git(&["diff", "HEAD"]).unwrap_or_default());
    let diff = truncate_diff(diff, 2500);
    Ok(format!(
        "Branch: {branch}\n\nRecent commits:\n{log}\n\nDiff:\n```diff\n{diff}\n```"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_diff_short_passthrough() {
        let d = "a\nb\nc".to_string();
        assert_eq!(truncate_diff(d.clone(), 10), d);
    }

    #[test]
    fn truncate_diff_caps_lines() {
        let d = (0..20)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = truncate_diff(d, 5);
        assert!(out.contains("truncated at 5 lines; 20 total"));
        assert_eq!(out.lines().filter(|l| l.starts_with("line")).count(), 5);
    }
}
