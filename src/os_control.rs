//! Cross-platform OS control — dry-run by default; execute requires `--confirm`.
//!
//! Policy mirrors ABI's claim-honest gate: whitelist only, no shell strings.
//! Unrestricted / autonomous OS is Out of scope (`abbey shell refuse`).
//!
//! Allowlists are real executables only (no shell builtins like Windows `cd`/`dir`).

use anyhow::{Context, Result, bail};
use std::process::Command;

/// Portable commands that exist as real binaries on both Unix and Windows.
const ALLOWED_COMMON: &[&str] = &["whoami", "hostname"];

#[cfg(unix)]
const ALLOWED_UNIX: &[&str] = &[
    "echo", "date", "uname", "pwd", "ls", "true", "false", "id", "uptime",
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
    // Approve names, never caller-selected paths (including Windows separators
    // on Unix). A matching basename is not evidence of a trusted executable.
    if cmd.contains(['/', '\\', ':']) {
        return false;
    }
    let base = cmd;
    let base = if cfg!(windows) {
        base.to_ascii_lowercase()
    } else {
        base.to_string()
    };
    let base = if cfg!(windows) {
        base.strip_suffix(".exe").unwrap_or(&base)
    } else {
        &base
    };
    allowed_names().contains(&base)
}

/// Enforce Abbey's policy locally, independently of installed peer policies.
/// The preference argument is retained for caller compatibility, not delegation.
pub fn run_os(args: &[String], _prefer_abi: bool) -> Result<i32> {
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
    local_os(args)
}

/// Ignore PATH: an allowlisted name must resolve in an OS-owned binary directory.
fn trusted_binary(cmd: &str) -> Result<std::path::PathBuf> {
    if !is_allowed(cmd) {
        bail!("denied: `{cmd}` is not on the OS-control allowlist");
    }
    #[cfg(unix)]
    let candidates = ["/usr/bin", "/bin"].map(|dir| std::path::Path::new(dir).join(cmd));
    #[cfg(windows)]
    let candidates = {
        let name = cmd.to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        [windows_system_directory()?.join(format!("{name}.exe"))]
    };
    #[cfg(not(any(unix, windows)))]
    let candidates: [std::path::PathBuf; 0] = [];
    candidates
        .into_iter()
        .find(|p| crate::host::is_executable(p))
        .with_context(|| format!("trusted system executable `{cmd}` is unavailable"))
}

/// Ask Windows directly; neither PATH nor the mutable SystemRoot environment
/// variable may redirect this execution authority.
#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_system_directory() -> Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    // SAFETY: buffer is writable for the supplied number of UTF-16 code units;
    // the Windows API retains no pointer. Check failure/truncation before use.
    let size = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if size == 0 || size as usize >= buffer.len() {
        bail!("trusted Windows system directory is unavailable");
    }
    Ok(std::ffi::OsString::from_wide(&buffer[..size as usize]).into())
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
    println!("backend: local whitelist (system binary directories only; PATH ignored)");
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
            let bin = trusted_binary(cmd)?;
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

    #[test]
    fn paths_and_launchers_are_denied_even_with_peer_preference() {
        for cmd in [
            "env",
            "/somewhere/whoami",
            "./whoami",
            "C:\\tools\\whoami.exe",
            "whoami.exe.exe",
        ] {
            assert!(!is_allowed(cmd), "{cmd}");
            for prefer_abi in [false, true] {
                assert!(
                    run_os(
                        &["execute".into(), "--confirm".into(), cmd.into()],
                        prefer_abi
                    )
                    .is_err()
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn trusted_resolution_ignores_path_and_unix_exe_suffix() {
        assert!(!is_allowed("whoami.exe"));
        let path = trusted_binary("whoami").unwrap();
        assert!(
            ["/usr/bin", "/bin"]
                .iter()
                .any(|dir| path.parent() == Some(std::path::Path::new(dir)))
        );
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
