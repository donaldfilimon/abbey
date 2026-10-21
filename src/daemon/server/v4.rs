//! Protocol-v4 dispatch. Commands stay fail-closed until a handler opts in.

use crate::app_core::{APP_PROTOCOL_V4, APP_SCHEMA_V4, V4Command, V4ErrorCode, V4Event};

use super::super::protocol::{V4RequestEnvelope, V4ResponseEnvelope};
use super::{DaemonHandler, HandlerFailure, valid_request_id};

pub(super) fn dispatch_authenticated_v4<H: DaemonHandler>(
    request: V4RequestEnvelope,
    handler: &H,
) -> V4ResponseEnvelope {
    if !valid_request_id(&request.request_id) {
        return V4ResponseEnvelope::error("", V4ErrorCode::InvalidCommand, "request_id is invalid");
    }
    if request.version != APP_PROTOCOL_V4
        || request.schema_version != APP_SCHEMA_V4
        || !handler.supports_v4()
    {
        return V4ResponseEnvelope::error(
            request.request_id,
            V4ErrorCode::UnsupportedVersion,
            "protocol version is unsupported",
        );
    }
    if request.grants.validate().is_err()
        || (matches!(request.command, V4Command::Negotiate(_))
            && !request.grants.as_slice().is_empty())
    {
        return V4ResponseEnvelope::error(
            request.request_id,
            V4ErrorCode::InvalidCommand,
            "grant declaration is invalid",
        );
    }
    if !request.grants.permits(&request.command) {
        return V4ResponseEnvelope::error(
            request.request_id,
            V4ErrorCode::CapabilityDenied,
            "command lacks its advertised capability grant",
        );
    }
    if !matches!(request.command, V4Command::Negotiate(_))
        && !handler.authorizes_v4(&request.grants, &request.command)
    {
        return V4ResponseEnvelope::error(
            request.request_id,
            V4ErrorCode::CapabilityDenied,
            "advertised grants exceed daemon authority",
        );
    }
    if request.command.validate().is_err() {
        return V4ResponseEnvelope::error(
            request.request_id,
            V4ErrorCode::InvalidCommand,
            "command payload is invalid",
        );
    }

    match handler.handle_v4(request.command.clone()) {
        Ok(event) if event_matches(&request.command, &event) => {
            V4ResponseEnvelope::ok(request.request_id, event)
        }
        Ok(_) => V4ResponseEnvelope::error(
            request.request_id,
            V4ErrorCode::Internal,
            "handler returned an invalid event",
        ),
        Err(failure) => V4ResponseEnvelope::error(
            request.request_id,
            error_code(failure),
            safe_message(failure.message()),
        ),
    }
}

fn event_matches(command: &V4Command, event: &V4Event) -> bool {
    match (command, event) {
        (V4Command::Negotiate(request), V4Event::Negotiated(negotiation)) => {
            negotiation.protocol_version == APP_PROTOCOL_V4
                && negotiation.schema_version == APP_SCHEMA_V4
                && request.supported_versions.contains(&APP_PROTOCOL_V4)
                && negotiation.granted.validate().is_ok()
                && negotiation
                    .granted
                    .as_slice()
                    .iter()
                    .all(|capability| request.requested.contains(*capability))
                && event.validate().is_ok()
        }
        (V4Command::Negotiate(_), _) | (_, V4Event::Negotiated(_)) => false,
        (V4Command::CancelTurn(conversation_id, run_id), V4Event::TurnCancelled(c, r, _)) => {
            c == conversation_id && r == run_id && event.validate().is_ok()
        }
        (V4Command::CancelTurn(_, _), _) | (_, V4Event::TurnCancelled(_, _, _)) => false,
        (_, other) => other.validate().is_ok(),
    }
}

fn error_code(failure: HandlerFailure) -> V4ErrorCode {
    match failure.code() {
        "capability_denied" => V4ErrorCode::CapabilityDenied,
        "invalid_command" => V4ErrorCode::InvalidCommand,
        "run_not_found" | "not_found" => V4ErrorCode::NotFound,
        "idempotency_conflict" | "conflict" => V4ErrorCode::Conflict,
        "cancelled" => V4ErrorCode::Cancelled,
        "deadline_exceeded" => V4ErrorCode::DeadlineExceeded,
        "budget_exceeded" => V4ErrorCode::BudgetExceeded,
        "rate_limited" => V4ErrorCode::RateLimited,
        "unauthorized" => V4ErrorCode::Unauthorized,
        _ => V4ErrorCode::Internal,
    }
}

fn safe_message(message: &str) -> &str {
    let candidate = crate::app_core::V4Error {
        code: V4ErrorCode::Internal,
        message: message.to_owned(),
    };
    if candidate.validate().is_ok() {
        message
    } else {
        "protocol-v4 request failed"
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::protocol::V4ResponsePayload;
    use super::*;
    use crate::app_core::{
        AppCommand, AppEvent, V4Capability, V4CapabilitySet, V4ConversationQuery,
        V4GrantNegotiation,
    };
    use crate::daemon::server::DaemonHandler;

    struct NegotiatingHandler;

    impl DaemonHandler for NegotiatingHandler {
        fn supports_version(&self, version: u16) -> bool {
            version == 1
        }

        fn handle_versioned(
            &self,
            _version: u16,
            _command: AppCommand,
        ) -> Result<AppEvent, HandlerFailure> {
            Err(HandlerFailure::new(
                "capability_denied",
                "legacy commands are not this test",
            ))
        }

        fn supports_v4(&self) -> bool {
            true
        }

        fn authorizes_v4(&self, grants: &V4CapabilitySet, command: &V4Command) -> bool {
            grants.permits(command) && grants.contains(V4Capability::ReadConversations)
        }

        fn handle_v4(&self, command: V4Command) -> Result<V4Event, HandlerFailure> {
            let V4Command::Negotiate(req) = command else {
                panic!("unauthorized command must never reach the handler");
            };
            Ok(V4Event::Negotiated(V4GrantNegotiation {
                protocol_version: APP_PROTOCOL_V4,
                schema_version: APP_SCHEMA_V4,
                granted: req.requested,
            }))
        }
    }

    fn negotiate(requested: V4CapabilitySet) -> V4RequestEnvelope {
        V4RequestEnvelope {
            version: APP_PROTOCOL_V4,
            schema_version: APP_SCHEMA_V4,
            request_id: "v4-negotiate".into(),
            bearer: "not-read-by-dispatch".into(),
            grants: V4CapabilitySet::deny_all(),
            command: V4Command::Negotiate(crate::app_core::V4GrantRequest {
                supported_versions: vec![APP_PROTOCOL_V4],
                requested,
            }),
        }
    }

    #[test]
    fn protocol_v4_negotiation_grants_only_the_requested_subset() {
        let requested =
            V4CapabilitySet::from_sorted(vec![V4Capability::ReadConversations]).unwrap();
        let response = dispatch_authenticated_v4(negotiate(requested.clone()), &NegotiatingHandler);
        match response.payload {
            V4ResponsePayload::Ok {
                event: V4Event::Negotiated(negotiation),
            } => assert_eq!(negotiation.granted, requested),
            other => panic!("expected negotiation, got {other:?}"),
        }
    }

    #[test]
    fn protocol_v4_accepts_the_exact_json_envelope_used_by_the_scratch_fixture() {
        let raw = serde_json::json!({
            "version": 4,
            "schema_version": APP_SCHEMA_V4,
            "request_id": "00000000-0000-4000-8000-000000000001",
            "bearer": "fixture-bearer",
            "grants": {"capabilities": []},
            "command": {
                "command": "negotiate",
                "data": {
                    "supported_versions": [4],
                    "requested": {"capabilities": ["read_conversations", "write_conversations", "submit_turns", "read_routes"]}
                }
            }
        });
        let request: V4RequestEnvelope = serde_json::from_value(raw).expect("valid v4 envelope");
        assert_eq!(request.version, APP_PROTOCOL_V4);
        assert_eq!(request.schema_version, APP_SCHEMA_V4);
        assert!(matches!(request.command, V4Command::Negotiate(_)));
    }

    #[test]
    fn protocol_v4_rejects_a_missing_grant_without_calling_the_handler() {
        let mut request = negotiate(V4CapabilitySet::deny_all());
        request.command = V4Command::ListConversations(V4ConversationQuery {
            search: None,
            after: None,
            limit: 1,
            archived: false,
        });
        let response = dispatch_authenticated_v4(request, &NegotiatingHandler);
        match response.payload {
            V4ResponsePayload::Error { error } => {
                assert_eq!(error.code, V4ErrorCode::CapabilityDenied);
            }
            V4ResponsePayload::Ok { .. } => panic!("list must not run without its grant"),
        }
    }

    #[test]
    fn protocol_v4_cancel_missing_grant_never_calls_the_handler() {
        let mut request = negotiate(V4CapabilitySet::deny_all());
        request.command = V4Command::CancelTurn(
            crate::app_core::ConversationId::new(),
            crate::app_core::RunId::new(),
        );
        let response = dispatch_authenticated_v4(request, &NegotiatingHandler);
        assert!(
            matches!(response.payload, V4ResponsePayload::Error { error } if error.code == V4ErrorCode::CapabilityDenied)
        );
    }

    #[test]
    fn protocol_v4_cancel_acknowledgement_requires_exact_pair_and_honest_state() {
        use crate::app_core::{ConversationId, RunId, RunState};
        let c = ConversationId::new();
        let r = RunId::new();
        let command = V4Command::CancelTurn(c.clone(), r.clone());
        for state in [
            RunState::CancelRequested,
            RunState::Cancelled,
            RunState::Succeeded,
            RunState::Failed,
            RunState::Interrupted,
        ] {
            assert!(event_matches(
                &command,
                &V4Event::TurnCancelled(c.clone(), r.clone(), state)
            ));
        }
        assert!(!event_matches(
            &command,
            &V4Event::TurnCancelled(ConversationId::new(), r.clone(), RunState::Cancelled)
        ));
        assert!(!event_matches(
            &command,
            &V4Event::TurnCancelled(c.clone(), RunId::new(), RunState::Cancelled)
        ));
        assert!(!event_matches(
            &command,
            &V4Event::TurnCancelled(c, r, RunState::Running)
        ));
        assert!(!event_matches(&command, &V4Event::Routes(vec![])));
    }

    #[test]
    fn default_handler_does_not_accept_protocol_v4() {
        struct ReadOnly;
        impl crate::daemon::ReadOnlyHandler for ReadOnly {
            fn handle(&self, _command: AppCommand) -> Result<AppEvent, String> {
                Err("unused".into())
            }
        }
        let response = dispatch_authenticated_v4(negotiate(V4CapabilitySet::deny_all()), &ReadOnly);
        match response.payload {
            V4ResponsePayload::Error { error } => {
                assert_eq!(error.code, V4ErrorCode::UnsupportedVersion);
            }
            V4ResponsePayload::Ok { .. } => panic!("default handler must not negotiate v4"),
        }
    }
}
