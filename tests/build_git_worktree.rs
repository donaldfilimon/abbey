//! The build stamp must follow the commit in every checkout layout.
//!
//! `build.rs` watches the Git metadata that a build stamp depends on, so a
//! commit switch re-stamps the build. The metadata is *resolved*, not
//! assumed: in a linked worktree `.git` is a file that points at the real
//! metadata directory, and the refs live in the repository's common
//! directory. Reading `.git/HEAD` directly — the layout a plain checkout
//! has — finds nothing in a worktree, and the build then silently reuses
//! the previous stamp.

#[path = "../build_git_head.rs"]
mod build_git_head;

use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `git` in `dir` and fail the test if it does not succeed.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A scratch directory that cleans itself up.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let base =
            std::env::temp_dir().join(format!("abbey-build-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("scratch directory");
        Self(base)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_linked_worktree_resolves_to_its_own_metadata_directory() {
    let scratch = Scratch::new("worktree");
    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repository");
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    git(&repo, &["config", "user.email", "gate@example.invalid"]);
    git(&repo, &["config", "user.name", "Gate"]);
    std::fs::write(repo.join("README.md"), "seed\n").expect("seed file");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "--quiet", "-m", "seed"]);

    let worktree = scratch.path().join("worktree");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            worktree.to_str().expect("path"),
        ],
    );

    // The linked checkout's `.git` is a file, so the old `.git/HEAD` path
    // does not exist there at all.
    assert!(
        !worktree.join(".git/HEAD").exists(),
        "a linked worktree must not have a `.git/HEAD` file"
    );

    let metadata = build_git_head::git_metadata_dir_in(&worktree).expect("metadata directory");
    assert!(
        metadata.join("HEAD").exists(),
        "the resolved metadata directory must hold HEAD: {}",
        metadata.display()
    );

    // The refs of a linked worktree live in the repository's common
    // directory, so the branch file is watched where it actually is.
    let common = build_git_head::git_common_dir_in(&metadata).expect("common directory");
    assert!(
        common.join("refs/heads/linked").exists(),
        "the linked branch ref must be found in the common directory: {}",
        common.display()
    );
}

#[test]
fn a_plain_checkout_resolves_to_its_own_dot_git_directory() {
    let scratch = Scratch::new("plain");
    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repository");
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(repo.join("README.md"), "seed\n").expect("seed file");
    git(&repo, &["add", "README.md"]);
    git(
        &repo,
        &[
            "-c",
            "user.email=gate@example.invalid",
            "-c",
            "user.name=Gate",
            "commit",
            "--quiet",
            "-m",
            "seed",
        ],
    );

    let metadata = build_git_head::git_metadata_dir_in(&repo).expect("metadata directory");
    assert!(metadata.join("HEAD").exists());
    let common = build_git_head::git_common_dir_in(&metadata).expect("common directory");
    assert!(common.join("refs/heads/main").exists());
}
