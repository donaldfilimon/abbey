use super::*;
use std::os::unix::fs::PermissionsExt;
fn fixture() -> PathBuf {
    let root = Path::new("/tmp").canonicalize().unwrap().join(format!(
        "ad-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    root
}
fn configured(root: &Path, edition: Edition) -> InstalledServicePaths {
    let p = InstalledServicePaths::for_home(edition, root);
    std::fs::create_dir_all(&p.config).unwrap();
    std::fs::set_permissions(&p.config, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(p.config.join("bearer"), b"0123456789abcdef0123456789abcdef").unwrap();
    std::fs::set_permissions(
        p.config.join("bearer"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    p
}
#[test]
fn retained_configuration_is_configured_without_a_socket() {
    let root = fixture();
    for &edition in Edition::ALL {
        let p = configured(&root, edition);
        let config = DaemonConfig::from_installed_service(&p)
            .unwrap()
            .expect("retained config");
        assert_eq!(config.socket_path, p.socket);
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn absent_other_edition_is_ignored() {
    let root = fixture();
    configured(&root, Edition::Personal);
    assert!(
        DaemonConfig::from_installed_service(&InstalledServicePaths::for_home(
            Edition::Safe,
            &root
        ))
        .unwrap()
        .is_none()
    );
    std::fs::remove_dir_all(root).unwrap();
}
fn reject_unchanged(root: &Path, p: &InstalledServicePaths) {
    let token = p.config.join("bearer");
    let before = std::fs::symlink_metadata(&token)
        .ok()
        .map(|m| m.permissions().mode());
    let bytes = std::fs::symlink_metadata(&token)
        .ok()
        .filter(|m| m.is_file())
        .map(|_| std::fs::read(&token).unwrap());
    let error = DaemonConfig::from_installed_service(p).expect_err("invalid config");
    assert_eq!(
        error.to_string(),
        "local service configuration is invalid or cannot be inspected"
    );
    assert!(!format!("{error:?}").contains(&root.display().to_string()));
    assert_eq!(
        before,
        std::fs::symlink_metadata(&token)
            .ok()
            .map(|m| m.permissions().mode())
    );
    if let Some(bytes) = bytes {
        assert_eq!(bytes, std::fs::read(token).unwrap());
    }
}
#[test]
fn missing_token_and_non_directory_configuration_fail_closed() {
    let root = fixture();
    let p = configured(&root, Edition::Safe);
    std::fs::remove_file(p.config.join("bearer")).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_dir(&p.config).unwrap();
    std::fs::write(&p.config, b"not a directory").unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn malformed_oversized_and_public_credentials_fail_without_writes() {
    let root = fixture();
    let p = configured(&root, Edition::Safe);
    let token = p.config.join("bearer");
    for bytes in [
        vec![b'a'; 4099],
        vec![b'a'; 31],
        vec![0xff; 32],
        vec![b'\t'; 32],
    ] {
        std::fs::write(&token, bytes).unwrap();
        reject_unchanged(&root, &p);
    }
    std::fs::write(&token, vec![b'a'; 32]).unwrap();
    for mode in [0o640, 0o604, 0o660] {
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(mode)).unwrap();
        reject_unchanged(&root, &p);
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn token_symlink_directory_and_fifo_are_refused_without_reading() {
    use std::os::unix::fs::symlink;
    let root = fixture();
    let p = configured(&root, Edition::Safe);
    let token = p.config.join("bearer");
    std::fs::remove_file(&token).unwrap();
    symlink(root.join("missing"), &token).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_file(&token).unwrap();
    std::fs::create_dir(&token).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_dir(&token).unwrap();
    nix::unistd::mkfifo(&token, nix::sys::stat::Mode::S_IRUSR).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn symlinked_managed_root_and_ancestor_are_refused() {
    use std::os::unix::fs::symlink;
    let root = fixture();
    let p = configured(&root, Edition::Safe);
    let moved = root.join("moved");
    std::fs::rename(&p.config, &moved).unwrap();
    symlink(&moved, &p.config).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_file(&p.config).unwrap();
    std::fs::rename(&moved, &p.config).unwrap();
    std::fs::rename(root.join(".config"), &moved).unwrap();
    symlink(&moved, root.join(".config")).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn unrelated_absent_home_layout_does_not_block_unconfigured_users() {
    let root = fixture();
    std::os::unix::fs::symlink(root.join("missing"), root.join(".config")).unwrap();
    assert!(
        DaemonConfig::from_installed_service(&InstalledServicePaths::for_home(
            Edition::Safe,
            &root
        ))
        .unwrap()
        .is_none()
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn private_roots_and_unsafe_ancestors_are_refused() {
    let root = fixture();
    let p = configured(&root, Edition::Safe);
    std::fs::set_permissions(&p.config, std::fs::Permissions::from_mode(0o750)).unwrap();
    reject_unchanged(&root, &p);
    std::fs::set_permissions(&p.config, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(root.join(".config"), std::fs::Permissions::from_mode(0o777)).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn existing_socket_requires_private_socket_type_and_private_state() {
    let root = fixture();
    let p = configured(&root, Edition::Safe);
    let daemon = p.socket.parent().unwrap();
    let state = daemon.parent().unwrap();
    std::fs::create_dir_all(daemon).unwrap();
    for dir in [state, daemon] {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(&p.socket, b"not socket").unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_file(&p.socket).unwrap();
    std::os::unix::fs::symlink(root.join("missing-socket"), &p.socket).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_file(&p.socket).unwrap();
    let _listener = std::os::unix::net::UnixListener::bind(&p.socket).unwrap();
    std::fs::set_permissions(&p.socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(DaemonConfig::from_installed_service(&p).unwrap().is_some());
    std::fs::set_permissions(&p.socket, std::fs::Permissions::from_mode(0o660)).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_file(&p.socket).unwrap();
    std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o755)).unwrap();
    reject_unchanged(&root, &p);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn fixed_paths_bind_edition_and_ignore_environment_overrides() {
    let root = Path::new("/example/home");
    let safe = InstalledServicePaths::for_home(Edition::Safe, root);
    let personal = InstalledServicePaths::for_home(Edition::Personal, root);
    assert_eq!(safe.config, root.join(".config/abbey-user-service"));
    assert_eq!(
        safe.socket,
        root.join(".local/state/abbey/daemon/abbeyd.sock")
    );
    assert_eq!(
        personal.config,
        root.join(".config/abbey-personal-user-service")
    );
    assert_eq!(
        personal.socket,
        root.join(".local/state/abbey-personal/daemon/abbey-personal-daemon.sock")
    );
}
#[cfg(not(target_os = "linux"))]
#[test]
fn production_discovery_is_disabled_on_this_host() {
    assert!(
        DaemonConfig::discover_installed_service()
            .unwrap()
            .is_none()
    );
}
#[test]
fn fixed_layout_credentials_authenticate_real_scratch_daemon_reads() {
    use crate::app_core::{AppCommand, AppEvent, AppService, ClaimsQuery};
    use crate::daemon::{DaemonClient, DaemonServer, Shutdown};
    let root = fixture();
    let p = configured(&root, crate::edition::ACTIVE);
    let mut config = DaemonConfig::from_installed_service(&p).unwrap().unwrap();
    std::fs::create_dir_all(p.socket.parent().unwrap()).unwrap();
    for dir in [
        p.socket.parent().unwrap(),
        p.socket.parent().unwrap().parent().unwrap(),
    ] {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    config.accept_poll_interval = std::time::Duration::from_millis(5);
    let shutdown = Shutdown::default();
    let stop = shutdown.clone();
    let server =
        std::thread::spawn(move || DaemonServer::new(config, AppService::default()).serve(stop));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !p.socket.exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let found = DaemonConfig::from_installed_service(&p).unwrap().unwrap();
    let client = DaemonClient::new(found);
    let status = client.request(AppCommand::Status);
    let claims = client.request(AppCommand::Claims(ClaimsQuery::default()));
    shutdown.request();
    server.join().unwrap().unwrap();
    assert!(matches!(status, Ok(AppEvent::Status(_))));
    assert!(matches!(claims, Ok(AppEvent::Claims(_))));
    // Credentials survive daemon stop, still selecting daemon configuration.
    assert!(DaemonConfig::from_installed_service(&p).unwrap().is_some());
    assert_eq!(
        std::fs::read(p.config.join("bearer")).unwrap(),
        b"0123456789abcdef0123456789abcdef"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn control_characters_in_recognized_paths_fail_without_mutation() {
    let root = fixture();
    let home = root.join("invalid\nhome");
    std::fs::create_dir(&home).unwrap();
    let paths = configured(&home, Edition::Safe);
    reject_unchanged(&root, &paths);
    std::fs::remove_dir_all(root).unwrap();
}
