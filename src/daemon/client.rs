//! Bounded client for Abbey's authenticated local daemon transport.

use std::fmt;
use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::app_core::{
    APP_PROTOCOL_V1, APP_PROTOCOL_VERSION, APP_SCHEMA_V1, APP_SCHEMA_VERSION, AppCommand, AppEvent,
    CapabilitySet, ClaimsSnapshot, RuntimeStatus, V3Capability, V3CapabilitySet, V3Command,
    V3Event, V3GrantNegotiation, V3GrantRequest,
};

use super::{
    CURRENT_PROTOCOL_VERSION, DaemonConfig, PROTOCOL_VERSION, RequestEnvelope, ResponseEnvelope,
    ResponsePayload, SUPPORTED_PROTOCOL_VERSIONS, V3RequestEnvelope, V3ResponseEnvelope,
    V3ResponsePayload,
};

mod v3;

/// Object-oriented client for one configured Abbey daemon installation.
#[derive(Clone)]
pub struct DaemonClient {
    config: DaemonConfig,
}

impl DaemonClient {
    #[must_use]
    pub fn new(config: DaemonConfig) -> Self {
        Self { config }
    }

    /// Send one typed application command using the newest protocol.
    ///
    /// Read-only v1 commands may retry once when an older daemon explicitly
    /// reports `unsupported_version`. Mutating v2 commands are never
    /// downgraded or replayed automatically.
    #[cfg(unix)]
    pub fn request(&self, command: AppCommand) -> Result<AppEvent, ClientError> {
        let may_downgrade = command.minimum_protocol_version() == APP_PROTOCOL_V1;
        match unix::request(&self.config, command.clone(), CURRENT_PROTOCOL_VERSION) {
            Err(ClientError::Daemon { code, .. })
                if may_downgrade && code == "unsupported_version" =>
            {
                unix::request(&self.config, command, PROTOCOL_VERSION)
            }
            result => result,
        }
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn request(&self, _command: AppCommand) -> Result<AppEvent, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }

    /// Negotiate a separate protocol-v3 session without changing the legacy
    /// v1/v2 request path.
    ///
    /// This sends exactly one request and never downgrades or retries. The
    /// returned session owns the daemon's canonical grant set so later typed
    /// calls echo that exact set instead of accepting caller-constructed
    /// authority.
    #[cfg(unix)]
    pub fn negotiate_v3(&self, requested: V3CapabilitySet) -> Result<V3DaemonSession, ClientError> {
        let grant_request = V3GrantRequest {
            supported_versions: SUPPORTED_PROTOCOL_VERSIONS.to_vec(),
            requested,
        };
        grant_request
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        let event = unix::request_v3(
            &self.config,
            V3CapabilitySet::deny_all(),
            V3Command::Negotiate(grant_request.clone()),
        )?;
        let V3Event::Negotiated(negotiation) = event else {
            return Err(ClientError::UnexpectedV3Event {
                expected: "capability negotiation",
                received: v3_event_name(&event),
            });
        };
        negotiation
            .validate_for(&grant_request)
            .map_err(|_| ClientError::InvalidV3Response)?;
        Ok(V3DaemonSession {
            client: self.clone(),
            negotiation,
        })
    }

    /// Windows remains fail-closed until the named-pipe transport lands.
    #[cfg(not(unix))]
    pub fn negotiate_v3(
        &self,
        _requested: V3CapabilitySet,
    ) -> Result<V3DaemonSession, ClientError> {
        Err(ClientError::UnsupportedPlatform)
    }
}

/// One explicitly negotiated protocol-v3 client session.
///
/// The grant set is private and can only originate in a validated daemon
/// negotiation response. That makes grant echo an invariant of this type.
#[derive(Clone)]
pub struct V3DaemonSession {
    client: DaemonClient,
    negotiation: V3GrantNegotiation,
}

impl fmt::Debug for V3DaemonSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("V3DaemonSession")
            .field("client", &self.client)
            .field("negotiation", &self.negotiation)
            .finish()
    }
}

impl fmt::Debug for DaemonClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DaemonClient")
            .field("socket_path", &self.config.socket_path)
            .field("bearer", &"[REDACTED]")
            .field("max_frame_len", &self.config.max_frame_len)
            .field("read_timeout", &self.config.read_timeout)
            .field("write_timeout", &self.config.write_timeout)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error(
        "abbeyd client transport is not implemented on this platform; named-pipe support is required"
    )]
    UnsupportedPlatform,
    #[error("timed out establishing a ready abbeyd connection at {path}")]
    ConnectTimeout { path: PathBuf },
    #[error("cannot connect to abbeyd at {path}: {source}")]
    Connect { path: PathBuf, source: io::Error },
    /// Retained for source compatibility. Connection setup now completes in
    /// the bounded worker and reports peer-close races as `ConnectionHandoff`.
    #[error("cannot configure abbeyd socket {operation}: {source}")]
    Configure {
        operation: &'static str,
        source: io::Error,
    },
    #[error("abbeyd connection closed before request handoff completed")]
    ConnectionHandoff,
    #[error("cannot serialize abbeyd request: {0}")]
    Serialize(serde_json::Error),
    #[error("abbeyd request exceeds the configured frame limit")]
    RequestTooLarge,
    #[error("cannot write abbeyd request: {0}")]
    Write(io::Error),
    #[error("cannot read abbeyd response: {0}")]
    Read(io::Error),
    #[error("abbeyd returned an empty frame")]
    EmptyResponse,
    #[error("abbeyd response exceeds the configured frame limit")]
    ResponseTooLarge,
    #[error("abbeyd returned malformed response JSON")]
    MalformedResponse,
    #[error("abbeyd protocol mismatch: expected {expected}, received {received}")]
    ProtocolMismatch { expected: u16, received: u16 },
    #[error("abbeyd response request_id does not match the request")]
    RequestIdMismatch,
    #[error("abbeyd returned {received} for a {expected} request")]
    UnexpectedEvent {
        expected: &'static str,
        received: &'static str,
    },
    #[error("abbeyd returned invalid runtime status: {0}")]
    InvalidRuntimeStatus(&'static str),
    #[error("abbeyd returned an inconsistent claims snapshot")]
    InvalidClaimsSnapshot,
    #[error("abbeyd returned an unsanitized or mismatched route audit page")]
    InvalidRouteAudit,
    #[error("abbeyd returned an invalid or mismatched run response")]
    InvalidRunResponse,
    #[error("protocol-v3 request is invalid")]
    InvalidV3Request,
    #[error("abbeyd returned an invalid protocol-v3 response")]
    InvalidV3Response,
    #[error(
        "tool call {call_id} requires exact-call approval for digest {call_digest} before {expires_at_ms}"
    )]
    V3ToolApprovalRequired {
        call_id: String,
        call_digest: String,
        expires_at_ms: u64,
    },
    #[error("abbeyd did not grant required protocol-v3 capability {capability:?}")]
    V3CapabilityNotGranted { capability: V3Capability },
    #[error("abbeyd returned {received} for expected protocol-v3 {expected}")]
    UnexpectedV3Event {
        expected: &'static str,
        received: &'static str,
    },
    #[error("abbeyd rejected the protocol-v3 request ({code:?}): {message}")]
    DaemonV3 {
        code: crate::app_core::V3ErrorCode,
        message: String,
    },
    #[error("abbeyd rejected the request ({code}): {message}")]
    Daemon { code: String, message: String },
    #[error("abbeyd connection worker stopped unexpectedly")]
    ConnectWorkerStopped,
}

#[cfg(unix)]
mod unix {
    use std::io::{Read as _, Write as _};
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    pub(super) fn request(
        config: &DaemonConfig,
        command: AppCommand,
        version: u16,
    ) -> Result<AppEvent, ClientError> {
        let expected_event = ExpectedEvent::for_command(&command);
        let request_id = uuid::Uuid::new_v4().to_string();
        let bearer = config.bearer.as_str().to_owned();
        let request = RequestEnvelope {
            version,
            request_id: request_id.clone(),
            bearer,
            command,
        };
        let bytes = round_trip(config, &request)?;
        let response = serde_json::from_slice::<ResponseEnvelope>(&bytes)
            .map_err(|_| ClientError::MalformedResponse)?;
        if response.request_id != request_id {
            return Err(ClientError::RequestIdMismatch);
        }
        if response.version != version {
            if version == CURRENT_PROTOCOL_VERSION
                && response.version == PROTOCOL_VERSION
                && matches!(
                    &response.payload,
                    ResponsePayload::Error { code, .. } if code == "unsupported_version"
                )
            {
                return Err(response_error(response.payload, config));
            }
            return Err(ClientError::ProtocolMismatch {
                expected: version,
                received: response.version,
            });
        }
        match response.payload {
            ResponsePayload::Ok { event } => validate_event(expected_event, event, version),
            payload @ ResponsePayload::Error { .. } => Err(response_error(payload, config)),
        }
    }

    pub(super) fn request_v3(
        config: &DaemonConfig,
        grants: V3CapabilitySet,
        command: V3Command,
    ) -> Result<V3Event, ClientError> {
        grants
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        command
            .validate()
            .map_err(|_| ClientError::InvalidV3Request)?;
        if !grants.permits(&command)
            || (matches!(command, V3Command::Negotiate(_)) && !grants.as_slice().is_empty())
        {
            return Err(ClientError::InvalidV3Request);
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let request = V3RequestEnvelope {
            version: crate::app_core::APP_PROTOCOL_V3,
            schema_version: crate::app_core::APP_SCHEMA_V3,
            request_id: request_id.clone(),
            bearer: config.bearer.as_str().to_owned(),
            grants,
            command,
        };
        let bytes = round_trip(config, &request)?;
        let response = serde_json::from_slice::<V3ResponseEnvelope>(&bytes)
            .map_err(|_| ClientError::MalformedResponse)?;
        if response.request_id != request_id {
            return Err(ClientError::RequestIdMismatch);
        }
        if response.version != crate::app_core::APP_PROTOCOL_V3
            || response.schema_version != crate::app_core::APP_SCHEMA_V3
        {
            return Err(ClientError::InvalidV3Response);
        }
        match response.payload {
            V3ResponsePayload::Ok { event } => {
                event
                    .validate()
                    .map_err(|_| ClientError::InvalidV3Response)?;
                Ok(event)
            }
            V3ResponsePayload::Error { error } => {
                error
                    .validate()
                    .map_err(|_| ClientError::InvalidV3Response)?;
                Err(ClientError::DaemonV3 {
                    code: error.code,
                    message: redact_bearer(error.message, config.bearer.as_str()),
                })
            }
        }
    }

    fn round_trip<T: serde::Serialize>(
        config: &DaemonConfig,
        request: &T,
    ) -> Result<Vec<u8>, ClientError> {
        let bytes = serde_json::to_vec(request).map_err(ClientError::Serialize)?;
        if bytes.is_empty() || bytes.len() > config.max_frame_len {
            return Err(ClientError::RequestTooLarge);
        }
        let length = u32::try_from(bytes.len()).map_err(|_| ClientError::RequestTooLarge)?;
        let mut frame = Vec::with_capacity(4_usize.saturating_add(bytes.len()));
        frame.extend_from_slice(&length.to_be_bytes());
        frame.extend_from_slice(&bytes);
        let mut stream = connect(config, frame)?;
        read_response(&mut stream, config.max_frame_len)
    }

    fn response_error(payload: ResponsePayload, config: &DaemonConfig) -> ClientError {
        let ResponsePayload::Error { code, message } = payload else {
            return ClientError::MalformedResponse;
        };
        ClientError::Daemon {
            code: redact_bearer(code, config.bearer.as_str()),
            message: redact_bearer(message, config.bearer.as_str()),
        }
    }

    #[derive(Clone)]
    enum ExpectedEvent {
        Status,
        Claims,
        RouteAudit {
            limit: u16,
        },
        RunSubmitted {
            idempotency_key: crate::app_core::IdempotencyKey,
            conversation_id: Option<crate::app_core::ConversationId>,
        },
        RunStatus(crate::app_core::RunId),
        Cancellation(crate::app_core::RunId),
        RunEvents {
            run_id: crate::app_core::RunId,
            after_sequence: u64,
            through_sequence: Option<u64>,
            limit: u16,
        },
    }

    impl ExpectedEvent {
        fn for_command(command: &AppCommand) -> Self {
            match command {
                AppCommand::Status => Self::Status,
                AppCommand::Claims(_) => Self::Claims,
                AppCommand::ReadRoutes(query) => Self::RouteAudit { limit: query.limit },
                AppCommand::SubmitRun(request) => Self::RunSubmitted {
                    idempotency_key: request.idempotency_key.clone(),
                    conversation_id: request.conversation_id.clone(),
                },
                AppCommand::GetRun(query) => Self::RunStatus(query.run_id.clone()),
                AppCommand::CancelRun(query) => Self::Cancellation(query.run_id.clone()),
                AppCommand::RunEvents(query) => Self::RunEvents {
                    run_id: query.run_id.clone(),
                    after_sequence: query.after_sequence,
                    through_sequence: query.through_sequence,
                    limit: query.limit,
                },
            }
        }

        fn name(self) -> &'static str {
            match self {
                Self::Status => "status event",
                Self::Claims => "claims event",
                Self::RouteAudit { .. } => "route audit page",
                Self::RunSubmitted { .. } => "run submitted event",
                Self::RunStatus(_) => "run status event",
                Self::Cancellation(_) => "cancellation acknowledgement",
                Self::RunEvents { .. } => "run events page",
            }
        }
    }

    fn validate_event(
        expected: ExpectedEvent,
        event: AppEvent,
        version: u16,
    ) -> Result<AppEvent, ClientError> {
        match (&expected, &event) {
            (ExpectedEvent::Status, AppEvent::Status(status)) => validate_status(status, version)?,
            (ExpectedEvent::Claims, AppEvent::Claims(snapshot)) => validate_claims(snapshot)?,
            (ExpectedEvent::RouteAudit { limit }, AppEvent::RouteAudit(page)) => {
                // `page.validate()` is where the sanitization guarantees become
                // wire invariants: it rejects an absolute path, a control
                // character, or a non-digest workspace in any field. A daemon
                // built from a different revision cannot push a raw `cwd` here.
                page.validate()
                    .map_err(|_| ClientError::InvalidRouteAudit)?;
                if page.limit != *limit || page.returned > *limit {
                    return Err(ClientError::InvalidRouteAudit);
                }
            }
            (
                ExpectedEvent::RunSubmitted {
                    idempotency_key,
                    conversation_id,
                },
                AppEvent::RunSubmitted(submission),
            ) => {
                submission
                    .validate()
                    .map_err(|_| ClientError::InvalidRunResponse)?;
                if &submission.run.idempotency_key != idempotency_key
                    || &submission.run.conversation_id != conversation_id
                {
                    return Err(ClientError::InvalidRunResponse);
                }
            }
            (ExpectedEvent::RunStatus(expected_id), AppEvent::RunStatus(snapshot))
            | (
                ExpectedEvent::Cancellation(expected_id),
                AppEvent::CancellationAcknowledged(snapshot),
            ) => {
                snapshot
                    .validate()
                    .map_err(|_| ClientError::InvalidRunResponse)?;
                if &snapshot.run_id != expected_id {
                    return Err(ClientError::InvalidRunResponse);
                }
            }
            (
                ExpectedEvent::RunEvents {
                    run_id,
                    after_sequence,
                    through_sequence,
                    limit,
                },
                AppEvent::RunEvents(page),
            ) => {
                page.validate()
                    .map_err(|_| ClientError::InvalidRunResponse)?;
                if &page.run_id != run_id
                    || page.after_sequence != *after_sequence
                    || through_sequence.is_some_and(|through| page.through_sequence != through)
                    || page.events.len() > usize::from(*limit)
                {
                    return Err(ClientError::InvalidRunResponse);
                }
            }
            (_, received) => {
                return Err(ClientError::UnexpectedEvent {
                    expected: expected.name(),
                    received: event_name(received),
                });
            }
        }
        Ok(event)
    }

    fn validate_status(status: &RuntimeStatus, version: u16) -> Result<(), ClientError> {
        let (expected_protocol, expected_schema) = match version {
            APP_PROTOCOL_V1 => (APP_PROTOCOL_V1, APP_SCHEMA_V1),
            APP_PROTOCOL_VERSION => (APP_PROTOCOL_VERSION, APP_SCHEMA_VERSION),
            _ => {
                return Err(ClientError::InvalidRuntimeStatus(
                    "unsupported application protocol version",
                ));
            }
        };
        if status.protocol_version != expected_protocol {
            return Err(ClientError::InvalidRuntimeStatus(
                "application protocol version does not match",
            ));
        }
        if status.schema_version != expected_schema {
            return Err(ClientError::InvalidRuntimeStatus(
                "application schema version does not match",
            ));
        }
        status
            .validate()
            .map_err(|_| ClientError::InvalidRuntimeStatus("capability set is invalid"))?;
        if version == APP_PROTOCOL_V1
            && (status.capabilities != CapabilitySet::standard() || !status.run_routes.is_empty())
        {
            return Err(ClientError::InvalidRuntimeStatus(
                "read-only capability set is not supported",
            ));
        }
        if version == APP_PROTOCOL_VERSION
            && status
                .capabilities
                .as_slice()
                .iter()
                .any(|capability| !CapabilitySet::runtime_v2().as_slice().contains(capability))
        {
            return Err(ClientError::InvalidRuntimeStatus(
                "runtime capability set is not supported",
            ));
        }
        Ok(())
    }

    fn validate_claims(snapshot: &ClaimsSnapshot) -> Result<(), ClientError> {
        if snapshot.matched != snapshot.claims.len() {
            return Err(ClientError::InvalidClaimsSnapshot);
        }
        Ok(())
    }

    fn event_name(event: &AppEvent) -> &'static str {
        match event {
            AppEvent::Status(_) => "status event",
            AppEvent::Claims(_) => "claims event",
            AppEvent::RouteAudit(_) => "route audit page",
            AppEvent::ApprovalRequested(_) => "approval request",
            AppEvent::RunSubmitted(_) => "run submitted event",
            AppEvent::RunStatus(_) => "run status event",
            AppEvent::CancellationAcknowledged(_) => "cancellation acknowledgement",
            AppEvent::RunEvents(_) => "run events page",
        }
    }

    fn redact_bearer(value: String, bearer: &str) -> String {
        value.replace(bearer, "[REDACTED]")
    }

    enum ReadyError {
        Connect(io::Error),
        Handoff,
    }

    fn connect(config: &DaemonConfig, frame: Vec<u8>) -> Result<UnixStream, ClientError> {
        let path = config.socket_path.clone();
        let worker_path = path.clone();
        let read_timeout = config.read_timeout;
        let write_timeout = config.write_timeout;
        #[cfg(test)]
        let handoff_barrier = config.client_handoff_barrier.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        // `recv_timeout` strictly bounds the caller. A timed-out worker owns
        // only the socket path, timeout values, and one bounded serialized
        // request frame; it has no client or configuration handle, never
        // formats the frame, and exits after connect/setup/write completes.
        // Local UDS missing-socket behavior is covered by the bounded-connect
        // test.
        thread::spawn(move || {
            let result = UnixStream::connect(worker_path)
                .map_err(ReadyError::Connect)
                .and_then(|mut stream| {
                    // The deterministic race fixture stops here: the kernel
                    // connection exists, but no timeout or request byte has
                    // been configured yet. A peer may close while this worker
                    // is descheduled.
                    #[cfg(test)]
                    if let Some(barrier) = handoff_barrier {
                        barrier.wait();
                    }
                    stream
                        .set_read_timeout(Some(read_timeout))
                        .and_then(|()| stream.set_write_timeout(Some(write_timeout)))
                        .and_then(|()| stream.write_all(&frame))
                        .and_then(|()| stream.flush())
                        .map_err(|_| ReadyError::Handoff)?;
                    Ok(stream)
                });
            let _ = sender.send(result);
        });
        match receiver.recv_timeout(config.read_timeout) {
            Ok(Ok(stream)) => Ok(stream),
            Ok(Err(ReadyError::Connect(source))) => Err(ClientError::Connect { path, source }),
            Ok(Err(ReadyError::Handoff)) => Err(ClientError::ConnectionHandoff),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ClientError::ConnectTimeout { path }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ClientError::ConnectWorkerStopped),
        }
    }

    fn read_response(
        stream: &mut UnixStream,
        max_frame_len: usize,
    ) -> Result<Vec<u8>, ClientError> {
        let mut prefix = [0_u8; 4];
        stream.read_exact(&mut prefix).map_err(ClientError::Read)?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length == 0 {
            return Err(ClientError::EmptyResponse);
        }
        if length > max_frame_len {
            return Err(ClientError::ResponseTooLarge);
        }
        let mut bytes = vec![0_u8; length];
        stream.read_exact(&mut bytes).map_err(ClientError::Read)?;
        Ok(bytes)
    }
}

fn v3_event_name(event: &V3Event) -> &'static str {
    match event {
        V3Event::Negotiated(_) => "capability negotiation",
        V3Event::Tools(_) => "tool inventory",
        V3Event::ToolResult(_) => "tool result",
        V3Event::ToolStatus(_) => "tool status",
        V3Event::ToolApprovalStatus(_) => "tool approval status",
        V3Event::MemorySpaces(_) => "memory spaces",
        V3Event::MemorySearchResults(_) => "memory search results",
        V3Event::MemoryMetadata(_) => "memory metadata",
        V3Event::Models(_) => "model inventory",
        V3Event::ModelStatus(_) => "model status",
        V3Event::TrainingDatasetStatus(_) => "training dataset status",
        V3Event::TrainingStatus(_) => "training status",
        V3Event::TrainingMetrics(_) => "training metrics",
        V3Event::AdapterStatus(_) => "adapter status",
        V3Event::Clusters(_) => "cluster inventory",
        V3Event::Workers(_) => "worker inventory",
        V3Event::WorkerHealth(_) => "worker health",
        V3Event::JobStatus(_) => "job status",
        V3Event::Claim(_) => "claim",
        V3Event::Events(_) => "event page",
        V3Event::Error(_) => "error event",
    }
}

#[cfg(all(test, unix))]
#[path = "client/tests.rs"]
mod tests;
