// Unique compilation identity for Abbey builds.
//
// The Git-metadata resolution lives in `build_git_head.rs` so the same code
// can be proven by an integration test: a build script cannot depend on the
// crate it builds. It is included rather than a module so the file stays
// usable by the integration test as well.
include!("build_git_head.rs");

fn main() {
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".into());
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".into());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let git = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "nogit".into());

    let host = std::env::var("HOST").unwrap_or_else(|_| "unknown".into());
    let stamp = format!("abbey-{git}-{profile}-{target}-t{now}");

    println!("cargo:rustc-env=ABBEY_BUILD_STAMP={stamp}");
    println!("cargo:rustc-env=ABBEY_BUILD_GIT={git}");
    println!("cargo:rustc-env=ABBEY_BUILD_TARGET={target}");
    println!("cargo:rustc-env=ABBEY_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=ABBEY_BUILD_HOST={host}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_git_head.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    watch_git_head();
}

/// Re-run this script when the checked-out commit changes.
///
/// Without this, cargo caches the build-script output across commits: the
/// binary keeps reporting whatever sha was current the last time `build.rs` or
/// `Cargo.toml` happened to change. `abbey doctor` then points a debugger at
/// the wrong commit, and the "unique build stamp" claim is false — observed
/// live with HEAD three commits ahead of the reported sha.
///
/// `.git/HEAD` covers checkouts and branch switches. When it is a symbolic ref
/// the branch file must be watched too, since committing advances that file and
/// leaves `.git/HEAD` untouched. A branch whose ref lives only in `packed-refs`
/// has no loose file to watch; that path is covered once the first commit on it
/// writes one.
///
/// The metadata directory is resolved, not assumed: in a linked worktree
/// `.git` is a *file* that points at the real metadata directory, so
/// `.git/HEAD` does not exist there and reading it directly would register no
/// Git metadata at all — a commit switch would then reuse the previous
/// stamp. The refs of a linked worktree live in the repository's common
/// directory, so that is where the branch file is watched.
fn watch_git_head() {
    let Some(git_dir) = git_metadata_dir() else {
        return; // building from a tarball / vendored source
    };
    let head = git_dir.join("HEAD");
    if !head.exists() {
        return; // building from a tarball / vendored source
    }
    println!("cargo:rerun-if-changed={}", head.display());

    let Ok(contents) = std::fs::read_to_string(&head) else {
        return;
    };
    if let Some(reference) = contents.strip_prefix("ref:") {
        let common_dir = git_common_dir(&git_dir);
        let ref_path = common_dir.join(reference.trim());
        if ref_path.exists() {
            println!("cargo:rerun-if-changed={}", ref_path.display());
        } else {
            // Packed ref: watch the pack file so re-packing still re-stamps.
            let packed = common_dir.join("packed-refs");
            if packed.exists() {
                println!("cargo:rerun-if-changed={}", packed.display());
            }
        }
    }
}
