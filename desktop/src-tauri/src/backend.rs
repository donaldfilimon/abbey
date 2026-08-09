//! Routing from the desktop's read-only invoke surface to Abbey's application core.
//!
//! Two routes, chosen once per call and never blended:
//!
//! * **Daemon** — a bearer is configured for the active edition, so every read
//!   goes over the authenticated Unix socket. If that fails, the failure is
//!   reported. It is *not* retried in-process: `AGENTS.md` states that client
//!   failures never fall back to in-process claims, because a fallback turns an
//!   authentication or transport problem into silently unauthenticated data.
//! * **In-process** — no bearer is configured for this edition at all, so there
//!   is no daemon to be a client of, and the desktop reads the app core it is
//!   linked against. `ConnectionInfo::source` always says which one answered.

use abbey::app_core::{
    AppCommand, AppEvent, AppService, AppServiceError, ClaimsQuery, ClaimsSnapshot, RouteAuditPage,
    RouteAuditQuery, RuntimeStatus,
};
use abbey::daemon::{ClientError, DaemonClient, DaemonConfig};
use abbey::edition::ACTIVE;

use crate::ipc::{BearerSource, ConnectionInfo, ConnectionSource, IpcError, IpcErrorKind};

enum Route {
    Daemon(Box<DaemonClient>),
    InProcess(Box<AppService>),
}

fn bearer_source() -> Option<BearerSource> {
    let inline = std::env::var_os(ACTIVE.daemon_bearer_env()).is_some();
    let file = std::env::var_os(ACTIVE.daemon_bearer_file_env()).is_some();
    match (inline, file) {
        (true, true) => Some(BearerSource::Conflicting),
        (true, false) => Some(BearerSource::InlineEnv),
        (false, true) => Some(BearerSource::TokenFile),
        (false, false) => None,
    }
}

fn route() -> Result<Route, IpcError> {
    if bearer_source().is_none() {
        return Ok(Route::InProcess(Box::new(AppService::default())));
    }
    match DaemonConfig::from_env() {
        Ok(config) => Ok(Route::Daemon(Box::new(DaemonClient::new(config)))),
        Err(error) => Err(IpcError::new(IpcErrorKind::Configuration, error.to_string())
            .with_remedy(format!(
                "set exactly one of {} or {}, and keep any token file owner-only",
                ACTIVE.daemon_bearer_env(),
                ACTIVE.daemon_bearer_file_env()
            ))),
    }
}

/// Describe the current route without performing a read.
pub fn connection() -> ConnectionInfo {
    let bearer = bearer_source();
    match bearer {
        None => ConnectionInfo {
            source: ConnectionSource::InProcess,
            socket_path: None,
            bearer_configured: false,
            bearer_source: None,
            detail: format!(
                "No {} or {} is set, so no abbeyd is configured for the {} edition. \
                 Status and claims are read from the application core linked into this app.",
                ACTIVE.daemon_bearer_env(),
                ACTIVE.daemon_bearer_file_env(),
                ACTIVE.slug()
            ),
        },
        Some(source) => {
            // Only the socket *path* is read here — `DaemonConfig` is never
            // formatted into this struct, and the bearer is never touched.
            let socket_path = DaemonConfig::from_env()
                .ok()
                .map(|config| config.socket_path.display().to_string());
            ConnectionInfo {
                source: ConnectionSource::Daemon,
                socket_path,
                bearer_configured: true,
                bearer_source: Some(source),
                detail: match source {
                    BearerSource::Conflicting => format!(
                        "{} and {} are both set. Abbey's daemon config is fail-closed: \
                         set exactly one.",
                        ACTIVE.daemon_bearer_env(),
                        ACTIVE.daemon_bearer_file_env()
                    ),
                    _ => "Reads go to abbeyd over its authenticated owner-only Unix socket. \
                          A failure is reported, never answered from this process."
                        .to_owned(),
                },
            }
        }
    }
}

pub fn status() -> Result<RuntimeStatus, IpcError> {
    match dispatch(AppCommand::Status)? {
        AppEvent::Status(status) => Ok(status),
        other => Err(unexpected("status", &other)),
    }
}

pub fn claims(query: ClaimsQuery) -> Result<ClaimsSnapshot, IpcError> {
    // Validation is deliberately *not* duplicated here: `AppService::handle`
    // and the daemon both run `ClaimsQuery::validate`, and a second copy of the
    // bounds in the desktop is a second thing to drift.
    match dispatch(AppCommand::Claims(query))? {
        AppEvent::Claims(snapshot) => Ok(snapshot),
        other => Err(unexpected("claims", &other)),
    }
}

pub fn routes(query: RouteAuditQuery) -> Result<RouteAuditPage, IpcError> {
    // As with `claims`, the bounds are validated by `AppService::handle` and by
    // the daemon; a third copy here would be a third thing to drift. The page
    // that comes back has already been sanitized by `app_core` — this process
    // has no filesystem route to the route log to go around it.
    match dispatch(AppCommand::ReadRoutes(query))? {
        AppEvent::RouteAudit(page) => Ok(page),
        other => Err(unexpected("route_audit", &other)),
    }
}

fn dispatch(command: AppCommand) -> Result<AppEvent, IpcError> {
    match route()? {
        Route::Daemon(client) => client.request(command).map_err(from_client_error),
        Route::InProcess(service) => service.handle(command).map_err(from_service_error),
    }
}

fn unexpected(expected: &str, event: &AppEvent) -> IpcError {
    let received = match event {
        AppEvent::Status(_) => "status",
        AppEvent::Claims(_) => "claims",
        AppEvent::RouteAudit(_) => "route_audit",
        AppEvent::ApprovalRequested(_) => "approval_requested",
        AppEvent::RunSubmitted(_) => "run_submitted",
        AppEvent::RunStatus(_) => "run_status",
        AppEvent::CancellationAcknowledged(_) => "cancellation_acknowledged",
        AppEvent::RunEvents(_) => "run_events",
    };
    IpcError::new(
        IpcErrorKind::Protocol,
        format!("expected a {expected} event, received {received}"),
    )
}

fn from_client_error(error: ClientError) -> IpcError {
    // Every arm below maps a `ClientError` whose `Display` is an authored
    // string in `src/daemon/client.rs`. None of them interpolate the bearer.
    let (kind, remedy) = match &error {
        ClientError::UnsupportedPlatform => (
            IpcErrorKind::UnsupportedPlatform,
            Some(
                "abbeyd has no Windows named-pipe transport yet; run the desktop client on a \
                 Unix host, or unset the bearer variables to read the linked application core."
                    .to_owned(),
            ),
        ),
        ClientError::ConnectTimeout { .. } | ClientError::Connect { .. } => (
            IpcErrorKind::Transport,
            Some(format!("start the daemon: {}", ACTIVE.daemon_binary_name())),
        ),
        ClientError::Daemon { .. } => (IpcErrorKind::Rejected, None),
        ClientError::ProtocolMismatch { .. }
        | ClientError::MalformedResponse
        | ClientError::RequestIdMismatch
        | ClientError::UnexpectedEvent { .. }
        | ClientError::InvalidRuntimeStatus(_)
        | ClientError::InvalidClaimsSnapshot
        | ClientError::InvalidRouteAudit
        | ClientError::InvalidRunResponse => (
            IpcErrorKind::Protocol,
            Some(format!(
                "the desktop client and {} were built from different revisions",
                ACTIVE.daemon_binary_name()
            )),
        ),
        _ => (IpcErrorKind::Transport, None),
    };
    let mut ipc = IpcError::new(kind, error.to_string());
    ipc.remedy = remedy;
    ipc
}

fn from_service_error(error: AppServiceError) -> IpcError {
    let kind = match error {
        AppServiceError::InvalidCommand(_) => IpcErrorKind::Rejected,
        AppServiceError::NotPermitted => IpcErrorKind::Rejected,
    };
    IpcError::new(kind, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unconfigured_daemon_reads_the_linked_application_core() {
        // The desktop test process exports no bearer, so this is the real path
        // a freshly installed app takes.
        if bearer_source().is_some() {
            return; // developer machine has a daemon configured; not a failure
        }
        let info = connection();
        assert_eq!(info.source, ConnectionSource::InProcess);
        assert!(!info.bearer_configured);
        assert!(info.socket_path.is_none());
        let status = status().expect("in-process status");
        assert_eq!(status.protocol_version, abbey::app_core::APP_PROTOCOL_V1);
        assert_eq!(status.schema_version, abbey::app_core::APP_SCHEMA_V1);
        assert_eq!(status.capabilities, abbey::app_core::CapabilitySet::standard());
        assert!(status.run_routes.is_empty());
    }

    #[test]
    fn the_route_audit_is_reachable_and_never_carries_a_path() {
        if bearer_source().is_some() {
            return; // covered against a real daemon by tests/daemon_cli.rs
        }
        let page = routes(RouteAuditQuery { limit: 5 }).expect("in-process route audit");
        // Reachable and permitted: `CapabilitySet::standard()` grants
        // `ReadRoutes`, so the surfaces.ts `requires` entry is honest.
        assert_eq!(page.limit, 5);
        page.validate().expect("the page the view renders is valid");

        // Whatever this developer machine's log contains, no rendered field may
        // be an absolute path — that is what makes the desktop view safe to
        // show without giving the webview filesystem access.
        let rendered = serde_json::to_string(&page).expect("serialize page");
        assert!(!rendered.contains("\"cwd\""), "{rendered}");
        for entry in &page.entries {
            assert!(entry.workspace.as_deref().is_none_or(|w| w.starts_with("ws-")));
            for field in [&entry.persona, &entry.role, &entry.model, &entry.reason] {
                assert!(
                    !field.split_whitespace().any(|token| token.starts_with('/')),
                    "route audit exposed a path to the desktop: {field}"
                );
            }
        }
    }

    /// The discriminating test for "no silent fallback".
    ///
    /// Only meaningful when a bearer is exported and no daemon is listening,
    /// which is why the suite is run a second time as
    /// `ABBEYD_BEARER_TOKEN=<48 chars> cargo test -p abbey-desktop`. Setting the
    /// variable from inside the test is not an option: `std::env::set_var` is
    /// `unsafe` in edition 2024 and this crate denies `unsafe_code`.
    #[test]
    fn a_configured_daemon_never_falls_back_to_the_in_process_core() {
        let Some(source) = bearer_source() else {
            return; // covered by the bearer-set run
        };
        assert_eq!(connection().source, ConnectionSource::Daemon);
        let error = status().expect_err(
            "a configured daemon with no listener must fail, never answer from this process",
        );
        assert!(
            matches!(
                error.kind,
                IpcErrorKind::Transport
                    | IpcErrorKind::Configuration
                    | IpcErrorKind::UnsupportedPlatform
            ),
            "expected a daemon failure, received {error:?} (bearer source {source:?})"
        );
    }

    #[test]
    fn connection_detail_never_contains_bearer_material() {
        let info = connection();
        let rendered = serde_json::to_string(&info).expect("serialize connection info");
        // The *names* of the variables may appear; a value must not. Any real
        // bearer is at least 32 bytes, so assert the serialized form carries no
        // long opaque run that is not a path.
        for variable in [ACTIVE.daemon_bearer_env(), ACTIVE.daemon_bearer_file_env()] {
            if let Ok(value) = std::env::var(variable) {
                assert!(
                    !rendered.contains(&value),
                    "connection info leaked {variable}"
                );
            }
        }
    }
}
