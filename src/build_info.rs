//! Unique compilation identity (from build.rs).

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_STAMP: &str = env!("ABBEY_BUILD_STAMP");
pub const BUILD_GIT: &str = env!("ABBEY_BUILD_GIT");
pub const BUILD_TARGET: &str = env!("ABBEY_BUILD_TARGET");
pub const BUILD_PROFILE: &str = env!("ABBEY_BUILD_PROFILE");
pub const BUILD_HOST: &str = env!("ABBEY_BUILD_HOST");

pub fn lines() -> Vec<String> {
    vec![
        format!("abbey {VERSION}"),
        format!("build stamp:  {BUILD_STAMP}"),
        format!("git:          {BUILD_GIT}"),
        format!("target:       {BUILD_TARGET}"),
        format!("profile:      {BUILD_PROFILE}"),
        format!("host triple:  {BUILD_HOST}"),
        format!("os:           {}", std::env::consts::OS),
        format!("arch:         {}", std::env::consts::ARCH),
    ]
}
