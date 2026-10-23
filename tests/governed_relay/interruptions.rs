//! Parent-driven hard interruption of real durable stores at committed boundaries.
use super::*;
use abi_worker::{ControlEvent, JobControlStore, WorkerError};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone)]
struct Checkpoint {
    root: PathBuf,
    boundary: String,
}
impl Checkpoint {
    fn hit(&self, boundary: &str) {
        if self.boundary != boundary {
            return;
        }
        let mut marker = std::fs::File::create(self.root.join("checkpoint.tmp")).unwrap();
        marker.write_all(boundary.as_bytes()).unwrap();
        marker.sync_all().unwrap();
        std::fs::rename(
            self.root.join("checkpoint.tmp"),
            self.root.join("checkpoint"),
        )
        .unwrap();
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
}
struct HookStore {
    inner: FileControlStore,
    checkpoint: Checkpoint,
}
impl JobControlStore for HookStore {
    fn load(&self) -> Result<Vec<ControlEvent>, WorkerError> {
        self.inner.load()
    }
    fn append(&mut self, event: &ControlEvent) -> Result<(), WorkerError> {
        self.inner.append(event)?;
        match event {
            ControlEvent::Admitted { .. } => self.checkpoint.hit("admission"),
            ControlEvent::Completed { .. } => self.checkpoint.hit("completion"),
            _ => {}
        }
        Ok(())
    }
}
struct HookAuthority {
    inner: AbbeyLocalApprovalAuthority,
    checkpoint: Checkpoint,
}
impl DurableLifecycle for HookAuthority {
    fn refuse_replay(&mut self, id: Digest) -> Result<bool, LifecycleError> {
        self.inner.refuse_replay(id)
    }
    fn append_stage(&mut self, audit: &StageAudit) -> Result<(), LifecycleError> {
        self.inner.append_stage(audit)
    }
    fn prepare(&mut self, request: &PrepareRequest) -> Result<PrepareOutcome, LifecycleError> {
        let value = self.inner.prepare(request)?;
        if matches!(value, PrepareOutcome::Prepared(_)) {
            self.checkpoint.hit("prepare");
        }
        Ok(value)
    }
    fn finish(
        &mut self,
        prepared: &PreparedExecution,
        outcome: &TerminalOutcome,
    ) -> Result<(), LifecycleError> {
        self.inner.finish(prepared, outcome)?;
        self.checkpoint.hit("terminal");
        Ok(())
    }
    fn lookup(&mut self, id: Digest) -> Result<Option<TerminalOutcome>, LifecycleError> {
        self.inner.lookup(id)
    }
}
impl LocalApprovalStore for HookAuthority {
    fn read_local_approval(
        &self,
        call: &str,
    ) -> Result<Option<LocalApprovalPreview>, LifecycleError> {
        assert_ne!(self.checkpoint.boundary, "no-approval");
        self.inner.read_local_approval(call)
    }
    fn request_local_approval(
        &self,
        preview: &LocalApprovalPreview,
    ) -> Result<LocalApprovalStatus, LifecycleError> {
        assert_ne!(self.checkpoint.boundary, "no-approval");
        let value = self.inner.request_local_approval(preview)?;
        let mut saved = std::fs::File::create(self.checkpoint.root.join("preview.json")).unwrap();
        saved
            .write_all(&serde_json::to_vec(preview).unwrap())
            .unwrap();
        saved.sync_all().unwrap();
        self.checkpoint.hit("preview");
        Ok(value)
    }
    fn decide_local_approval(
        &self,
        call: &str,
        digest: Digest,
        decision: &str,
        approve: bool,
        now: u64,
    ) -> Result<LocalApprovalStatus, LifecycleError> {
        let value = self
            .inner
            .decide_local_approval(call, digest, decision, approve, now)?;
        self.checkpoint.hit("approval");
        Ok(value)
    }
    fn exact_local_approval(
        &self,
        call: &str,
        digest: Digest,
        now: u64,
    ) -> Result<Option<ExactApproval>, LifecycleError> {
        assert_ne!(self.checkpoint.boundary, "no-approval");
        self.inner.exact_local_approval(call, digest, now)
    }
}
struct CountedEffect {
    effect: FileEffect,
    checkpoint: Checkpoint,
}
impl GovernedLiveExecutor for CountedEffect {
    fn binding(&self) -> AdapterBinding {
        self.effect.binding()
    }
    fn execute(
        &mut self,
        call: &ToolCall,
        request: &Request,
        source: &CompiledSource,
        prepared: &PreparedExecution,
        context: ToolExecutionContext<'_>,
    ) -> Result<ToolOutput, ToolExecutionError> {
        let mut count = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.checkpoint.root.join("invocations"))
            .unwrap();
        count.write_all(b"invoked\n").unwrap();
        count.sync_all().unwrap();
        let output = self
            .effect
            .execute(call, request, source, prepared, context)?;
        self.checkpoint.hit("effect");
        Ok(output)
    }
}
fn hooked_runtime(
    f: &Fixture,
    root: &Path,
    checkpoint: Checkpoint,
) -> cp::DurableRelayRuntime<HookStore> {
    let inner =
        FileControlStore::create(&root.join("worker.log"), FileControlStoreLimits::default())
            .unwrap();
    let runtime = cp::RelayRuntime::new_tools(
        f.cert.clone(),
        f.node.clone(),
        cp::RelayKey::from_seed(&[43; 32]).unwrap(),
        f.authority.verifying_key().to_bytes(),
        "tools-v1",
        HookStore { inner, checkpoint },
        2,
    )
    .unwrap();
    let outbox =
        cp::FileRelayOutbox::create(&root.join("outbox.sqlite"), cp::OutboxLimits::default())
            .unwrap();
    cp::DurableRelayRuntime::new(runtime, outbox).unwrap()
}
#[test]
fn hard_kill_child() {
    let Some(root) = std::env::var_os("ABBEY_HARD_KILL_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let checkpoint = Checkpoint {
        root: root.clone(),
        boundary: std::env::var("ABBEY_HARD_KILL_BOUNDARY").unwrap(),
    };
    let f = Fixture::new();
    let mut fixture = std::fs::File::create(root.join("fixture.json")).unwrap();
    fixture
        .write_all(&serde_json::to_vec(&(&f.dispatch, &f.policy)).unwrap())
        .unwrap();
    fixture.sync_all().unwrap();
    let mut runtime = hooked_runtime(&f, &root, checkpoint.clone());
    let mut authority = HookAuthority {
        inner: f.approvals(&root.join("abbey.sqlite")),
        checkpoint: checkpoint.clone(),
    };
    let mut effect = CountedEffect {
        effect: FileEffect(root.join("effect.txt")),
        checkpoint: checkpoint.clone(),
    };
    let refreshes = AtomicUsize::new(0);
    let pending = match f
        .execute(&mut runtime, &mut authority, &mut effect, &refreshes)
        .unwrap()
    {
        cp::GovernedToolOutcome::Pending(p) => p,
        value => panic!("{value:?}"),
    };
    authority
        .decide_local_approval(
            &pending.call_id,
            pending.preview_digest,
            "hard-kill-decision",
            true,
            2000,
        )
        .unwrap();
    assert!(matches!(
        f.execute(&mut runtime, &mut authority, &mut effect, &refreshes)
            .unwrap(),
        cp::GovernedToolOutcome::Recorded(_)
    ));
    // execute_governed has committed the entire outbox before any transport exists.
    checkpoint.hit("outbox");
    panic!("boundary not reached");
}
struct ReapedChild(Child);
impl Drop for ReapedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn interrupt(root: &Path, boundary: &str) {
    let mut child = ReapedChild(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "interruptions::hard_kill_child", "--nocapture"])
            .env("ABBEY_HARD_KILL_ROOT", root)
            .env("ABBEY_HARD_KILL_BOUNDARY", boundary)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !root.join("checkpoint").exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "child exited before {boundary}"
        );
        assert!(Instant::now() < deadline, "timed out at {boundary}");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("checkpoint")).unwrap(),
        boundary
    );
    child.0.kill().unwrap();
    let status = child.0.wait().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(9));
    }
}
fn snapshot(path: &Path) -> Option<(Vec<u8>, SystemTime)> {
    path.exists().then(|| {
        (
            std::fs::read(path).unwrap(),
            std::fs::metadata(path).unwrap().modified().unwrap(),
        )
    })
}
fn fixture(root: &Path) -> Fixture {
    let (dispatch, policy): (cp::Dispatch, cp::SignedDeliveryPolicy) =
        serde_json::from_slice(&std::fs::read(root.join("fixture.json")).unwrap()).unwrap();
    let b = &dispatch.submission.binding;
    let mut f = Fixture::from_ids(
        b.tenant_id.clone(),
        b.account_id.clone(),
        b.node_id.clone(),
        b.job_id.clone(),
    );
    f.dispatch = dispatch;
    f.policy = policy;
    f
}
fn artifacts(runtime: &cp::DurableRelayRuntime<FileControlStore>, f: &Fixture) -> Vec<u8> {
    let entry = runtime
        .outbox
        .entry(&f.dispatch.submission.binding.job_id)
        .unwrap()
        .unwrap();
    serde_json::to_vec(&(entry.packets, entry.manifest)).unwrap()
}
struct ObservedFacts {
    inner: Facts,
    observes: usize,
    postconditions: usize,
    refuse: bool,
}
impl GovernedFactProvider for ObservedFacts {
    fn observe(&mut self, request: &Request) -> Result<GovernedFacts, LifecycleError> {
        assert!(!self.refuse, "cached replay consulted fresh facts");
        self.observes += 1;
        self.inner.observe(request)
    }
    fn postconditions(
        &mut self,
        request: &Request,
        required: &BTreeSet<String>,
    ) -> Result<Vec<PostconditionObservation>, LifecycleError> {
        assert!(!self.refuse, "cached replay consulted fresh postconditions");
        self.postconditions += 1;
        self.inner.postconditions(request, required)
    }
}
fn assert_terminal_state(root: &Path, boundary: &str) {
    if matches!(
        boundary,
        "prepare" | "effect" | "terminal" | "completion" | "outbox"
    ) {
        let connection = rusqlite::Connection::open(root.join("abbey.sqlite")).unwrap();
        let state: String = connection
            .query_row("SELECT state FROM governed_tool_intents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            state,
            if matches!(boundary, "prepare" | "effect") {
                "interrupted"
            } else {
                "succeeded"
            }
        );
        let approval: String = connection
            .query_row("SELECT state FROM tool_approvals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(approval, "consumed");
    }
}
#[allow(clippy::too_many_arguments)]
fn continue_approved(
    root: &Path,
    boundary: &str,
    f: &Fixture,
    runtime: &mut cp::DurableRelayRuntime<FileControlStore>,
    authority: &mut HookAuthority,
    executor: &mut CountedEffect,
    refreshes: &AtomicUsize,
    facts: &mut ObservedFacts,
) {
    let connection = rusqlite::Connection::open(root.join("abbey.sqlite")).unwrap();
    let call_id: String = connection
        .query_row("SELECT call_id FROM local_governed_approvals", [], |row| {
            row.get(0)
        })
        .unwrap();
    drop(connection);
    let preview = authority.read_local_approval(&call_id).unwrap().unwrap();
    let saved: LocalApprovalPreview =
        serde_json::from_slice(&std::fs::read(root.join("preview.json")).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_vec(&preview).unwrap(),
        serde_json::to_vec(&saved).unwrap()
    );
    assert_eq!(preview.digest(), saved.digest());
    assert_eq!(preview.expires_at_ms, saved.expires_at_ms);
    assert_eq!(preview.parameters, json!({"value":"private-file-canary"}));
    if boundary == "preview" {
        assert!(matches!(
            f.execute_with_facts(runtime, authority, executor, refreshes, 2000, facts)
                .unwrap(),
            cp::GovernedToolOutcome::Pending(_)
        ));
        assert_eq!(snapshot(&executor.effect.0), None);
        authority
            .decide_local_approval(
                &call_id,
                preview.digest(),
                "parent-explicit-decision",
                true,
                2000,
            )
            .unwrap();
    }
    let before = (
        facts.observes,
        facts.postconditions,
        refreshes.load(Ordering::SeqCst),
    );
    assert!(matches!(
        f.execute_with_facts(runtime, authority, executor, refreshes, 2000, facts)
            .unwrap(),
        cp::GovernedToolOutcome::Recorded(_)
    ));
    assert!(facts.observes > before.0);
    assert!(facts.postconditions > before.1);
    assert!(refreshes.load(Ordering::SeqCst) > before.2);
}
fn recover(boundary: &str) {
    let directory = Directory::new();
    interrupt(&directory.0, boundary);
    let f = fixture(&directory.0);
    let admitted = !matches!(boundary, "preview" | "approval");
    let completed = matches!(boundary, "completion" | "outbox");
    let effect_happened = matches!(boundary, "effect" | "terminal" | "completion" | "outbox");
    let mut expected_effect = snapshot(&directory.0.join("effect.txt"));
    let mut expected_count = snapshot(&directory.0.join("invocations"));
    assert_eq!(expected_effect.is_some(), effect_happened);
    assert_eq!(expected_count.is_some(), effect_happened);
    let mut persisted_artifacts = None;
    for replay in 0..3 {
        let mut runtime = f.runtime(&directory.0, true);
        let before = artifacts(&runtime, &f);
        if (boundary == "outbox" || replay > 0)
            && let Some(expected) = &persisted_artifacts
        {
            assert_eq!(&before, expected);
        }
        runtime.reconcile_local(2).unwrap();
        let mut authority = HookAuthority {
            inner: f.approvals(&directory.0.join("abbey.sqlite")),
            checkpoint: Checkpoint {
                root: directory.0.clone(),
                boundary: if completed || replay > 0 {
                    "no-approval".into()
                } else {
                    String::new()
                },
            },
        };
        let mut executor = CountedEffect {
            effect: FileEffect(directory.0.join("effect.txt")),
            checkpoint: Checkpoint {
                root: directory.0.clone(),
                boundary: String::new(),
            },
        };
        let refreshes = AtomicUsize::new(0);
        let mut facts = ObservedFacts {
            inner: Facts(f.facts.clone(), Duration::ZERO),
            observes: 0,
            postconditions: 0,
            refuse: completed || replay > 0,
        };
        if !admitted && replay == 0 {
            continue_approved(
                &directory.0,
                boundary,
                &f,
                &mut runtime,
                &mut authority,
                &mut executor,
                &refreshes,
                &mut facts,
            );
            expected_effect = snapshot(&executor.effect.0);
            expected_count = snapshot(&directory.0.join("invocations"));
        }
        facts.refuse = true;
        let counts_before = (facts.observes, facts.postconditions);
        let refresh_before = refreshes.load(Ordering::SeqCst);
        let result = f
            .execute_with_facts(
                &mut runtime,
                &mut authority,
                &mut executor,
                &refreshes,
                2000,
                &mut facts,
            )
            .unwrap();
        assert_eq!(refreshes.load(Ordering::SeqCst), refresh_before);
        assert_eq!((facts.observes, facts.postconditions), counts_before);
        if completed || !admitted {
            assert!(matches!(result, cp::GovernedToolOutcome::Recorded(_)));
        } else {
            assert!(matches!(result, cp::GovernedToolOutcome::Unresolved));
        }
        assert_eq!(snapshot(&executor.effect.0), expected_effect);
        assert_eq!(snapshot(&directory.0.join("invocations")), expected_count);
        if let Some((bytes, _)) = &expected_count {
            assert_eq!(bytes, b"invoked\n");
        }
        let after = artifacts(&runtime, &f);
        if boundary == "outbox" && replay == 0 {
            assert_eq!(after, before);
        }
        if let Some(expected) = &persisted_artifacts {
            assert_eq!(&after, expected);
        }
        persisted_artifacts = Some(after);
        assert_terminal_state(&directory.0, boundary);
    }
}
#[test]
fn kill_after_preview_persistence() {
    recover("preview");
}
#[test]
fn kill_after_approval_persistence() {
    recover("approval");
}
#[test]
fn kill_after_worker_admission() {
    recover("admission");
}
#[test]
fn kill_after_lifecycle_prepare() {
    recover("prepare");
}
#[test]
fn kill_after_synced_effect() {
    recover("effect");
}
#[test]
fn kill_after_lifecycle_terminal() {
    recover("terminal");
}
#[test]
fn kill_after_worker_completion() {
    recover("completion");
}
#[test]
fn kill_after_complete_outbox_before_transport() {
    recover("outbox");
}
