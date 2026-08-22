use abbey::abbey_contracts::{Consequence, ContractCorpus, FederationProfile, FixtureDisposition};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const ABI_REVISION: &str = "348754bdaaf59a40fbb858380f925e0aba95a23b";
const AGGREGATE_DIGEST: &str = "72e241e34967df318376bf68f4a0e2db13f5ebf17d1a219709731f1f470dbe8e";

#[test]
fn bundled_corpus_qualifies_exact_bytes_and_every_declared_fixture() {
    let corpus = ContractCorpus::qualified().expect("the pinned bundled corpus qualifies");

    assert_eq!(corpus.source_revision(), ABI_REVISION);
    assert_eq!(corpus.aggregate_digest(), AGGREGATE_DIGEST);
    assert_eq!(corpus.artifact_count(), 81);
    assert_eq!(corpus.total_bytes(), 88_328);
    assert_eq!(corpus.fixtures().len(), 52);
    assert_eq!(corpus.schema_count(), 27);

    let mut categories = BTreeSet::new();
    let mut outcomes = BTreeSet::new();
    for fixture in corpus.fixtures() {
        let result = corpus.validate_fixture(fixture);
        assert!(
            result.matches_expected(),
            "{} expected {}, observed {}",
            fixture.path(),
            result.expected_code(),
            result.actual_code()
        );
        categories.insert(fixture.category());
        outcomes.insert(result.actual_code());
    }

    assert_eq!(
        categories,
        BTreeSet::from([
            "boundary",
            "cancellation",
            "degraded",
            "invalid",
            "privacy",
            "unknown-field",
            "valid",
        ])
    );
    for required in [
        "valid",
        "schema_invalid",
        "numeric_domain",
        "forbidden_content",
        "cancellation_mismatch",
        "degraded_authority",
        "learning_authority_forbidden",
    ] {
        assert!(outcomes.contains(required), "missing outcome {required}");
    }
}

#[test]
fn tolerant_metadata_extensions_are_preserved_but_authority_unknowns_are_rejected() {
    let corpus = ContractCorpus::qualified().expect("the pinned bundled corpus qualifies");

    let extension = corpus
        .fixture("execution_metadata_extension_preserved")
        .expect("extension fixture exists");
    let result = corpus.validate_fixture(extension);
    assert_eq!(result.disposition(), FixtureDisposition::Valid);
    assert_eq!(
        result.preserved_extensions(),
        Some(&json!({"future_counter": 7, "future_flag": true}))
    );

    let authority_unknown = corpus
        .fixture("authorization_unknown_authority_field_denied")
        .expect("strict authority fixture exists");
    assert_eq!(
        corpus.validate_fixture(authority_unknown).disposition(),
        FixtureDisposition::SchemaInvalid
    );
}

#[test]
fn a_byte_mismatch_degrades_to_diagnostics_and_blocks_every_consequence() {
    let scratch = ScratchCorpus::copy_from_bundled();
    let artifact = scratch
        .root
        .join("corpus/v1/fixtures/valid/consent-operator-flow.json");
    fs::write(&artifact, b"{}\n").expect("mutate the isolated corpus byte");

    let qualification = ContractCorpus::qualify_at(&scratch.root);
    let error = qualification
        .as_ref()
        .expect_err("changed bytes must not qualify");
    assert_eq!(error.code(), "artifact_digest_mismatch");
    assert!(
        !error
            .to_string()
            .contains(scratch.root.to_string_lossy().as_ref())
    );

    let profile = FederationProfile::from_corpus(&qualification);
    assert_eq!(profile, FederationProfile::DiagnosticOnly);
    for consequence in [
        Consequence::GrantNegotiation,
        Consequence::ApprovalDecision,
        Consequence::ConsentOpen,
        Consequence::ToolExecution,
        Consequence::MemoryWrite,
    ] {
        assert!(profile.blocks(consequence));
    }
}

struct ScratchCorpus {
    root: PathBuf,
}

impl ScratchCorpus {
    fn copy_from_bundled() -> Self {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts/abbey");
        let root = std::env::temp_dir().join(format!(
            "abbey-contracts-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        copy_tree(&source, &root);
        Self { root }
    }
}

impl Drop for ScratchCorpus {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove isolated corpus copy");
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir(destination).expect("create destination directory");
    for entry in fs::read_dir(source).expect("read source directory") {
        let entry = entry.expect("read source entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("read source type");
        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path);
        } else {
            assert!(
                file_type.is_file(),
                "test corpus must contain regular files"
            );
            fs::copy(source_path, destination_path).expect("copy corpus artifact");
        }
    }
}
