use super::*;

fn fixture_config() -> DaemonConfig {
    DaemonConfig::local(
        "/absent/socket",
        abbey::daemon::BearerSecret::parse(b"0123456789abcdef0123456789abcdef").unwrap(),
    )
}
#[test]
fn resolution_precedence_is_fail_closed_and_resolves_once() {
    for (inline, file, expected) in [
        (true, false, BearerSource::InlineEnv),
        (false, true, BearerSource::TokenFile),
        (true, true, BearerSource::Conflicting),
    ] {
        let resolved = resolve_with(
            inline,
            file,
            false,
            || Ok(fixture_config()),
            || panic!("explicit source must win"),
        )
        .unwrap();
        assert!(matches!(resolved,ResolvedConnection::Daemon { source,.. } if source==expected));
        assert!(
            resolve_with(
                inline,
                file,
                false,
                || Err(configuration_error()),
                || panic!("invalid explicit source must not discover")
            )
            .is_err()
        );
    }
    assert!(matches!(
        resolve_with(
            false,
            false,
            true,
            || panic!("socket only"),
            || panic!("socket only")
        )
        .unwrap(),
        ResolvedConnection::InProcess
    ));
    assert!(matches!(
        resolve_with(
            false,
            false,
            false,
            || panic!("no explicit source"),
            || Ok(None)
        )
        .unwrap(),
        ResolvedConnection::InProcess
    ));
    assert!(matches!(
        resolve_with(
            false,
            false,
            false,
            || panic!("no explicit source"),
            || Ok(Some(fixture_config()))
        )
        .unwrap(),
        ResolvedConnection::Daemon {
            source: BearerSource::InstalledService,
            ..
        }
    ));
    assert_eq!(
        resolve_with(
            false,
            false,
            false,
            || panic!("no explicit source"),
            || Err(configuration_error())
        )
        .err()
        .unwrap()
        .kind,
        IpcErrorKind::Configuration
    );
}
#[test]
fn discovered_offline_route_is_transport_failure_never_fallback() {
    let resolved = ResolvedConnection::Daemon {
        config: fixture_config(),
        source: BearerSource::InstalledService,
    };
    assert_eq!(
        resolved.into_route().err().unwrap().kind,
        IpcErrorKind::Transport
    );
}
#[test]
fn peer_edition_must_match_active_identity() {
    let (expected, other) = match ACTIVE {
        abbey::edition::Edition::Safe => (
            abbey::app_core::Edition::Standard,
            abbey::app_core::Edition::Personal,
        ),
        _ => (
            abbey::app_core::Edition::Personal,
            abbey::app_core::Edition::Standard,
        ),
    };
    validate_peer_edition(expected).unwrap();
    assert_eq!(
        validate_peer_edition(other).unwrap_err().kind,
        IpcErrorKind::Protocol
    );
}
#[test]
fn configuration_errors_and_debug_do_not_expose_credentials() {
    let rendered = format!("{:?} {:?}", fixture_config(), configuration_error());
    assert!(!rendered.contains("0123456789abcdef0123456789abcdef"));
    assert!(!rendered.contains("bearer-file-path"));
}

#[cfg(unix)]
#[test]
fn scratch_authenticated_discovered_clients_bind_edition_and_fail_closed() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Handler {
        other: bool,
        calls: Arc<AtomicUsize>,
    }
    impl abbey::daemon::ReadOnlyHandler for Handler {
        fn handle(&self, command: AppCommand) -> Result<AppEvent, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut event = AppService::default()
                .handle(command)
                .map_err(|e| e.to_string())?;
            if self.other
                && let AppEvent::Status(status) = &mut event
            {
                status.edition = match status.edition {
                    abbey::app_core::Edition::Standard => abbey::app_core::Edition::Personal,
                    _ => abbey::app_core::Edition::Standard,
                };
            }
            Ok(event)
        }
    }
    for other in [false, true] {
        let root = std::path::Path::new("/tmp")
            .canonicalize()
            .unwrap()
            .join(format!("ad-d-{}-{}", std::process::id(), other));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut config = fixture_config();
        config.socket_path = root.join("s");
        config.accept_poll_interval = std::time::Duration::from_millis(5);
        let calls = Arc::new(AtomicUsize::new(0));
        let handler = Handler {
            other,
            calls: calls.clone(),
        };
        let shutdown = abbey::daemon::Shutdown::default();
        let stop = shutdown.clone();
        let server_config = config.clone();
        let server = std::thread::spawn(move || {
            abbey::daemon::DaemonServer::new(server_config, handler).serve(stop)
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !config.socket_path.exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let resolved = ResolvedConnection::Daemon {
            config: config.clone(),
            source: BearerSource::InstalledService,
        }
        .into_route();
        let checked = if other {
            resolved
                .err()
                .map(|e| e.kind == IpcErrorKind::Protocol)
                .unwrap_or(false)
                && calls.load(Ordering::SeqCst) == 1
        } else {
            match resolved {
                Ok(Route::Daemon(client)) => matches!(
                    client.request(AppCommand::Claims(ClaimsQuery::default())),
                    Ok(AppEvent::Claims(_))
                ),
                _ => false,
            }
        };
        let mut wrong = config;
        wrong.bearer =
            abbey::daemon::BearerSecret::parse(b"ffffffffffffffffffffffffffffffff").unwrap();
        let before = calls.load(Ordering::SeqCst);
        let denied = ResolvedConnection::Daemon {
            config: wrong,
            source: BearerSource::InstalledService,
        }
        .into_route()
        .is_err();
        let untouched = calls.load(Ordering::SeqCst) == before;
        shutdown.request();
        server.join().unwrap().unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(checked, "authenticated edition/claims check");
        assert!(denied && untouched, "wrong bearer must not reach handler");
    }
}

#[test]
fn discovered_connection_metadata_does_not_connect_or_expose_a_secret() {
    let info = connection_metadata(ResolvedConnection::Daemon {
        config: fixture_config(),
        source: BearerSource::InstalledService,
    });
    assert_eq!(info.source, ConnectionSource::Daemon);
    assert_eq!(info.bearer_source, Some(BearerSource::InstalledService));
    assert!(info.detail.contains("does not establish"));
    let json = serde_json::to_string(&info).unwrap();
    assert!(!json.contains("0123456789abcdef0123456789abcdef"));
    assert!(!json.contains("bearer-file-path"));
    // The intentionally absent socket still returns metadata successfully.
    assert_eq!(info.socket_path.as_deref(), Some("/absent/socket"));
}
