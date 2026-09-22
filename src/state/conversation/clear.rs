//! Canonical clear tombstones and idempotent compatibility-mirror removal.

use super::*;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

const MAX_CLEAR_TARGETS: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ClearOperation {
    ClearScope,
    ClearAll,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ClearTargetRole {
    Active,
    Global,
    Export,
    Cwd,
}

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
struct ClearTarget {
    role: ClearTargetRole,
    path_hex: String,
    before_hex: Option<String>,
}

#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
struct ClearPlan {
    schema_version: u32,
    operation: ClearOperation,
    mutation_token: String,
    edition_slug: String,
    cwd_hex: String,
    per_cwd: bool,
    targets: Vec<ClearTarget>,
}

impl fmt::Debug for ClearPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClearPlan")
            .field("schema_version", &self.schema_version)
            .field("operation", &self.operation)
            .field("mutation_token", &"[REDACTED]")
            .field("edition_slug", &self.edition_slug)
            .field("cwd", &"[REDACTED]")
            .field("per_cwd", &self.per_cwd)
            .field("target_count", &self.targets.len())
            .finish()
    }
}

impl ClearPlan {
    fn cwd(&self) -> Result<PathBuf> {
        path_from_hex(&self.cwd_hex).context("conversation clear journal cwd is invalid")
    }

    fn scopes(&self) -> Result<Option<Vec<ConversationIdentityScope>>> {
        match self.operation {
            ClearOperation::ClearScope if self.per_cwd => {
                Ok(Some(vec![ConversationIdentityScope::working_directory(
                    &self.cwd()?,
                )]))
            }
            ClearOperation::ClearScope => Ok(Some(vec![ConversationIdentityScope::global()])),
            ClearOperation::ClearAll => Ok(None),
        }
    }
}

pub(super) fn clear_chat(state: &AbbeyState, all: bool) -> Result<()> {
    validate_layout(state)?;
    let journal = lock_journal(state)?;
    super::recover_pending(state, &journal)?;
    let plan = prepare_plan(state, all)?;
    write_plan(&journal, &plan)?;
    maybe_failpoint("after_clear_journal_prepare");

    let store = open_metadata_store(state)?;
    let scopes = plan.scopes()?;
    let commit = store
        .clear_conversation_identity(&plan.edition_slug, scopes.as_deref(), &plan.mutation_token)
        .context("canonical conversation identity clear failed")?;
    maybe_failpoint("after_clear_canonical_commit");
    ensure_commit_effect(&store, &plan, &commit)?;
    apply_committed(state, &plan)?;
    remove_pending(&journal)?;
    Ok(())
}

pub(super) fn is_clear_plan(bytes: &[u8]) -> Result<bool> {
    let value: Value = serde_json::from_slice(bytes)
        .context("conversation mirror journal is malformed; refusing recovery")?;
    Ok(matches!(
        value.get("operation").and_then(Value::as_str),
        Some("clear_scope" | "clear_all")
    ))
}

pub(super) fn recover_pending(
    state: &AbbeyState,
    journal: &JournalGuard,
    bytes: &[u8],
) -> Result<()> {
    let plan: ClearPlan = serde_json::from_slice(bytes)
        .context("conversation clear journal is malformed; refusing recovery")?;
    validate_plan(state, &plan)?;
    let store = open_metadata_store(state)?;
    let committed = store
        .current_identity_commit()
        .context("canonical conversation clear marker could not be read")?;
    let authenticated = committed
        .as_ref()
        .is_some_and(|commit| match plan.operation {
            ClearOperation::ClearScope => plan.scopes().ok().flatten().is_some_and(|scopes| {
                commit.matches_clear_scopes(&plan.edition_slug, &scopes, &plan.mutation_token)
            }),
            ClearOperation::ClearAll => {
                commit.matches_clear_all(&plan.edition_slug, &plan.mutation_token)
            }
        });
    if let (true, Some(commit)) = (authenticated, committed.as_ref()) {
        ensure_commit_effect(&store, &plan, commit)?;
        apply_committed(state, &plan)?;
    }
    remove_pending(journal)
}

fn ensure_commit_effect(
    store: &RuntimeStore,
    plan: &ClearPlan,
    commit: &IdentityCommit,
) -> Result<()> {
    let scopes = plan.scopes()?;
    store
        .verify_clear_conversation_identity(&plan.edition_slug, scopes.as_deref(), commit)
        .context("canonical conversation clear effect is inconsistent")
}

fn prepare_plan(state: &AbbeyState, all: bool) -> Result<ClearPlan> {
    let operation = if all {
        ClearOperation::ClearAll
    } else {
        ClearOperation::ClearScope
    };
    let paths = if all {
        all_targets(state)?
    } else {
        current_targets(state)?
    };
    if !all {
        let candidate = read_first_line_bounded(&paths[0].1)?;
        let scope = if state.per_cwd {
            ConversationIdentityScope::working_directory(&state.cwd)
        } else {
            ConversationIdentityScope::global()
        };
        open_metadata_store(state)?
            .authorize_clear_scope_candidate(
                crate::edition::ACTIVE.slug(),
                &scope,
                candidate.as_deref(),
            )
            .context("conversation clear target is not uniquely owned by its scope")?;
    }
    let targets = paths
        .into_iter()
        .map(|(role, path)| {
            let before = read_optional_bounded(&path, MAX_ID_FILE_BYTES)?;
            Ok(ClearTarget {
                role,
                path_hex: path_to_hex(&path),
                before_hex: before.as_deref().map(lower_hex),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let plan = ClearPlan {
        schema_version: JOURNAL_SCHEMA,
        operation,
        mutation_token: String::new(),
        edition_slug: crate::edition::ACTIVE.slug().to_owned(),
        cwd_hex: path_to_hex(&state.cwd),
        per_cwd: state.per_cwd,
        targets,
    };
    let nonce = uuid::Uuid::new_v4().to_string();
    let mut plan = plan;
    plan.mutation_token = format!("{nonce}:{}", plan_binding_digest(&plan, &nonce));
    validate_plan(state, &plan)?;
    Ok(plan)
}

fn current_targets(state: &AbbeyState) -> Result<Vec<(ClearTargetRole, PathBuf)>> {
    if state.per_cwd {
        return Ok(vec![(
            ClearTargetRole::Active,
            resolved_mirror_path(state, &state.active_chat_file())?,
        )]);
    }
    let global = resolved_mirror_path(state, &state.chat_file)?;
    Ok(vec![
        (ClearTargetRole::Global, global.clone()),
        (ClearTargetRole::Export, global.with_extension("export")),
    ])
}

fn all_targets(state: &AbbeyState) -> Result<Vec<(ClearTargetRole, PathBuf)>> {
    let global = resolved_mirror_path(state, &state.chat_file)?;
    let cwd_dir = canonical_directory(&resolved_path(state, &state.cwd_dir)?)?;
    let mut targets = vec![
        (ClearTargetRole::Global, global.clone()),
        (ClearTargetRole::Export, global.with_extension("export")),
    ];
    let mut cwd_targets = Vec::new();
    for entry in fs::read_dir(&cwd_dir)? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "conversation by-cwd clear target is not a direct regular file"
        );
        cwd_targets.push((ClearTargetRole::Cwd, canonical_target(&entry.path())?));
        ensure!(
            cwd_targets.len() <= MAX_CLEAR_TARGETS.saturating_sub(2),
            "conversation clear target count exceeds its bound"
        );
    }
    cwd_targets.sort_by_key(|target| path_to_hex(&target.1));
    targets.extend(cwd_targets);
    Ok(targets)
}

fn validate_plan(state: &AbbeyState, plan: &ClearPlan) -> Result<()> {
    ensure!(
        plan.schema_version == JOURNAL_SCHEMA,
        "unsupported conversation clear journal schema"
    );
    ensure!(
        plan.edition_slug == crate::edition::ACTIVE.slug(),
        "conversation clear journal belongs to another edition"
    );
    let (nonce, binding) = plan
        .mutation_token
        .split_once(':')
        .context("conversation clear journal mutation token is invalid")?;
    ensure!(
        uuid::Uuid::parse_str(nonce).is_ok() && binding == plan_binding_digest(plan, nonce),
        "conversation clear journal mutation token is invalid"
    );
    ensure!(
        !plan.targets.is_empty() && plan.targets.len() <= MAX_CLEAR_TARGETS,
        "conversation clear target count is invalid"
    );
    let mut paths = Vec::with_capacity(plan.targets.len());
    for target in &plan.targets {
        let path = path_from_hex(&target.path_hex)
            .context("conversation clear journal target path is invalid")?;
        ensure!(
            path.is_absolute(),
            "conversation clear target is not absolute"
        );
        ensure_not_runtime_path(state, &path)?;
        validate_target_parent(&path)?;
        ensure!(
            !paths.contains(&path),
            "conversation clear targets must be pairwise distinct"
        );
        paths.push(path);
        if let Some(before) = target.before_hex.as_deref() {
            ensure!(
                before.len() <= MAX_ID_FILE_BYTES * 2 && before.len().is_multiple_of(2),
                "conversation clear snapshot exceeds its bound"
            );
            let _ = decode_hex(before)?;
        }
    }
    match plan.operation {
        ClearOperation::ClearScope if plan.per_cwd => {
            ensure!(
                plan.targets.len() == 1,
                "current-scope clear target count is invalid"
            );
            ensure!(
                plan.targets[0].role == ClearTargetRole::Active,
                "current-scope clear target role is invalid"
            );
            let expected = resolved_mirror_path(
                state,
                &state.cwd_dir.join(AbbeyState::cwd_key(&plan.cwd()?)),
            )?;
            ensure!(
                paths[0] == expected,
                "current-scope clear target is invalid"
            );
        }
        ClearOperation::ClearScope => {
            ensure!(
                plan.targets.len() == 2
                    && plan.targets.iter().all(|target| matches!(
                        target.role,
                        ClearTargetRole::Global | ClearTargetRole::Export
                    )),
                "global-scope clear target roles are invalid"
            );
            validate_global_pair(plan, &paths)?;
        }
        ClearOperation::ClearAll => {
            ensure!(
                plan.targets.iter().all(|target| matches!(
                    target.role,
                    ClearTargetRole::Global | ClearTargetRole::Export | ClearTargetRole::Cwd
                )),
                "all-scope clear target roles are invalid"
            );
            validate_global_pair(plan, &paths)?;
            let cwd_dir = canonical_directory(&resolved_path(state, &state.cwd_dir)?)?;
            for (target, path) in plan.targets.iter().zip(&paths) {
                if target.role == ClearTargetRole::Cwd {
                    ensure!(
                        path.parent() == Some(cwd_dir.as_path()),
                        "all-scope clear target is outside the by-cwd directory"
                    );
                }
            }
            for entry in fs::read_dir(&cwd_dir)? {
                let entry = entry?;
                ensure!(
                    entry.file_type()?.is_file(),
                    "conversation by-cwd clear target is not a direct regular file"
                );
                let present = canonical_target(&entry.path())?;
                ensure!(
                    paths.contains(&present),
                    "all-scope clear journal omits a current by-cwd mirror"
                );
            }
        }
    }
    Ok(())
}

fn validate_global_pair(plan: &ClearPlan, paths: &[PathBuf]) -> Result<()> {
    let global = plan
        .targets
        .iter()
        .position(|target| target.role == ClearTargetRole::Global)
        .context("conversation clear global target is missing")?;
    let export = plan
        .targets
        .iter()
        .position(|target| target.role == ClearTargetRole::Export)
        .context("conversation clear export target is missing")?;
    ensure!(
        paths[export] == paths[global].with_extension("export"),
        "conversation clear export target does not match global"
    );
    ensure!(
        plan.targets
            .iter()
            .filter(|target| target.role == ClearTargetRole::Global)
            .count()
            == 1
            && plan
                .targets
                .iter()
                .filter(|target| target.role == ClearTargetRole::Export)
                .count()
                == 1,
        "conversation clear global target roles are duplicated"
    );
    Ok(())
}

fn write_plan(journal: &JournalGuard, plan: &ClearPlan) -> Result<()> {
    let bytes = serde_json::to_vec(plan)?;
    ensure!(
        bytes.len() <= MAX_JOURNAL_BYTES,
        "conversation clear journal exceeds its size bound"
    );
    atomic_replace(&journal.pending, &bytes)
}

fn apply_committed(state: &AbbeyState, plan: &ClearPlan) -> Result<()> {
    validate_plan(state, plan)?;
    for (index, target) in plan.targets.iter().enumerate() {
        let path = path_from_hex(&target.path_hex)
            .context("conversation clear journal target path is invalid")?;
        let before = target.before_hex.as_deref().map(decode_hex).transpose()?;
        let current = read_optional_bounded(&path, MAX_ID_FILE_BYTES)?;
        ensure!(
            current.is_none() || current == before,
            "conversation mirror diverged after its canonical clear"
        );
        if current.is_some() {
            remove_mirror(&path)?;
        }
        if index == 0 {
            maybe_failpoint("after_clear_first_removal");
        }
    }
    Ok(())
}

fn remove_mirror(path: &Path) -> Result<()> {
    let _ = open_private_file(path, false)?;
    fs::remove_file(path)?;
    sync_directory(
        path.parent()
            .context("conversation clear target has no parent directory")?,
    )
}

fn plan_binding_digest(plan: &ClearPlan, nonce: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"abbey:conversation-clear-plan:v1\0");
    digest.update(nonce.as_bytes());
    digest.update([match plan.operation {
        ClearOperation::ClearScope => 1,
        ClearOperation::ClearAll => 2,
    }]);
    digest.update(plan.edition_slug.as_bytes());
    digest.update(plan.cwd_hex.as_bytes());
    digest.update([u8::from(plan.per_cwd)]);
    digest.update((plan.targets.len() as u64).to_be_bytes());
    for target in &plan.targets {
        digest.update([match target.role {
            ClearTargetRole::Active => 1,
            ClearTargetRole::Global => 2,
            ClearTargetRole::Export => 3,
            ClearTargetRole::Cwd => 4,
        }]);
        digest.update((target.path_hex.len() as u64).to_be_bytes());
        digest.update(target.path_hex.as_bytes());
        match target.before_hex.as_deref() {
            Some(before) => {
                digest.update([1]);
                digest.update((before.len() as u64).to_be_bytes());
                digest.update(before.as_bytes());
            }
            None => digest.update([0]),
        }
    }
    lower_hex(&digest.finalize())
}

#[cfg(test)]
#[path = "clear_tests.rs"]
mod tests;
