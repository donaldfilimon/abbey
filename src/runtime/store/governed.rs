//! Recording-only governance adapter on the existing RuntimeStore authority.
//! The trusted host evaluates current grants; this owner atomically reserves
//! exact authority and consumes approvals. Neither lookup nor recovery dispatches.
use super::tool_approval as approval;
use super::{RuntimeStore, StoreError, ToolApprovalState};
use abi_capability::lifecycle::*;
use abi_capability::{Decision, Digest, ReasonCode};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest as _, Sha256};

const CAPACITY: i64 = 100_000;

fn unavailable(_: impl std::fmt::Display) -> LifecycleError {
    LifecycleError::Unavailable
}
fn invalid(_: impl std::fmt::Display) -> LifecycleError {
    LifecycleError::InvalidState
}
fn json<T: serde::Serialize>(value: &T, limit: usize) -> Result<String, LifecycleError> {
    let text = serde_json::to_string(value).map_err(invalid)?;
    if text.len() > limit {
        return Err(LifecycleError::Capacity);
    }
    Ok(text)
}
fn binding_digest(binding: &AuthorizationBinding) -> Digest {
    Digest::from_bytes(
        Sha256::digest(serde_json::to_vec(binding).expect("fixed binding serializes")).into(),
    )
}
fn hex(digest: Digest) -> String {
    digest.to_string()[7..].to_owned()
}
fn outcome(state: OutcomeState, reason: LifecycleReason) -> TerminalOutcome {
    TerminalOutcome {
        version: 1,
        state,
        result_digest: None,
        reason,
    }
}
fn room(conn: &Connection, table: &str) -> Result<(), LifecycleError> {
    let count: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .map_err(unavailable)?;
    if count >= CAPACITY {
        Err(LifecycleError::Capacity)
    } else {
        Ok(())
    }
}
fn append(conn: &Connection, record: &StageAudit, admitted: bool) -> Result<(), LifecycleError> {
    if record.version != 1
        || record.stage > 16
        || (record.stage == 11 && !admitted && record.decision != Decision::Deny)
    {
        return Err(LifecycleError::InvalidState);
    }
    if let Some(binding) = conn
        .query_row(
            "SELECT binding_json FROM governed_tool_intents WHERE execution_id=?1",
            [record.execution_id.to_string()],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(unavailable)?
    {
        let binding: AuthorizationBinding = serde_json::from_str(&binding).map_err(invalid)?;
        if record.binding_digest != binding_digest(&binding) {
            return Err(LifecycleError::InvalidState);
        }
    } else if record.stage >= 12 {
        return Err(LifecycleError::InvalidState);
    }
    if record.stage == 11 && !admitted {
        room(conn, "governed_admission_refusals")?;
        conn.execute("INSERT INTO governed_admission_refusals(execution_id,stage,record_json) VALUES(?1,11,?2)",
            params![record.execution_id.to_string(),json(record,1024)?]).map_err(unavailable)?;
        return Ok(());
    }
    room(conn, "governed_stage_events")?;
    let id = record.execution_id.to_string();
    conn.execute("INSERT INTO governed_stage_events(execution_id,intent_id,stage,record_json) VALUES (?1,?2,?3,?4)",
        params![id, admitted.then_some(&id),record.stage,json(record,1024)?]).map_err(unavailable)?;
    Ok(())
}

impl RuntimeStore {
    /// Persist the full preview before an explicit legacy approval decision.
    /// Governed preparation refuses legacy approvals without this exact preview.
    pub fn create_governed_tool_approval(
        &self,
        request: super::NewToolApproval,
        binding: &AuthorizationBinding,
    ) -> Result<super::ToolApprovalRecord, LifecycleError> {
        if binding.required_approval == abi_capability::ApprovalLevel::A0None
            || request.call_digest != hex(binding.request_digest)
        {
            return Err(LifecycleError::InvalidState);
        }
        let mut conn = self.conn.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        room(&tx, "governed_approval_previews")?;
        let record = approval::create_on(&tx, request).map_err(invalid)?;
        tx.execute(
            "INSERT INTO governed_approval_previews(call_id,binding_json) VALUES(?1,?2)",
            params![record.call_id, json(binding, 2048)?],
        )
        .map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(record)
    }
}

impl DurableLifecycle for RuntimeStore {
    fn refuse_replay(&mut self, execution_id: Digest) -> Result<bool, LifecycleError> {
        let mut conn = self.conn.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        validate_on(&tx).map_err(invalid)?;
        let stored: Option<String> = tx
            .query_row(
                "SELECT binding_json FROM governed_tool_intents WHERE execution_id=?1",
                [execution_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(unavailable)?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        let binding: AuthorizationBinding = serde_json::from_str(&stored).map_err(invalid)?;
        append(
            &tx,
            &StageAudit {
                version: 1,
                execution_id,
                stage: 11,
                binding_digest: binding_digest(&binding),
                decision: Decision::Deny,
                reason: LifecycleReason::Authorization(ReasonCode::ApprovalReplayed),
            },
            false,
        )?;
        tx.commit().map_err(unavailable)?;
        Ok(true)
    }

    fn append_stage(&mut self, record: &StageAudit) -> Result<(), LifecycleError> {
        let mut conn = self.conn.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        append(&tx, record, false)?;
        tx.commit().map_err(unavailable)
    }

    fn prepare(&mut self, request: &PrepareRequest) -> Result<PrepareOutcome, LifecycleError> {
        if !request.is_valid() {
            return Err(LifecycleError::InvalidState);
        }
        let binding_json = json(&request.binding, 2048)?;
        let approval_json = request
            .approval
            .as_ref()
            .map(|a| json(a, 8192))
            .transpose()?;
        let now = approval::sql_u64(request.now_ms).map_err(invalid)?;
        let mut conn = self.conn.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        validate_on(&tx).map_err(invalid)?;
        let duplicate: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM governed_tool_intents WHERE execution_id=?1 OR call_id=?2)",
            params![request.execution_id.to_string(),request.call_id],|r| r.get(0)).map_err(unavailable)?;
        if duplicate {
            return Ok(PrepareOutcome::Uncertain);
        }
        if approval::resolution_exists(&tx, &request.execution_id.to_string())
            .map_err(unavailable)?
        {
            return Ok(PrepareOutcome::Uncertain);
        }
        room(&tx, "governed_tool_intents")?;
        if let Some(origin) = &request.binding.compensation_origin {
            if !valid_origin(&tx, &request.binding).map_err(invalid)? {
                return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient));
            }
            let reserved: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM governed_tool_intents WHERE original_execution_id=?1)",[origin.original_execution_id.to_string()],|r|r.get(0)).map_err(unavailable)?;
            if reserved {
                return Ok(PrepareOutcome::Uncertain);
            }
        }
        let legacy = if let Some(exact) = &request.approval {
            let preview: Option<String> = tx
                .query_row(
                    "SELECT binding_json FROM governed_approval_previews WHERE call_id=?1",
                    [&request.call_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(unavailable)?;
            if preview.as_deref() != Some(binding_json.as_str()) {
                return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient));
            }

            let current = match approval::require_record(&tx, &request.call_id) {
                Ok(row) => row,
                Err(StoreError::ToolApprovalNotFound(_)) => {
                    return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalMissing));
                }
                Err(e) => return Err(unavailable(e)),
            };
            if current.call_digest != hex(request.binding.request_digest)
                || current.tool_id != request.tool_id
                || current.decision_id.as_deref() != Some(exact.approval.decision_id.as_str())
                || current.created_at_ms != exact.issued_at_ms
                || current.expires_at_ms != exact.approval.expires_at_ms
            {
                return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalInsufficient));
            }
            if approval::expire_if_needed(&tx, &current, now)
                .map_err(unavailable)?
                .is_some()
            {
                tx.commit().map_err(unavailable)?;
                return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalExpired));
            }
            if current.state != ToolApprovalState::Approved {
                return Ok(PrepareOutcome::Refused(ReasonCode::ApprovalReplayed));
            }
            let execution_id = request.execution_id.to_string();
            if approval::resolution_exists(&tx, &execution_id).map_err(unavailable)? {
                return Ok(PrepareOutcome::Uncertain);
            }
            tx.execute("UPDATE tool_approvals SET state='consumed',updated_at_ms=?1 WHERE call_id=?2 AND state='approved'",params![now,request.call_id]).map_err(unavailable)?;
            approval::insert_event(
                &tx,
                &request.call_id,
                ToolApprovalState::Consumed,
                current.decision_id.as_deref(),
                None,
                now,
            )
            .map_err(unavailable)?;
            tx.execute("INSERT INTO tool_executions(call_id,execution_id,state,started_at_ms) VALUES (?1,?2,'prepared',?3)",params![request.call_id,execution_id,now]).map_err(unavailable)?;
            tx.execute("INSERT INTO tool_execution_events(call_id,execution_id,state,occurred_at_ms) VALUES (?1,?2,'prepared',?3)",params![request.call_id,execution_id,now]).map_err(unavailable)?;
            Some(request.call_id.as_str())
        } else {
            // A0 may not shadow a legacy approval or execution call identity.
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM tool_approvals WHERE call_id=?1)",
                    [&request.call_id],
                    |r| r.get(0),
                )
                .map_err(unavailable)?;
            if exists {
                return Ok(PrepareOutcome::Uncertain);
            }
            None
        };
        tx.execute("INSERT INTO governed_tool_intents(execution_id,call_id,tool_id,approval_call_id,legacy_execution_call_id,binding_json,approval_json,outcome_json,state,started_at_ms,original_execution_id,original_binding_digest) VALUES (?1,?2,?3,?4,?4,?5,?6,?7,'prepared',?8,?9,?10)",
            params![request.execution_id.to_string(),request.call_id,request.tool_id,legacy,binding_json,approval_json,json(&outcome(OutcomeState::Prepared,LifecycleReason::Authorization(ReasonCode::Authorized)),1024)?,now,request.binding.compensation_origin.as_ref().map(|o|o.original_execution_id.to_string()),request.binding.compensation_origin.as_ref().map(|o|o.original_binding_digest.to_string())]).map_err(unavailable)?;
        tx.execute("INSERT INTO governed_outcome_events(execution_id,outcome_json) SELECT execution_id,outcome_json FROM governed_tool_intents WHERE execution_id=?1",[request.execution_id.to_string()]).map_err(unavailable)?;
        append(
            &tx,
            &StageAudit {
                version: 1,
                execution_id: request.execution_id,
                stage: 11,
                binding_digest: binding_digest(&request.binding),
                decision: Decision::Allow,
                reason: LifecycleReason::Authorization(ReasonCode::Authorized),
            },
            true,
        )?;
        tx.commit().map_err(unavailable)?;
        Ok(PrepareOutcome::Prepared(PreparedExecution::after_commit(
            request.execution_id,
            request.binding.clone(),
        )))
    }

    fn finish(
        &mut self,
        prepared: &PreparedExecution,
        terminal: &TerminalOutcome,
    ) -> Result<(), LifecycleError> {
        if !valid_terminal(terminal) {
            return Err(LifecycleError::InvalidState);
        }
        let mut conn = self.conn.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        validate_on(&tx).map_err(invalid)?;
        let (binding, state, legacy, started): (String,String,Option<String>,i64) = tx.query_row(
            "SELECT binding_json,state,legacy_execution_call_id,started_at_ms FROM governed_tool_intents WHERE execution_id=?1",[prepared.execution_id().to_string()],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(invalid)?;
        if state != "prepared"
            || serde_json::from_str::<AuthorizationBinding>(&binding).map_err(invalid)?
                != *prepared.binding()
        {
            return Err(LifecycleError::InvalidState);
        }
        let base = base_state(terminal.state);
        let finished = i64::try_from(super::epoch_ms().map_err(unavailable)?)
            .map_err(invalid)?
            .max(started);
        if let Some(call) = legacy {
            let digest = legacy_result_digest(terminal);
            tx.execute("UPDATE tool_executions SET state=?1,result_digest=?2,finished_at_ms=?3 WHERE call_id=?4 AND state='prepared'",params![base,digest,finished,call]).map_err(unavailable)?;
            tx.execute("INSERT INTO tool_execution_events(call_id,execution_id,state,result_digest,occurred_at_ms) VALUES (?1,?2,?3,?4,?5)",params![call,prepared.execution_id().to_string(),base,digest,finished]).map_err(unavailable)?;
        }
        tx.execute(
            "UPDATE governed_tool_intents SET state=?1,outcome_json=?2 WHERE execution_id=?3",
            params![
                base,
                json(terminal, 1024)?,
                prepared.execution_id().to_string()
            ],
        )
        .map_err(unavailable)?;
        tx.execute("INSERT INTO governed_outcome_events(execution_id,outcome_json) SELECT execution_id,outcome_json FROM governed_tool_intents WHERE execution_id=?1",[prepared.execution_id().to_string()]).map_err(unavailable)?;
        tx.commit().map_err(unavailable)
    }

    fn lookup(&mut self, execution_id: Digest) -> Result<Option<TerminalOutcome>, LifecycleError> {
        let conn = self.conn.lock().map_err(unavailable)?;
        validate_on(&conn).map_err(invalid)?;
        let text: Option<String> = conn
            .query_row(
                "SELECT outcome_json FROM governed_tool_intents WHERE execution_id=?1",
                [execution_id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(unavailable)?;
        text.map(|s| serde_json::from_str(&s).map_err(invalid))
            .transpose()
    }
}
fn base_state(state: OutcomeState) -> &'static str {
    match state {
        OutcomeState::Prepared => "prepared",
        OutcomeState::Succeeded => "succeeded",
        OutcomeState::Uncertain | OutcomeState::NotDispatched => "interrupted",
        _ => "failed",
    }
}
fn valid_terminal(value: &TerminalOutcome) -> bool {
    value.is_valid_terminal()
}
// Existing legacy interrupted records cannot carry result evidence. Preserve the
// full evidence and known no-dispatch distinction in the governed extension.
fn legacy_result_digest(value: &TerminalOutcome) -> Option<String> {
    if matches!(
        value.state,
        OutcomeState::Uncertain | OutcomeState::NotDispatched
    ) {
        None
    } else {
        value.result_digest.map(hex)
    }
}

/// The original terminal row is immutable; inverse admission reserves it atomically.
fn valid_origin(conn: &Connection, binding: &AuthorizationBinding) -> Result<bool, StoreError> {
    let Some(origin) = &binding.compensation_origin else {
        return Ok(true);
    };
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT binding_json,outcome_json FROM governed_tool_intents WHERE execution_id=?1",
            [origin.original_execution_id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((original, terminal)) = row else {
        return Ok(false);
    };
    let original: AuthorizationBinding = serde_json::from_str(&original)?;
    let terminal: TerminalOutcome = serde_json::from_str(&terminal)?;
    Ok(terminal.state == OutcomeState::Failed
        && terminal.version == 1
        && origin.original_binding_digest == binding_digest(&original)
        && binding.scope_digest == original.scope_digest)
}

pub(super) fn validate_on(conn: &Connection) -> Result<(), StoreError> {
    let bad_fk: bool = conn.prepare("PRAGMA foreign_key_check")?.exists([])?;
    if bad_fk {
        return Err(StoreError::CorruptData("governed foreign key mismatch"));
    }
    let integrity: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err(StoreError::CorruptData("runtime integrity check failed"));
    }
    for table in [
        "governed_tool_intents",
        "governed_stage_events",
        "governed_admission_refusals",
        "governed_approval_previews",
    ] {
        let count: i64 =
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        if count > CAPACITY {
            return Err(StoreError::CorruptData("governed capacity exceeded"));
        }
    }
    let mut previews=conn.prepare("SELECT p.binding_json,a.call_digest FROM governed_approval_previews p JOIN tool_approvals a ON a.call_id=p.call_id")?;
    for row in previews.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (text, digest) = row?;
        let binding: AuthorizationBinding = serde_json::from_str(&text)?;
        if hex(binding.request_digest) != digest
            || binding.required_approval == abi_capability::ApprovalLevel::A0None
        {
            return Err(StoreError::CorruptData("invalid governed preview"));
        }
    }
    let invalid_refusals: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM governed_admission_refusals WHERE stage != 11 OR json_extract(record_json,'$.decision') IS NOT 'deny')",[],|row|row.get(0))?;
    if invalid_refusals {
        return Err(StoreError::CorruptData("invalid governed refusal"));
    }
    let mut audit_stmt = conn.prepare("SELECT execution_id,stage,record_json FROM governed_stage_events UNION ALL SELECT execution_id,stage,record_json FROM governed_admission_refusals")?;
    let audits = audit_stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, u8>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in audits {
        let (id, stage, text) = row?;
        let record: StageAudit = serde_json::from_str(&text)?;
        if record.version != 1
            || record.stage != stage
            || record.execution_id.to_string() != id
            || stage > 16
        {
            return Err(StoreError::CorruptData("invalid governed audit"));
        }
    }
    // Every post-admission event is bound to the durable prepared authority.
    let mut stmt = conn.prepare("SELECT e.record_json,i.binding_json FROM governed_stage_events e LEFT JOIN governed_tool_intents i ON e.execution_id=i.execution_id WHERE e.stage>=12")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })? {
        let (record, binding) = row?;
        let record: StageAudit = serde_json::from_str(&record)?;
        let binding: AuthorizationBinding = serde_json::from_str(
            &binding.ok_or(StoreError::CorruptData("orphan post-admission audit"))?,
        )?;
        if record.binding_digest != binding_digest(&binding) {
            return Err(StoreError::CorruptData("unbound post-admission audit"));
        }
    }
    let mut stmt = conn.prepare("SELECT execution_id,call_id,tool_id,binding_json,approval_json,outcome_json,state,started_at_ms,legacy_execution_call_id FROM governed_tool_intents")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, Option<String>>(8)?,
        ))
    })?;
    for row in rows {
        let (id, call, tool, binding, approval_json, terminal, state, started, legacy) = row?;
        let binding: AuthorizationBinding = serde_json::from_str(&binding)?;
        let latest: String = conn.query_row("SELECT outcome_json FROM governed_outcome_events WHERE execution_id=?1 ORDER BY sequence DESC LIMIT 1",[&id],|r|r.get(0))?;
        if latest != terminal {
            return Err(StoreError::CorruptData(
                "inconsistent governed outcome history",
            ));
        }
        let terminal: TerminalOutcome = serde_json::from_str(&terminal)?;
        let stored_origin: (Option<String>,Option<String>) = conn.query_row("SELECT original_execution_id,original_binding_digest FROM governed_tool_intents WHERE execution_id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let expected_origin = binding
            .compensation_origin
            .as_ref()
            .map(|o| {
                (
                    Some(o.original_execution_id.to_string()),
                    Some(o.original_binding_digest.to_string()),
                )
            })
            .unwrap_or((None, None));
        if stored_origin != expected_origin || !valid_origin(conn, &binding)? {
            return Err(StoreError::CorruptData("invalid compensation origin"));
        }

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM governed_outcome_events WHERE execution_id=?1",
            [&id],
            |r| r.get(0),
        )?;
        if count
            != if terminal.state == OutcomeState::Prepared {
                1
            } else {
                2
            }
        {
            return Err(StoreError::CorruptData("invalid governed transition count"));
        }

        let request = PrepareRequest {
            call_id: call.clone(),
            tool_id: tool,
            execution_id: serde_json::from_value(serde_json::Value::String(id.clone()))?,
            binding,
            approval: approval_json
                .map(|s| serde_json::from_str(&s))
                .transpose()?,
            now_ms: u64::try_from(started)
                .map_err(|_| StoreError::CorruptData("invalid governed time"))?,
        };
        if !request.is_valid()
            || base_state(terminal.state) != state
            || (terminal.state != OutcomeState::Prepared && !valid_terminal(&terminal))
            || terminal.version != 1
            || (terminal.state == OutcomeState::Prepared
                && (terminal.result_digest.is_some()
                    || terminal.reason != LifecycleReason::Authorization(ReasonCode::Authorized)))
        {
            return Err(StoreError::CorruptData("invalid governed record"));
        }
        let expected = StageAudit {
            version: 1,
            execution_id: request.execution_id,
            stage: 11,
            binding_digest: binding_digest(&request.binding),
            decision: Decision::Allow,
            reason: LifecycleReason::Authorization(ReasonCode::Authorized),
        };
        let audits: Vec<String> = conn
            .prepare(
                "SELECT record_json FROM governed_stage_events WHERE intent_id=?1 AND stage=11",
            )?
            .query_map([&id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if audits.len() != 1 || serde_json::from_str::<StageAudit>(&audits[0])? != expected {
            return Err(StoreError::CorruptData("missing governed admission audit"));
        }
        if let Some(exact) = &request.approval {
            let preview: String = conn.query_row(
                "SELECT binding_json FROM governed_approval_previews WHERE call_id=?1",
                [&call],
                |r| r.get(0),
            )?;
            if serde_json::from_str::<AuthorizationBinding>(&preview)? != request.binding {
                return Err(StoreError::CorruptData("governed preview mismatch"));
            }
            let record = approval::require_record(conn, &call)?;
            if record.tool_id != request.tool_id
                || record.call_digest != hex(request.binding.request_digest)
                || record.created_at_ms != exact.issued_at_ms
                || record.expires_at_ms != exact.approval.expires_at_ms
                || record.decision_id.as_deref() != Some(exact.approval.decision_id.as_str())
            {
                return Err(StoreError::CorruptData(
                    "governed approval identity mismatch",
                ));
            }
        }
        if request.approval.is_some() != legacy.is_some() {
            return Err(StoreError::CorruptData("invalid governed approval linkage"));
        }
        if let Some(call) = legacy {
            let pair: (String,String,String,Option<String>) = conn.query_row("SELECT a.state,e.state,e.execution_id,e.result_digest FROM tool_approvals a JOIN tool_executions e ON e.call_id=a.call_id WHERE a.call_id=?1",[call],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            if pair.0 != "consumed"
                || pair.1 != state
                || pair.2 != id
                || pair.3 != legacy_result_digest(&terminal)
            {
                return Err(StoreError::CorruptData(
                    "inconsistent governed legacy state",
                ));
            }
        }
    }
    Ok(())
}
pub(super) fn recover_on(conn: &mut Connection) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let recovered_at = approval::sql_u64(super::epoch_ms()?)?;
    let recovered = serde_json::to_string(&outcome(
        OutcomeState::Uncertain,
        LifecycleReason::Interrupted,
    ))?;
    tx.execute("INSERT INTO tool_execution_events(call_id,execution_id,state,occurred_at_ms) SELECT call_id,execution_id,'interrupted',MAX(started_at_ms,?1) FROM governed_tool_intents WHERE state='prepared' AND legacy_execution_call_id IS NOT NULL",[recovered_at])?;
    tx.execute("UPDATE tool_executions SET state='interrupted',finished_at_ms=MAX(started_at_ms,?1) WHERE call_id IN (SELECT legacy_execution_call_id FROM governed_tool_intents WHERE state='prepared')",[recovered_at])?;
    tx.execute("INSERT INTO governed_outcome_events(execution_id,outcome_json) SELECT execution_id,?1 FROM governed_tool_intents WHERE state='prepared'",[&recovered])?;
    tx.execute("UPDATE governed_tool_intents SET state='interrupted',outcome_json=?1 WHERE state='prepared'",[recovered])?;
    tx.commit()?;
    Ok(())
}
#[cfg(test)]
mod tests;
