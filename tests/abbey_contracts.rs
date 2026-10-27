use abbey::abbey_contracts::{Consequence, ContractCorpus, FederationProfile, FixtureDisposition};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const ABI_REVISION: &str = "63e6d6a79d0b8745a652803887d07665245ddb39";
const AGGREGATE_DIGEST: &str = "3ffd487bdc497b7ce54b8c29978a3686dcbffdb66a85957a0ee4f99ba576cdfd";

#[test]
fn bundled_corpus_qualifies_exact_bytes_and_every_declared_fixture() {
    let corpus = ContractCorpus::qualified().expect("the pinned bundled corpus qualifies");

    assert_eq!(corpus.source_revision(), ABI_REVISION);
    assert_eq!(corpus.aggregate_digest(), AGGREGATE_DIGEST);
    assert_eq!(corpus.artifact_count(), 113);
    assert_eq!(corpus.total_bytes(), 133_200);
    assert_eq!(corpus.fixtures().len(), 70);
    assert_eq!(corpus.schema_count(), 40);

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

#[test]
#[ignore = "corpus-patterns: awaits anchored upstream corpus (index Task 2 a)"]
fn bundled_corpus_rejects_unanchored_digest_prefixes_and_suffixes() {
    let corpus_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts/abbey/corpus");
    let mut schema_files = Vec::new();
    collect_schema_files(&corpus_root, &mut schema_files);
    schema_files.sort();
    assert_eq!(schema_files.len(), 40, "expected every vendored schema");

    let mut pattern_count = 0;
    let mut unanchored = Vec::new();
    for path in &schema_files {
        let bytes = fs::read(path).expect("read schema");
        let schema: serde_json::Value = serde_json::from_slice(&bytes).expect("parse schema");
        let mut patterns = Vec::new();
        collect_patterns(&schema, &mut patterns);
        pattern_count += patterns.len();
        let relative = path
            .strip_prefix(&corpus_root)
            .expect("schema lies under the corpus root")
            .display();
        for pattern in patterns {
            if !pattern.starts_with('^') || !pattern.ends_with('$') {
                unanchored.push(format!("{relative}: {pattern}"));
            }
        }
    }
    assert!(pattern_count > 0, "the walk must find at least one pattern");

    let mut accepted_junk = Vec::new();
    for version in ["v1", "v2"] {
        let path = corpus_root.join(format!("{version}/schemas/common/definitions.schema.json"));
        let definitions: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("read definitions"))
                .expect("parse definitions");
        let pattern = definitions["$defs"]["digest_id"]["pattern"]
            .as_str()
            .expect("digest_id declares a string pattern");
        let validator = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build(&json!({"type": "string", "pattern": pattern}))
            .expect("digest_id pattern compiles");
        let digits = "a".repeat(64);
        assert!(
            validator.is_valid(&json!(format!("sha256:{digits}"))),
            "{version} digest_id must accept a well-formed digest"
        );
        if validator.is_valid(&json!(format!("junksha256:{digits}suffix"))) {
            accepted_junk.push(format!(
                "{version} digest_id accepts junk prefix/suffix: {pattern}"
            ));
        }
    }

    assert!(
        unanchored.is_empty() && accepted_junk.is_empty(),
        "{} unanchored schema patterns:\n{}\n{} digest_id patterns accept junk:\n{}",
        unanchored.len(),
        unanchored.join("\n"),
        accepted_junk.len(),
        accepted_junk.join("\n")
    );
}

fn collect_schema_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read corpus directory") {
        let path = entry.expect("read corpus entry").path();
        if path.is_dir() {
            collect_schema_files(&path, files);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".schema.json"))
        {
            files.push(path);
        }
    }
}

fn collect_patterns(value: &serde_json::Value, patterns: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                match child.as_str() {
                    Some(pattern) if key == "pattern" => patterns.push(pattern.to_owned()),
                    _ => collect_patterns(child, patterns),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_patterns(item, patterns);
            }
        }
        _ => {}
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
