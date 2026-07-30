//! Language/tooling detectors for `/init` scans.

use super::probe::ProjectProbe;
use std::fs;
use std::path::Path;

pub(crate) fn push_unique(list: &mut Vec<&'static str>, item: &'static str) {
    if !list.contains(&item) {
        list.push(item);
    }
}

pub(crate) fn detect_rust(root: &Path, probe: &mut ProjectProbe) {
    let cargo = root.join("Cargo.toml");
    if !cargo.is_file() {
        return;
    }
    push_unique(&mut probe.languages, "Rust");
    probe.package_files.push("Cargo.toml".into());
    if let Ok(text) = fs::read_to_string(&cargo) {
        if let Some(n) = toml_string(&text, "name") {
            probe.name = n;
        }
        if let Some(d) = toml_string(&text, "description") {
            probe.description = Some(d);
        }
        if text.contains("edition = \"2024\"") || text.contains("edition = '2024'") {
            probe
                .gotchas
                .push("Rust edition 2024 — needs a recent nightly/stable that supports it.".into());
        }
    }
    if root.join("rust-toolchain.toml").is_file() || root.join("rust-toolchain").is_file() {
        probe.package_files.push("rust-toolchain.toml".into());
        probe.gotchas.push(
            "Toolchain is pinned via rust-toolchain — use that channel, not a random rustc.".into(),
        );
    }
    probe.build.push("cargo build --release".into());
    probe.test.push("cargo test".into());
    probe.lint.push("cargo clippy --all-targets".into());
    if root.join("install.sh").is_file() {
        probe.build.push("./install.sh".into());
    }
}

pub(crate) fn detect_node(root: &Path, probe: &mut ProjectProbe) {
    let pkg = root.join("package.json");
    if !pkg.is_file() {
        return;
    }
    push_unique(&mut probe.languages, "JavaScript/TypeScript");
    probe.package_files.push("package.json".into());
    let pm = if root.join("bun.lockb").is_file() || root.join("bun.lock").is_file() {
        "bun"
    } else if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if root.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    };
    if let Ok(text) = fs::read_to_string(&pkg) {
        if let Some(n) = json_string(&text, "name") {
            probe.name = n;
        }
        if let Some(d) = json_string(&text, "description") {
            probe.description = Some(d);
        }
        if text.contains("\"build\"") {
            probe.build.push(format!("{pm} run build"));
        }
        if text.contains("\"test\"") {
            probe.test.push(format!("{pm} test"));
        }
        if text.contains("\"lint\"") {
            probe.lint.push(format!("{pm} run lint"));
        }
    }
    if probe.build.is_empty() {
        probe.build.push(format!("{pm} install"));
    }
    probe.gotchas.push(format!(
        "Package manager appears to be `{pm}` — stick to it."
    ));
}

pub(crate) fn detect_zig(root: &Path, probe: &mut ProjectProbe) {
    if !(root.join("build.zig").is_file() || root.join("build.zig.zon").is_file()) {
        return;
    }
    push_unique(&mut probe.languages, "Zig");
    if root.join("build.zig").is_file() {
        probe.package_files.push("build.zig".into());
    }
    if root.join("build.zig.zon").is_file() {
        probe.package_files.push("build.zig.zon".into());
    }
    if root.join(".zigversion").is_file() {
        probe.package_files.push(".zigversion".into());
        if let Ok(v) = fs::read_to_string(root.join(".zigversion")) {
            probe.gotchas.push(format!(
                "Zig is pinned to `{}` via `.zigversion` — mismatch breaks the build.",
                v.trim()
            ));
        }
    }
    if root.join("build.sh").is_file() {
        probe.build.push("./build.sh check".into());
        probe.test.push("./build.sh test".into());
    } else {
        probe.build.push("zig build".into());
        probe.test.push("zig build test".into());
    }
}

pub(crate) fn detect_go(root: &Path, probe: &mut ProjectProbe) {
    if !root.join("go.mod").is_file() {
        return;
    }
    push_unique(&mut probe.languages, "Go");
    probe.package_files.push("go.mod".into());
    probe.build.push("go build ./...".into());
    probe.test.push("go test ./...".into());
}

pub(crate) fn detect_python(root: &Path, probe: &mut ProjectProbe) {
    let mut hit = false;
    for name in ["pyproject.toml", "requirements.txt", "setup.py", "Pipfile"] {
        if root.join(name).is_file() {
            hit = true;
            probe.package_files.push(name.into());
        }
    }
    if !hit {
        return;
    }
    push_unique(&mut probe.languages, "Python");
    if root.join("pyproject.toml").is_file() {
        probe.build.push("pip install -e .".into());
        probe.test.push("pytest".into());
    } else {
        probe.build.push("pip install -r requirements.txt".into());
        probe.test.push("pytest".into());
    }
}

pub(crate) fn detect_swift(root: &Path, probe: &mut ProjectProbe) {
    if !root.join("Package.swift").is_file() {
        return;
    }
    push_unique(&mut probe.languages, "Swift");
    probe.package_files.push("Package.swift".into());
    probe.build.push("swift build".into());
    probe.test.push("swift test".into());
}

pub(crate) fn detect_make(root: &Path, probe: &mut ProjectProbe) {
    if root.join("Makefile").is_file() || root.join("makefile").is_file() {
        probe.package_files.push("Makefile".into());
        if probe.build.is_empty() {
            probe.build.push("make".into());
        }
        if probe.test.is_empty() {
            probe.test.push("make test".into());
        }
    }
}

pub(crate) fn detect_cmake(root: &Path, probe: &mut ProjectProbe) {
    if !root.join("CMakeLists.txt").is_file() {
        return;
    }
    push_unique(&mut probe.languages, "C/C++");
    probe.package_files.push("CMakeLists.txt".into());
    if probe.build.is_empty() {
        probe
            .build
            .push("cmake -B build && cmake --build build".into());
    }
}

pub(crate) fn detect_git(root: &Path, probe: &mut ProjectProbe) {
    if root.join(".git").exists() {
        return;
    }
    // May be a worktree / subdirectory of a repo
    let ok = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        probe.gotchas.push(
            "Not inside a git work tree — commit/PR helpers will fail until `git init`.".into(),
        );
    }
}

/// Minimal TOML string extractor for `key = "value"` (good enough for Cargo.toml).
fn toml_string(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim();
        if let Some(s) = strip_quotes(rest) {
            return Some(s);
        }
    }
    None
}

fn json_string(text: &str, key: &str) -> Option<String> {
    // Prefer first occurrence of "key": "value"
    let needle = format!("\"{key}\"");
    let mut search = text;
    while let Some(idx) = search.find(&needle) {
        let after = search[idx + needle.len()..].trim_start();
        if let Some(after) = after.strip_prefix(':') {
            let after = after.trim_start();
            if let Some(s) = strip_quotes(after.split([',', '\n']).next().unwrap_or(after).trim()) {
                return Some(s);
            }
        }
        search = &search[idx + needle.len()..];
    }
    None
}

fn strip_quotes(s: &str) -> Option<String> {
    let s = s.trim().trim_end_matches(',');
    if let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return Some(inner.to_string());
    }
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return Some(inner.to_string());
    }
    None
}
