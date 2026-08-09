//! Generates the desktop client's TypeScript IPC contracts from Abbey's Rust
//! source, and writes edition identity into the artifacts that cannot call
//! Rust.
//!
//!   cargo run -p abbey-desktop-codegen            # write
//!   cargo run -p abbey-desktop-codegen -- --check # fail on drift
//!
//! `--check` is what makes "generated, not hand-written" enforceable: it
//! regenerates everything in memory and exits non-zero if any output on disk
//! differs, naming the file.

mod editions;
mod samples;
mod tsgen;

use abbey::edition::Edition;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Rust sources whose serde contracts become TypeScript, in emission order.
///
/// Paths are relative to the repository root. Adding an IPC type means adding
/// its file here — there is no second, hand-maintained TypeScript copy to keep
/// in step.
const SOURCES: &[(&str, &str)] = &[
    (
        "src/app_core/ids.rs",
        "src/app_core/ids.rs — opaque identifiers",
    ),
    (
        "src/app_core/contracts.rs",
        "src/app_core/contracts.rs — read-only application contracts",
    ),
    (
        "desktop/src-tauri/src/ipc.rs",
        "desktop/src-tauri/src/ipc.rs — desktop transport envelope",
    ),
];

/// Tauri configs whose bundle identity is written from `abbey::edition`.
const TAURI_CONFIGS: &[(&str, Edition)] = &[
    ("desktop/src-tauri/tauri.conf.json", Edition::Safe),
    (
        "desktop/src-tauri/tauri.personal.conf.json",
        Edition::Personal,
    ),
];

const TYPES_OUT: &str = "desktop/src/ipc/generated.ts";
const SAMPLES_OUT: &str = "desktop/src/ipc/generated.samples.ts";
const EDITIONS_OUT: &str = "desktop/src/ipc/generated.editions.ts";

fn main() -> Result<()> {
    let check_only = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        other => bail!("usage: abbey-desktop-codegen [--check] (received {other:?})"),
    };

    let root = repo_root()?;
    let mut outputs: Vec<(PathBuf, String)> = Vec::new();

    let mut sources = Vec::new();
    for (relative, label) in SOURCES {
        sources.push(tsgen::parse(&root.join(relative), label)?);
    }
    outputs.push((root.join(TYPES_OUT), tsgen::render(&sources)?));
    outputs.push((root.join(SAMPLES_OUT), samples::render()?));
    outputs.push((root.join(EDITIONS_OUT), editions::render_typescript()?));

    for (relative, edition) in TAURI_CONFIGS {
        let path = root.join(relative);
        let existing = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        outputs.push((path, editions::patch_tauri_config(&existing, *edition)?));
    }

    let mut drifted = Vec::new();
    for (path, expected) in &outputs {
        let current = std::fs::read_to_string(path).ok();
        if current.as_deref() == Some(expected.as_str()) {
            continue;
        }
        if check_only {
            drifted.push(path.clone());
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        std::fs::write(path, expected)
            .with_context(|| format!("cannot write {}", path.display()))?;
        println!("wrote {}", display_relative(&root, path));
    }

    if !drifted.is_empty() {
        for path in &drifted {
            eprintln!("stale: {}", display_relative(&root, path));
        }
        bail!(
            "{} generated file(s) are out of date; run `cd desktop && bun run codegen`",
            drifted.len()
        );
    }
    if check_only {
        println!("codegen: {} generated file(s) up to date", outputs.len());
    }
    Ok(())
}

/// The repository root, resolved from this crate's manifest directory
/// (`<root>/desktop/codegen`) so the generator works from any cwd.
fn repo_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .context("cannot resolve the repository root from CARGO_MANIFEST_DIR")?
        .to_path_buf();
    if !root.join("src/app_core/contracts.rs").is_file() {
        bail!(
            "{} does not look like the abbey repository root",
            root.display()
        );
    }
    Ok(root)
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
