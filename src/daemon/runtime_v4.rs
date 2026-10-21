//! Store-backed protocol-v4 command dispatch.
//!
//! This module reads and writes assistant records. It does not start a provider,
//! resubmit a turn, or replay a computer effect. `SubmitTurn` admits into the
//! store and leaves the run queued for the run manager; without a manager the
//! caller must fail or adopt the turn explicitly.

use crate::app_core::{
    APP_PROTOCOL_V4, APP_SCHEMA_V4, ConversationId, V4Capability, V4CapabilitySet, V4Command,
    V4Event, V4GrantNegotiation, V4ResourceQuery,
};

#[cfg(test)]
use crate::app_core::RunState;
use crate::runtime::{RuntimeStore, StoreError};

use super::server::HandlerFailure;

const SERVED: &[V4Capability] = &[
    V4Capability::ReadConversations,
    V4Capability::WriteConversations,
    V4Capability::SubmitTurns,
    V4Capability::ReadTurns,
    V4Capability::ReadRoutes,
];

/// Authorize a grant set that is entirely inside the served v4 surface.
pub(crate) fn authorizes(grants: &V4CapabilitySet, command: &V4Command) -> bool {
    grants.validate().is_ok()
        && grants.as_slice().iter().all(|capability| {
            SERVED.contains(capability) || *capability == V4Capability::CancelTurns
        })
        && grants.permits(command)
}

/// Only RuntimeHandler supplies cancellation authority; store-only dispatch cannot cancel.
pub(crate) fn negotiate(
    req: crate::app_core::V4GrantRequest,
    cancel_turns: bool,
) -> Result<V4Event, HandlerFailure> {
    if !req.supported_versions.contains(&APP_PROTOCOL_V4) {
        return Err(invalid());
    }
    let granted = V4CapabilitySet::from_sorted(
        req.requested
            .as_slice()
            .iter()
            .copied()
            .filter(|capability| {
                SERVED.contains(capability)
                    || (cancel_turns && *capability == V4Capability::CancelTurns)
            })
            .collect(),
    )
    .map_err(|_| internal())?;
    Ok(V4Event::Negotiated(V4GrantNegotiation {
        protocol_version: APP_PROTOCOL_V4,
        schema_version: APP_SCHEMA_V4,
        granted,
    }))
}

/// Dispatch one validated v4 command against edition runtime storage.
pub(crate) fn handle(
    store: &RuntimeStore,
    command: V4Command,
    now_ms: u64,
) -> Result<V4Event, HandlerFailure> {
    let event = match command {
        V4Command::Negotiate(req) => return negotiate(req, false),
        V4Command::CreateConversation(title) => V4Event::Conversation(
            store
                .v4_create_conversation(&title, now_ms)
                .map_err(map_store)?,
        ),
        V4Command::ListConversations(query) => {
            V4Event::Conversations(store.v4_list_conversations(&query).map_err(map_store)?)
        }
        V4Command::SearchConversations(query) => {
            let page = crate::app_core::V4ConversationQuery {
                search: Some(query.query),
                after: None,
                limit: query.page.limit,
                archived: false,
            };
            V4Event::Conversations(store.v4_search_conversations(&page).map_err(map_store)?)
        }
        V4Command::ReadConversation(conversation_id) => {
            V4Event::Conversation(store.v4_conversation(&conversation_id).map_err(map_store)?)
        }
        V4Command::ExportConversation(query) => V4Event::Conversation(
            store
                .v4_conversation(&conversation_id(&query)?)
                .map_err(map_store)?,
        ),
        V4Command::UpdateConversation(conversation_id, patch) => V4Event::Conversation(
            store
                .v4_patch_conversation(&conversation_id, &patch, now_ms)
                .map_err(map_store)?,
        ),
        V4Command::ArchiveConversation(query) => V4Event::Conversation(
            store
                .v4_archive_conversation(&conversation_id(&query)?, now_ms)
                .map_err(map_store)?,
        ),
        V4Command::RenameConversation(rename) => {
            let id = rename.conversation_id.parse().map_err(|_| invalid())?;
            V4Event::Conversation(
                store
                    .v4_rename_conversation(&id, &rename.title, now_ms)
                    .map_err(map_store)?,
            )
        }
        V4Command::ReadMessages(query) => {
            V4Event::Messages(store.v4_messages(&query).map_err(map_store)?)
        }
        V4Command::SubmitTurn(request) => {
            // Admission only. The authenticated RuntimeHandler enqueues through
            // RunManager::submit_v4; store-only callers keep the queued receipt.
            V4Event::TurnSubmitted(store.v4_admit_turn(&request, now_ms).map_err(map_store)?)
        }
        V4Command::ListMessages(query) => {
            V4Event::Messages(store.v4_messages(&query).map_err(map_store)?)
        }
        V4Command::ListContentEvents(query) => {
            V4Event::Content(store.v4_content(&query).map_err(map_store)?)
        }
        V4Command::DeleteConversation(query) => {
            store
                .v4_delete_conversation(&conversation_id(&query)?, now_ms)
                .map_err(map_store)?;
            V4Event::Deleted(query)
        }
        V4Command::ListTasks(_)
        | V4Command::CreateTask(_)
        | V4Command::ListAutomations(_)
        | V4Command::UpsertAutomation(_)
        | V4Command::ReadPermissionProfile(_)
        | V4Command::PutPermissionProfile(_)
        | V4Command::DecideAction(_)
        | V4Command::ReadTask(_)
        | V4Command::CancelTask(_)
        | V4Command::RetryTask(_)
        | V4Command::SetAutomationEnabled(_)
        | V4Command::RunAutomationNow(_) => {
            return Err(HandlerFailure::new(
                "invalid_command",
                "command is not served by this daemon",
            ));
        }
        V4Command::ReadContent(query) => {
            V4Event::Content(store.v4_content(&query).map_err(map_store)?)
        }
        V4Command::ListRoutes => {
            // Startup-bound routes are published by RuntimeHandler. Store-only
            // dispatch has no provider table, so the page stays empty.
            V4Event::Routes(Vec::new())
        }
        V4Command::CancelTurn(_, _) => {
            return Err(HandlerFailure::new(
                "invalid_command",
                "command is not served by this daemon",
            ));
        }
    };
    event.validate().map_err(|_| internal())?;
    Ok(event)
}

fn conversation_id(query: &V4ResourceQuery) -> Result<ConversationId, HandlerFailure> {
    query.resource_id.parse().map_err(|_| invalid())
}

fn map_store(error: StoreError) -> HandlerFailure {
    match error {
        StoreError::AssistantNotFound
        | StoreError::ConversationNotFound(_)
        | StoreError::RunNotFound(_) => HandlerFailure::new("not_found", "resource was not found"),
        StoreError::AssistantConflict | StoreError::TerminalRun { .. } => {
            HandlerFailure::new("conflict", "resource conflicts with stored state")
        }
        StoreError::IdempotencyConflict => HandlerFailure::new(
            "idempotency_conflict",
            "idempotency key belongs to a different request",
        ),
        StoreError::InvalidInput(_) => invalid(),
        StoreError::CorruptData(_)
        | StoreError::Migration(_)
        | StoreError::Database(_)
        | StoreError::Io(_)
        | StoreError::Json(_)
        | StoreError::InvalidAuditMetadata(_)
        | StoreError::UnexpectedStatus { .. }
        | StoreError::InvalidTransition { .. }
        | StoreError::ToolApprovalConflict
        | StoreError::ToolApprovalDigestMismatch
        | StoreError::ToolExecutionConflict
        | StoreError::ModelOperationConflict
        | StoreError::ModelOperationCapacity
        | StoreError::ToolApprovalNotFound(_)
        | StoreError::ToolExecutionNotFound(_)
        | StoreError::ModelOperationNotFound(_) => internal(),
    }
}

const fn invalid() -> HandlerFailure {
    HandlerFailure::new("invalid_command", "command payload is invalid")
}

const fn internal() -> HandlerFailure {
    HandlerFailure::new("runtime_unavailable", "runtime operation is unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_core::{V4ConversationQuery, V4GrantRequest, V4TurnSubmit};
    use std::path::{Path, PathBuf};

    const NOW: u64 = 1_700_000_000_000;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "abbey-v4-dispatch-{name}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn store(name: &str) -> (Scratch, RuntimeStore) {
        let scratch = Scratch::new(name);
        let opened = RuntimeStore::open(&RuntimeStore::path_for_state_dir(scratch.path())).unwrap();
        (scratch, opened)
    }

    fn submit(conversation_id: ConversationId, key: &str, input: &str) -> V4TurnSubmit {
        V4TurnSubmit {
            conversation_id,
            idempotency_key: key.parse().unwrap(),
            route_id: "route.ollama".into(),
            input: input.into(),
        }
    }

    #[test]
    fn v4_dispatch_admits_a_turn_without_copying_the_prompt_or_executing() {
        let (_scratch, store) = store("round");
        let V4Event::Conversation(conversation) =
            handle(&store, V4Command::CreateConversation("Notes".into()), NOW).unwrap()
        else {
            panic!("expected a conversation");
        };
        let request = submit(
            conversation.conversation_id.clone(),
            "turn-1",
            "secret prompt",
        );
        let V4Event::TurnSubmitted(submission) =
            handle(&store, V4Command::SubmitTurn(request.clone()), NOW).unwrap()
        else {
            panic!("expected a submission");
        };
        assert_eq!(submission.state, RunState::Queued);
        assert!(!submission.duplicate);
        assert!(
            !serde_json::to_string(&submission)
                .unwrap()
                .contains("secret")
        );
        let V4Event::TurnSubmitted(again) =
            handle(&store, V4Command::SubmitTurn(request), NOW).unwrap()
        else {
            panic!("expected the same submission");
        };
        assert!(again.duplicate);
        assert_eq!(again.run_id, submission.run_id);
        assert_eq!(again.state, RunState::Queued);
        let conflict = handle(
            &store,
            V4Command::SubmitTurn(submit(
                conversation.conversation_id.clone(),
                "turn-1",
                "other prompt",
            )),
            NOW,
        )
        .unwrap_err();
        assert_eq!(conflict.code(), "idempotency_conflict");
        // A second distinct turn conflicts while the admitted run is still active.
        let active = handle(
            &store,
            V4Command::SubmitTurn(submit(
                conversation.conversation_id.clone(),
                "turn-2",
                "next",
            )),
            NOW,
        )
        .unwrap_err();
        assert_eq!(active.code(), "conflict");
        // Store-only callers must fail or adopt the queued turn explicitly.
        let failed = store.v4_fail_unadopted_turn(&submission.run_id).unwrap();
        assert_eq!(failed.state, RunState::Failed);
        let V4Event::TurnSubmitted(follow) = handle(
            &store,
            V4Command::SubmitTurn(submit(
                conversation.conversation_id.clone(),
                "turn-2",
                "next",
            )),
            NOW,
        )
        .unwrap() else {
            panic!("expected a second submission");
        };
        assert_eq!(follow.state, RunState::Queued);
        assert_ne!(follow.run_id, submission.run_id);
        let events = store.run_events(&submission.run_id).unwrap();
        let encoded = events
            .iter()
            .map(|event| format!("{}:{}", event.kind, event.payload))
            .collect::<String>();
        assert!(!encoded.contains("secret"));
        let V4Event::Conversations(page) = handle(
            &store,
            V4Command::ListConversations(V4ConversationQuery {
                search: None,
                after: None,
                limit: 10,
                archived: false,
            }),
            NOW,
        )
        .unwrap() else {
            panic!("expected a page");
        };
        assert_eq!(page.conversations.len(), 1);
        assert!(!serde_json::to_string(&page).unwrap().contains("secret"));
    }

    #[test]
    fn v4_negotiation_returns_only_requested_served_grants() {
        let requested =
            V4CapabilitySet::from_sorted(vec![V4Capability::ReadConversations]).unwrap();
        let V4Event::Negotiated(n) = handle(
            &store("negotiate").1,
            V4Command::Negotiate(V4GrantRequest {
                supported_versions: vec![APP_PROTOCOL_V4],
                requested: requested.clone(),
            }),
            NOW,
        )
        .unwrap() else {
            panic!("expected negotiation");
        };
        let granted = n.granted;
        assert_eq!(granted, requested);
        assert!(authorizes(
            &requested,
            &V4Command::ListConversations(V4ConversationQuery {
                search: None,
                after: None,
                limit: 1,
                archived: false,
            })
        ));
        assert!(!authorizes(
            &V4CapabilitySet::deny_all(),
            &V4Command::ListRoutes
        ));
        assert!(!authorizes(
            &V4CapabilitySet::from_sorted(vec![V4Capability::CancelTurns]).unwrap(),
            &V4Command::ListRoutes
        ));
    }

    #[test]
    fn v4_negotiation_never_grants_stubbed_authorities() {
        let requested = V4CapabilitySet::from_sorted(vec![
            V4Capability::ReadConversations,
            V4Capability::WriteConversations,
            V4Capability::SubmitTurns,
            V4Capability::ReadTurns,
            V4Capability::CancelTurns,
            V4Capability::ReadRoutes,
            V4Capability::ReadTasks,
            V4Capability::WriteTasks,
            V4Capability::ReadAutomations,
            V4Capability::WriteAutomations,
            V4Capability::ReadPermissions,
            V4Capability::WritePermissions,
            V4Capability::DecideActions,
        ])
        .unwrap();
        let V4Event::Negotiated(result) = handle(
            &store("negotiation-stubs").1,
            V4Command::Negotiate(V4GrantRequest {
                supported_versions: vec![APP_PROTOCOL_V4],
                requested,
            }),
            NOW,
        )
        .unwrap() else {
            panic!("expected negotiation")
        };
        assert_eq!(
            result.granted.as_slice(),
            &[
                V4Capability::ReadConversations,
                V4Capability::WriteConversations,
                V4Capability::SubmitTurns,
                V4Capability::ReadTurns,
                V4Capability::ReadRoutes,
            ]
        );
        assert!(authorizes(&result.granted, &V4Command::ListRoutes));
        assert!(!authorizes(
            &result.granted,
            &V4Command::CancelTurn(ConversationId::new(), crate::app_core::RunId::new())
        ));
        assert!(!authorizes(
            &V4CapabilitySet::from_sorted(vec![V4Capability::ReadConversations]).unwrap(),
            &V4Command::SubmitTurn(submit(ConversationId::new(), "auth", "hello"))
        ));
    }
}
