//! Canonical identity commit plus crash-safe legacy-mirror projection.

use super::{AbbeyState, HistoryEntry};
use crate::runtime::{
    ConversationIdentityScope, IdentityCommit, IdentityScopeSelection, RuntimeStore,
};
// Retained for the nested clear module's focused legacy-classifier tests.
#[allow(unused_imports)]
use crate::runtime::IdentityScopeState;
use anyhow::{Context, Result, bail, ensure};
use fs4::fs_std::FileExt as _;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

mod clear;
mod private_fs;

use private_fs::*;

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

    let store = open_metadata_store(state)?;
    if state.per_cwd {
        let active = resolved_mirror_path(state, &state.active_chat_file())?;
        let scope = ConversationIdentityScope::working_directory(&state.cwd);
        if let Some(id) = read_authoritative_mirror(&store, &scope, &active)? {
            return Ok(Some(id));
        }
    }
    let global = resolved_mirror_path(state, &state.chat_file)?;
    read_authoritative_mirror(&store, &ConversationIdentityScope::global(), &global)
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

pub(super) fn clear_chat(state: &AbbeyState, all: bool) -> Result<()> {
    clear::clear_chat(state, all)
}

fn read_authoritative_mirror(
    store: &RuntimeStore,
    scope: &ConversationIdentityScope,
    path: &Path,
) -> Result<Option<String>> {
    let selection = store
        .identity_scope_selection(crate::edition::ACTIVE.slug(), scope)
        .context("canonical conversation identity selection could not be read")?;
    match selection {
        IdentityScopeSelection::Untracked => read_first_line_bounded(path),
        IdentityScopeSelection::Tombstoned => Ok(None),
        IdentityScopeSelection::Selected { .. } => {
            let candidate = read_first_line_bounded(path)?
                .context("selected canonical conversation mirror is missing or malformed")?;
            ensure!(
                selection
                    .matches_external_id(&candidate)
                    .context("selected conversation mirror identity is invalid")?,
                "conversation mirror diverged from canonical identity selection"
            );
            Ok(Some(candidate))
        }
    }
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
    if clear::is_clear_plan(&bytes)? {
        return clear::recover_pending(state, journal, &bytes);
    }
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
