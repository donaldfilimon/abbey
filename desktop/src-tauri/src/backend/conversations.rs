//! Daemon-only conversation authority. Turns execute in abbeyd, never in this process.
use super::{Route, bearer_source, route};
use crate::ipc::{IpcError, IpcErrorKind};
use abbey::app_core::*;
use abbey::daemon::{V4ClientError, V4DaemonSession};

fn session() -> Result<V4DaemonSession, IpcError> {
    if bearer_source().is_none() {
        return Err(IpcError::new(
            IpcErrorKind::Rejected,
            "conversations require a configured daemon",
        ));
    }
    match route()? {
        Route::Daemon(client) => client.negotiate_v4(requested()).map_err(error),
        Route::InProcess(_) => Err(IpcError::new(
            IpcErrorKind::Rejected,
            "conversations are daemon-only",
        )),
    }
}
fn requested() -> V4CapabilitySet {
    V4CapabilitySet::from_sorted(vec![
        V4Capability::ReadConversations,
        V4Capability::WriteConversations,
        V4Capability::SubmitTurns,
        V4Capability::ReadTurns,
        V4Capability::CancelTurns,
        V4Capability::ReadRoutes,
    ])
    .expect("canonical conversation grants")
}
fn error(value: V4ClientError) -> IpcError {
    let kind = match value {
        V4ClientError::UnsupportedPlatform => IpcErrorKind::UnsupportedPlatform,
        V4ClientError::Transport => IpcErrorKind::Transport,
        V4ClientError::InvalidResponse => IpcErrorKind::Protocol,
        V4ClientError::Rejected(V4ErrorCode::NotFound) => IpcErrorKind::NotFound,
        // Internal/deadline errors can follow a committed mutation. Preserve ambiguity.
        V4ClientError::Rejected(
            V4ErrorCode::Internal
            | V4ErrorCode::DeadlineExceeded
            | V4ErrorCode::Cancelled
            | V4ErrorCode::BudgetExceeded
            | V4ErrorCode::RateLimited
            | V4ErrorCode::ResponseTooLarge,
        ) => IpcErrorKind::Transport,
        _ => IpcErrorKind::Rejected,
    };
    IpcError::new(kind, value.to_string())
}
pub fn conversation_grants() -> Result<V4CapabilitySet, IpcError> {
    if bearer_source().is_none() {
        return Ok(V4CapabilitySet::deny_all());
    }
    Ok(session()?.grants().clone())
}
pub fn conversations_list(query: V4ConversationQuery) -> Result<V4ConversationPage, IpcError> {
    session()?.list_conversations(query).map_err(error)
}
pub fn conversations_search(query: V4ConversationQuery) -> Result<V4ConversationPage, IpcError> {
    session()?.search_conversations(query).map_err(error)
}
pub fn conversation_read(query: V4ResourceQuery) -> Result<V4Conversation, IpcError> {
    let id = conversation_id(&query.resource_id)?;
    session()?.read_conversation(id).map_err(error)
}
pub fn conversation_create(value: V4ConversationCreate) -> Result<V4Conversation, IpcError> {
    session()?.create_conversation(value.title).map_err(error)
}
pub fn conversation_rename(value: V4ConversationRename) -> Result<V4Conversation, IpcError> {
    // Rename command rejected in handler; use update patch for title.
    let patch = V4ConversationPatch {
        title: Some(value.title),
        archived: None,
        pinned: None,
    };
    let id = conversation_id(&value.conversation_id)?;
    session()?.update_conversation(id, patch).map_err(error)
}
pub fn conversation_archive(query: V4ResourceQuery) -> Result<V4Conversation, IpcError> {
    let patch = V4ConversationPatch {
        title: None,
        archived: Some(true),
        pinned: None,
    };
    let id = conversation_id(&query.resource_id)?;
    session()?.update_conversation(id, patch).map_err(error)
}
pub fn conversation_delete(query: V4ResourceQuery) -> Result<(), IpcError> {
    session()?.delete_conversation(query).map_err(error)
}
pub fn conversation_messages(query: V4MessageQuery) -> Result<V4MessagePage, IpcError> {
    session()?.read_messages(query).map_err(error)
}
pub fn conversation_content(query: V4ContentQuery) -> Result<V4ContentPage, IpcError> {
    session()?.read_content(query).map_err(error)
}
pub fn conversation_routes() -> Result<Vec<V4Route>, IpcError> {
    session()?.list_routes().map_err(error)
}
pub fn conversation_submit_turn(request: V4SubmitTurn) -> Result<V4TurnSubmission, IpcError> {
    session()?.submit_turn(request).map_err(error)
}

pub fn conversation_cancel_turn(
    conversation_id: ConversationId,
    run_id: RunId,
) -> Result<RunState, IpcError> {
    session()?
        .cancel_turn(conversation_id, run_id)
        .map_err(error)
}

fn conversation_id(value: &str) -> Result<ConversationId, IpcError> {
    value
        .parse::<ConversationId>()
        .map_err(|_| IpcError::new(IpcErrorKind::Rejected, "invalid conversation id"))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn all_conversations() -> V4ConversationQuery {
        V4ConversationQuery {
            search: None,
            after: None,
            limit: 16,
            archived: false,
        }
    }
    #[test]
    fn conversation_authority_is_exact_and_never_executes() {
        assert_eq!(
            requested().as_slice(),
            &[
                V4Capability::ReadConversations,
                V4Capability::WriteConversations,
                V4Capability::SubmitTurns,
                V4Capability::ReadTurns,
                V4Capability::CancelTurns,
                V4Capability::ReadRoutes,
            ]
        );
        assert!(!requested().contains(V4Capability::DecideActions));
        assert_eq!(
            error(V4ClientError::Rejected(V4ErrorCode::NotFound)).kind,
            IpcErrorKind::NotFound
        );
        assert_eq!(
            error(V4ClientError::Rejected(V4ErrorCode::Internal)).kind,
            IpcErrorKind::Transport
        );
    }
    #[test]
    fn missing_daemon_never_opens_conversation_storage() {
        if bearer_source().is_some() {
            return;
        }
        assert!(conversation_grants().unwrap().as_slice().is_empty());
        assert_eq!(
            conversation_create(V4ConversationCreate {
                title: "fixture".into()
            })
            .unwrap_err()
            .kind,
            IpcErrorKind::Rejected
        );
        assert!(conversations_list(all_conversations()).is_err());
        assert_eq!(
            conversation_cancel_turn(ConversationId::new(), RunId::new())
                .unwrap_err()
                .kind,
            IpcErrorKind::Rejected
        );
    }
    #[cfg(unix)]
    #[test]
    fn live_daemon_desktop_conversation_crud_and_history() {
        if std::env::var_os("ABBEY_DESKTOP_LIVE_DAEMON").is_none() {
            return;
        }
        let grants = conversation_grants().unwrap();
        assert_eq!(grants, requested());
        let created = conversation_create(V4ConversationCreate {
            title: "Desktop conversation fixture".into(),
        })
        .unwrap();
        let q = V4ResourceQuery {
            resource_id: created.id.clone(),
        };
        assert_eq!(conversation_read(q.clone()).unwrap(), created);
        assert!(
            conversations_list(all_conversations())
                .unwrap()
                .conversations
                .iter()
                .any(|c| c.id == created.id)
        );
        assert_eq!(
            conversation_rename(V4ConversationRename {
                conversation_id: created.id.clone(),
                title: "Renamed desktop fixture".into()
            })
            .unwrap()
            .title,
            "Renamed desktop fixture"
        );
        assert!(
            conversations_search(V4ConversationQuery {
                search: Some("Renamed desktop".into()),
                after: None,
                limit: 16,
                archived: false,
            })
            .unwrap()
            .conversations
            .iter()
            .any(|c| c.id == created.id)
        );
        let pinned = conversation_update(
            created.conversation_id.clone(),
            V4ConversationPatch {
                title: None,
                archived: None,
                pinned: Some(true),
            },
        )
        .unwrap();
        assert!(pinned.pinned);
        assert!(conversation_archive(q.clone()).unwrap().archived);
        assert!(
            !conversation_update(
                created.conversation_id.clone(),
                V4ConversationPatch {
                    title: None,
                    archived: Some(false),
                    pinned: None
                }
            )
            .unwrap()
            .archived
        );
        let route = conversation_routes()
            .unwrap()
            .into_iter()
            .find(|route| route.available)
            .expect("scratch available route");
        let request = V4SubmitTurn {
            conversation_id: created.conversation_id.clone(),
            idempotency_key: IdempotencyKey::new(),
            route_id: route.route_id,
            input: "Lookup desktop turn fixture".into(),
        };
        let accepted = conversation_submit_turn(request.clone()).unwrap();
        let mut terminal = false;
        for _ in 0..100 {
            let found = conversation_lookup_turn(
                created.conversation_id.clone(),
                request.idempotency_key.clone(),
            )
            .unwrap()
            .expect("admitted turn");
            assert_eq!(found.run_id, accepted.run_id);
            if found.state.is_terminal() {
                terminal = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(terminal, "scratch turn finishes without resubmission");
        assert!(
            conversation_lookup_turn(created.conversation_id.clone(), IdempotencyKey::new())
                .unwrap()
                .is_none()
        );
        let history = conversations_search(V4ConversationQuery {
            search: Some("Seeded desktop history".into()),
            after: None,
            limit: 16,
            archived: false,
        })
        .unwrap();
        let seeded = history
            .conversations
            .first()
            .expect("scratch fixture seeded one conversation");
        let messages = conversation_messages(V4MessageQuery {
            conversation_id: seeded.conversation_id.clone(),
            after: None,
            limit: 16,
        })
        .unwrap();
        assert!(messages.messages.iter().any(|message| {
            message.role == V4MessageRole::User
                && message.text == "Authorized desktop history fixture"
        }));
        assert!(
            messages.messages.iter().any(|message| {
                message.role == V4MessageRole::Assistant
                    && message.text == "desktop-live-provider-output\n"
                    && message.text_complete
            }),
            "the canonical runtime must execute the seeded turn and persist its final reply"
        );
        conversation_delete(q.clone()).unwrap();
        assert_eq!(
            conversation_read(q).unwrap_err().kind,
            IpcErrorKind::NotFound
        );
    }
}

/// Exact read-only recovery of an admitted turn; never submits work.
pub fn conversation_lookup_turn(
    conversation_id: ConversationId,
    idempotency_key: IdempotencyKey,
) -> Result<Option<V4TurnSubmission>, IpcError> {
    session()?
        .lookup_turn(conversation_id, idempotency_key)
        .map_err(error)
}

/// Only the canonical bounded title/pin/archive patch crosses this boundary.
pub fn conversation_update(
    conversation_id: ConversationId,
    patch: V4ConversationPatch,
) -> Result<V4Conversation, IpcError> {
    session()?
        .update_conversation(conversation_id, patch)
        .map_err(error)
}
