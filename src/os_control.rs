//! Cross-platform OS control — dry-run by default; execute requires `--confirm`.
//!
//! Policy mirrors ABI's claim-honest gate: whitelist only, no shell strings.
//! Unrestricted / autonomous OS is Out of scope (`abbey shell refuse`).
//!
//! Allowlists are real executables only (no shell builtins like Windows `cd`/`dir`).

use anyhow::{Result, bail};
use std::process::Command;

/// Portable commands that exist as real binaries on both Unix and Windows.
const ALLOWED_COMMON: &[&str] = &["whoami", "hostname"];

#[cfg(unix)]
const ALLOWED_UNIX: &[&str] = &[
    "echo", "date", "uname", "pwd", "ls", "true", "false", "env", "id", "uptime",
];

/// Windows System32 tools only — not cmd.exe builtins (`cd`, `dir`, `echo`, `ver`).
#[cfg(windows)]
const ALLOWED_WIN: &[&str] = &["where", "systeminfo"];

pub fn allowed_names() -> Vec<&'static str> {
    let mut v = ALLOWED_COMMON.to_vec();
    #[cfg(unix)]
    v.extend_from_slice(ALLOWED_UNIX);
    #[cfg(windows)]
    v.extend_from_slice(ALLOWED_WIN);
    v
}

/// Whether `cmd` names an allowlisted executable.
///
/// Matching is case-insensitive on Windows (its filesystem and `PATHEXT` are)
/// but **case-sensitive on Unix**, even though macOS ships a case-insensitive
/// filesystem by default. Folding case on Unix would let a caller name a
/// command the allowlist did not literally approve: `WHOAMI` resolves to the
/// same inode as `whoami`, but that inode is a multi-call binary that switches
/// on `argv[0]`, so it runs as `id` and prints uid/gid/groups instead of a
/// username. The command that executed would then differ from the one the
/// policy recorded — exactly the mismatch this surface exists to prevent.
pub fn is_allowed(cmd: &str) -> bool {
    let base = std::path::Path::new(cmd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd);
    let base = if cfg!(windows) {
        base.to_ascii_lowercase()
    } else {
        base.to_string()
    };
    let base = base.trim_end_matches(".exe");
    allowed_names().contains(&base)
}

/// Prefer `abi agent os …` when abi is on PATH (shared policy); else local whitelist.
pub fn run_os(args: &[String], prefer_abi: bool) -> Result<i32> {
    if args.is_empty()
        || matches!(
            args.first().map(String::as_str),
            Some("status" | "help" | "-h" | "--help")
        )
    {
        return print_policy(matches!(
            args.first().map(String::as_str),
            Some("help" | "-h" | "--help")
        ));
    }
    if prefer_abi
        && !matches!(
            args.first().map(String::as_str),
            Some("allowlist" | "policy" | "list" | "unrestricted" | "refuse" | "yolo")
        )
        && let Some(abi) = crate::agent::which_bin("abi")
    {
        let mut cmd = Command::new(abi);
        cmd.arg("agent").arg("os");
        cmd.args(args);
        let st = cmd.status()?;
        return Ok(st.code().unwrap_or(1));
    }
    local_os(args)
}

/// Print the allowlist + safety rules (also `abbey allowlist`).
pub fn print_policy(verbose_help: bool) -> Result<i32> {
    println!("abbey os — allowlist OS control (not an unrestricted shell)\n");
    println!(
        "host: {} / {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("allowlist ({} entries):", allowed_names().len());
    for a in allowed_names() {
        println!("  {a}");
    }
    println!();
    println!("modes:");
    println!("  dry-run|plan <cmd> [args…]     preview (no execute)");
    println!("  execute|run --confirm <cmd> …  run allowlisted only");
    println!("  allowlist|policy|list          this panel");
    println!("  refuse                         OOS: unrestricted shell");
    println!();
    println!("backend: local whitelist (abi agent os when abi on PATH + prefer_abi)");
    println!("rule:    execute always needs --confirm; real executables only (no shell strings)");
    println!("refuse:  abbey shell refuse · abbey claims refuse shell · abbey os refuse");
    if verbose_help {
        println!(
            "\nexamples:\n\
             \x20  abbey os dry-run whoami\n\
             \x20  abbey os execute --confirm whoami\n\
             \x20  abbey allowlist"
        );
    }
    Ok(0)
}

fn local_os(args: &[String]) -> Result<i32> {
    if args.is_empty() {
        return print_policy(false);
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
            if rest.first().map(String::as_str) != Some("--confirm") {
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
            // Resolve via PATH so Windows finds whoami.exe without an absolute path.
            let bin = crate::agent::which_bin(cmd).unwrap_or_else(|| std::path::PathBuf::from(cmd));
            let out = Command::new(&bin).args(&rest[1..]).output()?;
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            print!("{stdout}");
            eprint!("{stderr}");
            Ok(out.status.code().unwrap_or(1))
        }
        "allowlist" | "policy" | "list" => print_policy(false),
        "unrestricted" | "refuse" | "yolo" => crate::claims::refuse("shell"),
        other => bail!("unknown os mode `{other}` (dry-run|execute|allowlist|refuse)"),
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

    #[cfg(windows)]
    #[test]
    fn windows_allowlist_excludes_cmd_builtins() {
        assert!(!is_allowed("cd"));
        assert!(!is_allowed("dir"));
        assert!(!is_allowed("echo"));
        assert!(is_allowed("where"));
        assert!(is_allowed("whoami.exe"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_matching_is_case_sensitive() {
        // macOS ships a case-insensitive filesystem, so `WHOAMI` opens the same
        // inode as `whoami` — but that inode dispatches on argv[0] and runs as
        // `id`. Accepting the case variant would execute a different program
        // than the allowlist approved.
        assert!(is_allowed("whoami"));
        assert!(!is_allowed("WHOAMI"));
        assert!(!is_allowed("EcHo"));
        // Case folding must not become a way to reach a denied command either.
        assert!(!is_allowed("RM"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_allowlist_has_uname() {
        assert!(is_allowed("uname"));
        assert!(is_allowed("ls"));
    }

    #[test]
    fn dry_run_ok() {
        let code = local_os(&["dry-run".into(), "whoami".into()]).unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn empty_args_prints_policy() {
        let code = run_os(&[], false).unwrap();
        assert_eq!(code, 0);
        let code = local_os(&["allowlist".into()]).unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn execute_without_confirm_is_denied() {
        let err = local_os(&["execute".into(), "whoami".into()]).unwrap_err();
        assert!(err.to_string().contains("--confirm"));
    }

    #[test]
    fn dry_run_denies_off_list() {
        let err = local_os(&["dry-run".into(), "rm".into()]).unwrap_err();
        assert!(err.to_string().contains("allowlist"));
    }
}
