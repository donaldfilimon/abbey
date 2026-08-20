//! Compile-time runtime editions — the single identity source for the safe
//! public build and the separately packaged personal build.
//!
//! Abbey ships two deliberately separate editions:
//!
//! * [`Edition::Safe`] — the default build. Shell, filesystem mutation,
//!   network, and elevation stay behind the `os_control` allowlist plus an
//!   explicit `--confirm`. This is what `cargo build` produces.
//! * [`Edition::Personal`] — compiled only with `--features personal-edition`.
//!   It is a *separately identified* product: different binary name, bundle
//!   identifier, configuration root, credential namespace, and audit log path,
//!   so the two installs can never read each other's state or secrets.
//!
//! What this module is **not**: the personal-unrestricted *runtime*. There is
//! no `shell.exec` tool, no privileged helper, and no always-on unrestricted
//! mode in either edition — [`Edition::unrestricted_runtime_implemented`]
//! returns `false` unconditionally, and the isolation tests pin that. This
//! module implements the separation mechanism only.
//!
//! Every per-edition string lives in exactly one table (`SAFE`/`PERSONAL`).
//! Callers must derive names and paths from `ACTIVE` rather than repeating a
//! literal, or the two editions drift and can collide.

use std::path::{Path, PathBuf};

/// Which runtime edition this binary was compiled as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edition {
    /// Default public build: allowlist + explicit confirmation, no bypass.
    Safe,
    /// Separately packaged personal build (`--features personal-edition`).
    Personal,
}

/// The edition this binary was compiled as.
///
/// Deliberately a compile-time constant: no environment variable, config key,
/// or runtime flag can move a running process between editions.
#[cfg(not(feature = "personal-edition"))]
pub const ACTIVE: Edition = Edition::Safe;

/// The edition this binary was compiled as (personal build).
#[cfg(feature = "personal-edition")]
pub const ACTIVE: Edition = Edition::Personal;

/// Per-edition identity. One row per edition; no other module may hardcode
/// these strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditionIdentity {
    /// Directory/namespace component: config root, state root, audit file stem.
    pub slug: &'static str,
    /// Human product name.
    pub product_name: &'static str,
    /// Installed CLI binary name.
    pub binary_name: &'static str,
    /// Installed daemon binary name.
    pub daemon_binary_name: &'static str,
    /// Desktop/bundle identifier for packaged builds.
    pub bundle_id: &'static str,
    /// Environment variable overriding the config file path.
    pub config_path_env: &'static str,
    /// Environment variable overriding the state root.
    pub state_dir_env: &'static str,
    /// Prefix for this edition's credential environment variables.
    pub credential_env_prefix: &'static str,
    /// Environment variable overriding the daemon socket path.
    pub daemon_socket_env: &'static str,
    /// Daemon socket file name inside `<state>/daemon/`.
    pub daemon_socket_name: &'static str,
    /// Inline daemon bearer-token environment variable.
    pub daemon_bearer_env: &'static str,
    /// Daemon bearer-token file environment variable.
    pub daemon_bearer_file_env: &'static str,
}

/// Safe public edition. These values are the paths Abbey has always used —
/// changing any of them re-points every existing install.
const SAFE: EditionIdentity = EditionIdentity {
    slug: "abbey",
    product_name: "Abbey",
    binary_name: "abbey",
    daemon_binary_name: "abbeyd",
    bundle_id: "com.donaldfilimon.abbey",
    config_path_env: "ABBEY_CONFIG",
    state_dir_env: "ABBEY_STATE_DIR",
    credential_env_prefix: "ABBEY",
    daemon_socket_env: "ABBEYD_SOCKET_PATH",
    daemon_socket_name: "abbeyd.sock",
    daemon_bearer_env: "ABBEYD_BEARER_TOKEN",
    daemon_bearer_file_env: "ABBEYD_BEARER_TOKEN_FILE",
};

/// Personal edition. Every field must differ from [`SAFE`] — the isolation
/// tests enforce that field by field.
const PERSONAL: EditionIdentity = EditionIdentity {
    slug: "abbey-personal",
    product_name: "Abbey Personal",
    binary_name: "abbey-personal",
    daemon_binary_name: "abbey-personal-daemon",
    bundle_id: "com.donaldfilimon.abbey-personal",
    config_path_env: "ABBEY_PERSONAL_CONFIG",
    state_dir_env: "ABBEY_PERSONAL_STATE_DIR",
    credential_env_prefix: "ABBEY_PERSONAL",
    daemon_socket_env: "ABBEY_PERSONAL_DAEMON_SOCKET_PATH",
    daemon_socket_name: "abbey-personal-daemon.sock",
    daemon_bearer_env: "ABBEY_PERSONAL_DAEMON_BEARER_TOKEN",
    daemon_bearer_file_env: "ABBEY_PERSONAL_DAEMON_BEARER_TOKEN_FILE",
};

/// Suffixes of edition-scoped variables that name a file or a secret, beyond
/// the roots and daemon variables that have their own identity fields.
///
/// A path- or secret-bearing variable left off this list is a namespace the two
/// editions would silently share — that is exactly how `ABBEY_CHAT_FILE` let a
/// personal build adopt the safe build's chat id before it was scoped.
const SCOPED_ENV_SUFFIXES: &[&str] = &[
    "CHAT_FILE",
    "MODEL_FILE",
    "HISTORY_FILE",
    "MODEL_RUNTIME_CONFIG",
    "EMBEDDING_API_KEY",
];

impl Edition {
    /// Every edition, for exhaustive isolation checks.
    pub const ALL: &'static [Edition] = &[Edition::Safe, Edition::Personal];

    #[must_use]
    pub const fn identity(self) -> &'static EditionIdentity {
        match self {
            Self::Safe => &SAFE,
            Self::Personal => &PERSONAL,
        }
    }

    #[must_use]
    pub const fn slug(self) -> &'static str {
        self.identity().slug
    }

    #[must_use]
    pub const fn product_name(self) -> &'static str {
        self.identity().product_name
    }

    #[must_use]
    pub const fn binary_name(self) -> &'static str {
        self.identity().binary_name
    }

    #[must_use]
    pub const fn daemon_binary_name(self) -> &'static str {
        self.identity().daemon_binary_name
    }

    #[must_use]
    pub const fn bundle_id(self) -> &'static str {
        self.identity().bundle_id
    }

    #[must_use]
    pub const fn config_path_env(self) -> &'static str {
        self.identity().config_path_env
    }

    #[must_use]
    pub const fn state_dir_env(self) -> &'static str {
        self.identity().state_dir_env
    }

    #[must_use]
    pub const fn daemon_socket_env(self) -> &'static str {
        self.identity().daemon_socket_env
    }

    #[must_use]
    pub const fn daemon_bearer_env(self) -> &'static str {
        self.identity().daemon_bearer_env
    }

    #[must_use]
    pub const fn daemon_bearer_file_env(self) -> &'static str {
        self.identity().daemon_bearer_file_env
    }

    /// Name of an edition-scoped environment variable.
    ///
    /// `Safe.scoped_env("CHAT_FILE")` is `ABBEY_CHAT_FILE` — the variable Abbey
    /// has always read — while the personal edition reads
    /// `ABBEY_PERSONAL_CHAT_FILE`. Any variable that names a file, directory, or
    /// secret must go through here: one exported value must never be adopted by
    /// both editions. Pure behaviour knobs (`ABBEY_PER_CWD`, `ABBEY_MODEL`) are
    /// deliberately shared — they select preferences, not storage.
    #[must_use]
    pub fn scoped_env(self, suffix: &str) -> String {
        format!("{}_{suffix}", self.identity().credential_env_prefix)
    }

    /// Name of an edition-scoped credential variable (a [`Self::scoped_env`]).
    #[must_use]
    pub fn credential_env(self, suffix: &str) -> String {
        self.scoped_env(suffix)
    }

    /// Every environment variable that names storage or a secret for this
    /// edition.
    ///
    /// Exhaustive on purpose: the isolation tests iterate this list instead of
    /// a hand-written sample, so a newly scoped variable is covered the moment
    /// it is added here.
    #[must_use]
    pub fn scoped_env_names(self) -> Vec<String> {
        let mut names = vec![
            self.config_path_env().to_owned(),
            self.state_dir_env().to_owned(),
            self.daemon_socket_env().to_owned(),
            self.daemon_bearer_env().to_owned(),
            self.daemon_bearer_file_env().to_owned(),
        ];
        names.extend(SCOPED_ENV_SUFFIXES.iter().map(|s| self.scoped_env(s)));
        names
    }

    /// Configuration root directory for this edition (no environment override).
    #[must_use]
    pub fn config_root(self) -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(self.slug())
    }

    /// Configuration file path, honoring this edition's override variable.
    #[must_use]
    pub fn config_path(self) -> PathBuf {
        if let Some(p) = std::env::var_os(self.config_path_env()) {
            return PathBuf::from(p);
        }
        self.config_root().join("config.toml")
    }

    /// Default state root when this edition's override variable is unset.
    ///
    /// Unix uses XDG state (`~/.local/state/<slug>`); Windows uses
    /// `LocalAppData\<slug>`. `data_local_dir` is deliberately not used on
    /// Unix — that would relocate existing stores into Application Support.
    #[must_use]
    pub fn default_state_root(self) -> Option<PathBuf> {
        #[cfg(windows)]
        {
            dirs::data_local_dir()
                .map(|d| d.join(self.slug()))
                .or_else(|| dirs::home_dir().map(|h| h.join("AppData\\Local").join(self.slug())))
        }
        #[cfg(not(windows))]
        {
            dirs::state_dir()
                .map(|d| d.join(self.slug()))
                .or_else(|| dirs::home_dir().map(|h| h.join(".local/state").join(self.slug())))
        }
    }

    /// Daemon socket path under a resolved state root.
    #[must_use]
    pub fn daemon_socket_path(self, state_dir: &Path) -> PathBuf {
        state_dir
            .join("daemon")
            .join(self.identity().daemon_socket_name)
    }

    /// Edition-scoped audit log path.
    ///
    /// Reserved destination for edition-scoped audit records. The file name
    /// carries the slug as well as the (already distinct) state root, so the
    /// two editions stay separate even if one is pointed at a shared root.
    /// Nothing writes unrestricted-runtime audit records yet — that runtime
    /// does not exist. Routing audit remains `route.jsonl`; run/tool audit
    /// remains the runtime SQLite store.
    #[must_use]
    pub fn audit_log_path(self, state_dir: &Path) -> PathBuf {
        state_dir
            .join("audit")
            .join(format!("{}.jsonl", self.slug()))
    }

    /// Whether this edition's *policy* would permit an unrestricted runtime.
    ///
    /// The safe edition answers `false` as a standing invariant. A `true` here
    /// is a statement about packaging policy only — see
    /// [`Edition::unrestricted_runtime_implemented`] for what actually exists.
    #[must_use]
    pub const fn policy_allows_unrestricted_runtime(self) -> bool {
        match self {
            Self::Safe => false,
            Self::Personal => true,
        }
    }

    /// Whether an unrestricted runtime is actually implemented. Always `false`.
    ///
    /// No edition ships `shell.exec`, a signed privileged helper, or an
    /// always-on unrestricted mode. Flipping this to `true` without those
    /// implementations would be a false capability claim; the isolation tests
    /// assert it stays `false` in every edition and in every feature build.
    #[must_use]
    pub const fn unrestricted_runtime_implemented(self) -> bool {
        false
    }
}

/// Human report for `abbey doctor` / `abbey edition`.
#[must_use]
pub fn identity_lines(state_dir: &Path) -> Vec<String> {
    let e = ACTIVE;
    let install = dirs::home_dir().map(|h| crate::host::installed_binary_path(&h));
    vec![
        format!(
            "edition:   {} ({})",
            e.product_name(),
            match e {
                Edition::Safe => "safe public edition — default build",
                Edition::Personal => "personal edition — built with --features personal-edition",
            }
        ),
        format!(
            "binary:    {}{}",
            e.binary_name(),
            install
                .map(|p| format!(" → {}", p.display()))
                .unwrap_or_default()
        ),
        format!("daemon:    {}", e.daemon_binary_name()),
        format!("bundle id: {}", e.bundle_id()),
        format!("config:    {}", e.config_path().display()),
        format!("state root:{}", state_dir.display()),
        format!(
            "creds:     {}_* env namespace (daemon bearer: {})",
            e.identity().credential_env_prefix,
            e.daemon_bearer_env()
        ),
        format!("audit log: {}", e.audit_log_path(state_dir).display()),
        format!(
            "os policy: allowlist + --confirm (unrestricted runtime implemented: {}; \
             edition policy would allow: {})",
            e.unrestricted_runtime_implemented(),
            e.policy_allows_unrestricted_runtime()
        ),
    ]
}

/// `abbey edition` — identity report, or a single name for packaging scripts.
pub fn cmd_edition(state_dir: &Path, name_only: bool, daemon_name_only: bool) -> i32 {
    if name_only {
        let _ = crate::output::println(ACTIVE.binary_name());
        return 0;
    }
    if daemon_name_only {
        let _ = crate::output::println(ACTIVE.daemon_binary_name());
        return 0;
    }
    for line in identity_lines(state_dir) {
        let _ = crate::output::println(line);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression pin: the safe edition's identity is what shipped installs
    /// already use. A change here silently re-points every existing config,
    /// state store, credential, and socket.
    #[test]
    fn safe_edition_identity_is_unchanged() {
        let id = Edition::Safe.identity();
        assert_eq!(id.slug, "abbey");
        assert_eq!(id.binary_name, "abbey");
        assert_eq!(id.daemon_binary_name, "abbeyd");
        assert_eq!(id.config_path_env, "ABBEY_CONFIG");
        assert_eq!(id.state_dir_env, "ABBEY_STATE_DIR");
        assert_eq!(id.credential_env_prefix, "ABBEY");
        assert_eq!(id.daemon_socket_env, "ABBEYD_SOCKET_PATH");
        assert_eq!(id.daemon_socket_name, "abbeyd.sock");
        assert_eq!(id.daemon_bearer_env, "ABBEYD_BEARER_TOKEN");
        assert_eq!(id.daemon_bearer_file_env, "ABBEYD_BEARER_TOKEN_FILE");
        assert_eq!(
            Edition::Safe.credential_env("EMBEDDING_API_KEY"),
            "ABBEY_EMBEDDING_API_KEY"
        );
    }

    /// The safe edition's resolved roots must still be `<config>/abbey` and
    /// `<state>/abbey`, not a slug-derived rename.
    #[test]
    fn safe_edition_paths_are_unchanged() {
        let expected_config = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("abbey");
        assert_eq!(Edition::Safe.config_root(), expected_config);
        assert_eq!(
            Edition::Safe.config_root().join("config.toml"),
            expected_config.join("config.toml")
        );
        if let Some(root) = Edition::Safe.default_state_root() {
            assert!(
                root.ends_with("abbey"),
                "safe state root moved: {}",
                root.display()
            );
        }
        let state = Path::new("/tmp/abbey-state");
        assert_eq!(
            Edition::Safe.daemon_socket_path(state),
            state.join("daemon").join("abbeyd.sock")
        );
    }

    /// Every identity field must differ, so no pair of editions can collide on
    /// any namespace — not just the ones the caller happened to test.
    #[test]
    fn every_identity_field_differs_between_editions() {
        let s = Edition::Safe.identity();
        let p = Edition::Personal.identity();
        for (field, safe, personal) in [
            ("slug", s.slug, p.slug),
            ("product_name", s.product_name, p.product_name),
            ("binary_name", s.binary_name, p.binary_name),
            (
                "daemon_binary_name",
                s.daemon_binary_name,
                p.daemon_binary_name,
            ),
            ("bundle_id", s.bundle_id, p.bundle_id),
            ("config_path_env", s.config_path_env, p.config_path_env),
            ("state_dir_env", s.state_dir_env, p.state_dir_env),
            (
                "credential_env_prefix",
                s.credential_env_prefix,
                p.credential_env_prefix,
            ),
            (
                "daemon_socket_env",
                s.daemon_socket_env,
                p.daemon_socket_env,
            ),
            (
                "daemon_socket_name",
                s.daemon_socket_name,
                p.daemon_socket_name,
            ),
            (
                "daemon_bearer_env",
                s.daemon_bearer_env,
                p.daemon_bearer_env,
            ),
            (
                "daemon_bearer_file_env",
                s.daemon_bearer_file_env,
                p.daemon_bearer_file_env,
            ),
        ] {
            assert_ne!(safe, personal, "editions share `{field}`");
        }
    }

    /// A shared state root is the worst case an operator can create by hand;
    /// the audit log must still land in separate files.
    #[test]
    fn audit_log_paths_differ_even_under_a_shared_state_root() {
        let shared = Path::new("/tmp/shared-state");
        assert_ne!(
            Edition::Safe.audit_log_path(shared),
            Edition::Personal.audit_log_path(shared)
        );
        assert_ne!(
            Edition::Safe.daemon_socket_path(shared),
            Edition::Personal.daemon_socket_path(shared)
        );
    }

    /// Config and state roots are derived from the slug, so they diverge for
    /// every host layout, not only this machine's.
    #[test]
    fn config_and_state_roots_differ() {
        assert_ne!(Edition::Safe.config_root(), Edition::Personal.config_root());
        assert_ne!(
            Edition::Safe.default_state_root(),
            Edition::Personal.default_state_root()
        );
    }

    /// Credential namespaces cannot overlap for any variable suffix.
    #[test]
    fn credential_namespaces_never_overlap() {
        for suffix in [
            "EMBEDDING_API_KEY",
            "EMBEDDING_ENDPOINT",
            "TOKEN",
            "ABI_BIN",
        ] {
            let safe = Edition::Safe.credential_env(suffix);
            let personal = Edition::Personal.credential_env(suffix);
            assert_ne!(safe, personal);
            // The safe name must not be a suffix of the personal one either —
            // a prefix-matching credential loader would otherwise leak.
            assert!(!personal.ends_with(&format!("_{safe}")));
        }
    }

    /// The safety invariant: the shipped edition never permits unrestricted
    /// operation, and no edition implements one today.
    #[test]
    fn no_edition_implements_an_unrestricted_runtime() {
        for edition in Edition::ALL {
            assert!(
                !edition.unrestricted_runtime_implemented(),
                "{:?} claims an unrestricted runtime that does not exist",
                edition
            );
        }
        assert!(!Edition::Safe.policy_allows_unrestricted_runtime());
    }

    /// The default (no-feature) build is the safe edition and enables no
    /// unrestricted capability.
    #[cfg(not(feature = "personal-edition"))]
    #[test]
    fn default_build_is_the_safe_edition() {
        assert_eq!(ACTIVE, Edition::Safe);
        assert!(!ACTIVE.policy_allows_unrestricted_runtime());
        assert!(!ACTIVE.unrestricted_runtime_implemented());
        assert_eq!(ACTIVE.binary_name(), "abbey");
        assert_eq!(ACTIVE.state_dir_env(), "ABBEY_STATE_DIR");
    }

    /// The feature build selects the personal identity — and still ships no
    /// unrestricted runtime.
    #[cfg(feature = "personal-edition")]
    #[test]
    fn feature_build_is_the_personal_edition() {
        assert_eq!(ACTIVE, Edition::Personal);
        assert_eq!(ACTIVE.binary_name(), "abbey-personal");
        assert_eq!(ACTIVE.state_dir_env(), "ABBEY_PERSONAL_STATE_DIR");
        assert!(!ACTIVE.unrestricted_runtime_implemented());
    }

    #[test]
    fn identity_lines_report_the_active_edition() {
        let lines = identity_lines(Path::new("/tmp/state"));
        let text = lines.join("\n");
        assert!(text.contains(ACTIVE.product_name()));
        assert!(text.contains(ACTIVE.bundle_id()));
        assert!(text.contains("unrestricted runtime implemented: false"));
    }
}
