//! Portable host helpers — PATH lookup, install/state locations, argv budgets.
//!
//! Keeps linux/macos/windows primary-target support honest: Windows needs
//! PATHEXT-aware binary discovery and a tighter CreateProcess argv budget.

use std::path::{Path, PathBuf};

/// Soft ceiling for a single prompt argv string (platform-aware).
///
/// - Unix: macOS/Linux `ARG_MAX` is typically hundreds of KiB; 96 KiB leaves room
///   for env + other argv.
/// - Windows: `CreateProcess` command-line limit is ~32767 chars for the whole
///   line — keep each prompt chunk well under that.
pub fn max_prompt_argv_bytes() -> usize {
    if cfg!(windows) { 24 * 1024 } else { 96 * 1024 }
}

/// First matching executable on `PATH` (Windows: tries PATHEXT suffixes).
pub fn which_bin(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names = candidate_bin_names(bin);
    for dir in std::env::split_paths(&path) {
        for name in &names {
            let p = dir.join(name);
            if is_executable(&p) {
                return Some(p);
            }
        }
    }
    None
}

fn candidate_bin_names(bin: &str) -> Vec<String> {
    let mut names = vec![bin.to_string()];
    if cfg!(windows) {
        let lower = bin.to_ascii_lowercase();
        let has_ext = Path::new(bin)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| !e.is_empty());
        if !has_ext {
            let pathext = std::env::var("PATHEXT")
                .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.MSC".into());
            for ext in pathext.split(';').filter(|s| !s.is_empty()) {
                let ext = ext.trim();
                let with = format!("{bin}{ext}");
                if !names.iter().any(|n| n.eq_ignore_ascii_case(&with)) {
                    names.push(with);
                }
            }
            // Always try .exe even if PATHEXT is odd/empty.
            if !lower.ends_with(".exe") {
                let exe = format!("{bin}.exe");
                if !names.iter().any(|n| n.eq_ignore_ascii_case(&exe)) {
                    names.push(exe);
                }
            }
        }
    }
    names
}

fn is_executable(path: &Path) -> bool {
    // Match historical Abbey behavior: presence on PATH as a regular file.
    // (Avoid false negatives for odd install layouts without the exec bit.)
    path.is_file()
}

/// Well-known locations for the active backend binary (before PATH).
pub fn agent_candidate_paths(backend: &str, home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    match backend {
        "grok" => {
            out.push(home.join(".grok/bin/grok"));
            out.push(home.join(".local/bin/grok"));
            out.push(PathBuf::from("/opt/homebrew/bin/grok"));
            #[cfg(windows)]
            {
                out.push(home.join(".grok\\bin\\grok.exe"));
                out.push(home.join(".local\\bin\\grok.exe"));
                if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                    out.push(PathBuf::from(local).join("grok\\grok.exe"));
                }
            }
        }
        "fm" => {
            out.push(PathBuf::from("/usr/bin/fm"));
        }
        "abi" => {
            // Never fall through to the cursor arm — that would make
            // ABBEY_BACKEND=abi exec cursor-agent whenever it sits in
            // ~/.local/bin (the common install layout).
            out.push(home.join(".local/bin/abi"));
            out.push(home.join(".cargo/bin/abi"));
            out.push(PathBuf::from("/opt/homebrew/bin/abi"));
            #[cfg(windows)]
            {
                out.push(home.join(".local\\bin\\abi.exe"));
                out.push(home.join(".cargo\\bin\\abi.exe"));
                if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                    out.push(PathBuf::from(local).join("abi\\abi.exe"));
                }
            }
        }
        // cursor (default)
        _ => {
            out.push(home.join(".local/bin/cursor-agent"));
            out.push(home.join(".local/bin/agent"));
            #[cfg(windows)]
            {
                out.push(home.join(".local\\bin\\cursor-agent.exe"));
                out.push(home.join(".local\\bin\\agent.exe"));
                if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                    let local = PathBuf::from(local);
                    out.push(local.join("cursor-agent\\cursor-agent.exe"));
                    out.push(local.join("Programs\\cursor\\resources\\app\\bin\\cursor-agent.exe"));
                }
            }
        }
    }
    out
}

/// Default install directory for the abbey binary on this host.
pub fn default_install_dir(home: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("abbey\\bin");
        }
        return home.join("AppData\\Local\\abbey\\bin");
    }
    #[cfg(not(windows))]
    {
        home.join(".local/bin")
    }
}

/// Human lines for `abbey platform paths`.
pub fn path_report_lines(state_dir: &Path, config_path: &Path) -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut lines = vec![
        format!(
            "os/arch:     {} / {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        format!("family:      {}", std::env::consts::FAMILY),
        format!(
            "exe:         {}",
            std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "?".into())
        ),
        format!("state:       {}", state_dir.display()),
        format!("config:      {}", config_path.display()),
        format!("install_dir: {}", default_install_dir(&home).display()),
        format!("argv_clamp:  {} bytes/prompt", max_prompt_argv_bytes()),
    ];
    if let Ok(path) = std::env::var("PATH") {
        let n = std::env::split_paths(&path).count();
        lines.push(format!("PATH_entries:{n}"));
    }
    #[cfg(windows)]
    {
        lines.push(format!(
            "PATHEXT:     {}",
            std::env::var("PATHEXT").unwrap_or_else(|_| "(default)".into())
        ));
    }
    lines.push(format!(
        "cursor-agent:{}",
        which_bin("cursor-agent")
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(not on PATH)".into())
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_budget_at_least_16k() {
        assert!(max_prompt_argv_bytes() >= 16 * 1024);
    }

    #[test]
    fn candidate_names_include_bare() {
        let names = candidate_bin_names("cursor-agent");
        assert!(names.iter().any(|n| n == "cursor-agent"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_candidates_include_exe() {
        let names = candidate_bin_names("cursor-agent");
        assert!(
            names
                .iter()
                .any(|n| n.eq_ignore_ascii_case("cursor-agent.exe"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_candidates_stay_bare() {
        let names = candidate_bin_names("cursor-agent");
        assert_eq!(names, vec!["cursor-agent".to_string()]);
    }

    #[test]
    fn install_dir_is_absolute_or_home_relative() {
        let home = PathBuf::from("/tmp/home");
        let d = default_install_dir(&home);
        assert!(!d.as_os_str().is_empty());
    }

    #[test]
    fn abi_candidates_never_include_cursor_agent() {
        let home = PathBuf::from("/tmp/home");
        let paths = agent_candidate_paths("abi", &home);
        assert!(
            paths
                .iter()
                .any(|p| p.ends_with("abi") || p.ends_with("abi.exe")),
            "expected an abi candidate, got {paths:?}"
        );
        for p in &paths {
            let s = p.to_string_lossy();
            assert!(
                !s.contains("cursor-agent")
                    && !s.ends_with("/agent")
                    && !s.ends_with("\\agent.exe"),
                "abi backend must not fall through to cursor paths: {s}"
            );
        }
    }
}
