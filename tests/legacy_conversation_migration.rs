//! Real-daemon proof for bounded, backup-first legacy conversation metadata migration.

#![cfg(unix)]

use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use abbey::app_core::{AppCommand, AppEvent};
use abbey::daemon::{BearerSecret, DaemonClient, DaemonConfig};
use abbey::edition;

const ABBEYD_BIN: &str = env!("CARGO_BIN_EXE_abbeyd");
const BEARER: &str = "abbey-legacy-migration-bearer-00000001";
const HISTORY_ID: &str = "private-history-conversation-id";
const DIRECT_ID: &str = "private-direct-conversation-id";
const EXPORT_ONLY: &str = "private-export-only-value";
const PRIVATE_CWD: &str = "/private/legacy/workspace";
const TRANSCRIPT_CANARY: &[u8] = b"excluded-fm-transcript-canary";
const ROUTE_CANARY: &[u8] = b"excluded-route-audit-canary";
const MEMORY_CANARY: &[u8] = b"excluded-memory-sqlite-canary";
const WDBX_CANARY: &[u8] = b"excluded-wdbx-store-canary";

#[test]
fn real_daemon_retains_imports_and_reopens_canonical_legacy_metadata() {
    let root = scratch();
    let socket = root.join("abbeyd.sock");
    let stdout = root.join("daemon.stdout");
    let stderr = root.join("daemon.stderr");
    let ignored = root.join("ignored-overrides");
    private_dir(&ignored);
    write_private(&ignored.join("chat-id"), b"override-poison-chat\n");
    write_private(
        &ignored.join("history.log"),
        b"2026-01-01T00:00:00Z\toverride-poison-history\n",
    );

    write_private(&root.join("chat-id"), format!("{DIRECT_ID}\n").as_bytes());
    write_private(
        &root.join("chat-id.export"),
        format!("ABBEY_CHAT_ID={EXPORT_ONLY}\n").as_bytes(),
    );
    write_private(
        &root.join("history.log"),
        format!(
            "2026-08-08T03:02:03+02:00\t{HISTORY_ID}\t{PRIVATE_CWD}\n\
2026-08-08T02:03:04Z\t{HISTORY_ID}\n"
        )
        .as_bytes(),
    );
    let by_cwd = root.join("by-cwd");
    private_dir(&by_cwd);
    write_private(
        &by_cwd.join("opaque-name"),
        format!("{DIRECT_ID}\n").as_bytes(),
    );
    let fm = root.join("fm");
    private_dir(&fm);
    write_private(&fm.join("private.transcript"), TRANSCRIPT_CANARY);
    write_private(&root.join("route.jsonl"), ROUTE_CANARY);
    write_private(&root.join("memory.sqlite"), MEMORY_CANARY);
    let wdbx = root.join("wdbx");
    private_dir(&wdbx);
    write_private(&wdbx.join("sentinel"), WDBX_CANARY);

    let mut first = spawn(&root, &socket, &stdout, &stderr, &ignored);
    wait_ready(&socket);
    terminate(&mut first);

    let database = root.join("daemon/runtime.sqlite");
    assert_database(&database);
    let backup = only_backup(&root.join("daemon/legacy-conversation-backups"));
    assert_owner_only_tree(backup.parent().unwrap());
    assert_eq!(
        fs::read(backup.join("history.log")).unwrap(),
        fs::read(root.join("history.log")).unwrap()
    );
    assert_eq!(
        fs::read(backup.join("chat-id.export")).unwrap(),
        fs::read(root.join("chat-id.export")).unwrap()
    );
    assert_eq!(
        fs::read(backup.join("chat-id")).unwrap(),
        fs::read(root.join("chat-id")).unwrap()
    );
    assert_eq!(
        fs::read(backup.join("by-cwd/opaque-name")).unwrap(),
        fs::read(root.join("by-cwd/opaque-name")).unwrap()
    );
    for excluded in ["fm", "route.jsonl", "memory.sqlite", "wdbx"] {
        assert!(!backup.join(excluded).exists());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert!(manifest["captured_at"].as_str().is_some());
    assert!(
        manifest["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["source_role"] == "backup_only_export")
    );

    let captured_at = manifest["captured_at"].as_str().unwrap().to_owned();
    let mut second = spawn(&root, &socket, &stdout, &stderr, &ignored);
    wait_ready(&socket);
    terminate(&mut second);
    assert_database(&database);
    assert_eq!(
        only_backup(&root.join("daemon/legacy-conversation-backups")),
        backup
    );
    let reopened_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(reopened_manifest["captured_at"], captured_at);
    for (path, expected) in [
        (root.join("fm/private.transcript"), TRANSCRIPT_CANARY),
        (root.join("route.jsonl"), ROUTE_CANARY),
        (root.join("memory.sqlite"), MEMORY_CANARY),
        (root.join("wdbx/sentinel"), WDBX_CANARY),
    ] {
        assert_eq!(fs::read(path).unwrap(), expected);
    }
    #[cfg(feature = "wdbx")]
    assert_eq!(fs::read(root.join("wdbx/sentinel")).unwrap(), WDBX_CANARY);

    let mut output = fs::read_to_string(&stdout).unwrap_or_default();
    output.push_str(&fs::read_to_string(&stderr).unwrap_or_default());
    for private in [
        HISTORY_ID,
        DIRECT_ID,
        EXPORT_ONLY,
        PRIVATE_CWD,
        BEARER,
        "override-poison-chat",
        "override-poison-history",
        std::str::from_utf8(TRANSCRIPT_CANARY).unwrap(),
        std::str::from_utf8(ROUTE_CANARY).unwrap(),
        std::str::from_utf8(MEMORY_CANARY).unwrap(),
        std::str::from_utf8(WDBX_CANARY).unwrap(),
    ] {
        assert!(
            !output.contains(private),
            "daemon output disclosed legacy data"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

fn assert_database(database: &Path) {
    let conn = rusqlite::Connection::open(database).unwrap();
    assert_eq!(
        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        4
    );
    for (table, expected) in [
        ("legacy_conversation_imports", 1),
        ("legacy_conversation_aliases", 2),
        ("legacy_conversation_entries", 4),
        ("conversation_identity_aliases", 2),
        ("conversation_identity_scopes", 0),
        ("conversation_identity_commit", 0),
        ("conversation_identity_tombstones", 0),
        ("conversation_identity_clear_all", 0),
        ("conversation_identity_mutations", 0),
        ("conversation_identity_mutation_scopes", 0),
        ("conversation_identity_migrated_scopes", 0),
        ("runs", 0),
        ("conversation_backends", 0),
    ] {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        assert_eq!(
            conn.query_row(&sql, [], |row| row.get::<_, i64>(0))
                .unwrap(),
            expected
        );
    }
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM legacy_conversation_entries
             WHERE source_kind='history' AND observed_at IS NOT NULL",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    let columns: String = conn
        .query_row(
            "SELECT group_concat(name, ',')
             FROM pragma_table_info('legacy_conversation_entries')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for forbidden in ["legacy_id", "cwd", "source_locator"] {
        assert!(!columns.split(',').any(|column| column == forbidden));
    }
    drop(conn);

    let bytes = fs::read(database).unwrap();
    for private in [
        HISTORY_ID,
        DIRECT_ID,
        EXPORT_ONLY,
        PRIVATE_CWD,
        "override-poison-chat",
        "override-poison-history",
        "chat-id.export",
        std::str::from_utf8(TRANSCRIPT_CANARY).unwrap(),
        std::str::from_utf8(ROUTE_CANARY).unwrap(),
        std::str::from_utf8(MEMORY_CANARY).unwrap(),
        std::str::from_utf8(WDBX_CANARY).unwrap(),
    ] {
        assert!(
            !bytes
                .windows(private.len())
                .any(|window| window == private.as_bytes()),
            "runtime database retained raw legacy data"
        );
    }
}

fn spawn(root: &Path, socket: &Path, stdout: &Path, stderr: &Path, ignored: &Path) -> Child {
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(stdout)
        .unwrap();
    let stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .open(stderr)
        .unwrap();
    Command::new(ABBEYD_BIN)
        .env(edition::ACTIVE.state_dir_env(), root)
        .env(edition::ACTIVE.daemon_socket_env(), socket)
        .env(edition::ACTIVE.daemon_bearer_env(), BEARER)
        .env(
            edition::ACTIVE.scoped_env("CHAT_FILE"),
            ignored.join("chat-id"),
        )
        .env(
            edition::ACTIVE.scoped_env("HISTORY_FILE"),
            ignored.join("history.log"),
        )
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap()
}

fn wait_ready(socket: &Path) {
    let client = DaemonClient::new(DaemonConfig::local(
        socket,
        BearerSecret::parse(BEARER).unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if socket.exists() && matches!(client.request(AppCommand::Status), Ok(AppEvent::Status(_)))
        {
            return;
        }
        assert!(Instant::now() < deadline, "abbeyd did not become ready");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn terminate(child: &mut Child) {
    let pid = nix::unistd::Pid::from_raw(i32::try_from(child.id()).unwrap());
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "abbeyd SIGTERM shutdown failed: {status}");
            return;
        }
        assert!(Instant::now() < deadline, "abbeyd ignored SIGTERM");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn only_backup(backups: &Path) -> PathBuf {
    let entries = fs::read_dir(backups)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("v1-")
    );
    entries[0].clone()
}

fn assert_owner_only_tree(root: &Path) {
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(
            metadata.permissions().mode() & 0o077,
            0,
            "retained backup granted group or other permissions"
        );
        if metadata.is_dir() {
            pending.extend(
                fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn private_dir(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn scratch() -> PathBuf {
    let root = PathBuf::from("/tmp").join(format!(
        "abbey-legacy-migration-{}-{}",
        std::process::id(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    private_dir(&root);
    root
}
