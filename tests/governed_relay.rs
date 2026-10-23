//! Actual encrypted relay -> Abbey approval -> governed file adapter integration.
use abbey::runtime::{AbbeyLocalApprovalAuthority, RuntimeStore};
use abi_agent_host::{local_approval::*, *};
use abi_agent_runtime::{CancellationToken, ToolCall};
use abi_capability::{lifecycle::*, source::*, *};
use abi_control_plane as cp;
use abi_worker::{FileControlStore, FileControlStoreLimits};
use ed25519_dalek::{Signer, SigningKey};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
fn sign<T: Serialize>(key: &SigningKey, value: &T) -> Vec<u8> {
    key.sign(&serde_json::to_vec(value).unwrap())
        .to_bytes()
        .to_vec()
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("abbey-governed-relay-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    authority: SigningKey,
    node: SigningKey,
    cert: cp::NodeCertificate,
    dispatch: cp::Dispatch,
    policy: cp::SignedDeliveryPolicy,
    sources: SourceRegistry,
    registration: GovernedRegistration,
    facts: GovernedFacts,
}
impl Fixture {
    fn new() -> Self {
        Self::from_ids(
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
        )
    }
    fn from_ids(tenant: String, account: String, node_id: String, job_id: String) -> Self {
        // This fixture proves authorization and crash recovery, not a disk-I/O
        // latency target. Keep a finite budget without racing loaded CI hosts.
        Self::with_deadline(tenant, account, node_id, job_id, 10_000)
    }
    fn with_deadline(
        tenant: String,
        account: String,
        node_id: String,
        job_id: String,
        deadline_ms: u64,
    ) -> Self {
        let authority = SigningKey::from_bytes(&[41; 32]);
        let node = SigningKey::from_bytes(&[42; 32]);
        let encryption = cp::RelayKey::from_seed(&[43; 32]).unwrap();
        let reply = cp::RelayKey::from_seed(&[44; 32]).unwrap();
        let mut cert = cp::NodeCertificate {
            protocol: "abi.node-certificate.v1".into(),
            audience: "http://localhost".into(),
            node_id: node_id.clone(),
            claim: cp::PairClaim {
                protocol: "abi.node-pair.v1".into(),
                enrollment_id: uuid::Uuid::new_v4().to_string(),
                account_id: account.clone(),
                tenant_id: tenant.clone(),
                public_key: node.verifying_key().to_bytes(),
                encryption_public_key: Some(encryption.public_key()),
                capabilities: vec!["tool.execute".into()],
                expires_at: 300,
            },
            expires_at: 300,
            signature: vec![],
        };
        cert.signature = authority
            .sign(&cert.signing_bytes().unwrap())
            .to_bytes()
            .to_vec();
        let binding = cp::JobBinding {
            protocol: "abi.relay-job.v1".into(),
            audience: "http://localhost".into(),
            job_id,
            account_id: account.clone(),
            tenant_id: tenant.clone(),
            node_id,
            capability: "tool.execute".into(),
            expires_at: 300,
            submitted_at: 1,
            max_result_bytes: 16384,
            reply_key: reply.public_key(),
        };
        let job = cp::ToolJob {
            protocol: "abi.governed-tool.v1".into(),
            tool: "write_record".into(),
            revision: "tools-v1".into(),
            parameters: json!({"value":"private-file-canary"}),
        };
        let submission = cp::Submission {
            payload: cp::seal(
                &encryption.public_key(),
                &binding.aad("request").unwrap(),
                &serde_json::to_vec(&job).unwrap(),
            )
            .unwrap(),
            binding,
        };
        let dispatch = cp::Dispatch {
            signature: sign(&authority, &("abi.relay-dispatch.v1", &submission)),
            submission,
        };
        let delivery = cp::DeliveryPolicy::for_submission(&dispatch.submission, 86401).unwrap();
        let policy = cp::SignedDeliveryPolicy {
            signature: sign(&authority, &("abi.relay-delivery-policy.v1", &delivery)),
            policy: delivery,
        };
        let source:PackageSource=serde_json::from_value(json!({
 "id":"local.write.record","version":"1.0.0","input_schema":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false},"output_schema":{"type":"boolean"},"error_schema":{"type":"null"},"streaming":"None","cancellation":"BeforeEffect","required_platform_permissions":[],
 "resource_scope":{"organization_id":"org","deployment_id":"deploy","tenant_id":tenant,"platform":"local","guild_ref":null,"resource_digest":Digest::from_bytes([1;32]),"subject_digest":Digest::from_bytes([2;32])},
 "side_effect":"PlatformWrite","reversibility":"ReversibleWithLoss","risk_class":"Low","data_classes":["Operational"],"residency":"LocalOnly","retention":"Session","preconditions":["fresh"],"invariants":["scoped"],"deadline_ms":deadline_ms,"rate_class":"bounded","budgets":{"max_input_bytes":512,"max_output_bytes":512,"max_events":4,"max_calls":1},"confirmation_policy":"A1Actor","delegation_policy":"Forbidden","expiry_policy_ms":10000,"revocation_policy":"RecheckBeforeEffect","approval_policy":"A1Actor","idempotency":"RequiresKey","compensation":null,"rollback":"None","postconditions":["written"],"adapter_bindings":[{"adapter_id":"local-file-fixture","api_version":"1"}],"receipt_requirements":{"redacted":true,"require_postcondition_evidence":true},"fixtures":{"deterministic":Digest::from_bytes([3;32]),"adversarial":Digest::from_bytes([4;32]),"failure":Digest::from_bytes([5;32])},"promotion_thresholds":{"minimum_passed":1,"maximum_failed":0},"expiration_criteria":{"expires_at_ms":10000,"disable_on_schema_drift":true,"disable_on_missing_evidence":true}
 })).unwrap();
        let sources = SourceRegistry::compile([source]).unwrap();
        let src = sources.get("local.write.record", "1.0.0").unwrap();
        let principal = Principal::new(account, PrincipalKind::HumanSubject).unwrap();
        let request = Request::new(
            "attempt",
            "call",
            principal.clone(),
            src.source().resource_scope.clone(),
            "local.write.record",
            "1.0.0",
            src.package().digest,
            Digest::default(),
            "p",
            "g",
            "s",
            0,
        )
        .unwrap();
        let grant = Grant::active(
            "grant",
            Principal::new("owner", PrincipalKind::OrganizationOwner).unwrap(),
            principal,
            request.scope.clone(),
            src.package(),
            0,
            10000,
            ApprovalLevel::A1Actor,
            0,
            "p",
            "g",
            "s",
        )
        .unwrap();
        let facts = GovernedFacts {
            platform: PlatformFacts::new(BTreeSet::new(), 0, "p", "g", "s"),
            grants: vec![grant],
            used_decision_ids: BTreeSet::new(),
            predicates: BTreeSet::from(["fresh".into(), "scoped".into()]),
            captured_pre_state: None,
            observed_at_ms: 1000,
            expires_at_ms: 10000,
            permitted_data_classes: BTreeSet::from([DataClass::Operational]),
            residency: ResidencyPolicy::LocalOnly,
            retention: RetentionClass::Session,
            safety_allows: true,
            rate_allows: true,
        };
        Self {
            authority,
            node,
            cert,
            dispatch,
            policy,
            sources,
            registration: GovernedRegistration {
                tool_name: "write_record".into(),
                request,
            },
            facts,
        }
    }
    fn permit(&self) -> cp::DispatchPermit {
        let mut p = cp::DispatchPermit {
            binding: self.dispatch.submission.binding.clone(),
            submission_digest: self.policy.policy.submission_digest.clone(),
            issued_at: 2,
            expires_at: 7,
            signature: vec![],
        };
        p.signature = sign(
            &self.authority,
            &(
                "abi.relay-permit.v1",
                &p.binding,
                &p.submission_digest,
                p.issued_at,
                p.expires_at,
            ),
        );
        p
    }
    fn approvals(&self, path: &Path) -> AbbeyLocalApprovalAuthority {
        let b = &self.dispatch.submission.binding;
        AbbeyLocalApprovalAuthority::new(
            RuntimeStore::open(path).unwrap(),
            b.tenant_id.clone(),
            b.account_id.clone(),
            b.node_id.clone(),
            "local-user".into(),
            self.registration.request.principal.clone(),
        )
        .unwrap()
    }
    fn runtime(&self, dir: &Path, reopen: bool) -> cp::DurableRelayRuntime<FileControlStore> {
        let store = if reopen {
            FileControlStore::open(&dir.join("worker.log"), FileControlStoreLimits::default())
        } else {
            FileControlStore::create(&dir.join("worker.log"), FileControlStoreLimits::default())
        }
        .unwrap();
        let runtime = cp::RelayRuntime::new_tools(
            self.cert.clone(),
            self.node.clone(),
            cp::RelayKey::from_seed(&[43; 32]).unwrap(),
            self.authority.verifying_key().to_bytes(),
            "tools-v1",
            store,
            2,
        )
        .unwrap();
        assert!(
            runtime.recorded_dispatch(&self.dispatch).is_ok(),
            "dispatch envelope refused"
        );
        let outbox = if reopen {
            cp::FileRelayOutbox::open(&dir.join("outbox.sqlite"), cp::OutboxLimits::default())
        } else {
            cp::FileRelayOutbox::create(&dir.join("outbox.sqlite"), cp::OutboxLimits::default())
        }
        .unwrap();
        cp::DurableRelayRuntime::new(runtime, outbox).unwrap()
    }
    fn execute<S: abi_worker::JobControlStore>(
        &self,
        runtime: &mut cp::DurableRelayRuntime<S>,
        approvals: &mut dyn LocalApprovalStore,
        executor: &mut dyn GovernedLiveExecutor,
        refreshes: &AtomicUsize,
    ) -> cp::Result<cp::GovernedToolOutcome> {
        self.execute_at(runtime, approvals, executor, refreshes, 2000)
    }
    fn execute_at<S: abi_worker::JobControlStore>(
        &self,
        runtime: &mut cp::DurableRelayRuntime<S>,
        approvals: &mut dyn LocalApprovalStore,
        executor: &mut dyn GovernedLiveExecutor,
        refreshes: &AtomicUsize,
        now_ms: u64,
    ) -> cp::Result<cp::GovernedToolOutcome> {
        self.execute_with_observation_delay(
            runtime,
            approvals,
            executor,
            refreshes,
            now_ms,
            std::time::Duration::ZERO,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn execute_with_observation_delay<S: abi_worker::JobControlStore>(
        &self,
        runtime: &mut cp::DurableRelayRuntime<S>,
        approvals: &mut dyn LocalApprovalStore,
        executor: &mut dyn GovernedLiveExecutor,
        refreshes: &AtomicUsize,
        now_ms: u64,
        observation_delay: std::time::Duration,
    ) -> cp::Result<cp::GovernedToolOutcome> {
        let mut facts = Facts(self.facts.clone(), observation_delay);
        self.execute_with_facts(runtime, approvals, executor, refreshes, now_ms, &mut facts)
    }
    #[allow(clippy::too_many_arguments)]
    fn execute_with_facts<S: abi_worker::JobControlStore>(
        &self,
        runtime: &mut cp::DurableRelayRuntime<S>,
        approvals: &mut dyn LocalApprovalStore,
        executor: &mut dyn GovernedLiveExecutor,
        refreshes: &AtomicUsize,
        now_ms: u64,
        facts: &mut dyn GovernedFactProvider,
    ) -> cp::Result<cp::GovernedToolOutcome> {
        self.dispatch
            .verify(&self.authority.verifying_key().to_bytes())
            .unwrap();
        self.policy
            .verify(
                &self.authority.verifying_key().to_bytes(),
                &self.dispatch.submission.binding,
                &self.policy.policy.submission_digest.clone(),
            )
            .unwrap();
        self.permit()
            .verify(&self.authority.verifying_key().to_bytes(), 2)
            .unwrap();
        let expected = vec!["append exactly one local record".into()];
        let mut refresh = || {
            refreshes.fetch_add(1, Ordering::SeqCst);
            Ok(self.permit())
        };
        let mut ctx = cp::GovernedToolContext {
            user_id: "local-user",
            resource: "startup-owned scratch file",
            expected_effects: &expected,
            sources: &self.sources,
            registration: self.registration.clone(),
            facts,
            approvals,
            executor,
            clock: &FixedClock::new(now_ms),
            refresh: &mut refresh,
        };
        runtime.execute_governed(
            &self.dispatch,
            &self.policy,
            &mut ctx,
            &CancellationToken::new(),
        )
    }
}
struct Facts(GovernedFacts, std::time::Duration);
impl GovernedFactProvider for Facts {
    fn observe(&mut self, _: &Request) -> Result<GovernedFacts, LifecycleError> {
        std::thread::sleep(self.1);
        Ok(self.0.clone())
    }
    fn postconditions(
        &mut self,
        r: &Request,
        required: &BTreeSet<String>,
    ) -> Result<Vec<PostconditionObservation>, LifecycleError> {
        Ok(required
            .iter()
            .map(|predicate| PostconditionObservation {
                predicate: predicate.clone(),
                request_digest: r.request_digest,
                observed_at_ms: 2000,
                expires_at_ms: 10000,
                evidence_digest: Some(Digest::from_bytes([9; 32])),
                source: ObservationSource::RecordingFixture,
                status: Postconditions::Verified,
            })
            .collect())
    }
}
struct FileEffect(PathBuf);
impl GovernedLiveExecutor for FileEffect {
    fn binding(&self) -> AdapterBinding {
        AdapterBinding {
            adapter_id: "local-file-fixture".into(),
            api_version: "1".into(),
        }
    }
    fn execute(
        &mut self,
        call: &ToolCall,
        _: &Request,
        _: &CompiledSource,
        _: &PreparedExecution,
        _: ToolExecutionContext<'_>,
    ) -> Result<ToolOutput, ToolExecutionError> {
        let params: serde_json::Value = serde_json::from_str(&call.input).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.0)
            .unwrap();
        writeln!(f, "{}", params["value"].as_str().unwrap()).unwrap();
        f.sync_all().unwrap();
        Ok(ToolOutput::ok("true"))
    }
}
#[test]
fn encrypted_proposal_requires_local_decision_then_reopen_never_repeats_effect() {
    let d = Directory::new();
    let f = Fixture::new();
    let mut runtime = f.runtime(&d.0, false);
    let mut authority = f.approvals(&d.0.join("abbey.sqlite"));
    let mut effect = FileEffect(d.0.join("effect.txt"));
    let refreshes = AtomicUsize::new(0);
    let before = std::fs::read(d.0.join("worker.log")).unwrap();
    let pending = match f
        .execute(&mut runtime, &mut authority, &mut effect, &refreshes)
        .unwrap()
    {
        cp::GovernedToolOutcome::Pending(p) => p,
        x => panic!("{x:?}"),
    };
    assert_eq!(pending.state, LocalApprovalState::Pending);
    assert!(!effect.0.exists());
    assert_eq!(std::fs::read(d.0.join("worker.log")).unwrap(), before);
    let repeated = match f
        .execute_at(&mut runtime, &mut authority, &mut effect, &refreshes, 2500)
        .unwrap()
    {
        cp::GovernedToolOutcome::Pending(p) => p,
        x => panic!("{x:?}"),
    };
    assert_eq!(repeated, pending);
    let visible = authority
        .read_local_approval(&pending.call_id)
        .unwrap()
        .unwrap();
    assert_eq!(visible.parameters, json!({"value":"private-file-canary"}));
    assert_eq!(visible.account_id, f.cert.claim.account_id);
    authority
        .decide_local_approval(
            &pending.call_id,
            pending.preview_digest,
            "local-decision",
            true,
            2500,
        )
        .unwrap();
    assert!(matches!(
        f.execute_at(&mut runtime, &mut authority, &mut effect, &refreshes, 2500)
            .unwrap(),
        cp::GovernedToolOutcome::Recorded(_)
    ));
    assert_eq!(
        std::fs::read_to_string(&effect.0).unwrap(),
        "private-file-canary\n"
    );
    assert!(refreshes.load(Ordering::SeqCst) >= 4);
    assert!(
        runtime
            .outbox
            .entry(&f.dispatch.submission.binding.job_id)
            .unwrap()
            .unwrap()
            .manifest
            .is_some()
    );
    drop(runtime);
    drop(authority);
    let mut runtime = f.runtime(&d.0, true);
    let mut authority = f.approvals(&d.0.join("abbey.sqlite"));
    runtime.reconcile_local(2).unwrap();
    let refresh_before = refreshes.load(Ordering::SeqCst);
    assert!(matches!(
        f.execute(&mut runtime, &mut authority, &mut effect, &refreshes)
            .unwrap(),
        cp::GovernedToolOutcome::Recorded(_)
    ));
    assert_eq!(
        std::fs::read_to_string(&effect.0).unwrap(),
        "private-file-canary\n"
    );
    assert_eq!(refreshes.load(Ordering::SeqCst), refresh_before);
    let outbox = std::fs::read(d.0.join("outbox.sqlite")).unwrap();
    assert!(
        !outbox
            .windows(b"private-file-canary".len())
            .any(|w| w == b"private-file-canary")
    );
}
#[test]
fn another_tenant_cannot_use_the_same_local_approval_authority() {
    let d = Directory::new();
    let a = Fixture::new();
    let b = Fixture::new();
    let mut runtime = b.runtime(&d.0, false);
    let mut authority = a.approvals(&d.0.join("abbey.sqlite"));
    let mut effect = FileEffect(d.0.join("effect.txt"));
    let before = std::fs::read(d.0.join("worker.log")).unwrap();
    assert!(
        b.execute(
            &mut runtime,
            &mut authority,
            &mut effect,
            &AtomicUsize::new(0)
        )
        .is_err()
    );
    assert!(!effect.0.exists());
    assert_eq!(std::fs::read(d.0.join("worker.log")).unwrap(), before);
}

struct ExitAfterPrepare(AbbeyLocalApprovalAuthority);
impl DurableLifecycle for ExitAfterPrepare {
    fn refuse_replay(&mut self, id: Digest) -> Result<bool, LifecycleError> {
        self.0.refuse_replay(id)
    }
    fn append_stage(&mut self, a: &StageAudit) -> Result<(), LifecycleError> {
        self.0.append_stage(a)
    }
    fn prepare(&mut self, r: &PrepareRequest) -> Result<PrepareOutcome, LifecycleError> {
        let p = self.0.prepare(r)?;
        if matches!(p, PrepareOutcome::Prepared(_)) {
            std::process::exit(82);
        }
        Ok(p)
    }
    fn finish(&mut self, p: &PreparedExecution, o: &TerminalOutcome) -> Result<(), LifecycleError> {
        self.0.finish(p, o)
    }
    fn lookup(&mut self, id: Digest) -> Result<Option<TerminalOutcome>, LifecycleError> {
        self.0.lookup(id)
    }
}
impl LocalApprovalStore for ExitAfterPrepare {
    fn read_local_approval(&self, c: &str) -> Result<Option<LocalApprovalPreview>, LifecycleError> {
        self.0.read_local_approval(c)
    }
    fn request_local_approval(
        &self,
        p: &LocalApprovalPreview,
    ) -> Result<LocalApprovalStatus, LifecycleError> {
        self.0.request_local_approval(p)
    }
    fn decide_local_approval(
        &self,
        c: &str,
        d: Digest,
        i: &str,
        a: bool,
        n: u64,
    ) -> Result<LocalApprovalStatus, LifecycleError> {
        self.0.decide_local_approval(c, d, i, a, n)
    }
    fn exact_local_approval(
        &self,
        c: &str,
        d: Digest,
        n: u64,
    ) -> Result<Option<ExactApproval>, LifecycleError> {
        self.0.exact_local_approval(c, d, n)
    }
}
#[test]
fn crash_after_abbey_prepare_child() {
    let Some(root) = std::env::var_os("ABBEY_RELAY_CRASH_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    let f = Fixture::new();
    std::fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&(&f.dispatch, &f.policy)).unwrap(),
    )
    .unwrap();
    let mut runtime = f.runtime(&root, false);
    let mut authority = f.approvals(&root.join("abbey.sqlite"));
    let mut effect = FileEffect(root.join("effect.txt"));
    let count = AtomicUsize::new(0);
    let pending = match f
        .execute(&mut runtime, &mut authority, &mut effect, &count)
        .unwrap()
    {
        cp::GovernedToolOutcome::Pending(p) => p,
        x => panic!("{x:?}"),
    };
    authority
        .decide_local_approval(
            &pending.call_id,
            pending.preview_digest,
            "crash-decision",
            true,
            2000,
        )
        .unwrap();
    if std::env::var_os("ABBEY_RELAY_CRASH_AFTER_EFFECT").is_some() {
        let mut effect = ExitAfterEffect(effect);
        let _ = f.execute(&mut runtime, &mut authority, &mut effect, &count);
    } else {
        let mut authority = ExitAfterPrepare(authority);
        let _ = f.execute(&mut runtime, &mut authority, &mut effect, &count);
    }
    panic!("fixture did not exit at expected crash boundary");
}
#[test]
fn crash_after_prepare_recovers_unresolved_without_effect_or_readmission() {
    recover_crash(false);
}
#[test]
fn crash_after_real_file_effect_recovers_unresolved_without_repeating_effect() {
    recover_crash(true);
}
fn recover_crash(after_effect: bool) {
    let d = Directory::new();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    if after_effect {
        child.env("ABBEY_RELAY_CRASH_AFTER_EFFECT", "1");
    }
    let result = child
        .args(["--exact", "crash_after_abbey_prepare_child", "--nocapture"])
        .env("ABBEY_RELAY_CRASH_FIXTURE", &d.0)
        .stdin(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(result.code(), Some(if after_effect { 83 } else { 82 }));
    let (dispatch, policy): (cp::Dispatch, cp::SignedDeliveryPolicy) =
        serde_json::from_slice(&std::fs::read(d.0.join("fixture.json")).unwrap()).unwrap();
    let b = &dispatch.submission.binding;
    let mut f = Fixture::from_ids(
        b.tenant_id.clone(),
        b.account_id.clone(),
        b.node_id.clone(),
        b.job_id.clone(),
    );
    f.dispatch = dispatch;
    f.policy = policy;
    let mut runtime = f.runtime(&d.0, true);
    let mut authority = f.approvals(&d.0.join("abbey.sqlite"));
    let mut effect = FileEffect(d.0.join("effect.txt"));
    let manifests = runtime.reconcile_local(2).unwrap();
    assert!(!manifests.is_empty());
    assert_effect(&effect.0, after_effect);
    let count = AtomicUsize::new(0);
    assert!(matches!(
        f.execute(&mut runtime, &mut authority, &mut effect, &count)
            .unwrap(),
        cp::GovernedToolOutcome::Unresolved
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_effect(&effect.0, after_effect);
    let conn = rusqlite::Connection::open(d.0.join("abbey.sqlite")).unwrap();
    let state: String = conn
        .query_row("SELECT state FROM governed_tool_intents", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "interrupted");
}

fn assert_effect(path: &Path, happened: bool) {
    if happened {
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "private-file-canary\n"
        );
    } else {
        assert!(!path.exists());
    }
}
struct ExitAfterEffect(FileEffect);
impl GovernedLiveExecutor for ExitAfterEffect {
    fn binding(&self) -> AdapterBinding {
        self.0.binding()
    }
    fn execute(
        &mut self,
        call: &ToolCall,
        request: &Request,
        source: &CompiledSource,
        prepared: &PreparedExecution,
        context: ToolExecutionContext<'_>,
    ) -> Result<ToolOutput, ToolExecutionError> {
        self.0.execute(call, request, source, prepared, context)?;
        std::process::exit(83)
    }
}

#[test]
fn expired_observation_budget_never_admits_worker_or_creates_local_approval() {
    let d = Directory::new();
    let f = Fixture::with_deadline(
        uuid::Uuid::new_v4().to_string(),
        uuid::Uuid::new_v4().to_string(),
        uuid::Uuid::new_v4().to_string(),
        uuid::Uuid::new_v4().to_string(),
        5,
    );
    let mut runtime = f.runtime(&d.0, false);
    let mut authority = f.approvals(&d.0.join("abbey.sqlite"));
    let mut effect = FileEffect(d.0.join("effect.txt"));
    let before = std::fs::read(d.0.join("worker.log")).unwrap();
    let result = f.execute_with_observation_delay(
        &mut runtime,
        &mut authority,
        &mut effect,
        &AtomicUsize::new(0),
        2000,
        std::time::Duration::from_millis(50),
    );
    assert!(matches!(result, Err(cp::Error::Denied)));
    assert!(!effect.0.exists());
    assert_eq!(std::fs::read(d.0.join("worker.log")).unwrap(), before);
    let connection = rusqlite::Connection::open(d.0.join("abbey.sqlite")).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM local_governed_approvals", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}

#[path = "governed_relay/interruptions.rs"]
mod interruptions;
