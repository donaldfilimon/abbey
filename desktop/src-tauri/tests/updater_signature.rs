// Signature component and shipped-key checks. The companion
// updater_cached_verification suite exercises the actual public plugin API.
//
// `tauri-plugin-updater` verifies a downloaded artifact like this (updater.rs
// 2.14.0, `verify_signature`):
//
//     let pub_key_decoded = base64_to_string(pub_key)?;   // the pinned pubkey
//     let public_key = PublicKey::decode(&pub_key_decoded)?;
//     let signature_base64_decoded = base64_to_string(release_signature)?;
//     let signature = Signature::decode(&signature_base64_decoded)?;
//     public_key.verify(data, &signature, true)?;
//     verify_signed_version(signature.trusted_comment(), announced_version, …)
//
// This test runs that same path with the same crate (`minisign-verify`, already
// in the dependency graph through the updater plugin) so a mistake in what the
// desktop PINS or what the release pipeline SIGNS is caught here rather than by
// a user whose update silently fails to verify.
//
// Three properties are checked:
//
//   1. the key pinned in `tauri.conf.json` is a real minisign public key;
//   2. a real Tauri-signed fixture verifies against its own key, and its
//      trusted comment carries the version/file binding the plugin reads;
//   3. OPTIONALLY — when `ABBEY_DESKTOP_UPDATE_ARTIFACT` points at a built
//      updater artifact, that artifact verifies against the SHIPPED pinned key.
//      The release workflow sets it, so a release can never publish an artifact
//      the shipped client would reject. The path is read relative to the
//      crate root (desktop/src-tauri), so a bundle produced by
//      `release-macos.sh` — which writes into the workspace's shared target
//      dir — is reached as
//      `../../target/release/bundle/macos/<Product>.app.tar.gz`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use minisign_verify::{PublicKey, Signature};
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_DIR: &str = "tests/fixtures/update-signature";
const CONFIG_PATH: &str = "tauri.conf.json";

/// The environment variable that points at a real built updater artifact.
const ARTIFACT_ENV: &str = "ABBEY_DESKTOP_UPDATE_ARTIFACT";

fn fixture(name: &str) -> PathBuf {
    Path::new(FIXTURE_DIR).join(name)
}

/// The pinned `plugins.updater.pubkey`, straight out of the shipped config.
fn shipped_pubkey() -> String {
    let raw = fs::read_to_string(CONFIG_PATH).expect("tauri.conf.json is readable");
    let config: serde_json::Value =
        serde_json::from_str(&raw).expect("tauri.conf.json is valid JSON");
    config
        .get("plugins")
        .and_then(|plugins| plugins.get("updater"))
        .and_then(|updater| updater.get("pubkey"))
        .and_then(|pubkey| pubkey.as_str())
        .expect("tauri.conf.json declares plugins.updater.pubkey")
        .to_owned()
}

/// Decode one base64 text into the minisign document it carries — the exact
/// single layer of base64 the plugin decodes.
fn decode_document(encoded: &str, what: &str) -> String {
    let bytes = BASE64
        .decode(encoded.trim())
        .unwrap_or_else(|error| panic!("{what} is base64: {error}"));
    String::from_utf8(bytes).unwrap_or_else(|error| panic!("{what} is UTF-8: {error}"))
}

/// The version the signature was made for, read exactly as the plugin reads it:
/// the trusted comment's tab-separated `version:` field, untrimmed.
fn signed_version(signature: &Signature) -> Option<String> {
    signature.trusted_comment().split('\t').find_map(|field| {
        field
            .strip_prefix("version:")
            .map(|version| version.to_owned())
    })
}

/// The artifact name the signature covers, read the same way.
fn signed_file(signature: &Signature) -> Option<String> {
    signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("file:").map(|file| file.to_owned()))
}

#[test]
fn the_shipped_updater_pin_is_a_real_minisign_public_key() {
    let pubkey = shipped_pubkey();
    assert!(
        !pubkey.trim().is_empty(),
        "the shipped updater pubkey is empty — no artifact would ever verify"
    );
    let document = decode_document(&pubkey, "the shipped updater pubkey");
    assert!(
        document.starts_with("untrusted comment: minisign public key"),
        "the shipped updater pubkey does not look like a minisign public key file: {document:?}"
    );
    assert!(
        document.lines().any(|line| line.starts_with("RW")),
        "the shipped updater pubkey carries no minisign key line"
    );
    // The crate must accept it — this is the same parse the plugin performs.
    PublicKey::decode(&document)
        .expect("the shipped updater pubkey parses as a minisign public key");
}

#[test]
fn the_signed_fixture_verifies_and_carries_its_version_binding() {
    let artifact = fs::read(fixture("Abbey.app.tar.gz")).expect("the fixture artifact is readable");
    let signature_text = fs::read_to_string(fixture("Abbey.app.tar.gz.sig"))
        .expect("the fixture signature is readable");
    let pubkey_text =
        fs::read_to_string(fixture("updater.pubkey")).expect("the fixture public key is readable");

    // The `.sig` file the Tauri CLI writes is base64 of the minisign text, and
    // the plugin decodes exactly one layer before parsing it.
    let signature_document = decode_document(&signature_text, "the fixture signature");
    let pubkey_document = decode_document(&pubkey_text, "the fixture public key");

    let public_key = PublicKey::decode(&pubkey_document).expect("the fixture public key parses");
    let signature = Signature::decode(&signature_document).expect("the fixture signature parses");

    public_key
        .verify(&artifact, &signature, true)
        .expect("the fixture signature verifies against the fixture public key");

    assert_eq!(
        signed_file(&signature).as_deref(),
        Some("Abbey.app.tar.gz"),
        "the signature does not name the artifact it covers"
    );
    assert_eq!(
        signed_version(&signature).as_deref(),
        Some("2.6.0"),
        "the signature does not record the version it was made for"
    );
}

#[test]
fn a_built_artifact_verifies_against_the_shipped_pin() {
    let Ok(path) = std::env::var(ARTIFACT_ENV) else {
        eprintln!(
            "{ARTIFACT_ENV} is not set — the built-artifact verification is opt-in \
             (the release workflow sets it; locally, point it at \
             ../../target/release/bundle/macos/<Product>.app.tar.gz)"
        );
        return;
    };
    let artifact_path = PathBuf::from(&path);
    assert!(
        artifact_path.is_file(),
        "{ARTIFACT_ENV} points at {path}, which is not a file"
    );
    let artifact = fs::read(&artifact_path).expect("the built artifact is readable");

    // The signature sits next to the artifact, exactly as the bundler writes it.
    // Built by string append, not `set_extension`: the artifact name ends in
    // `.tar.gz`, and replacing the last extension would produce
    // `…tar.tar.gz.sig`.
    let signature_path = PathBuf::from(format!("{}.sig", artifact_path.display()));
    let signature_text = fs::read_to_string(&signature_path).unwrap_or_else(|error| {
        panic!(
            "the signature beside {path} is not readable ({error}); the bundler must \
             produce <artifact>.sig next to every updater artifact"
        )
    });

    let public_key = PublicKey::decode(&decode_document(
        &shipped_pubkey(),
        "the shipped updater pubkey",
    ))
    .expect("the shipped updater pubkey parses");
    let signature = Signature::decode(&decode_document(
        &signature_text,
        "the built artifact signature",
    ))
    .expect("the built artifact signature parses");

    public_key
        .verify(&artifact, &signature, true)
        .unwrap_or_else(|error| {
            panic!(
                "the built artifact {path} does NOT verify against the pinned updater key \
                 ({error}) — the client would reject this update"
            )
        });

    let version = signed_version(&signature).unwrap_or_else(|| {
        panic!(
            "the built artifact {path} was signed without a version; requireSignedVersion \
             is enabled, so the client would reject this update"
        )
    });
    assert!(
        !version.trim().is_empty(),
        "the built artifact {path} carries an empty signed version"
    );
    eprintln!("verified {path} against the shipped updater pin (signed version {version})");
}
