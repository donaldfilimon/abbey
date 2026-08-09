//! Canonical identity commit plus crash-safe legacy-mirror projection.

use super::{AbbeyState, HistoryEntry};
use crate::runtime::{ConversationIdentityScope, IdentityCommit, RuntimeStore};
use anyhow::{Context, Result, bail, ensure};
use fs4::fs_std::FileExt as _;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

const JOURNAL_SCHEMA: u32 = 1;
const JOURNAL_DIR: &str = "conversation-mirror-journal";
const JOURNAL_LOCK: &str = "lock";
const JOURNAL_PENDING: &str = "pending.json";
const MAX_ID_FILE_BYTES: usize = 4 * 1024;
const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;
const MAX_JOURNAL_BYTES: usize = 2 * MAX_HISTORY_BYTES + 64 * 1024;
#[cfg(debug_assertions)]
const FAILPOINT_ENV: &str = "ABBEY_TEST_CONVERSATION_FAILPOINT";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TargetRole {
    Active,
    Global,
    Export,
    History,
}

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
struct MirrorSnapshot {
    role: TargetRole,
    path_hex: String,
    before_hex: Option<String>,
}

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
struct JournalPlan {
    schema_version: u32,
    mutation_token: String,
    edition_slug: String,
    external_id: String,
    cwd_hex: String,
    cwd_display: String,
    per_cwd: bool,
    targets: Vec<MirrorSnapshot>,
}

impl fmt::Debug for JournalPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JournalPlan")
            .field("schema_version", &self.schema_version)
            .field("mutation_token", &"[REDACTED]")
            .field("edition_slug", &self.edition_slug)
            .field("external_id", &"[REDACTED]")
            .field("cwd", &"[REDACTED]")
            .field("per_cwd", &self.per_cwd)
            .field("target_count", &self.targets.len())
            .finish()
    }
}

struct JournalGuard {
    _lock: File,
    directory: PathBuf,
    pending: PathBuf,
}

/// Blocks saves across daemon legacy capture and import.
pub(crate) struct LegacyCaptureGuard {
    _journal: JournalGuard,
}

pub(crate) fn lock_legacy_capture(state_root: &Path) -> Result<LegacyCaptureGuard> {
    let cwd = std::env::current_dir()?;
    let state = AbbeyState {
        state_dir: state_root.to_path_buf(),
        chat_file: state_root.join("chat-id"),
        model_file: state_root.join("model"),
        history_file: state_root.join("history.log"),
        cwd_dir: state_root.join("by-cwd"),
        per_cwd: true,
        cwd,
    };
    validate_layout(&state)?;
    let journal = lock_journal(&state)?;
    recover_pending(&state, &journal)?;
    Ok(LegacyCaptureGuard { _journal: journal })
}

pub(super) fn read_chat(state: &AbbeyState) -> Result<Option<String>> {
    validate_layout(state)?;
    let journal = lock_journal(state)?;
    recover_pending(state, &journal)?;

    let active = resolved_mirror_path(state, &state.active_chat_file())?;
    if let Some(id) = read_first_line_bounded(&active)? {
        return Ok(Some(id));
    }
    if state.per_cwd {
        return read_first_line_bounded(&resolved_mirror_path(state, &state.chat_file)?);
    }
    Ok(None)
}

pub(super) fn ensure_ready(state: &AbbeyState) -> Result<()> {
    validate_layout(state)?;
    let journal = lock_journal(state)?;
    recover_pending(state, &journal)
}

pub(super) fn history(state: &AbbeyState, count: usize) -> Result<Vec<HistoryEntry>> {
    validate_layout(state)?;
    let journal = lock_journal(state)?;
    recover_pending(state, &journal)?;
    let history_path = resolved_mirror_path(state, &state.history_file)?;
    let Some(bytes) = read_optional_bounded(&history_path, MAX_HISTORY_BYTES)? else {
        return Ok(Vec::new());
    };
    let text = std::str::from_utf8(&bytes).context("conversation history is not UTF-8")?;
    Ok(text
        .lines()
        .rev()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            Some(HistoryEntry {
                timestamp: parts.next()?.to_owned(),
                chat_id: parts.next()?.to_owned(),
                cwd: parts.next().unwrap_or("").to_owned(),
            })
        })
        .take(count)
        .collect())
}

pub(super) fn compact_history(state: &AbbeyState, keep: usize) -> Result<usize> {
    validate_layout(state)?;
    let journal = lock_journal(state)?;
    recover_pending(state, &journal)?;
    let path = resolved_mirror_path(state, &state.history_file)?;
    let Some(bytes) = read_optional_bounded(&path, MAX_HISTORY_BYTES)? else {
        return Ok(0);
    };
    let text = std::str::from_utf8(&bytes).context("conversation history is not UTF-8")?;
    let lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let start = lines.len().saturating_sub(keep.max(1));
    let kept = &lines[start..];
    let mut output = kept.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    atomic_replace(&path, output.as_bytes())?;
    Ok(kept.len())
}

pub(super) fn write_model(state: &AbbeyState, model: &str) -> Result<()> {
    validate_layout(state)?;
    let journal = lock_journal(state)?;
    recover_pending(state, &journal)?;
    atomic_replace(
        &resolved_mirror_path(state, &state.model_file)?,
        format!("{model}\n").as_bytes(),
    )
}

pub(super) fn clear_model(state: &AbbeyState) -> Result<()> {
    validate_layout(state)?;
    let journal = lock_journal(state)?;
    recover_pending(state, &journal)?;
    let model = resolved_mirror_path(state, &state.model_file)?;
    let _ = fs::remove_file(&model);
    sync_directory(
        model
            .parent()
            .context("model file has no parent directory")?,
    )
}

pub(super) fn save_chat(state: &AbbeyState, id: &str) -> Result<()> {
    validate_layout(state)?;
    let id = validate_external_id(id)?;
    let journal = lock_journal(state)?;
    recover_pending(state, &journal)?;

    let plan = prepare_plan(state, id)?;
    write_pending(&journal, &plan)?;
    maybe_failpoint("after_journal_prepare");

    let store = open_metadata_store(state)?;
    let scopes = plan.scopes()?;
    let commit = store
        .save_conversation_identity(
            &plan.edition_slug,
            &scopes,
            &plan.external_id,
            &plan.mutation_token,
        )
        .context("canonical conversation identity save failed")?;
    maybe_failpoint("after_canonical_commit");

    apply_committed(state, &plan, &commit)?;
    remove_pending(&journal)?;
    Ok(())
}

/// Serialized legacy clear; canonical tombstones remain Phase 4B.6 work.
pub(super) fn clear_legacy_chat(state: &AbbeyState, all: bool) -> Result<()> {
    validate_layout(state)?;
    let journal = lock_journal(state)?;
    recover_pending(state, &journal)?;
    let _ = fs::remove_file(resolved_mirror_path(state, &state.active_chat_file())?);
    if all || !state.per_cwd {
        let global = resolved_mirror_path(state, &state.chat_file)?;
        let _ = fs::remove_file(&global);
        let _ = fs::remove_file(global.with_extension("export"));
    }
    let cwd_dir = canonical_directory(&resolved_path(state, &state.cwd_dir)?)?;
    if all && let Ok(entries) = fs::read_dir(&cwd_dir) {
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }
    sync_directory(&canonical_directory(&resolved_path(
        state,
        &state.state_dir,
    )?)?)?;
    sync_directory(&cwd_dir)?;
    Ok(())
}

fn validate_external_id(value: &str) -> Result<&str> {
    let value = value.trim();
    ensure!(
        !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control),
        "conversation id is empty, contains controls, or exceeds 512 bytes"
    );
    Ok(value)
}

impl JournalPlan {
    fn cwd(&self) -> Result<PathBuf> {
        path_from_hex(&self.cwd_hex).context("conversation journal cwd is invalid")
    }

    fn scopes(&self) -> Result<Vec<ConversationIdentityScope>> {
        let mut scopes = if self.per_cwd {
            vec![ConversationIdentityScope::working_directory(&self.cwd()?)]
        } else {
            Vec::new()
        };
        scopes.push(ConversationIdentityScope::global());
        Ok(scopes)
    }

    fn target_path(&self, role: TargetRole) -> Result<PathBuf> {
        let snapshot = self
            .targets
            .iter()
            .find(|target| target.role == role)
            .context("conversation mirror journal target is missing")?;
        path_from_hex(&snapshot.path_hex)
            .context("conversation mirror journal target path is invalid")
    }
}

fn prepare_plan(state: &AbbeyState, id: &str) -> Result<JournalPlan> {
    let cwd_display = state.cwd.to_string_lossy().into_owned();
    ensure!(
        !cwd_display.chars().any(char::is_control),
        "working directory cannot be represented in legacy history"
    );
    let mut plan = JournalPlan {
        schema_version: JOURNAL_SCHEMA,
        mutation_token: uuid::Uuid::new_v4().to_string(),
        edition_slug: crate::edition::ACTIVE.slug().to_owned(),
        external_id: id.to_owned(),
        cwd_hex: path_to_hex(&state.cwd),
        cwd_display,
        per_cwd: state.per_cwd,
        targets: Vec::new(),
    };
    plan.targets = configured_targets(state, &plan)?
        .into_iter()
        .map(|(role, path)| {
            let limit = target_limit(role);
            let before = read_optional_bounded(&path, limit)?;
            Ok(MirrorSnapshot {
                role,
                path_hex: path_to_hex(&path),
                before_hex: before.as_deref().map(lower_hex),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let history = plan
        .targets
        .iter()
        .find(|target| target.role == TargetRole::History)
        .and_then(|target| target.before_hex.as_deref())
        .map(decode_hex)
        .transpose()?
        .unwrap_or_default();
    ensure!(
        history.len() + id.len() + plan.cwd_display.len() + 96 <= MAX_HISTORY_BYTES,
        "conversation history exceeds its bounded mirror size"
    );
    validate_plan(state, &plan)?;
    Ok(plan)
}

fn recover_pending(state: &AbbeyState, journal: &JournalGuard) -> Result<()> {
    let Some(bytes) = read_optional_bounded(&journal.pending, MAX_JOURNAL_BYTES)? else {
        return Ok(());
    };
    let plan: JournalPlan = serde_json::from_slice(&bytes)
        .context("conversation mirror journal is malformed; refusing recovery")?;
    validate_plan(state, &plan)?;

    let store = open_metadata_store(state)?;
    let scopes = plan.scopes()?;
    let committed = store
        .current_identity_commit()
        .context("canonical conversation commit marker could not be read")?;
    match committed {
        Some(commit)
            if commit.matches_save_scopes(
                &plan.edition_slug,
                &scopes,
                &plan.external_id,
                &plan.mutation_token,
            ) =>
        {
            apply_committed(state, &plan, &commit)?;
            remove_pending(journal)?;
        }
        _ => {
            // A mismatched marker proves this prepared plan never committed.
            remove_pending(journal)?;
        }
    }
    Ok(())
}

fn apply_committed(state: &AbbeyState, plan: &JournalPlan, commit: &IdentityCommit) -> Result<()> {
    validate_plan(state, plan)?;
    for snapshot in &plan.targets {
        let path = path_from_hex(&snapshot.path_hex)
            .context("conversation mirror journal target path is invalid")?;
        let before = snapshot.before_hex.as_deref().map(decode_hex).transpose()?;
        let desired = desired_bytes(snapshot.role, plan, commit, before.as_deref())?;
        let current = read_optional_bounded(&path, target_limit(snapshot.role))?;
        if current.as_deref() == Some(desired.as_slice()) {
            continue;
        }
        ensure!(
            current.as_deref() == before.as_deref(),
            "conversation mirror diverged after its canonical commit"
        );
        atomic_replace(&path, &desired)?;
    }
    Ok(())
}

fn desired_bytes(
    role: TargetRole,
    plan: &JournalPlan,
    commit: &IdentityCommit,
    before: Option<&[u8]>,
) -> Result<Vec<u8>> {
    match role {
        TargetRole::Active | TargetRole::Global => {
            Ok(format!("{}\n", plan.external_id).into_bytes())
        }
        TargetRole::Export => {
            Ok(format!("ABBEY_CHAT_ID={}\n", posix_single_quote(&plan.external_id)).into_bytes())
        }
        TargetRole::History => {
            let mut bytes = before.unwrap_or_default().to_vec();
            if !bytes.is_empty() && !bytes.ends_with(b"\n") {
                bytes.push(b'\n');
            }
            writeln!(
                bytes,
                "{}\t{}\t{}",
                commit.committed_at, plan.external_id, plan.cwd_display
            )?;
            ensure!(
                bytes.len() <= MAX_HISTORY_BYTES,
                "conversation history exceeds its bounded mirror size"
            );
            Ok(bytes)
        }
    }
}

fn configured_targets(
    state: &AbbeyState,
    plan: &JournalPlan,
) -> Result<Vec<(TargetRole, PathBuf)>> {
    configured_targets_for(state, &plan.cwd()?, plan.per_cwd)
}

fn configured_targets_for(
    state: &AbbeyState,
    cwd: &Path,
    per_cwd: bool,
) -> Result<Vec<(TargetRole, PathBuf)>> {
    let active = if per_cwd {
        resolved_mirror_path(state, &state.cwd_dir.join(AbbeyState::cwd_key(cwd)))?
    } else {
        resolved_mirror_path(state, &state.chat_file)?
    };
    let global = resolved_mirror_path(state, &state.chat_file)?;
    let export = global.with_extension("export");
    let history = resolved_mirror_path(state, &state.history_file)?;
    let mut targets = Vec::with_capacity(4);
    if active != global {
        targets.push((TargetRole::Active, active));
    }
    targets.push((TargetRole::Global, global));
    targets.push((TargetRole::Export, export));
    targets.push((TargetRole::History, history));
    Ok(targets)
}

fn validate_plan(state: &AbbeyState, plan: &JournalPlan) -> Result<()> {
    ensure!(
        plan.schema_version == JOURNAL_SCHEMA,
        "unsupported conversation mirror journal schema"
    );
    ensure!(
        plan.edition_slug == crate::edition::ACTIVE.slug(),
        "conversation mirror journal belongs to another edition"
    );
    ensure!(
        uuid::Uuid::parse_str(&plan.mutation_token).is_ok(),
        "conversation mirror journal mutation token is invalid"
    );
    validate_external_id(&plan.external_id)?;
    ensure!(
        !plan.cwd_display.chars().any(char::is_control),
        "conversation mirror journal cwd is invalid"
    );
    let expected_count = if plan.per_cwd { 4 } else { 3 };
    ensure!(
        plan.targets.len() == expected_count,
        "conversation mirror journal target count is invalid"
    );
    let mut roles = Vec::with_capacity(expected_count);
    let mut paths = Vec::with_capacity(expected_count);
    for snapshot in &plan.targets {
        ensure!(
            !roles.contains(&snapshot.role),
            "conversation mirror journal contains a duplicate role"
        );
        roles.push(snapshot.role);
        let path = path_from_hex(&snapshot.path_hex)
            .context("conversation mirror journal target path is invalid")?;
        ensure!(
            path.is_absolute(),
            "conversation mirror journal target is not absolute"
        );
        ensure_not_runtime_path(state, &path)?;
        validate_target_parent(&path)?;
        ensure!(
            !paths.contains(&path),
            "conversation mirror targets must be pairwise distinct"
        );
        paths.push(path);
        if let Some(before) = snapshot.before_hex.as_deref() {
            ensure!(
                before.len() <= target_limit(snapshot.role) * 2 && before.len().is_multiple_of(2),
                "conversation mirror journal snapshot exceeds its bound"
            );
            let _ = decode_hex(before)?;
        }
    }
    for role in [TargetRole::Global, TargetRole::Export, TargetRole::History] {
        ensure!(
            roles.contains(&role),
            "conversation mirror journal target is missing"
        );
    }
    ensure!(
        roles.contains(&TargetRole::Active) == plan.per_cwd,
        "conversation mirror journal active target is invalid"
    );
    let global = plan.target_path(TargetRole::Global)?;
    ensure!(
        plan.target_path(TargetRole::Export)? == global.with_extension("export"),
        "conversation export target does not match the global mirror"
    );
    if plan.per_cwd {
        let expected_active = resolved_mirror_path(
            state,
            &state
                .cwd_dir
                .join(AbbeyState::cwd_key(plan.cwd()?.as_path())),
        )?;
        ensure!(
            plan.target_path(TargetRole::Active)? == expected_active,
            "conversation active target does not match its edition state"
        );
    }
    Ok(())
}

fn open_metadata_store(state: &AbbeyState) -> Result<RuntimeStore> {
    let runtime_dir = canonical_directory(&resolved_path(state, &state.state_dir)?)?.join("daemon");
    let path = RuntimeStore::path_for_state_dir(&runtime_dir);
    RuntimeStore::open_metadata_private(&path)
        .context("conversation metadata store could not be opened")
}

fn lock_journal(state: &AbbeyState) -> Result<JournalGuard> {
    let runtime_dir = canonical_directory(&resolved_path(state, &state.state_dir)?)?.join("daemon");
    private_directory(&runtime_dir)?;
    let directory = runtime_dir.join(JOURNAL_DIR);
    private_directory(&directory)?;
    let lock_path = directory.join(JOURNAL_LOCK);
    let lock = open_private_file(&lock_path, true)?;
    lock.lock_exclusive()
        .context("conversation mirror journal lock could not be acquired")?;
    Ok(JournalGuard {
        _lock: lock,
        pending: directory.join(JOURNAL_PENDING),
        directory,
    })
}

fn write_pending(journal: &JournalGuard, plan: &JournalPlan) -> Result<()> {
    let bytes = serde_json::to_vec(plan)?;
    ensure!(
        bytes.len() <= MAX_JOURNAL_BYTES,
        "conversation mirror journal exceeds its size bound"
    );
    atomic_replace(&journal.pending, &bytes)
}

fn remove_pending(journal: &JournalGuard) -> Result<()> {
    match fs::symlink_metadata(&journal.pending) {
        Ok(metadata) => {
            ensure!(
                metadata.file_type().is_file(),
                "conversation mirror journal is not a regular file"
            );
            fs::remove_file(&journal.pending)?;
            sync_directory(&journal.directory)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn validate_layout(state: &AbbeyState) -> Result<()> {
    private_directory(&resolved_path(state, &state.state_dir)?)?;
    private_directory(&resolved_path(state, &state.cwd_dir)?)?;
    let lexical_chat = resolved_path(state, &state.chat_file)?;
    for path in [
        resolved_path(state, &state.active_chat_file())?,
        lexical_chat.clone(),
        lexical_chat.with_extension("export"),
        resolved_path(state, &state.history_file)?,
        resolved_path(state, &state.model_file)?,
    ] {
        ensure_not_runtime_path(state, &path)?;
    }
    let targets = configured_targets_for(state, &state.cwd, state.per_cwd)?;
    let mut paths = Vec::with_capacity(targets.len() + 1);
    for (_, path) in targets {
        ensure_not_runtime_path(state, &path)?;
        validate_target_parent(&path)?;
        ensure!(
            !paths.contains(&path),
            "conversation mirror targets must be pairwise distinct"
        );
        paths.push(path);
    }
    let model = resolved_mirror_path(state, &state.model_file)?;
    ensure_not_runtime_path(state, &model)?;
    validate_target_parent(&model)?;
    ensure!(
        !paths.contains(&model),
        "model and conversation mirror targets must be distinct"
    );
    Ok(())
}

fn ensure_not_runtime_path(state: &AbbeyState, path: &Path) -> Result<()> {
    let lexical_runtime = resolved_path(state, &state.state_dir)?.join("daemon");
    let canonical_runtime =
        canonical_directory(&resolved_path(state, &state.state_dir)?)?.join("daemon");
    ensure!(
        !path.starts_with(&lexical_runtime) && !path.starts_with(&canonical_runtime),
        "conversation and model files cannot use the daemon runtime subtree"
    );
    Ok(())
}

fn resolved_path(state: &AbbeyState, path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let base = if state.cwd.is_absolute() {
            state.cwd.clone()
        } else {
            std::env::current_dir()?.join(&state.cwd)
        };
        base.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                ensure!(
                    normalized.pop(),
                    "conversation mirror path escapes the filesystem root"
                );
            }
        }
    }
    ensure!(
        normalized.is_absolute(),
        "conversation mirror path could not be resolved"
    );
    Ok(normalized)
}

fn resolved_mirror_path(state: &AbbeyState, path: &Path) -> Result<PathBuf> {
    canonical_target(&resolved_path(state, path)?)
}

fn canonical_target(path: &Path) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .context("conversation mirror target has no file name")?;
    let parent = path
        .parent()
        .context("conversation mirror target has no parent directory")?;
    let parent = canonical_directory(parent)?;
    Ok(parent.join(file_name))
}

fn canonical_directory(path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    validate_secure_directory(&canonical)?;
    Ok(canonical)
}

fn validate_target_parent(path: &Path) -> Result<()> {
    ensure!(
        canonical_target(path)? == path,
        "conversation mirror target parent is not canonical"
    );
    Ok(())
}

#[cfg(unix)]
fn validate_secure_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_dir(),
        "conversation mirror parent is not a real directory"
    );
    ensure!(
        metadata.uid() == nix::unistd::Uid::effective().as_raw(),
        "conversation mirror parent is not owned by the current user"
    );
    ensure!(
        metadata.permissions().mode() & 0o022 == 0,
        "conversation mirror parent is writable by another user"
    );
    Ok(())
}

#[cfg(not(unix))]
fn validate_secure_directory(path: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_dir(),
        "conversation mirror parent is not a real directory"
    );
    Ok(())
}

fn posix_single_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

fn target_limit(role: TargetRole) -> usize {
    match role {
        TargetRole::History => MAX_HISTORY_BYTES,
        TargetRole::Active | TargetRole::Global | TargetRole::Export => MAX_ID_FILE_BYTES,
    }
}

fn read_first_line_bounded(path: &Path) -> Result<Option<String>> {
    let Some(bytes) = read_optional_bounded(path, MAX_ID_FILE_BYTES)? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes).context("conversation mirror is not UTF-8")?;
    Ok(text
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned))
}

fn read_optional_bounded(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    let mut file = match open_private_file(path, false) {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    ensure!(
        usize::try_from(file.metadata()?.len()).unwrap_or(usize::MAX) <= limit,
        "conversation mirror exceeds its size bound"
    );
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "conversation mirror exceeds its size bound"
    );
    Ok(Some(bytes))
}

fn private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(
            metadata.file_type().is_dir(),
            "conversation journal path is not a directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(create_error) = fs::create_dir(path)
                && create_error.kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(create_error.into());
            }
        }
        Err(error) => return Err(error.into()),
    }
    set_private_directory_permissions(path)?;
    validate_owner(path, true)
}

fn open_private_file(path: &Path, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(create).create(create);
    configure_private_open(&mut options);
    let file = options.open(path)?;
    validate_open_file(&file)?;
    if create {
        set_private_file_permissions(path)?;
    }
    Ok(file)
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("conversation mirror has no parent directory")?;
    if !parent.exists() {
        fs::create_dir_all(parent)?;
    }
    validate_secure_directory(parent)?;
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let _ = read_optional_bounded(path, bytes.len().max(MAX_ID_FILE_BYTES))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temporary = parent.join(format!(".abbey-mirror-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_open(&mut options);
    let mut file = options.open(&temporary)?;
    set_private_file_permissions(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        sync_directory(parent)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn configure_private_open(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;
    options.mode(0o600);
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
}

#[cfg(not(unix))]
fn configure_private_open(_options: &mut OpenOptions) {}

fn validate_open_file(file: &File) -> Result<()> {
    ensure!(
        file.metadata()?.file_type().is_file(),
        "conversation mirror is not a regular file"
    );
    validate_open_owner(file)?;
    make_open_file_private(file)
}

#[cfg(unix)]
fn validate_open_owner(file: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    ensure!(
        file.metadata()?.uid() == nix::unistd::Uid::effective().as_raw(),
        "conversation mirror is not owned by the current user"
    );
    Ok(())
}

#[cfg(not(unix))]
fn validate_open_owner(_file: &File) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_open_file_private(file: &File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_open_file_private(_file: &File) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn validate_owner(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.uid() == nix::unistd::Uid::effective().as_raw(),
        "conversation journal is not owned by the current user"
    );
    ensure!(
        if directory {
            metadata.file_type().is_dir()
        } else {
            metadata.file_type().is_file()
        },
        "conversation journal has the wrong file type"
    );
    ensure!(
        metadata.permissions().mode() & 0o077 == 0,
        "conversation journal permissions are not owner-only"
    );
    Ok(())
}

#[cfg(not(unix))]
fn validate_owner(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        if directory {
            metadata.file_type().is_dir()
        } else {
            metadata.file_type().is_file()
        },
        "conversation journal has the wrong file type"
    );
    Ok(())
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn path_to_hex(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;
    lower_hex(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_to_hex(path: &Path) -> String {
    lower_hex(path.to_string_lossy().as_bytes())
}

#[cfg(unix)]
fn path_from_hex(value: &str) -> Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;
    Ok(PathBuf::from(OsString::from_vec(decode_hex(value)?)))
}

#[cfg(not(unix))]
fn path_from_hex(value: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(String::from_utf8(decode_hex(value)?)?))
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

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    ensure!(
        value.len().is_multiple_of(2),
        "conversation journal contains invalid hex"
    );
    (0..value.len())
        .step_by(2)
        .map(|index| {
            let bytes = value.as_bytes();
            let high = hex_nibble(bytes[index])?;
            let low = hex_nibble(bytes[index + 1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => bail!("conversation journal contains invalid hex"),
    }
}

fn maybe_failpoint(name: &str) {
    #[cfg(debug_assertions)]
    if std::env::var(FAILPOINT_ENV).as_deref() == Ok(name) {
        std::process::exit(86);
    }
    #[cfg(not(debug_assertions))]
    let _ = name;
}

#[cfg(test)]
mod tests;
