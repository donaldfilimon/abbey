// Unique compilation identity for Abbey builds.
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
    println!("cargo:rerun-if-changed=Cargo.toml");
}
