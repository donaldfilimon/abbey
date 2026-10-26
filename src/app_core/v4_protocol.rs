//! Narrow commands; the server must separately negotiate grants and authorize every request.
#![allow(
    dead_code,
    unreachable_patterns,
    clippy::manual_contains,
    reason = "v4 partial / drift fix"
)]
use super::v4::{V4GrantNegotiation, V4GrantRequest};
use super::*;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "command",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum V4Command {
    Negotiate(V4GrantRequest),
    ListConversations(V4ConversationQuery),
    CreateConversation(String),
    ReadConversation(ConversationId),
    ExportConversation(V4ResourceQuery),
    ArchiveConversation(V4ResourceQuery),
    RenameConversation(V4ConversationRename),
    UpdateConversation(ConversationId, V4ConversationPatch),
    ReadMessages(V4MessageQuery),
    SubmitTurn(V4SubmitTurn),
    ReadContent(V4ContentQuery),
    CancelTurn(ConversationId, RunId),
    LookupTurn(ConversationId, IdempotencyKey),
    ReadTaskEvents(V4TaskEventQuery),
    ReadAction(V4ActionQuery),
    ListRoutes,
    SearchConversations(V4SearchQuery),
    DeleteConversation(V4ResourceQuery),
    ListMessages(V4MessageQuery),
    ListContentEvents(V4ContentQuery),
    ListTasks(V4PageQuery),
    CreateTask(V4TaskCreate),
    ListAutomations(V4PageQuery),
    UpsertAutomation(V4AutomationUpsert),
    ReadPermissionProfile(V4ResourceQuery),
    PutPermissionProfile(V4ProfilePut),
    DecideAction(V4ActionDecision),
    ReadTask(V4ResourceQuery),
    CancelTask(V4TaskMutation),
    RetryTask(V4TaskRetry),
    PauseTask(V4TaskMutation),
    ResumeTask(V4TaskMutation),
    SetAutomationEnabled(V4AutomationToggle),
    RunAutomationNow(V4AutomationRun),
}
impl V4Command {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Negotiate(req) => {
                super::v4::ensure(req.supported_versions.contains(&APP_PROTOCOL_V4))?;
                req.requested.validate()
            }
            Self::CreateConversation(title) => super::v4::label(title),
            Self::ListConversations(query) => query.validate(),
            Self::ReadMessages(query) => query.validate(),
            Self::UpdateConversation(_, patch) => patch.validate(),
            Self::SubmitTurn(request) => request.validate(),
            Self::PutPermissionProfile(profile) => profile.validate(),
            Self::ReadContent(query) => query.validate(),
            Self::ListTasks(q) | Self::ListAutomations(q) => q.validate(),
            Self::CreateTask(q) => q.validate(),
            Self::UpsertAutomation(q) => q.validate(),
            Self::DecideAction(q) => q.validate(),
            Self::ReadTask(q) | Self::ReadPermissionProfile(q) => q.validate(),
            Self::CancelTask(q) | Self::PauseTask(q) | Self::ResumeTask(q) => q.validate(),
            Self::RetryTask(q) => q.validate(),
            Self::SetAutomationEnabled(q) => q.validate(),
            Self::RunAutomationNow(q) => q.validate(),
            Self::ReadTaskEvents(q) => q.validate(),
            Self::ReadAction(q) => q.validate(),
            Self::ExportConversation(q)
            | Self::ArchiveConversation(q)
            | Self::DeleteConversation(q) => q.validate(),
            Self::RenameConversation(q) => {
                q.conversation_id
                    .parse::<ConversationId>()
                    .map_err(|_| ValidationError::new("invalid v4 contract"))?;
                super::v4::label(&q.title)
            }
            Self::SearchConversations(q) => q.validate(),
            Self::ListMessages(q) => q.validate(),
            Self::ListContentEvents(q) => q.validate(),
            Self::ReadConversation(_)
            | Self::CancelTurn(_, _)
            | Self::LookupTurn(_, _)
            | Self::ListRoutes => Ok(()),
        }
    }
    pub fn validate_for_schema(&self, schema: u16) -> Result<(), ValidationError> {
        super::v4::ensure(matches!(
            schema,
            APP_SCHEMA_V4_CONVERSATIONS | APP_SCHEMA_V4
        ))?;
        self.validate()?;
        if schema == APP_SCHEMA_V4_CONVERSATIONS {
            super::v4::ensure(
                self.required_capability()
                    .is_none_or(|c| c.is_conversation()),
            )?;
            if let Self::Negotiate(r) = self {
                super::v4::ensure(r.requested.as_slice().iter().all(|c| c.is_conversation()))?;
            }
        }
        Ok(())
    }
    pub const fn required_capability(&self) -> Option<V4Capability> {
        Some(match self {
            Self::Negotiate(_) => return None,
            Self::ListConversations(_)
            | Self::ReadConversation(_)
            | Self::ExportConversation(_)
            | Self::SearchConversations(_)
            | Self::ListMessages(_)
            | Self::ReadMessages(_) => V4Capability::ReadConversations,
            Self::CreateConversation(_)
            | Self::UpdateConversation(_, _)
            | Self::DeleteConversation(_)
            | Self::ArchiveConversation(_)
            | Self::RenameConversation(_) => V4Capability::WriteConversations,
            Self::SubmitTurn(_) => V4Capability::SubmitTurns,
            Self::ReadContent(_) | Self::ListContentEvents(_) | Self::LookupTurn(_, _) => {
                V4Capability::ReadTurns
            }
            Self::CancelTurn(_, _) => V4Capability::CancelTurns,
            Self::ListRoutes => V4Capability::ReadRoutes,
            Self::ReadTaskEvents(_)
            | Self::ReadAction(_)
            | Self::ListTasks(_)
            | Self::ReadTask(_) => V4Capability::ReadTasks,
            Self::CreateTask(_)
            | Self::CancelTask(_)
            | Self::RetryTask(_)
            | Self::PauseTask(_)
            | Self::ResumeTask(_) => V4Capability::WriteTasks,
            Self::ListAutomations(_) => V4Capability::ReadAutomations,
            Self::UpsertAutomation(_)
            | Self::SetAutomationEnabled(_)
            | Self::RunAutomationNow(_) => V4Capability::WriteAutomations,
            Self::ReadPermissionProfile(_) => V4Capability::ReadPermissions,
            Self::PutPermissionProfile(_) => V4Capability::WritePermissions,
            Self::DecideAction(_) => V4Capability::DecideActions,
        })
    }
}
impl fmt::Debug for V4Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4Command([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "event",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum V4Event {
    Negotiated(V4GrantNegotiation),
    Conversations(V4ConversationPage),
    Conversation(V4Conversation),
    Messages(V4MessagePage),
    TurnSubmitted(V4TurnSubmission),
    Content(V4ContentPage),
    TurnCancelled(ConversationId, RunId, RunState),
    TurnLookup(ConversationId, IdempotencyKey, Option<V4TurnSubmission>),
    Routes(Vec<V4Route>),
    Deleted(V4ResourceQuery),
    TurnAccepted(V4TurnSubmission),
    ContentEvents(V4ContentPage),
    Tasks(V4TaskPage),
    Task(Box<V4Task>),
    Automations(V4AutomationPage),
    Automation(Box<V4Automation>),
    AutomationRun(V4AutomationOccurrence),
    TaskEvents(V4TaskEventPage),
    Action(Box<V4Action>),
    PermissionProfile(V4ProfileRevision),
    ActionDecision(V4ActionDecisionReceipt),
    Error(V4Error),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum V4ErrorCode {
    InvalidRequest,
    Unauthorized,
    NotFound,
    Conflict,
    Unavailable,
    LimitExceeded,
    Internal,
    InvalidCommand,
    UnsupportedVersion,
    CapabilityDenied,
    Cancelled,
    DeadlineExceeded,
    BudgetExceeded,
    RateLimited,
    ResponseTooLarge,
}
impl fmt::Debug for V4Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4Event([redacted])")
    }
}

impl V4Event {
    pub fn validate_for_schema(&self, schema: u16) -> Result<(), ValidationError> {
        super::v4::ensure(matches!(
            schema,
            APP_SCHEMA_V4 | APP_SCHEMA_V4_CONVERSATIONS
        ))?;
        self.validate()?;
        if let Self::Negotiated(n) = self {
            super::v4::ensure(n.schema_version == schema)?;
        }
        if schema == APP_SCHEMA_V4_CONVERSATIONS {
            super::v4::ensure(!matches!(
                self,
                Self::Tasks(_)
                    | Self::Task(_)
                    | Self::TaskEvents(_)
                    | Self::Action(_)
                    | Self::Automations(_)
                    | Self::Automation(_)
                    | Self::AutomationRun(_)
                    | Self::PermissionProfile(_)
                    | Self::ActionDecision(_)
            ))?;
            if let Self::Negotiated(n) = self {
                super::v4::ensure(n.granted.as_slice().iter().all(|c| c.is_conversation()))?;
            }
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Negotiated(n) => {
                super::v4::ensure(
                    n.protocol_version == APP_PROTOCOL_V4
                        && matches!(
                            n.schema_version,
                            APP_SCHEMA_V4 | APP_SCHEMA_V4_CONVERSATIONS
                        ),
                )?;
                n.granted.validate()
            }
            Self::Conversations(page) => page.validate(),
            Self::Conversation(conversation) => conversation.validate(),
            Self::Messages(page) => page.validate(),
            Self::TurnSubmitted(submission) => submission.validate(),
            Self::Content(page) => page.validate(),
            Self::TurnLookup(conversation, key, submission) => {
                if let Some(submission) = submission {
                    submission.validate()?;
                    super::v4::ensure(
                        &submission.conversation_id == conversation
                            && &submission.idempotency_key == key,
                    )?;
                }
                Ok(())
            }
            Self::TurnCancelled(_, _, state) => {
                super::v4::ensure(*state == RunState::CancelRequested || state.is_terminal())
            }
            Self::Routes(routes) => {
                super::v4::ensure(routes.len() <= usize::from(MAX_V4_PAGE))?;
                for route in routes {
                    route.validate()?;
                }
                let mut ids = std::collections::BTreeSet::new();
                super::v4::ensure(routes.iter().all(|r| ids.insert(&r.route_id)))
            }
            Self::Tasks(q) => q.validate(),
            Self::Task(q) => q.validate(),
            Self::Automations(q) => q.validate(),
            Self::Automation(q) => q.validate(),
            Self::AutomationRun(q) => q.validate(),
            Self::PermissionProfile(q) => q.validate(),
            Self::ActionDecision(q) => q.validate(),
            Self::TaskEvents(q) => q.validate(),
            Self::Action(q) => q.validate(),
            Self::Deleted(_) | Self::TurnAccepted(_) | Self::ContentEvents(_) => Ok(()),
            Self::Error(e) => e.validate(),
        }
    }
}

impl<'de> Deserialize<'de> for V4Command {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(
            tag = "command",
            content = "data",
            rename_all = "snake_case",
            deny_unknown_fields
        )]
        enum Wire {
            Negotiate(V4GrantRequest),
            ListConversations(V4ConversationQuery),
            CreateConversation(String),
            ReadConversation(ConversationId),
            ExportConversation(V4ResourceQuery),
            ArchiveConversation(V4ResourceQuery),
            RenameConversation(V4ConversationRename),
            UpdateConversation(ConversationId, V4ConversationPatch),
            ReadMessages(V4MessageQuery),
            SubmitTurn(V4SubmitTurn),
            ReadContent(V4ContentQuery),
            CancelTurn(ConversationId, RunId),
            LookupTurn(ConversationId, IdempotencyKey),
            ReadTaskEvents(V4TaskEventQuery),
            ReadAction(V4ActionQuery),
            ListRoutes,
            SearchConversations(V4SearchQuery),
            DeleteConversation(V4ResourceQuery),
            ListMessages(V4MessageQuery),
            ListContentEvents(V4ContentQuery),
            ListTasks(V4PageQuery),
            CreateTask(V4TaskCreate),
            ListAutomations(V4PageQuery),
            UpsertAutomation(V4AutomationUpsert),
            ReadPermissionProfile(V4ResourceQuery),
            PutPermissionProfile(V4ProfilePut),
            DecideAction(V4ActionDecision),
            ReadTask(V4ResourceQuery),
            CancelTask(V4TaskMutation),
            RetryTask(V4TaskRetry),
            PauseTask(V4TaskMutation),
            ResumeTask(V4TaskMutation),
            SetAutomationEnabled(V4AutomationToggle),
            RunAutomationNow(V4AutomationRun),
        }
        let value = match Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?
        {
            Wire::Negotiate(request) => Self::Negotiate(request),
            Wire::ListConversations(query) => Self::ListConversations(query),
            Wire::CreateConversation(title) => Self::CreateConversation(title),
            Wire::ReadConversation(id) => Self::ReadConversation(id),
            Wire::ExportConversation(query) => Self::ExportConversation(query),
            Wire::ArchiveConversation(query) => Self::ArchiveConversation(query),
            Wire::RenameConversation(request) => Self::RenameConversation(request),
            Wire::UpdateConversation(id, patch) => Self::UpdateConversation(id, patch),
            Wire::ReadMessages(query) => Self::ReadMessages(query),
            Wire::SubmitTurn(request) => Self::SubmitTurn(request),
            Wire::ReadContent(query) => Self::ReadContent(query),
            Wire::CancelTurn(conversation_id, run_id) => Self::CancelTurn(conversation_id, run_id),
            Wire::LookupTurn(conversation, key) => Self::LookupTurn(conversation, key),
            Wire::ReadTaskEvents(q) => Self::ReadTaskEvents(q),
            Wire::ReadAction(q) => Self::ReadAction(q),
            Wire::ListRoutes => Self::ListRoutes,
            Wire::SearchConversations(query) => Self::SearchConversations(query),
            Wire::DeleteConversation(query) => Self::DeleteConversation(query),
            Wire::ListMessages(query) => Self::ListMessages(query),
            Wire::ListContentEvents(query) => Self::ListContentEvents(query),
            Wire::ListTasks(query) => Self::ListTasks(query),
            Wire::CreateTask(task) => Self::CreateTask(task),
            Wire::ListAutomations(query) => Self::ListAutomations(query),
            Wire::UpsertAutomation(automation) => Self::UpsertAutomation(automation),
            Wire::ReadPermissionProfile(query) => Self::ReadPermissionProfile(query),
            Wire::PutPermissionProfile(profile) => Self::PutPermissionProfile(profile),
            Wire::DecideAction(decision) => Self::DecideAction(decision),
            Wire::ReadTask(query) => Self::ReadTask(query),
            Wire::CancelTask(query) => Self::CancelTask(query),
            Wire::RetryTask(query) => Self::RetryTask(query),
            Wire::PauseTask(query) => Self::PauseTask(query),
            Wire::ResumeTask(query) => Self::ResumeTask(query),
            Wire::SetAutomationEnabled(query) => Self::SetAutomationEnabled(query),
            Wire::RunAutomationNow(query) => Self::RunAutomationNow(query),
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4Event {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(
            tag = "event",
            content = "data",
            rename_all = "snake_case",
            deny_unknown_fields
        )]
        enum Wire {
            Negotiated(V4GrantNegotiation),
            Conversations(V4ConversationPage),
            Conversation(V4Conversation),
            Messages(V4MessagePage),
            TurnSubmitted(V4TurnSubmission),
            Content(V4ContentPage),
            TurnCancelled(ConversationId, RunId, RunState),
            TurnLookup(ConversationId, IdempotencyKey, Option<V4TurnSubmission>),
            Routes(Vec<V4Route>),
            Deleted(V4ResourceQuery),
            TurnAccepted(V4TurnSubmission),
            ContentEvents(V4ContentPage),
            Tasks(V4TaskPage),
            Task(Box<V4Task>),
            Automations(V4AutomationPage),
            Automation(Box<V4Automation>),
            AutomationRun(V4AutomationOccurrence),
            TaskEvents(V4TaskEventPage),
            Action(Box<V4Action>),
            PermissionProfile(V4ProfileRevision),
            ActionDecision(V4ActionDecisionReceipt),
            Error(V4Error),
        }
        let value = match Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?
        {
            Wire::Negotiated(value) => Self::Negotiated(value),
            Wire::Conversations(value) => Self::Conversations(value),
            Wire::Conversation(value) => Self::Conversation(value),
            Wire::Messages(value) => Self::Messages(value),
            Wire::TurnSubmitted(value) => Self::TurnSubmitted(value),
            Wire::Content(value) => Self::Content(value),
            Wire::TurnCancelled(conversation_id, run_id, state) => {
                Self::TurnCancelled(conversation_id, run_id, state)
            }
            Wire::TurnLookup(conversation, key, submission) => {
                Self::TurnLookup(conversation, key, submission)
            }
            Wire::Routes(value) => Self::Routes(value),
            Wire::Deleted(value) => Self::Deleted(value),
            Wire::TurnAccepted(value) => Self::TurnAccepted(value),
            Wire::ContentEvents(value) => Self::ContentEvents(value),
            Wire::Tasks(value) => Self::Tasks(value),
            Wire::Task(value) => Self::Task(value),
            Wire::Automations(value) => Self::Automations(value),
            Wire::Automation(value) => Self::Automation(value),
            Wire::AutomationRun(value) => Self::AutomationRun(value),
            Wire::TaskEvents(value) => Self::TaskEvents(value),
            Wire::Action(value) => Self::Action(value),
            Wire::PermissionProfile(value) => Self::PermissionProfile(value),
            Wire::ActionDecision(value) => Self::ActionDecision(value),
            Wire::Error(error) => Self::Error(V4Error {
                code: error.code,
                message: "protocol-v4 request failed".into(),
            }),
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4ErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Wire {
            InvalidRequest,
            Unauthorized,
            NotFound,
            Conflict,
            Unavailable,
            LimitExceeded,
            Internal,
            InvalidCommand,
            UnsupportedVersion,
            CapabilityDenied,
            Cancelled,
            DeadlineExceeded,
            BudgetExceeded,
            RateLimited,
            ResponseTooLarge,
        }
        Ok(
            match Wire::deserialize(deserializer)
                .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?
            {
                Wire::InvalidRequest => Self::InvalidRequest,
                Wire::Unauthorized => Self::Unauthorized,
                Wire::NotFound => Self::NotFound,
                Wire::Conflict => Self::Conflict,
                Wire::Unavailable => Self::Unavailable,
                Wire::LimitExceeded => Self::LimitExceeded,
                Wire::Internal => Self::Internal,
                Wire::InvalidCommand => Self::InvalidCommand,
                Wire::UnsupportedVersion => Self::UnsupportedVersion,
                Wire::CapabilityDenied => Self::CapabilityDenied,
                Wire::Cancelled => Self::Cancelled,
                Wire::DeadlineExceeded => Self::DeadlineExceeded,
                Wire::BudgetExceeded => Self::BudgetExceeded,
                Wire::RateLimited => Self::RateLimited,
                Wire::ResponseTooLarge => Self::ResponseTooLarge,
            },
        )
    }
}
