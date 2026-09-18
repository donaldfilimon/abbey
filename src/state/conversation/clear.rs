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
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    struct Scratch {
        root: PathBuf,
        state: AbbeyState,
    }

    impl Scratch {
        fn new(tag: &str, per_cwd: bool) -> Self {
            let root = std::env::temp_dir().join(format!(
                "abbey-clear-{tag}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            let state_dir = root.join("state");
            let cwd = root.join("private-cwd");
            fs::create_dir_all(state_dir.join("by-cwd")).unwrap();
            fs::create_dir(&cwd).unwrap();
            Self {
                state: AbbeyState {
                    chat_file: state_dir.join("chat-id"),
                    model_file: state_dir.join("model"),
                    history_file: state_dir.join("history.log"),
                    cwd_dir: state_dir.join("by-cwd"),
                    state_dir,
                    per_cwd,
                    cwd,
                },
                root,
            }
        }

        fn pending(&self) -> PathBuf {
            self.state
                .state_dir
                .join("daemon")
                .join(JOURNAL_DIR)
                .join(JOURNAL_PENDING)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn rebind(plan: &mut ClearPlan) {
        let nonce = plan.mutation_token.split_once(':').unwrap().0.to_owned();
        plan.mutation_token = format!("{nonce}:{}", plan_binding_digest(plan, &nonce));
    }

    #[test]
    fn uncommitted_clear_plan_is_discarded_without_removal() {
        let scratch = Scratch::new("uncommitted", true);
        save_chat(&scratch.state, "uncommitted-clear-secret").unwrap();
        let journal = lock_journal(&scratch.state).unwrap();
        let plan = prepare_plan(&scratch.state, false).unwrap();
        write_plan(&journal, &plan).unwrap();
        drop(journal);

        let journal = lock_journal(&scratch.state).unwrap();
        super::recover_pending(
            &scratch.state,
            &journal,
            &fs::read(scratch.pending()).unwrap(),
        )
        .unwrap();
        assert!(scratch.state.active_chat_file().exists());
        assert!(!scratch.pending().exists());
    }

    #[test]
    fn committed_scope_clear_recovers_once_and_preserves_global_history() {
        let scratch = Scratch::new("scope-recover", true);
        save_chat(&scratch.state, "scope-clear-secret").unwrap();
        let history = fs::read(&scratch.state.history_file).unwrap();
        let journal = lock_journal(&scratch.state).unwrap();
        let plan = prepare_plan(&scratch.state, false).unwrap();
        write_plan(&journal, &plan).unwrap();
        let store = open_metadata_store(&scratch.state).unwrap();
        let scopes = plan.scopes().unwrap().unwrap();
        store
            .clear_conversation_identity(&plan.edition_slug, Some(&scopes), &plan.mutation_token)
            .unwrap();
        drop(journal);

        read_chat(&scratch.state).unwrap();
        read_chat(&scratch.state).unwrap();
        assert!(!scratch.state.active_chat_file().exists());
        assert_eq!(
            fs::read_to_string(&scratch.state.chat_file).unwrap(),
            "scope-clear-secret\n"
        );
        assert_eq!(fs::read(&scratch.state.history_file).unwrap(), history);
        assert!(!scratch.pending().exists());
    }

    #[test]
    fn clear_all_recovers_partial_removal_and_preserves_retained_data() {
        let scratch = Scratch::new("all-partial", true);
        save_chat(&scratch.state, "all-clear-secret").unwrap();
        let other = scratch.state.cwd_dir.join("other-project");
        fs::write(&other, b"other-secret\n").unwrap();
        let history = fs::read(&scratch.state.history_file).unwrap();
        let journal = lock_journal(&scratch.state).unwrap();
        let plan = prepare_plan(&scratch.state, true).unwrap();
        write_plan(&journal, &plan).unwrap();
        let store = open_metadata_store(&scratch.state).unwrap();
        let commit = store
            .clear_conversation_identity(&plan.edition_slug, None, &plan.mutation_token)
            .unwrap();
        ensure_commit_effect(&store, &plan, &commit).unwrap();
        let first = path_from_hex(&plan.targets[0].path_hex).unwrap();
        if first.exists() {
            remove_mirror(&first).unwrap();
        }
        drop(journal);

        read_chat(&scratch.state).unwrap();
        assert!(!scratch.state.chat_file.exists());
        assert!(!scratch.state.chat_file.with_extension("export").exists());
        assert!(
            fs::read_dir(&scratch.state.cwd_dir)
                .unwrap()
                .next()
                .is_none()
        );
        assert_eq!(fs::read(&scratch.state.history_file).unwrap(), history);
        assert_eq!(store.current_identity_commit().unwrap(), Some(commit));
    }

    #[test]
    fn tombstones_refuse_stale_mirrors_and_save_supersedes_them() {
        let scratch = Scratch::new("stale-read", true);
        save_chat(&scratch.state, "stale-clear-secret").unwrap();
        let store = open_metadata_store(&scratch.state).unwrap();
        let cwd = ConversationIdentityScope::working_directory(&scratch.state.cwd);
        store
            .clear_conversation_identity(
                crate::edition::ACTIVE.slug(),
                Some(std::slice::from_ref(&cwd)),
                "direct-scope-clear",
            )
            .unwrap();
        assert_eq!(
            read_chat(&scratch.state).unwrap().as_deref(),
            Some("stale-clear-secret"),
            "tombstoned cwd must fall back to the still-current global scope"
        );
        store
            .clear_conversation_identity(crate::edition::ACTIVE.slug(), None, "direct-all-clear")
            .unwrap();
        assert_eq!(read_chat(&scratch.state).unwrap(), None);
        save_chat(&scratch.state, "resaved-secret").unwrap();
        assert_eq!(
            read_chat(&scratch.state).unwrap().as_deref(),
            Some("resaved-secret")
        );
    }

    #[test]
    fn concurrent_clear_and_save_serialize_without_resurrection() {
        let scratch = Scratch::new("clear-save-race", true);
        save_chat(&scratch.state, "race-old-secret").unwrap();
        let state = Arc::new(scratch.state.clone());
        let barrier = Arc::new(Barrier::new(3));
        let clear_state = Arc::clone(&state);
        let clear_barrier = Arc::clone(&barrier);
        let clear = std::thread::spawn(move || {
            clear_barrier.wait();
            clear_chat(&clear_state, false)
        });
        let save_state = Arc::clone(&state);
        let save_barrier = Arc::clone(&barrier);
        let save = std::thread::spawn(move || {
            save_barrier.wait();
            save_chat(&save_state, "race-new-secret")
        });
        barrier.wait();
        clear.join().unwrap().unwrap();
        save.join().unwrap().unwrap();
        assert_eq!(
            read_chat(&state).unwrap().as_deref(),
            Some("race-new-secret")
        );
        assert!(!scratch.pending().exists());
    }

    #[test]
    fn malformed_roles_and_omitted_inventory_fail_closed() {
        let scratch = Scratch::new("tampered-plan", true);
        save_chat(&scratch.state, "tampered-clear-secret").unwrap();
        fs::write(scratch.state.cwd_dir.join("other"), b"other\n").unwrap();

        let mut role = prepare_plan(&scratch.state, true).unwrap();
        role.targets[0].role = ClearTargetRole::Active;
        rebind(&mut role);
        assert!(validate_plan(&scratch.state, &role).is_err());

        let mut omitted = prepare_plan(&scratch.state, true).unwrap();
        let cwd = omitted
            .targets
            .iter()
            .position(|target| target.role == ClearTargetRole::Cwd)
            .unwrap();
        omitted.targets.remove(cwd);
        rebind(&mut omitted);
        let error = validate_plan(&scratch.state, &omitted).unwrap_err();
        assert!(error.to_string().contains("omits"));
    }

    #[test]
    fn lossy_cwd_key_collision_never_deletes_another_scope_mirror() {
        for (first_id, second_id) in [
            ("collision-first", "collision-second"),
            ("collision-shared", "collision-shared"),
        ] {
            let scratch = Scratch::new("cwd-key-collision", true);
            let nested = scratch.root.join("a/b");
            let underscored = scratch.root.join("a_b");
            fs::create_dir_all(&nested).unwrap();
            fs::create_dir(&underscored).unwrap();
            assert_eq!(
                AbbeyState::cwd_key(&nested),
                AbbeyState::cwd_key(&underscored)
            );

            let mut first = scratch.state.clone();
            first.cwd = nested;
            save_chat(&first, first_id).unwrap();
            let mut second = scratch.state.clone();
            second.cwd = underscored;
            save_chat(&second, second_id).unwrap();
            let shared_path = second.active_chat_file();
            let before = fs::read(&shared_path).unwrap();
            let before_commit = open_metadata_store(&second)
                .unwrap()
                .current_identity_commit()
                .unwrap();

            assert!(clear_chat(&first, false).is_err());
            assert_eq!(fs::read(&shared_path).unwrap(), before);
            assert_eq!(
                open_metadata_store(&second)
                    .unwrap()
                    .current_identity_commit()
                    .unwrap(),
                before_commit
            );
            assert_eq!(read_chat(&second).unwrap().as_deref(), Some(second_id));
        }
    }

    #[test]
    fn legacy_untracked_clear_cuts_forward_and_global_clear_preserves_cwd() {
        let empty = Scratch::new("empty-cutover", true);
        clear_chat(&empty.state, false).unwrap();
        assert!(!empty.state.active_chat_file().exists());
        let empty_scope = ConversationIdentityScope::working_directory(&empty.state.cwd);
        assert_eq!(
            open_metadata_store(&empty.state)
                .unwrap()
                .identity_scope_state(crate::edition::ACTIVE.slug(), &empty_scope, None)
                .unwrap(),
            IdentityScopeState::Tombstoned
        );

        let legacy = Scratch::new("legacy-cutover", true);
        fs::write(legacy.state.active_chat_file(), b"legacy-secret\n").unwrap();
        fs::write(&legacy.state.chat_file, b"legacy-secret\n").unwrap();
        clear_chat(&legacy.state, false).unwrap();
        assert!(!legacy.state.active_chat_file().exists());
        assert_eq!(
            fs::read_to_string(&legacy.state.chat_file).unwrap(),
            "legacy-secret\n"
        );
        let cwd = ConversationIdentityScope::working_directory(&legacy.state.cwd);
        assert_eq!(
            open_metadata_store(&legacy.state)
                .unwrap()
                .identity_scope_state(crate::edition::ACTIVE.slug(), &cwd, Some("legacy-secret"))
                .unwrap(),
            IdentityScopeState::Tombstoned
        );

        let scratch = Scratch::new("global-preserves-cwd", true);
        save_chat(&scratch.state, "global-clear-secret").unwrap();
        let cwd_file = scratch.state.active_chat_file();
        let mut global = scratch.state.clone();
        global.per_cwd = false;
        clear_chat(&global, false).unwrap();
        assert!(!global.chat_file.exists());
        assert!(!global.chat_file.with_extension("export").exists());
        assert_eq!(
            fs::read_to_string(&cwd_file).unwrap(),
            "global-clear-secret\n"
        );
        assert_eq!(
            read_chat(&scratch.state).unwrap().as_deref(),
            Some("global-clear-secret")
        );
    }

    #[test]
    fn clear_journal_debug_and_database_exclude_raw_path_canaries() {
        let scratch = Scratch::new("clear-redaction", true);
        save_chat(&scratch.state, "clear-redaction-secret").unwrap();
        let plan = prepare_plan(&scratch.state, true).unwrap();
        let debug = format!("{plan:?}");
        assert!(!debug.contains("clear-redaction-secret"));
        assert!(!debug.contains("private-cwd"));
        clear_chat(&scratch.state, true).unwrap();
        for entry in fs::read_dir(scratch.state.state_dir.join("daemon")).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                let bytes = fs::read(path).unwrap();
                for canary in [b"clear-redaction-secret".as_slice(), b"private-cwd"] {
                    assert!(!bytes.windows(canary.len()).any(|window| window == canary));
                }
            }
        }
    }

    #[test]
    fn deleted_clear_effect_rows_never_resurrect_recreated_stale_mirrors() {
        let scope = Scratch::new("deleted-scope-effect", true);
        save_chat(&scope.state, "deleted-scope-secret").unwrap();
        clear_chat(&scope.state, false).unwrap();
        fs::write(scope.state.active_chat_file(), b"deleted-scope-secret\n").unwrap();
        let database = RuntimeStore::path_for_state_dir(&scope.state.state_dir.join("daemon"));
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute("DELETE FROM conversation_identity_tombstones", [])
            .unwrap();
        assert!(read_chat(&scope.state).is_err());

        let all = Scratch::new("deleted-all-effect", true);
        save_chat(&all.state, "deleted-all-secret").unwrap();
        clear_chat(&all.state, true).unwrap();
        fs::write(&all.state.chat_file, b"deleted-all-secret\n").unwrap();
        let database = RuntimeStore::path_for_state_dir(&all.state.state_dir.join("daemon"));
        rusqlite::Connection::open(&database)
            .unwrap()
            .execute("DELETE FROM conversation_identity_clear_all", [])
            .unwrap();
        assert!(read_chat(&all.state).is_err());

        let forged = Scratch::new("forged-future-selection", true);
        save_chat(&forged.state, "forged-future-secret").unwrap();
        clear_chat(&forged.state, true).unwrap();
        fs::write(&forged.state.chat_file, b"forged-future-secret\n").unwrap();
        let database = RuntimeStore::path_for_state_dir(&forged.state.state_dir.join("daemon"));
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute(
                "INSERT INTO conversation_identity_scopes(
                    edition_sha256, scope_sha256, alias_sha256, conversation_id,
                    revision, updated_at
                 )
                 SELECT m.edition_sha256, s.scope_sha256, m.alias_sha256,
                        m.conversation_id, 999, '2026-08-09T00:00:00.000Z'
                 FROM conversation_identity_mutations m
                 JOIN conversation_identity_mutation_scopes s
                   ON s.mutation_sha256=m.mutation_sha256
                 WHERE m.operation='save' AND s.scope_sha256=?1",
                [ConversationIdentityScope::global().as_sha256()],
            )
            .unwrap();
        drop(connection);
        assert!(read_chat(&forged.state).is_err());
    }
}
