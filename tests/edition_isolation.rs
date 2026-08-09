//! Isolation proof for Abbey's two deliberately separate runtime editions.
//!
//! Two questions are answered here, both as an external client of the library
//! plus the real binary:
//!
//! 1. **Can the safe and personal editions collide?** Config root, state root,
//!    credential namespace, daemon socket, and audit log must differ — checked
//!    exhaustively rather than field by field, so a new namespace added to the
//!    identity table cannot be forgotten.
//! 2. **Did the safe edition move?** Its paths are pinned to what shipped
//!    installs already use, so edition work can never silently re-point an
//!    existing config, memory store, or bearer file.
//!
//! It also pins the honesty bar: no edition implements an unrestricted runtime,
//! and the personal build advertises exactly the safe build's capability set.

use abbey::app_core::{
    AppCommand, AppEvent, AppService, CapabilitySet, Edition as ContractEdition,
};
use abbey::edition::{self, Edition};
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_abbey");

/// Scratch state root, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "abbey-edition-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch state dir");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run the built binary with only *this* edition's state variable set.
fn run(scratch: &Scratch, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .env(edition::ACTIVE.state_dir_env(), &scratch.0)
        .output()
        .expect("spawn abbey");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

// ---------------------------------------------------------------- isolation

/// The namespaces an operator can actually collide on. Each must differ
/// between editions for every host layout, not just this machine's.
#[test]
fn every_namespace_differs_between_editions() {
    let shared = Path::new("/tmp/abbey-shared-root");
    let pairs: [(&str, String, String); 8] = [
        (
            "config root",
            Edition::Safe.config_root().display().to_string(),
            Edition::Personal.config_root().display().to_string(),
        ),
        (
            "config override variable",
            Edition::Safe.config_path_env().into(),
            Edition::Personal.config_path_env().into(),
        ),
        (
            "state override variable",
            Edition::Safe.state_dir_env().into(),
            Edition::Personal.state_dir_env().into(),
        ),
        (
            "credential namespace",
            Edition::Safe.credential_env("EMBEDDING_API_KEY"),
            Edition::Personal.credential_env("EMBEDDING_API_KEY"),
        ),
        (
            "daemon bearer variable",
            Edition::Safe.daemon_bearer_env().into(),
            Edition::Personal.daemon_bearer_env().into(),
        ),
        (
            "daemon socket",
            Edition::Safe
                .daemon_socket_path(shared)
                .display()
                .to_string(),
            Edition::Personal
                .daemon_socket_path(shared)
                .display()
                .to_string(),
        ),
        (
            "audit log",
            Edition::Safe.audit_log_path(shared).display().to_string(),
            Edition::Personal
                .audit_log_path(shared)
                .display()
                .to_string(),
        ),
        (
            "bundle id",
            Edition::Safe.bundle_id().into(),
            Edition::Personal.bundle_id().into(),
        ),
    ];
    for (label, safe, personal) in pairs {
        assert_ne!(safe, personal, "editions share the same {label}");
    }
}

/// The scoped-variable sets must be disjoint, and must cover the individual
/// state files as well as the roots.
#[test]
fn scoped_variable_namespaces_are_disjoint_and_cover_state_files() {
    let safe = Edition::Safe.scoped_env_names();
    let personal = Edition::Personal.scoped_env_names();
    assert_eq!(safe.len(), personal.len());
    for name in &safe {
        assert!(
            !personal.contains(name),
            "`{name}` is scoped to both editions"
        );
    }
    for suffix in [
        "CHAT_FILE",
        "MODEL_FILE",
        "HISTORY_FILE",
        "EMBEDDING_API_KEY",
    ] {
        assert!(
            safe.contains(&Edition::Safe.scoped_env(suffix)),
            "{suffix} is not a scoped variable — both editions would share it"
        );
    }
    // Safe-edition names are the historical ones.
    assert!(safe.contains(&"ABBEY_CHAT_FILE".to_string()));
    assert!(safe.contains(&"ABBEY_STATE_DIR".to_string()));
}

/// Default state roots diverge even before any override is applied.
#[test]
fn default_state_roots_differ() {
    assert_ne!(
        Edition::Safe.default_state_root(),
        Edition::Personal.default_state_root()
    );
}

/// A safe-edition credential must not satisfy a prefix match against the
/// personal namespace (or vice versa) — the isolation has to survive a loader
/// that searches by prefix.
#[test]
fn credential_namespaces_are_not_prefixes_of_each_other() {
    let safe = Edition::Safe.credential_env("TOKEN");
    let personal = Edition::Personal.credential_env("TOKEN");
    assert!(!personal.starts_with(&format!("{safe}_")));
    assert!(!safe.starts_with(&format!("{personal}_")));
}

// --------------------------------------------------------- safe regression

/// Regression pin. These are the locations existing installs already use; a
/// diff here re-points every shipped config, memory store, and bearer file.
#[test]
fn safe_edition_paths_are_exactly_what_they_are_today() {
    let expected_config_root = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("abbey");
    assert_eq!(Edition::Safe.config_root(), expected_config_root);

    let state = Path::new("/tmp/state");
    assert_eq!(
        Edition::Safe.daemon_socket_path(state),
        state.join("daemon").join("abbeyd.sock")
    );
    assert_eq!(Edition::Safe.config_path_env(), "ABBEY_CONFIG");
    assert_eq!(Edition::Safe.state_dir_env(), "ABBEY_STATE_DIR");
    assert_eq!(Edition::Safe.daemon_bearer_env(), "ABBEYD_BEARER_TOKEN");
    assert_eq!(
        Edition::Safe.daemon_bearer_file_env(),
        "ABBEYD_BEARER_TOKEN_FILE"
    );
    assert_eq!(
        Edition::Safe.credential_env("EMBEDDING_API_KEY"),
        "ABBEY_EMBEDDING_API_KEY"
    );
    assert_eq!(Edition::Safe.binary_name(), "abbey");
    assert_eq!(Edition::Safe.daemon_binary_name(), "abbeyd");

    // The XDG default keeps its historical leaf.
    if let Some(root) = Edition::Safe.default_state_root() {
        assert!(
            root.ends_with("abbey"),
            "safe state root moved to {}",
            root.display()
        );
    }
}

// ------------------------------------------------------------ safe invariant

/// No edition implements an unrestricted runtime — there is no `shell.exec`,
/// privileged helper, or always-on unrestricted mode to enable.
#[test]
fn no_edition_implements_an_unrestricted_runtime() {
    for e in Edition::ALL {
        assert!(!e.unrestricted_runtime_implemented());
    }
}

/// The default (no-feature) build is the safe edition and enables no
/// unrestricted capability.
#[cfg(not(feature = "personal-edition"))]
#[test]
fn default_build_enables_no_unrestricted_capability() {
    assert_eq!(edition::ACTIVE, Edition::Safe);
    assert!(!edition::ACTIVE.policy_allows_unrestricted_runtime());
    assert!(!edition::ACTIVE.unrestricted_runtime_implemented());
}

/// Building the personal edition changes identity only. The application
/// capability set — the safe tool registry exposed to any client — is byte
/// identical, so the feature cannot be used as a bypass.
#[test]
fn edition_never_widens_the_capability_set() {
    let event = AppService::default().handle(AppCommand::Status).unwrap();
    let AppEvent::Status(status) = event else {
        panic!("status command must return a status event");
    };
    assert_eq!(status.capabilities, CapabilitySet::standard());
    let expected = match edition::ACTIVE {
        Edition::Safe => ContractEdition::Standard,
        Edition::Personal => ContractEdition::Personal,
    };
    assert_eq!(
        status.edition, expected,
        "the app-core contract must report the compiled edition"
    );
}

/// The OS-control policy surface is unchanged by edition: the allowlist still
/// advertises dry-run by default and `--confirm` for execution, and still
/// points at the unrestricted-shell refusal.
///
/// (`abbey os execute` itself is not driven here: with `abi` on PATH that verb
/// delegates to `abi agent os`, so the deterministic proof of the gate is the
/// in-crate `os_control::tests::execute_without_confirm_is_denied`, which this
/// suite's own feature build also runs.)
#[test]
fn os_control_policy_is_identical_in_this_edition() {
    let s = Scratch::new("policy");
    let (code, out, _) = run(&s, &["allowlist"]);
    assert_eq!(code, 0);
    assert!(out.contains("execute always needs --confirm"), "{out}");
    assert!(out.contains("not an unrestricted shell"), "{out}");
    assert!(out.contains("whoami"), "{out}");
}

// ---------------------------------------------------------- process identity

/// The running binary reports its own edition, and packaging scripts get a
/// name that cannot clobber the other edition's install.
#[test]
fn binary_reports_its_compiled_identity() {
    let s = Scratch::new("identity");
    let (code, name, _) = run(&s, &["edition", "--name"]);
    assert_eq!(code, 0);
    assert_eq!(name.trim(), edition::ACTIVE.binary_name());

    let (code, daemon, _) = run(&s, &["edition", "--daemon-name"]);
    assert_eq!(code, 0);
    assert_eq!(daemon.trim(), edition::ACTIVE.daemon_binary_name());

    let (code, report, _) = run(&s, &["edition"]);
    assert_eq!(code, 0);
    assert!(report.contains(&format!("bundle id: {}", edition::ACTIVE.bundle_id())));
    assert!(report.contains("unrestricted runtime implemented: false"));
}

/// *Every* variable the other edition scopes must be inert here — iterated
/// from `scoped_env_names()` rather than hand-listed, so a newly scoped
/// variable is covered the moment it is added.
///
/// This is the check that caught a real leak: scoping only the state *root*
/// left `ABBEY_CHAT_FILE`/`ABBEY_MODEL_FILE`/`ABBEY_HISTORY_FILE` shared, so
/// one exported variable gave both editions the same chat id.
#[test]
fn every_scoped_variable_of_the_other_edition_is_inert() {
    let other = match edition::ACTIVE {
        Edition::Safe => Edition::Personal,
        Edition::Personal => Edition::Safe,
    };
    let s = Scratch::new("crossenv");
    let mut command = Command::new(BIN);
    command
        .arg("doctor")
        .env(edition::ACTIVE.state_dir_env(), &s.0)
        // Per-cwd chat ids would mask a leaked chat-file override.
        .env("ABBEY_PER_CWD", "0");

    // One distinguishable poison value per variable the other edition owns.
    let poisons: Vec<(String, String)> = other
        .scoped_env_names()
        .into_iter()
        .enumerate()
        .map(|(i, name)| (name, format!("/tmp/abbey-cross-edition-poison-{i}")))
        .collect();
    for (name, value) in &poisons {
        command.env(name, value);
    }

    let out = command.output().expect("spawn abbey");
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for (name, value) in &poisons {
        assert!(
            !report.contains(value.as_str()),
            "the other edition's `{name}` leaked into this edition: {report}"
        );
    }
    assert!(
        report.contains(&s.0.display().to_string()),
        "this edition's own state root was not used: {report}"
    );
}

/// The mirror of the test above: this edition's *own* scoped file overrides
/// must still be honored, or "inert" would be trivially true because nothing
/// reads them at all.
#[test]
fn this_editions_own_scoped_overrides_are_honored() {
    let s = Scratch::new("ownenv");
    let chat = s.0.join("explicit-chat-id");
    let out = Command::new(BIN)
        .arg("doctor")
        .env(edition::ACTIVE.state_dir_env(), &s.0)
        .env("ABBEY_PER_CWD", "0")
        .env(edition::ACTIVE.scoped_env("CHAT_FILE"), &chat)
        .output()
        .expect("spawn abbey");
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(
        report.contains(&chat.display().to_string()),
        "this edition ignored its own CHAT_FILE override: {report}"
    );
}
