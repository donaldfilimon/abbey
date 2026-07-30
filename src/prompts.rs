//! Agent prompt builders for review/commit.

use crate::gitops;
use anyhow::{Result, bail};

pub fn build_review_prompt(staged: bool, note: &[String], security: bool) -> Result<String> {
    let diff = gitops::diff_text(staged)?;
    let focus = if security {
        "Focus on security vulnerabilities, injection, secrets, authz, and unsafe defaults (OWASP-minded)."
    } else {
        "Focus on bugs, security, regressions, and missing tests."
    };
    let label = if staged {
        "staged changes"
    } else {
        "working tree vs HEAD"
    };
    let mut prompt = format!(
        "Please review these {label}. {focus} Be specific with file:line references.\n\n```diff\n{diff}\n```"
    );
    if !note.is_empty() {
        prompt.push_str("\n\nAdditional context: ");
        prompt.push_str(&note.join(" "));
    }
    Ok(prompt)
}

pub fn build_commit_prompt() -> Result<String> {
    if !gitops::is_repo() {
        bail!("not a git repository");
    }
    let staged = gitops::run_git(&["diff", "--cached", "--stat"])?;
    if staged.trim().is_empty() {
        bail!("nothing staged. git add files first, then: abbey commit");
    }
    let mut diff = gitops::run_git(&["diff", "--cached"])?;
    diff = gitops::truncate_diff(diff, 3000);
    Ok(format!(
        "Write a concise conventional commit message for the staged changes below. Reply with ONLY the commit message (subject + optional body), no code fences, no explanation.\n\n```diff\n{diff}\n```"
    ))
}
