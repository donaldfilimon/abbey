//! Bounded migration of canonical edition-owned legacy conversation metadata.
//!
//! The retained backup is byte-exact and content-addressed. Import parsing is
//! deliberately narrower: chat identity aliases and explicit history metadata
//! only. Transcripts, memory, titles, backends, prompts, providers, and runs are
//! neither opened nor inferred.

use crate::app_core::ConversationId;
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};
use thiserror::Error;

const MAX_SOURCE_FILES: usize = 1_024;
const MAX_SOURCE_BYTES: usize = 2 * 1_024 * 1_024;
const MAX_TOTAL_BYTES: usize = 8 * 1_024 * 1_024;
const MAX_ENTRIES: usize = 4_096;
const MAX_TIMESTAMP_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacySourceKind {
    History,
    ChatId,
    ByCwd,
}

impl LegacySourceKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::History => "history",
            Self::ChatId => "chat_id",
            Self::ByCwd => "by_cwd",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LegacyEntry {
    pub(crate) alias_sha256: String,
    pub(crate) conversation_id: ConversationId,
    pub(crate) source_kind: LegacySourceKind,
    pub(crate) observed_at: Option<String>,
}

impl fmt::Debug for LegacyEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LegacyEntry")
            .field("alias_sha256", &self.alias_sha256)
            .field("conversation_id", &"[REDACTED]")
            .field("source_kind", &self.source_kind)
            .field(
                "observed_at",
                &self.observed_at.as_ref().map(|_| "[PRESENT]"),
            )
            .finish()
    }
}

#[derive(Clone)]
struct LegacySource {
    relative: PathBuf,
    bytes: Vec<u8>,
    fingerprint: FileFingerprint,
}

impl fmt::Debug for LegacySource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LegacySource")
            .field("relative", &"[REDACTED]")
            .field("bytes", &format_args!("[{} BYTES]", self.bytes.len()))
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct PreparedLegacyImport {
    pub(crate) snapshot_sha256: String,
    pub(crate) captured_at: String,
    pub(crate) source_count: usize,
    pub(crate) entries: Vec<LegacyEntry>,
    pub(crate) skipped_count: usize,
}

impl fmt::Debug for PreparedLegacyImport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedLegacyImport")
            .field("snapshot_sha256", &self.snapshot_sha256)
            .field("captured_at", &"[PRESENT]")
            .field("source_count", &self.source_count)
            .field("entry_count", &self.entries.len())
            .field("skipped_count", &self.skipped_count)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum LegacyError {
    #[error("legacy metadata source is not owner-safe")]
    UnsafeSource,
    #[error("legacy metadata source set exceeds its fixed bounds")]
    Bounds,
    #[error("legacy metadata changed while it was being snapshotted")]
    UnstableSnapshot,
    #[error("legacy metadata retained backup could not be created or verified")]
    Backup,
}

#[cfg(unix)]
pub(crate) fn prepare(
    canonical_state_root: &Path,
    runtime_dir: &Path,
) -> Result<Option<PreparedLegacyImport>, LegacyError> {
    prepare_with_capture(runtime_dir, || capture_stable(canonical_state_root))
}

#[cfg(unix)]
fn prepare_with_capture(
    runtime_dir: &Path,
    capture: impl FnMut() -> Result<Vec<LegacySource>, LegacyError>,
) -> Result<Option<PreparedLegacyImport>, LegacyError> {
    let sources = capture_with_retry(capture)?;
    if sources.is_empty() {
        return Ok(None);
    }
    let snapshot_sha256 = snapshot_digest(&sources);
    let proposed_captured_at =
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let captured_at = retain_backup(
        runtime_dir,
        &snapshot_sha256,
        &proposed_captured_at,
        &sources,
    )?;
    let (entries, skipped_count) = parse_entries(&sources);
    Ok(Some(PreparedLegacyImport {
        snapshot_sha256,
        captured_at,
        source_count: sources.len(),
        entries,
        skipped_count,
    }))
}

#[cfg(unix)]
fn capture_with_retry(
    mut capture: impl FnMut() -> Result<Vec<LegacySource>, LegacyError>,
) -> Result<Vec<LegacySource>, LegacyError> {
    for _ in 0..3 {
        match capture() {
            Ok(sources) => return Ok(sources),
            Err(LegacyError::UnstableSnapshot) => {}
            Err(error) => return Err(error),
        }
    }
    Err(LegacyError::UnstableSnapshot)
}

#[cfg(not(unix))]
pub(crate) fn prepare(
    _canonical_state_root: &Path,
    _runtime_dir: &Path,
) -> Result<Option<PreparedLegacyImport>, LegacyError> {
    Ok(None)
}

#[cfg(unix)]
fn capture_stable(root: &Path) -> Result<Vec<LegacySource>, LegacyError> {
    validate_directory(root)?;
    let paths = collect_source_paths(root)?;
    if paths.len() > MAX_SOURCE_FILES {
        return Err(LegacyError::Bounds);
    }
    let mut total = 0usize;
    let mut sources = Vec::with_capacity(paths.len());
    for relative in &paths {
        let (bytes, fingerprint) = read_owner_file(&root.join(relative))?;
        total = total.checked_add(bytes.len()).ok_or(LegacyError::Bounds)?;
        if total > MAX_TOTAL_BYTES {
            return Err(LegacyError::Bounds);
        }
        sources.push(LegacySource {
            relative: relative.clone(),
            bytes,
            fingerprint,
        });
    }

    if collect_source_paths(root)? != paths {
        return Err(LegacyError::UnstableSnapshot);
    }
    for source in &sources {
        let (bytes, fingerprint) =
            read_owner_file(&root.join(&source.relative)).map_err(|error| match error {
                LegacyError::UnsafeSource => LegacyError::UnstableSnapshot,
                other => other,
            })?;
        if fingerprint != source.fingerprint || bytes != source.bytes {
            return Err(LegacyError::UnstableSnapshot);
        }
    }
    Ok(sources)
}

#[cfg(unix)]
fn collect_source_paths(root: &Path) -> Result<Vec<PathBuf>, LegacyError> {
    let mut paths = Vec::new();
    for name in ["history.log", "chat-id", "chat-id.export"] {
        let path = root.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => paths.push(PathBuf::from(name)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(LegacyError::UnsafeSource),
        }
    }

    let by_cwd = root.join("by-cwd");
    match std::fs::symlink_metadata(&by_cwd) {
        Ok(_) => {
            validate_directory(&by_cwd)?;
            for child in std::fs::read_dir(&by_cwd).map_err(|_| LegacyError::UnsafeSource)? {
                let child = child.map_err(|_| LegacyError::UnsafeSource)?;
                paths.push(PathBuf::from("by-cwd").join(child.file_name()));
                if paths.len() > MAX_SOURCE_FILES {
                    return Err(LegacyError::Bounds);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(LegacyError::UnsafeSource),
    }
    paths.sort_by(|left, right| path_bytes(left).cmp(path_bytes(right)));
    Ok(paths)
}

#[cfg(unix)]
fn validate_directory(path: &Path) -> Result<(), LegacyError> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let metadata = std::fs::symlink_metadata(path).map_err(|_| LegacyError::UnsafeSource)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(LegacyError::UnsafeSource);
    }
    Ok(())
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanos: i64,
    changed_seconds: i64,
    changed_nanos: i64,
    mode: u32,
}

#[cfg(not(unix))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint;

#[cfg(unix)]
fn read_owner_file(path: &Path) -> Result<(Vec<u8>, FileFingerprint), LegacyError> {
    read_owner_file_with_mode(path, false)
}

#[cfg(unix)]
fn read_owner_private_file(path: &Path) -> Result<(Vec<u8>, FileFingerprint), LegacyError> {
    read_owner_file_with_mode(path, true)
}

#[cfg(unix)]
fn read_owner_file_with_mode(
    path: &Path,
    owner_only: bool,
) -> Result<(Vec<u8>, FileFingerprint), LegacyError> {
    use std::io::Read as _;
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| LegacyError::UnsafeSource)?;
    let before = file.metadata().map_err(|_| LegacyError::UnsafeSource)?;
    if !before.file_type().is_file()
        || before.uid() != nix::unistd::Uid::effective().as_raw()
        || before.permissions().mode() & if owner_only { 0o077 } else { 0o022 } != 0
        || before.len() > MAX_SOURCE_BYTES as u64
    {
        return Err(if before.len() > MAX_SOURCE_BYTES as u64 {
            LegacyError::Bounds
        } else {
            LegacyError::UnsafeSource
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(MAX_SOURCE_BYTES));
    (&mut file)
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| LegacyError::UnsafeSource)?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(LegacyError::Bounds);
    }
    let after = file.metadata().map_err(|_| LegacyError::UnsafeSource)?;
    let before = fingerprint(&before);
    let after = fingerprint(&after);
    if before != after || after.size != bytes.len() as u64 {
        return Err(LegacyError::UnstableSnapshot);
    }
    Ok((bytes, after))
}

#[cfg(unix)]
fn fingerprint(metadata: &std::fs::Metadata) -> FileFingerprint {
    use std::os::unix::fs::MetadataExt as _;

    FileFingerprint {
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.size(),
        modified_seconds: metadata.mtime(),
        modified_nanos: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanos: metadata.ctime_nsec(),
        mode: metadata.mode(),
    }
}

#[cfg(unix)]
fn snapshot_digest(sources: &[LegacySource]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"abbey-legacy-snapshot-v1\0");
    for source in sources {
        let path = path_bytes(&source.relative);
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path);
        digest.update((source.bytes.len() as u64).to_be_bytes());
        digest.update(&source.bytes);
    }
    lower_hex(&digest.finalize())
}

#[cfg(unix)]
fn retain_backup(
    runtime_dir: &Path,
    digest: &str,
    proposed_captured_at: &str,
    sources: &[LegacySource],
) -> Result<String, LegacyError> {
    use std::io::Write as _;
    use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};

    let backups = runtime_dir.join("legacy-conversation-backups");
    create_private_directory(&backups)?;
    // Persist the parent entry before relying on any retained snapshot within
    // it during a later crash recovery.
    sync_directory(runtime_dir)?;
    let destination = backups.join(format!("v1-{digest}"));
    if destination.exists() {
        return verify_backup(&destination, sources);
    }

    let temporary = backups.join(format!(".tmp-{}", uuid::Uuid::new_v4()));
    let mut temporary_builder = std::fs::DirBuilder::new();
    temporary_builder.mode(0o700);
    temporary_builder
        .create(&temporary)
        .map_err(|_| LegacyError::Backup)?;
    let result = (|| {
        for source in sources {
            let target = temporary.join(&source.relative);
            if let Some(parent) = target.parent()
                && parent != temporary
            {
                create_private_directory(parent)?;
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&target)
                .map_err(|_| LegacyError::Backup)?;
            file.write_all(&source.bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| LegacyError::Backup)?;
        }
        if temporary.join("by-cwd").is_dir() {
            sync_directory(&temporary.join("by-cwd"))?;
        }
        let manifest = manifest_bytes(digest, proposed_captured_at, sources)?;
        let manifest_path = temporary.join("manifest.json");
        let mut manifest_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&manifest_path)
            .map_err(|_| LegacyError::Backup)?;
        manifest_file
            .write_all(&manifest)
            .and_then(|()| manifest_file.sync_all())
            .map_err(|_| LegacyError::Backup)?;
        sync_directory(&temporary)?;
        sync_directory(&backups)?;
        match std::fs::rename(&temporary, &destination) {
            Ok(()) => {
                sync_directory(&backups)?;
                Ok(())
            }
            Err(_) if destination.exists() => verify_backup(&destination, sources).map(|_| ()),
            Err(_) => Err(LegacyError::Backup),
        }
    })();
    if temporary.exists() {
        let _ = std::fs::remove_dir_all(&temporary);
    }
    result?;
    verify_backup(&destination, sources)
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> Result<(), LegacyError> {
    use std::os::unix::fs::DirBuilderExt as _;

    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    match builder.create(path) {
        Ok(()) => validate_private_backup_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            validate_private_backup_directory(path)
        }
        Err(_) => Err(LegacyError::Backup),
    }
}

#[cfg(unix)]
fn verify_backup(destination: &Path, sources: &[LegacySource]) -> Result<String, LegacyError> {
    validate_private_backup_directory(destination)?;
    let actual = collect_backup_paths(destination)?;
    let expected = sources
        .iter()
        .map(|source| source.relative.clone())
        .collect::<Vec<_>>();
    if actual != expected {
        return Err(LegacyError::Backup);
    }
    for source in sources {
        let (bytes, _) = read_owner_private_file(&destination.join(&source.relative))
            .map_err(|_| LegacyError::Backup)?;
        if bytes != source.bytes {
            return Err(LegacyError::Backup);
        }
    }
    let digest = destination
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("v1-"))
        .ok_or(LegacyError::Backup)?;
    let (manifest_bytes_on_disk, _) = read_owner_private_file(&destination.join("manifest.json"))
        .map_err(|_| LegacyError::Backup)?;
    let manifest: BackupManifest =
        serde_json::from_slice(&manifest_bytes_on_disk).map_err(|_| LegacyError::Backup)?;
    let canonical_captured_at = chrono::DateTime::parse_from_rfc3339(&manifest.captured_at)
        .map(|timestamp| {
            timestamp
                .with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
        })
        .map_err(|_| LegacyError::Backup)?;
    if manifest != build_manifest(digest, &manifest.captured_at, sources)
        || manifest.captured_at != canonical_captured_at
        || manifest_bytes_on_disk
            != serde_json::to_vec(&manifest).map_err(|_| LegacyError::Backup)?
    {
        return Err(LegacyError::Backup);
    }
    Ok(manifest.captured_at)
}

#[cfg(unix)]
fn collect_backup_paths(root: &Path) -> Result<Vec<PathBuf>, LegacyError> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|_| LegacyError::Backup)? {
        let entry = entry.map_err(|_| LegacyError::Backup)?;
        let name = entry.file_name();
        if name == "manifest.json" {
            continue;
        } else if name == "by-cwd" {
            validate_private_backup_directory(&entry.path())?;
            for child in std::fs::read_dir(entry.path()).map_err(|_| LegacyError::Backup)? {
                let child = child.map_err(|_| LegacyError::Backup)?;
                paths.push(PathBuf::from("by-cwd").join(child.file_name()));
            }
        } else if name == "history.log" || name == "chat-id" || name == "chat-id.export" {
            paths.push(PathBuf::from(name));
        } else {
            return Err(LegacyError::Backup);
        }
    }
    paths.sort_by(|left, right| path_bytes(left).cmp(path_bytes(right)));
    Ok(paths)
}

#[cfg(unix)]
fn validate_private_backup_directory(path: &Path) -> Result<(), LegacyError> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let metadata = std::fs::symlink_metadata(path).map_err(|_| LegacyError::Backup)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(LegacyError::Backup);
    }
    Ok(())
}

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct BackupManifest {
    schema_version: u8,
    snapshot_sha256: String,
    captured_at: String,
    files: Vec<BackupManifestFile>,
}

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct BackupManifestFile {
    path_hex: String,
    source_role: String,
    bytes: usize,
    sha256: String,
}

#[cfg(unix)]
fn manifest_bytes(
    digest: &str,
    captured_at: &str,
    sources: &[LegacySource],
) -> Result<Vec<u8>, LegacyError> {
    serde_json::to_vec(&build_manifest(digest, captured_at, sources))
        .map_err(|_| LegacyError::Backup)
}

#[cfg(unix)]
fn build_manifest(digest: &str, captured_at: &str, sources: &[LegacySource]) -> BackupManifest {
    let files = sources
        .iter()
        .map(|source| BackupManifestFile {
            path_hex: lower_hex(path_bytes(&source.relative)),
            source_role: source_role(&source.relative).to_owned(),
            bytes: source.bytes.len(),
            sha256: lower_hex(&Sha256::digest(&source.bytes)),
        })
        .collect();
    BackupManifest {
        schema_version: 1,
        snapshot_sha256: digest.to_owned(),
        captured_at: captured_at.to_owned(),
        files,
    }
}

#[cfg(unix)]
fn source_role(relative: &Path) -> &'static str {
    if relative == Path::new("history.log") {
        "history"
    } else if relative == Path::new("chat-id") {
        "chat_id"
    } else if relative == Path::new("chat-id.export") {
        "backup_only_export"
    } else {
        "by_cwd"
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), LegacyError> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| LegacyError::Backup)
}

fn parse_entries(sources: &[LegacySource]) -> (Vec<LegacyEntry>, usize) {
    let mut entries = Vec::new();
    let mut skipped = 0usize;
    for source in sources {
        if source.relative == Path::new("chat-id.export") {
            // Retained for rollback only. Its bytes are never decoded or
            // interpreted as runtime conversation metadata.
            continue;
        }
        let Some(text) = std::str::from_utf8(&source.bytes).ok() else {
            skipped += 1;
            continue;
        };
        if source.relative == Path::new("history.log") {
            for line in text.lines() {
                if entries.len() >= MAX_ENTRIES {
                    skipped += 1;
                    continue;
                }
                let mut parts = line.splitn(3, '\t');
                let timestamp = parts.next();
                let legacy_id = parts.next();
                let parsed = timestamp
                    .zip(legacy_id)
                    .and_then(|(timestamp, legacy_id)| parse_history_entry(legacy_id, timestamp));
                if let Some(entry) = parsed {
                    entries.push(entry);
                } else {
                    skipped += 1;
                }
            }
            continue;
        }

        let kind = if source.relative == Path::new("chat-id") {
            LegacySourceKind::ChatId
        } else if source.relative.starts_with("by-cwd") {
            LegacySourceKind::ByCwd
        } else {
            skipped += 1;
            continue;
        };
        let raw = text.lines().next().unwrap_or("");
        if entries.len() >= MAX_ENTRIES {
            skipped += 1;
        } else if let Some(entry) = parse_direct_entry(raw, kind) {
            entries.push(entry);
        } else {
            skipped += 1;
        }
    }
    (entries, skipped)
}

fn parse_history_entry(legacy_id: &str, observed_at: &str) -> Option<LegacyEntry> {
    let identity = super::identity::external_identity(legacy_id).ok()?;
    if observed_at.len() > MAX_TIMESTAMP_BYTES {
        return None;
    }
    let observed_at = chrono::DateTime::parse_from_rfc3339(observed_at)
        .ok()?
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    Some(LegacyEntry {
        conversation_id: identity.conversation_id,
        alias_sha256: identity.alias_sha256,
        source_kind: LegacySourceKind::History,
        observed_at: Some(observed_at),
    })
}

fn parse_direct_entry(legacy_id: &str, source_kind: LegacySourceKind) -> Option<LegacyEntry> {
    let identity = super::identity::external_identity(legacy_id).ok()?;
    Some(LegacyEntry {
        conversation_id: identity.conversation_id,
        alias_sha256: identity.alias_sha256,
        source_kind,
        observed_at: None,
    })
}

#[cfg(test)]
pub(crate) fn legacy_conversation_id(legacy_id: &str) -> ConversationId {
    super::identity::external_identity(legacy_id)
        .expect("legacy conversation id is valid")
        .conversation_id
}

#[cfg(test)]
fn legacy_alias_digest(legacy_id: &str) -> String {
    super::identity::external_identity(legacy_id)
        .expect("legacy conversation id is valid")
        .alias_sha256
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_mapping_is_deterministic_v8_and_debug_is_redacted() {
        let first = legacy_conversation_id("private-chat-id");
        assert_eq!(first, legacy_conversation_id("private-chat-id"));
        assert_ne!(first, legacy_conversation_id("another-private-chat-id"));
        assert_eq!(first.as_str().as_bytes()[14], b'8');
        let entry = LegacyEntry {
            alias_sha256: legacy_alias_digest("private-chat-id"),
            conversation_id: first,
            source_kind: LegacySourceKind::History,
            observed_at: Some("2026-08-08T00:00:00Z".into()),
        };
        let debug = format!("{entry:?}");
        for private in ["private-chat-id", "private-cwd", "/private/cwd"] {
            assert!(!debug.contains(private));
        }
    }

    #[cfg(unix)]
    #[test]
    fn stable_capture_retries_exactly_three_times() {
        let mut attempts = 0;
        let recovered = capture_with_retry(|| {
            attempts += 1;
            if attempts == 1 {
                Err(LegacyError::UnstableSnapshot)
            } else {
                Ok(Vec::new())
            }
        })
        .unwrap();
        assert!(recovered.is_empty());
        assert_eq!(attempts, 2);

        attempts = 0;
        let runtime = std::env::temp_dir().join(format!(
            "abbey-legacy-retry-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        assert!(matches!(
            prepare_with_capture(&runtime, || {
                attempts += 1;
                Err(LegacyError::UnstableSnapshot)
            }),
            Err(LegacyError::UnstableSnapshot)
        ));
        assert_eq!(attempts, 3);
        assert!(!runtime.join("legacy-conversation-backups").exists());
    }

    #[test]
    fn source_limits_are_realistic_and_still_fixed() {
        assert_eq!(MAX_SOURCE_FILES, 1_024);
        assert_eq!(MAX_SOURCE_BYTES, 2 * 1_024 * 1_024);
        assert_eq!(MAX_TOTAL_BYTES, 8 * 1_024 * 1_024);
    }
}
