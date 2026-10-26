//! Exact command/event pairing shared by client and authenticated server.
use super::*;
use crate::app_core::{V4Command as C, V4Event as E};
impl C {
    pub fn matches_event(&self, event: &E) -> bool {
        if event.validate().is_err() {
            return false;
        }
        match (self, event) {
            (C::Negotiate(req), E::Negotiated(n)) => {
                n.protocol_version == APP_PROTOCOL_V4
                    && req.supported_versions.contains(&APP_PROTOCOL_V4)
                    && n.granted
                        .as_slice()
                        .iter()
                        .all(|c| req.requested.contains(*c))
            }
            (C::ListConversations(q), E::Conversations(p)) => {
                p.conversations.len() <= usize::from(q.limit)
            }
            (C::SearchConversations(q), E::Conversations(p)) => {
                p.conversations.len() <= usize::from(q.page.limit)
            }
            (C::CreateConversation(title), E::Conversation(c)) => &c.title == title,
            (C::ReadConversation(id), E::Conversation(c)) => id == &c.conversation_id,
            (C::UpdateConversation(id, p), E::Conversation(c)) => {
                id == &c.conversation_id
                    && p.title.as_ref().is_none_or(|t| t == &c.title)
                    && p.archived.is_none_or(|v| v == c.archived)
                    && p.pinned.is_none_or(|v| v == c.pinned)
            }
            (C::ExportConversation(q), E::Conversation(c)) => q.resource_id == c.id,
            (C::ArchiveConversation(q), E::Conversation(c)) => q.resource_id == c.id && c.archived,
            (C::RenameConversation(q), E::Conversation(c)) => {
                q.conversation_id == c.id && q.title == c.title
            }
            (C::DeleteConversation(q), E::Deleted(r)) => q == r,
            (C::ReadMessages(q) | C::ListMessages(q), E::Messages(p)) => {
                q.conversation_id == p.conversation_id
                    && p.messages.len() <= usize::from(q.limit)
                    && p.messages
                        .first()
                        .is_none_or(|e| e.sequence > q.after.unwrap_or(0))
            }
            (C::SubmitTurn(q), E::TurnSubmitted(r) | E::TurnAccepted(r)) => {
                q.conversation_id == r.conversation_id && q.idempotency_key == r.idempotency_key
            }
            (C::LookupTurn(c, k), E::TurnLookup(rc, rk, _)) => c == rc && k == rk,
            (C::CancelTurn(c, r), E::TurnCancelled(rc, rr, _)) => c == rc && r == rr,
            (C::ReadContent(q) | C::ListContentEvents(q), E::Content(p) | E::ContentEvents(p)) => {
                q.conversation_id == p.conversation_id
                    && q.run_id == p.run_id
                    && q.after == p.after
                    && p.events.len() <= usize::from(q.limit)
            }
            (C::ListRoutes, E::Routes(_)) => true,
            (C::ListTasks(q), E::Tasks(p)) => {
                q.after == p.after
                    && q.through.is_none_or(|v| v == p.through)
                    && p.tasks.len() <= usize::from(q.limit)
            }
            (C::ReadTask(q), E::Task(t)) => q.resource_id == t.task_id,
            (C::CreateTask(q), E::Task(t)) => {
                q.conversation_id == t.conversation_id
                    && q.goal == t.goal
                    && q.workspace_id == t.workspace_id
                    && q.route_id == t.route_id
                    && q.permission_profile_id == t.permission_profile_id
                    && q.profile_revision == t.profile_revision
                    && q.budget_tokens == t.budget_tokens
                    && q.execution_kind == t.execution_kind
            }
            (C::CancelTask(q), E::Task(t)) => {
                q.task_id == t.task_id
                    && t.revision >= q.expected_revision
                    && matches!(
                        t.state,
                        V4TaskState::CancelRequested
                            | V4TaskState::Cancelled
                            | V4TaskState::Succeeded
                            | V4TaskState::Failed
                            | V4TaskState::Interrupted
                    )
            }
            (C::RetryTask(q), E::Task(t)) => {
                q.task_id == t.task_id
                    && t.revision > q.expected_revision
                    && t.current_attempt.attempt_id != q.expected_attempt_id
            }
            (C::ReadTaskEvents(q), E::TaskEvents(p)) => {
                q.task_id == p.task_id
                    && q.page.after == p.after
                    && q.page.through.is_none_or(|v| v == p.through)
                    && p.events.len() <= usize::from(q.page.limit)
            }
            (C::ReadAction(q), E::Action(a)) => {
                q.action_id == a.action_id && q.task_id == a.task_id && q.attempt_id == a.attempt_id
            }
            (C::DecideAction(q), E::ActionDecision(d)) => q == &d.decision,
            (C::ReadPermissionProfile(q), E::PermissionProfile(p)) => {
                q.resource_id == p.profile.profile_id
            }
            (C::PutPermissionProfile(q), E::PermissionProfile(p)) => {
                q.profile == p.profile && q.expected_revision.is_none_or(|r| p.revision > r)
            }
            (C::ListAutomations(q), E::Automations(p)) => {
                q.after == p.after
                    && q.through.is_none_or(|v| v == p.through)
                    && p.automations.len() <= usize::from(q.limit)
            }
            (C::UpsertAutomation(q), E::Automation(a)) => {
                q == &a.spec && q.expected_revision.is_none_or(|r| a.revision > r)
            }
            (C::SetAutomationEnabled(q), E::Automation(a)) => {
                q.automation_id == a.spec.automation_id
                    && q.enabled == a.spec.enabled
                    && a.revision >= q.expected_revision
            }
            (C::RunAutomationNow(q), E::AutomationRun(a)) => {
                q.automation_id == a.automation_id
                    && q.expected_revision == a.revision
                    && matches!(&a.source,V4OccurrenceSource::Manual{operation_id} if operation_id==&q.operation_id)
            }
            (C::PauseTask(q), E::Task(t)) => {
                q.task_id == t.task_id
                    && t.revision >= q.expected_revision
                    && t.state == V4TaskState::Paused
            }
            (C::ResumeTask(q), E::Task(t)) => {
                q.task_id == t.task_id
                    && t.revision >= q.expected_revision
                    && matches!(
                        t.state,
                        V4TaskState::Queued | V4TaskState::Starting | V4TaskState::Running
                    )
            }
            _ => false,
        }
    }
}
