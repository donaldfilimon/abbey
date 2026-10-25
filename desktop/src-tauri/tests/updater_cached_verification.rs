//! Exercises the pinned plugin's public cached verification/install boundary.
//! A valid artifact is only verified. Only tampered bytes are passed to install.
#[path = "fixtures/update-zip/api.rs"]
mod api;

use std::fs;
use tauri_plugin_updater::Error;

fn update(key: &str, signature: &str, version: &str) -> tauri_plugin_updater::Update {
    api::update(key, signature, version, &api::fixture("unused-app/Abbey"))
}

fn key() -> String {
    fs::read_to_string(api::fixture("updater.pubkey")).unwrap()
}

fn signature() -> String {
    fs::read_to_string(api::fixture("plain.zip.sig")).unwrap()
}

#[test]
fn valid_cached_bytes_verify_without_network_or_installation() {
    let bytes = fs::read(api::fixture("plain.zip")).unwrap();
    update(&key(), &signature(), "99.0.0")
        .verify(&bytes)
        .unwrap();
}

#[test]
fn altered_bytes_fail_verification_and_install_before_platform_effects() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("Abbey");
    fs::write(&executable, b"prior installed bytes").unwrap();
    let update = api::update(&key(), &signature(), "99.0.0", &executable);
    let mut bytes = fs::read(api::fixture("plain.zip")).unwrap();
    bytes[0] ^= 1;
    assert!(matches!(update.verify(&bytes), Err(Error::Minisign(_))));
    assert!(matches!(update.install(&bytes), Err(Error::Minisign(_))));
    assert_eq!(fs::read(&executable).unwrap(), b"prior installed bytes");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn invalid_signature_and_public_key_refuse() {
    let bytes = fs::read(api::fixture("plain.zip")).unwrap();
    assert!(matches!(
        update(&key(), "%%%", "99.0.0").verify(&bytes),
        Err(Error::Base64(_))
    ));
    assert!(matches!(
        update("%%%", &signature(), "99.0.0").verify(&bytes),
        Err(Error::Base64(_))
    ));
    // A well-formed signature over another payload must also refuse.
    let other = fs::read_to_string(api::fixture("parent.zip.sig")).unwrap();
    assert!(matches!(
        update(&key(), &other, "99.0.0").verify(&bytes),
        Err(Error::Minisign(_))
    ));
}

#[test]
fn announced_version_must_match_signed_version() {
    let bytes = fs::read(api::fixture("plain.zip")).unwrap();
    assert!(matches!(
        update(&key(), &signature(), "99.0.1").verify(&bytes),
        Err(Error::SignedVersionMismatch { signed, announced })
            if signed == "99.0.0" && announced == "99.0.1"
    ));
}

#[test]
fn cached_release_requires_a_signed_version() {
    let bytes = fs::read(api::fixture("plain.zip")).unwrap();
    let unsigned_version = fs::read_to_string(api::fixture("plain-unversioned.zip.sig")).unwrap();
    assert!(matches!(
        update(&key(), &unsigned_version, "99.0.0").verify(&bytes),
        Err(Error::MissingSignedVersion)
    ));
}
