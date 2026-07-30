//! Cross-platform OS control — dry-run by default; execute requires `--confirm`.
//!
//! Policy mirrors ABI's claim-honest gate: whitelist only, no shell strings.

use anyhow::{Result, bail};
use std::process::Command;

/// Commands allowed on all platforms (basename match).
const ALLOWED_COMMON: &[&str] = &["whoami", "hostname", "echo", "date", "uname"];

#[cfg(unix)]
const ALLOWED_UNIX: &[&str] = &["pwd", "ls", "true", "false", "env", "id", "uptime"];

#[cfg(windows)]
const ALLOWED_WIN: &[&str] = &["cd", "dir", "ver", "where", "set"];

fn allowed_names() -> Vec<&'static str> {
    let mut v = ALLOWED_COMMON.to_vec();
    #[cfg(unix)]
    v.extend_from_slice(ALLOWED_UNIX);
    #[cfg(windows)]
    v.extend_from_slice(ALLOWED_WIN);
    v
}

fn is_allowed(cmd: &str) -> bool {
    let base = std::path::Path::new(cmd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd)
        .to_ascii_lowercase();
    // Strip .exe on Windows
    let base = base.trim_end_matches(".exe");
    allowed_names().contains(&base)
}

/// Prefer `abi agent os …` when abi is on PATH (shared policy); else local whitelist.
pub fn run_os(args: &[String], prefer_abi: bool) -> Result<i32> {
    if prefer_abi {
        if let Some(abi) = crate::agent::which_bin("abi") {
            let mut cmd = Command::new(abi);
            cmd.arg("agent").arg("os");
            cmd.args(args);
            let st = cmd.status()?;
            return Ok(st.code().unwrap_or(1));
        }
    }
    local_os(args)
}

fn local_os(args: &[String]) -> Result<i32> {
    if args.is_empty() {
        bail!(
            "usage: abbey os <dry-run|execute --confirm> <cmd> [args...]\n\
             allowed: {}",
            allowed_names().join(", ")
        );
    }
    let mode = args[0].as_str();
    let rest = &args[1..];
    match mode {
        "dry-run" | "plan" => {
            if rest.is_empty() {
                bail!("usage: abbey os dry-run <cmd> [args...]");
            }
            let cmd = &rest[0];
            if !is_allowed(cmd) {
                bail!("denied: `{cmd}` is not on the OS-control allowlist");
            }
            let cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".into());
            println!(
                "dry-run: os={} arch={} cwd=\"{cwd}\" argv={rest:?} allowed=true",
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            println!("note: execute requires: abbey os execute --confirm {cmd} …");
            Ok(0)
        }
        "execute" | "run" => {
            let mut rest = rest;
            if rest.first().map(|s| s.as_str()) != Some("--confirm") {
                bail!("execute requires --confirm (safety gate)");
            }
            rest = &rest[1..];
            if rest.is_empty() {
                bail!("usage: abbey os execute --confirm <cmd> [args...]");
            }
            let cmd = &rest[0];
            if !is_allowed(cmd) {
                bail!("denied: `{cmd}` is not on the OS-control allowlist");
            }
            let out = Command::new(cmd).args(&rest[1..]).output()?;
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            print!("{stdout}");
            eprint!("{stderr}");
            Ok(out.status.code().unwrap_or(1))
        }
        "allowlist" | "policy" => {
            println!(
                "os-control allowlist ({}/{}):",
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            for a in allowed_names() {
                println!("  {a}");
            }
            println!(
                "backend: local whitelist (abi agent os used when abi on PATH and prefer_abi)"
            );
            Ok(0)
        }
        other => bail!("unknown os mode `{other}` (dry-run|execute|allowlist)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_has_whoami() {
        assert!(is_allowed("whoami"));
        assert!(!is_allowed("rm"));
        assert!(!is_allowed("curl"));
    }

    #[test]
    fn dry_run_ok() {
        let code = local_os(&["dry-run".into(), "whoami".into()]).unwrap();
        assert_eq!(code, 0);
    }
}
