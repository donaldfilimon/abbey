//! Private local approval authority. Construction is a trusted startup action.
use super::*;
use abi_agent_host::local_approval::{
    LocalApprovalPreview, LocalApprovalState, LocalApprovalStatus, LocalApprovalStore,
};
use abi_capability::{Approval, ApprovalLevel, ApprovalState, Principal, PrincipalKind};
use serde::{Deserialize, Serialize};

fn commitment<T: serde::Serialize>(value: &T) -> Digest {
    Digest::from_bytes(
        Sha256::digest(serde_json::to_vec(value).expect("closed fields serialize")).into(),
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Owner {
    tenant_id: String,
    account_id: String,
    node_id: String,
    user_id: String,
    principal: Principal,
}
type ExecutionOwnerRow = (Option<String>, Option<String>, Option<String>, String);

enum ExecutionOwner {
    Unknown,
    Foreign,
    Local(Box<LocalApprovalPreview>),
}
/// Owns one authenticated local user's authority and lifecycle ledger.
/// The host constructs this at startup after local authentication. Never derive
/// these identity values from a relay request or expose construction over MCP.
pub struct AbbeyLocalApprovalAuthority {
    store: RuntimeStore,
    owner: Owner,
}
impl AbbeyLocalApprovalAuthority {
    /// Bind an already-open local ledger to one authenticated identity.
    pub fn new(
        store: RuntimeStore,
        tenant_id: String,
        account_id: String,
        node_id: String,
        user_id: String,
        principal: Principal,
    ) -> Result<Self, LifecycleError> {
        for id in [&tenant_id, &account_id, &node_id, &user_id, &principal.id] {
            approval::validate_id(id, "invalid local owner").map_err(invalid)?;
        }
        if principal.kind != PrincipalKind::HumanSubject {
            return Err(LifecycleError::InvalidState);
        }
        Ok(Self {
            store,
            owner: Owner {
                tenant_id,
                account_id,
                node_id,
                user_id,
                principal,
            },
        })
    }
    fn execution_owner(&self, id: Digest) -> Result<ExecutionOwner, LifecycleError> {
        let conn = self.store.conn.lock().map_err(unavailable)?;
        let row: Option<ExecutionOwnerRow> = conn
            .query_row(
                "SELECT i.approval_call_id,l.owner_json,l.preview_json,i.binding_json FROM governed_tool_intents i LEFT JOIN local_governed_approvals l ON l.call_id=i.approval_call_id WHERE i.execution_id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(unavailable)?;
        let Some((call, owner, preview, binding)) = row else {
            return Ok(ExecutionOwner::Unknown);
        };
        let (Some(call), Some(owner), Some(preview)) = (call, owner, preview) else {
            return Ok(ExecutionOwner::Foreign);
        };
        if serde_json::from_str::<Owner>(&owner).map_err(invalid)? != self.owner {
            return Ok(ExecutionOwner::Foreign);
        }
        let preview: LocalApprovalPreview = serde_json::from_str(&preview).map_err(invalid)?;
        self.load(&conn, &call, preview.digest())?;
        if serde_json::from_str::<AuthorizationBinding>(&binding).map_err(invalid)?
            != preview.binding
        {
            return Err(LifecycleError::InvalidState);
        }
        Ok(ExecutionOwner::Local(Box::new(preview)))
    }
    fn validate(&self, preview: &LocalApprovalPreview) -> Result<(), LifecycleError> {
        let r = &preview.request;
        let b = &preview.binding;
        if preview.version != 1
            || preview.tenant_id != self.owner.tenant_id
            || preview.account_id != self.owner.account_id
            || preview.node_id != self.owner.node_id
            || preview.user_id != self.owner.user_id
            || r.principal != self.owner.principal
            || r.scope.tenant_id != self.owner.tenant_id
            || r.scope.platform != "local"
            || r.request_digest != r.computed_digest()
            || b.request_digest != r.request_digest
            || b.principal_digest != commitment(&r.principal)
            || b.scope_digest != commitment(&r.scope)
            || r.parameter_digest != commitment(&preview.parameters)
            || b.package_digest != r.package_digest
            || b.required_approval != ApprovalLevel::A1Actor
            || preview.operation.is_empty()
            || preview.resource.is_empty()
            || preview.expected_effects.is_empty()
        {
            return Err(LifecycleError::InvalidState);
        }
        for id in [&preview.tool_id, &preview.grant_id] {
            approval::validate_id(id, "invalid preview identity").map_err(invalid)?;
        }
        json(preview, 65536)?;
        Ok(())
    }
    fn load(
        &self,
        conn: &Connection,
        call: &str,
        digest: Digest,
    ) -> Result<LocalApprovalPreview, LifecycleError> {
        let (preview, owner): (String, String) = conn
            .query_row(
                "SELECT preview_json,owner_json FROM local_governed_approvals WHERE call_id=?1",
                [call],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(invalid)?;
        if serde_json::from_str::<Owner>(&owner).map_err(invalid)? != self.owner {
            return Err(LifecycleError::InvalidState);
        }
        let preview: LocalApprovalPreview = serde_json::from_str(&preview).map_err(invalid)?;
        self.validate(&preview)?;
        if preview.request.call_id != call || preview.digest() != digest {
            return Err(LifecycleError::InvalidState);
        }
        Ok(preview)
    }
}
fn status(call_id: &str, preview_digest: Digest, state: ToolApprovalState) -> LocalApprovalStatus {
    LocalApprovalStatus {
        call_id: call_id.into(),
        preview_digest,
        state: match state {
            ToolApprovalState::Pending => LocalApprovalState::Pending,
            ToolApprovalState::Approved => LocalApprovalState::Approved,
            ToolApprovalState::Denied => LocalApprovalState::Denied,
            ToolApprovalState::Cancelled => LocalApprovalState::Cancelled,
            ToolApprovalState::Expired => LocalApprovalState::Expired,
            ToolApprovalState::Consumed => LocalApprovalState::Consumed,
        },
    }
}
impl LocalApprovalStore for AbbeyLocalApprovalAuthority {
    fn read_local_approval(
        &self,
        call_id: &str,
    ) -> Result<Option<LocalApprovalPreview>, LifecycleError> {
        approval::validate_id(call_id, "invalid approval call").map_err(invalid)?;
        let conn = self.store.conn.lock().map_err(unavailable)?;
        let text: Option<String> = conn
            .query_row(
                "SELECT preview_json FROM local_governed_approvals WHERE call_id=?1",
                [call_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(unavailable)?;
        let Some(text) = text else {
            return Ok(None);
        };
        let preview: LocalApprovalPreview = serde_json::from_str(&text).map_err(invalid)?;
        self.load(&conn, call_id, preview.digest()).map(Some)
    }

    fn request_local_approval(
        &self,
        preview: &LocalApprovalPreview,
    ) -> Result<LocalApprovalStatus, LifecycleError> {
        self.validate(preview)?;
        let mut conn = self.store.conn.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_governed_approvals WHERE call_id=?1)",
                [&preview.request.call_id],
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        if exists {
            self.load(&tx, &preview.request.call_id, preview.digest())?;
            let record =
                approval::require_record(&tx, &preview.request.call_id).map_err(invalid)?;
            return Ok(status(&record.call_id, preview.digest(), record.state));
        }
        room(&tx, "local_governed_approvals")?;
        let record = approval::create_on(
            &tx,
            super::super::NewToolApproval {
                call_id: preview.request.call_id.clone(),
                tool_id: preview.tool_id.clone(),
                call_digest: hex(preview.binding.request_digest),
                created_at_ms: preview.created_at_ms,
                expires_at_ms: preview.expires_at_ms,
            },
        )
        .map_err(invalid)?;
        tx.execute(
            "INSERT INTO governed_approval_previews(call_id,binding_json) VALUES(?1,?2)",
            params![record.call_id, json(&preview.binding, 2048)?],
        )
        .map_err(unavailable)?;
        tx.execute("INSERT INTO local_governed_approvals(call_id,preview_json,owner_json) VALUES(?1,?2,?3)",params![record.call_id,json(preview,65536)?,json(&self.owner,4096)?]).map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(status(&record.call_id, preview.digest(), record.state))
    }
    fn decide_local_approval(
        &self,
        call_id: &str,
        preview_digest: Digest,
        decision_id: &str,
        approved: bool,
        now_ms: u64,
    ) -> Result<LocalApprovalStatus, LifecycleError> {
        approval::validate_id(decision_id, "invalid decision id").map_err(invalid)?;
        let now = approval::sql_u64(now_ms).map_err(invalid)?;
        let mut conn = self.store.conn.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let preview = self.load(&tx, call_id, preview_digest)?;
        let current = approval::require_record(&tx, call_id).map_err(invalid)?;
        if now_ms < current.created_at_ms || now_ms < current.updated_at_ms {
            return Err(LifecycleError::InvalidState);
        }
        if let Some(expired) =
            approval::expire_if_needed(&tx, &current, now).map_err(unavailable)?
        {
            tx.commit().map_err(unavailable)?;
            return Ok(status(call_id, preview_digest, expired.state));
        }
        if current.state != ToolApprovalState::Pending
            || approval::resolution_exists(&tx, decision_id).map_err(unavailable)?
        {
            return Err(LifecycleError::InvalidState);
        }
        let state = if approved {
            ToolApprovalState::Approved
        } else {
            ToolApprovalState::Denied
        };
        if approved {
            let exact = ExactApproval {
                binding: preview.binding.clone(),
                issued_at_ms: current.created_at_ms,
                approval: Approval {
                    decision_id: decision_id.into(),
                    call_id: call_id.into(),
                    call_digest: preview.binding.request_digest,
                    package_digest: preview.binding.package_digest,
                    grant_id: preview.grant_id,
                    approver: self.owner.principal.clone(),
                    coapprover: None,
                    level: ApprovalLevel::A1Actor,
                    expires_at_ms: current.expires_at_ms,
                    state: ApprovalState::Approved,
                },
            };
            tx.execute("UPDATE local_governed_approvals SET exact_json=?1 WHERE call_id=?2 AND exact_json IS NULL",params![json(&exact,8192)?,call_id]).map_err(unavailable)?;
        }
        tx.execute("UPDATE tool_approvals SET state=?1,decision_id=?2,updated_at_ms=?3 WHERE call_id=?4 AND state='pending'",params![state.as_str(),decision_id,now,call_id]).map_err(unavailable)?;
        approval::insert_event(&tx, call_id, state, Some(decision_id), None, now)
            .map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(status(call_id, preview_digest, state))
    }
    fn exact_local_approval(
        &self,
        call_id: &str,
        preview_digest: Digest,
        now_ms: u64,
    ) -> Result<Option<ExactApproval>, LifecycleError> {
        let now = approval::sql_u64(now_ms).map_err(invalid)?;
        let mut conn = self.store.conn.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let preview = self.load(&tx, call_id, preview_digest)?;
        let current = approval::require_record(&tx, call_id).map_err(invalid)?;
        if now_ms < current.created_at_ms || now_ms < current.updated_at_ms {
            return Err(LifecycleError::InvalidState);
        }
        if approval::expire_if_needed(&tx, &current, now)
            .map_err(unavailable)?
            .is_some()
        {
            tx.commit().map_err(unavailable)?;
            return Ok(None);
        }
        if current.state != ToolApprovalState::Approved {
            return Ok(None);
        }
        let exact: Option<String> = tx
            .query_row(
                "SELECT exact_json FROM local_governed_approvals WHERE call_id=?1",
                [call_id],
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        let exact = exact.ok_or(LifecycleError::InvalidState)?;
        let exact: ExactApproval = serde_json::from_str(&exact).map_err(invalid)?;
        if exact.binding != preview.binding
            || exact.approval.approver != self.owner.principal
            || exact.approval.decision_id
                != current.decision_id.ok_or(LifecycleError::InvalidState)?
            || !exact.matches(&preview.binding, now_ms)
        {
            return Err(LifecycleError::InvalidState);
        }
        Ok(Some(exact))
    }
}
/// New local rows must match their owner-authenticated durable decision exactly.
/// Legacy trusted in-process callers retain their existing API boundary.
pub(super) fn exact_matches(
    conn: &Connection,
    call: &str,
    exact: &ExactApproval,
) -> Result<bool, LifecycleError> {
    let stored: Option<Option<String>> = conn
        .query_row(
            "SELECT exact_json FROM local_governed_approvals WHERE call_id=?1",
            [call],
            |r| r.get(0),
        )
        .optional()
        .map_err(unavailable)?;
    match stored {
        None => Ok(true),
        Some(None) => Ok(false),
        Some(Some(value)) => {
            Ok(serde_json::from_str::<ExactApproval>(&value).map_err(invalid)? == *exact)
        }
    }
}
impl DurableLifecycle for AbbeyLocalApprovalAuthority {
    fn refuse_replay(&mut self, id: Digest) -> Result<bool, LifecycleError> {
        match self.execution_owner(id)? {
            ExecutionOwner::Unknown => Ok(false),
            ExecutionOwner::Foreign => Err(LifecycleError::InvalidState),
            ExecutionOwner::Local(_) => self.store.refuse_replay(id),
        }
    }
    fn append_stage(&mut self, record: &StageAudit) -> Result<(), LifecycleError> {
        if matches!(
            self.execution_owner(record.execution_id)?,
            ExecutionOwner::Foreign
        ) {
            return Err(LifecycleError::InvalidState);
        }
        // Before an intent exists, the trusted in-process host may append its
        // pre-admission audit (including stage11 refusal). No owner can be inferred
        // yet; the underlying ledger still rejects unknown post-admission stages.
        // This wrapper is not a public audit-ingestion endpoint.
        self.store.append_stage(record)
    }
    fn prepare(&mut self, request: &PrepareRequest) -> Result<PrepareOutcome, LifecycleError> {
        let principal_digest = Digest::from_bytes(
            Sha256::digest(serde_json::to_vec(&self.owner.principal).map_err(invalid)?).into(),
        );
        if request.binding.principal_digest != principal_digest
            || request.binding.required_approval != ApprovalLevel::A1Actor
        {
            return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient));
        }
        let Some(exact) = &request.approval else {
            return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalMissing));
        };
        if let Some(origin) = &request.binding.compensation_origin
            && !matches!(
                self.execution_owner(origin.original_execution_id)?,
                ExecutionOwner::Local(_)
            )
        {
            return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient));
        }
        {
            let conn = self.store.conn.lock().map_err(unavailable)?;
            let text: Option<String> = conn
                .query_row(
                    "SELECT preview_json FROM local_governed_approvals WHERE call_id=?1",
                    [&request.call_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(unavailable)?;
            let Some(text) = text else {
                return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalMissing));
            };
            let preview: LocalApprovalPreview = serde_json::from_str(&text).map_err(invalid)?;
            self.load(&conn, &request.call_id, preview.digest())?;
            if exact.approval.approver != self.owner.principal {
                return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient));
            }
        }
        self.store.prepare(request)
    }
    fn finish(
        &mut self,
        prepared: &PreparedExecution,
        outcome: &TerminalOutcome,
    ) -> Result<(), LifecycleError> {
        let ExecutionOwner::Local(preview) = self.execution_owner(prepared.execution_id())? else {
            return Err(LifecycleError::InvalidState);
        };
        if preview.binding != *prepared.binding() {
            return Err(LifecycleError::InvalidState);
        }
        self.store.finish(prepared, outcome)
    }
    fn lookup(&mut self, id: Digest) -> Result<Option<TerminalOutcome>, LifecycleError> {
        match self.execution_owner(id)? {
            ExecutionOwner::Local(_) => self.store.lookup(id),
            ExecutionOwner::Unknown | ExecutionOwner::Foreign => Ok(None),
        }
    }
}
#[cfg(test)]
mod tests;
