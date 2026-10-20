use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boundary {
    PrepareCancel,
    Stage12Cancel,
    EffectCancel,
    FinalFacts,
    OwnerRefuses,
}
struct BoundaryProvider;
impl ModelProvider for BoundaryProvider {
    fn id(&self) -> &str {
        "boundary"
    }
    fn run(&self, _: &ModelRequest, run: &mut RunContext<'_>) -> Result<(), RuntimeError> {
        run.emit(&ModelEvent::ToolCall(ToolCall::new(
            "boundary-call",
            "observe",
            "{}",
        )));
        Ok(())
    }
}
struct BoundaryLedger {
    store: RuntimeStore,
    token: CancellationToken,
    boundary: Boundary,
    prepared: Option<PreparedExecution>,
}
impl DurableLifecycle for BoundaryLedger {
    fn append_stage(&mut self, record: &StageAudit) -> Result<(), LifecycleError> {
        self.store.append_stage(record)?;
        if record.stage == 12 && self.boundary == Boundary::Stage12Cancel {
            self.token.cancel();
        }
        Ok(())
    }
    fn prepare(&mut self, request: &PrepareRequest) -> Result<PrepareOutcome, LifecycleError> {
        let result = self.store.prepare(request)?;
        if let PrepareOutcome::Prepared(prepared) = &result {
            self.prepared = Some(prepared.clone());
            if self.boundary == Boundary::PrepareCancel {
                self.token.cancel();
            }
        }
        Ok(result)
    }
    fn finish(
        &mut self,
        prepared: &PreparedExecution,
        outcome: &TerminalOutcome,
    ) -> Result<(), LifecycleError> {
        self.store.finish(prepared, outcome)
    }
    fn lookup(&mut self, id: Digest) -> Result<Option<TerminalOutcome>, LifecycleError> {
        self.store.lookup(id)
    }
}
struct BoundaryFacts {
    facts: GovernedFacts,
    token: CancellationToken,
    boundary: Boundary,
    reads: usize,
}
impl GovernedFactProvider for BoundaryFacts {
    fn observe(&mut self, _: &Request) -> Result<GovernedFacts, LifecycleError> {
        self.reads += 1;
        let mut facts = self.facts.clone();
        if self.boundary == Boundary::FinalFacts && self.reads == 3 {
            facts.platform.revocation_epoch += 1;
        }
        Ok(facts)
    }
    fn postconditions(
        &mut self,
        _: &Request,
        _: &BTreeSet<String>,
    ) -> Result<Vec<PostconditionObservation>, LifecycleError> {
        if self.boundary == Boundary::EffectCancel {
            self.token.cancel();
        }
        Ok(vec![])
    }
}
fn audits(path: &std::path::Path, table: &str) -> Vec<StageAudit> {
    let conn = rusqlite::Connection::open(path).unwrap();
    if !conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |r| r.get::<_, bool>(0),
        )
        .unwrap()
    {
        return vec![];
    }
    conn.prepare(&format!(
        "SELECT record_json FROM {table} ORDER BY sequence"
    ))
    .unwrap()
    .query_map([], |r| r.get::<_, String>(0))
    .unwrap()
    .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
    .collect()
}
#[allow(clippy::too_many_lines)]
fn boundary_case(boundary: Boundary, human: bool) {
    let root = std::env::temp_dir().join(format!(
        "abbey-final-boundary-{}-{boundary:?}-{human}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("runtime.sqlite");
    let mut src = source();
    src.id = "local.observe".into();
    src.input_schema = json!({"type":"object","additionalProperties":false});
    if human {
        src.approval_policy = ApprovalLevel::A2Manager;
    }
    let sources = SourceRegistry::compile([src]).unwrap();
    let compiled = sources.get("local.observe", "1.0.0").unwrap();
    let principal = Principal::new("actor", PrincipalKind::Service).unwrap();
    let host = GovernedHost::new(
        &BoundaryProvider,
        &sources,
        vec![GovernedRegistration {
            tool_name: "observe".into(),
            request: request(compiled, &principal, Digest::default()),
        }],
    )
    .unwrap();
    let clock = FixedClock::new(100);
    let token = CancellationToken::new();
    let mut ledger = BoundaryLedger {
        store: RuntimeStore::open(&path).unwrap(),
        token: token.clone(),
        boundary,
        prepared: None,
    };
    let base = GovernedFacts {
        platform: PlatformFacts::new(BTreeSet::new(), 0, "p", "g", "s"),
        grants: vec![grant(compiled, &principal)],
        used_decision_ids: BTreeSet::new(),
        predicates: BTreeSet::from(["fresh".into(), "scoped".into()]),
        captured_pre_state: None,
        observed_at_ms: 90,
        expires_at_ms: 200,
        permitted_data_classes: BTreeSet::from([DataClass::Operational]),
        residency: ResidencyPolicy::LocalOnly,
        retention: RetentionClass::Session,
        safety_allows: true,
        rate_allows: true,
    };
    let mut facts = BoundaryFacts {
        facts: base.clone(),
        token: token.clone(),
        boundary,
        reads: 0,
    };
    let options = |approval| GovernedOptions {
        clock: &clock,
        host_budget: HostBudget::default(),
        approval,
        idempotency_key: None,
        requested_deadline_ms: 1000,
        requested_budgets: compiled.source().budgets.clone(),
    };
    let model = ModelRequest::new("boundary");
    let mut actuator = RecordingActuator::new(Vec::<String>::new());
    let approval = if human {
        let pending = host
            .run(
                &model,
                &mut CollectingSink::default(),
                &token,
                options(None),
                &mut facts,
                &mut ledger,
                &mut actuator,
            )
            .unwrap();
        let preview = pending.pending_approval.unwrap();
        if boundary != Boundary::OwnerRefuses {
            ledger
                .store
                .create_governed_tool_approval(
                    abbey::runtime::NewToolApproval {
                        call_id: preview.call_reference.clone(),
                        tool_id: "observe".into(),
                        call_digest: preview.binding.request_digest.to_string()[7..].into(),
                        created_at_ms: 90,
                        expires_at_ms: 200,
                    },
                    &preview.binding,
                )
                .unwrap();
            ledger
                .store
                .decide_tool_approval(
                    &preview.call_reference,
                    &preview.binding.request_digest.to_string()[7..],
                    "decision",
                    abbey::runtime::ToolApprovalDecision::Approve,
                    95,
                )
                .unwrap();
        }
        Some(ExactApproval {
            issued_at_ms: 90,
            approval: Approval {
                decision_id: "decision".into(),
                call_id: preview.call_reference,
                call_digest: preview.binding.request_digest,
                package_digest: preview.binding.package_digest,
                grant_id: "grant".into(),
                approver: Principal::new("manager", PrincipalKind::GuildManager).unwrap(),
                coapprover: None,
                level: ApprovalLevel::A2Manager,
                expires_at_ms: 200,
                state: ApprovalState::Approved,
            },
            binding: preview.binding,
        })
    } else {
        None
    };
    facts.reads = 0;
    let report = host
        .run(
            &model,
            &mut CollectingSink::default(),
            &token,
            options(approval.clone()),
            &mut facts,
            &mut ledger,
            &mut actuator,
        )
        .unwrap();
    if matches!(boundary, Boundary::FinalFacts | Boundary::OwnerRefuses) {
        assert!(
            matches!(report.governance, GovernedStop::Refused(_)),
            "{report:?}"
        );
        assert_eq!(actuator.records(), []);
        assert!(ledger.prepared.is_none());
        let denials = audits(&path, "governed_admission_refusals");
        assert_eq!(denials.len(), 1);
        assert_eq!(denials[0].stage, 11);
        assert_eq!(denials[0].decision, Decision::Deny);
        assert_eq!(
            audits(&path, "governed_stage_events")
                .iter()
                .filter(|a| a.stage == 11)
                .count(),
            0
        );
        drop(ledger);
        RuntimeStore::open(&path).unwrap();
    } else {
        assert_eq!(report.outcomes.len(), 1, "{report:?}");
        let outcome = &report.outcomes[0];
        if boundary == Boundary::EffectCancel {
            assert_eq!(outcome.state, OutcomeState::Uncertain);
            assert_eq!(outcome.reason, LifecycleReason::OutcomeUnknown);
            assert!(outcome.result_digest.is_some());
            assert_eq!(actuator.records().len(), 1);
        } else {
            assert_eq!(
                serde_json::to_value(outcome.state).unwrap(),
                json!("not_dispatched")
            );
            assert_eq!(outcome.reason, LifecycleReason::CancelledBeforeDispatch);
            assert!(outcome.result_digest.is_none());
            assert_eq!(actuator.records(), []);
        }
        let prepared = report.executions[0].clone();
        assert_eq!(
            ledger.lookup(prepared.execution_id()).unwrap().as_ref(),
            Some(outcome)
        );
        let stage_records = audits(&path, "governed_stage_events");
        assert_eq!(
            stage_records
                .iter()
                .filter(|a| a.stage == 11 && a.decision == Decision::Allow)
                .count(),
            1
        );
        for audit in stage_records.iter().filter(|a| a.stage >= 11) {
            assert_eq!(
                audit.binding_digest,
                digest(&serde_json::to_value(prepared.binding()).unwrap())
            );
        }
        if boundary != Boundary::EffectCancel {
            assert!(
                stage_records
                    .iter()
                    .any(|a| a.stage == 12 && a.decision == Decision::Deny)
            );
        }
        drop(ledger);
        let mut store = RuntimeStore::open(&path).unwrap();
        assert_eq!(
            store.lookup(prepared.execution_id()).unwrap().as_ref(),
            Some(outcome)
        );
        let mut inverse_binding = prepared.binding().clone();
        inverse_binding.compensation_origin = Some(CompensationBinding::from_prepared(&prepared));
        inverse_binding.required_approval = ApprovalLevel::A0None;
        assert!(matches!(
            store
                .prepare(&PrepareRequest {
                    call_id: "inverse".into(),
                    tool_id: "observe".into(),
                    execution_id: Digest::from_bytes([9; 32]),
                    binding: inverse_binding,
                    approval: None,
                    now_ms: 100
                })
                .unwrap(),
            PrepareOutcome::Refused(_)
        ));
        let clean_token = CancellationToken::new();
        let mut clean_facts = Facts(base);
        let mut replay_actuator = RecordingActuator::new(Vec::<String>::new());
        let before_replay = audits(&path, "governed_stage_events");
        let replay = host
            .run(
                &model,
                &mut CollectingSink::default(),
                &clean_token,
                options(approval),
                &mut clean_facts,
                &mut store,
                &mut replay_actuator,
            )
            .unwrap();
        assert_eq!(
            replay.governance,
            GovernedStop::Infrastructure(LifecycleError::InvalidState)
        );
        assert_eq!(replay_actuator.records(), []);
        assert_eq!(audits(&path, "governed_stage_events"), before_replay);
        assert_eq!(
            store.lookup(prepared.execution_id()).unwrap().as_ref(),
            Some(outcome)
        );
        drop(store);
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn real_owner_cancellation_during_prepare_is_known_not_dispatched() {
    for human in [false, true] {
        boundary_case(Boundary::PrepareCancel, human);
    }
}
#[test]
fn real_owner_stage12_callback_cancellation_is_known_not_dispatched() {
    for human in [false, true] {
        boundary_case(Boundary::Stage12Cancel, human);
    }
}
#[test]
fn real_owner_after_effect_cancellation_retains_genuine_uncertain_evidence() {
    for human in [false, true] {
        boundary_case(Boundary::EffectCancel, human);
    }
}
#[test]
fn real_owner_final_facts_denial_has_no_admitted_intent() {
    boundary_case(Boundary::FinalFacts, false);
}
#[test]
fn real_owner_missing_persisted_approval_denial_has_no_admitted_intent() {
    boundary_case(Boundary::OwnerRefuses, true);
}
