//! Validate learning arguments before any store or route access.

use anyhow::{Result, bail};

pub(super) fn count(args: &[String], default: usize) -> Result<usize> {
    match args {
        [] => Ok(default),
        [n] if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => Ok(n.parse::<usize>()?),
        _ => bail!("expected at most one non-negative integer count"),
    }
}

pub(super) fn improve(args: &[String]) -> Result<(usize, bool)> {
    let mut apply = false;
    let mut counts = Vec::new();
    for arg in args {
        if arg == "--apply" {
            if apply {
                bail!("--apply may only be specified once");
            }
            apply = true;
        } else {
            counts.push(arg.clone());
        }
    }
    Ok((count(&counts, 20)?, apply))
}

pub(super) fn validate(args: &[String]) -> Result<()> {
    let tail = &args[1..];
    match args[0].as_str() {
        "routes" | "review" => {
            count(tail, 20)?;
        }
        "improve" => {
            improve(tail)?;
        }
        "status" | "show" | "help" | "-h" | "--help" | "stats" | "digest" | "sft"
            if !tail.is_empty() =>
        {
            bail!("learn {} takes no arguments", args[0]);
        }
        "export" if tail.len() > 1 || tail.first().is_some_and(|s| s.starts_with('-')) => {
            bail!("usage: abbey learn export [layer]");
        }
        _ => {}
    }
    Ok(())
}
