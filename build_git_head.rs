// Where the Git metadata that a build stamp depends on actually lives.
//
// Shared by `build.rs` (which watches those paths so a commit switch
// re-stamps the build) and the integration test that proves the resolution
// works in a linked worktree. `build.rs` cannot depend on the crate it
// builds, so the logic lives here and is included by both.
//
// The header uses plain comments, not `//!`: this file is `include!`d into
// the build script's crate root, where an inner doc comment is only legal at
// the very top of the file.
//
// The build script calls the two wrappers below; the integration test calls
// the directory-scoped variants, because a test must not change the process
// working directory. Both live here so neither copy can drift.

use std::path::{Path, PathBuf};

/// The directory that holds this checkout's Git metadata.
///
/// `git rev-parse --absolute-git-dir` is the answer in every layout: a plain
/// checkout, a linked worktree (where `.git` is a *file* pointing at the
/// real directory), and a submodule. Reading `.git/HEAD` directly only works
/// in the first layout, and a build that silently registers no Git metadata
/// reuses the previous stamp after a commit switch.
#[allow(dead_code)]
pub(crate) fn git_metadata_dir() -> Option<PathBuf> {
    git_metadata_dir_in(Path::new("."))
}

/// [`git_metadata_dir`] for a checkout in `dir`.
pub(crate) fn git_metadata_dir_in(dir: &Path) -> Option<PathBuf> {
    if let Some(dir) = run_git_in(dir, &["rev-parse", "--absolute-git-dir"]) {
        return Some(dir);
    }
    // No usable `git` binary: fall back to the plain-checkout layout, and to
    // nothing at all when there is no `.git` directory (a tarball build).
    let dot_git = dir.join(".git");
    dot_git.is_dir().then_some(dot_git)
}

/// The directory that holds the *refs*.
///
/// In a linked worktree the refs live in the repository's common directory,
/// not in the worktree's own metadata directory, so a branch ref is watched
/// where it actually is.
#[allow(dead_code)]
pub(crate) fn git_common_dir(metadata_dir: &Path) -> PathBuf {
    git_common_dir_in(metadata_dir).unwrap_or_else(|| metadata_dir.to_path_buf())
}

/// [`git_common_dir`] for a checkout whose metadata lives in `metadata_dir`.
///
/// The lookup runs in the metadata directory, which every layout can enter.
pub(crate) fn git_common_dir_in(metadata_dir: &Path) -> Option<PathBuf> {
    run_git_in(
        metadata_dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
}

/// Run `git` in `dir` and return its trimmed output, if it succeeded.
fn run_git_in(dir: &Path, args: &[&str]) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let line = text.trim();
    if line.is_empty() {
        return None;
    }
    Some(PathBuf::from(line))
}
