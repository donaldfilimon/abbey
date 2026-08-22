//! Data-only qualification for the pinned Program 1 Abbey contract corpus.
//!
//! This module validates bytes and synthetic fixtures. It is deliberately not
//! connected to Abbey's authorization, consent, execution, or memory paths.

use jsonschema::{Draft, Retrieve, Uri};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::{Component, Path};
use thiserror::Error;

mod fixture_validation;
use fixture_validation::validate_wire;

const SOURCE_REPOSITORY: &str = "https://github.com/donaldfilimon/abi";
const SOURCE_REVISION: &str = "348754bdaaf59a40fbb858380f925e0aba95a23b";
const AGGREGATE_DIGEST: &str = "72e241e34967df318376bf68f4a0e2db13f5ebf17d1a219709731f1f470dbe8e";
const CORPUS_DOMAIN: &[u8] = b"abbey-contract-corpus-v1\0";
const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024;
const MAX_CORPUS_BYTES: u64 = 16 * 1024 * 1024;
const JCS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// A closed qualification failure containing only corpus-relative labels.
#[derive(Debug, Error)]
pub enum ContractMismatch {
    /// A bounded corpus artifact could not be read.
    #[error("artifact_unreadable:{path}")]
    ArtifactUnreadable { path: String },
    /// An entry was a symlink, special file, or exceeded its byte bound.
    #[error("artifact_invalid:{path}")]
    ArtifactInvalid { path: String },
    /// JSON was malformed or contained a duplicate member where not permitted.
    #[error("json_invalid:{path}")]
    JsonInvalid { path: String },
    /// The lock or manifest did not have its closed v1 shape.
    #[error("metadata_invalid:{path}")]
    MetadataInvalid { path: String },
    /// The manifest inventory differed from the regular-file inventory.
    #[error("inventory_mismatch:{path}")]
    InventoryMismatch { path: String },
    /// One committed artifact differed in length or SHA-256.
    #[error("artifact_digest_mismatch:{path}")]
    ArtifactDigestMismatch { path: String },
    /// The domain-separated aggregate commitment differed.
    #[error("aggregate_digest_mismatch:{path}")]
    AggregateDigestMismatch { path: String },
    /// A schema could not be compiled using only corpus-local resources.
    #[error("schema_invalid:{path}")]
    SchemaInvalid { path: String },
}

impl ContractMismatch {
    /// Return the stable redacted reason code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ArtifactUnreadable { .. } => "artifact_unreadable",
            Self::ArtifactInvalid { .. } => "artifact_invalid",
            Self::JsonInvalid { .. } => "json_invalid",
            Self::MetadataInvalid { .. } => "metadata_invalid",
            Self::InventoryMismatch { .. } => "inventory_mismatch",
            Self::ArtifactDigestMismatch { .. } => "artifact_digest_mismatch",
            Self::AggregateDigestMismatch { .. } => "aggregate_digest_mismatch",
            Self::SchemaInvalid { .. } => "schema_invalid",
        }
    }
}

/// Entry point for verifying the repository-bundled Program 1 corpus.
pub struct ContractCorpus;

/// A digest-qualified, locally compiled, synthetic fixture corpus.
#[derive(Debug)]
pub struct QualifiedCorpus {
    source_revision: String,
    aggregate_digest: String,
    artifact_count: usize,
    total_bytes: u64,
    schemas: HashMap<String, Value>,
    fixtures: Vec<Fixture>,
}

/// One bounded synthetic fixture from the qualified corpus.
#[derive(Debug)]
pub struct Fixture {
    path: String,
    category: String,
    case_id: String,
    expected: String,
    raw: Vec<u8>,
}

/// The closed native outcome vocabulary for Program 1 fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FixtureDisposition {
    Valid,
    SchemaInvalid,
    NumericDomain,
    CancellationMismatch,
    DegradedAuthority,
    SelfApproval,
    ConsentOpenDenied,
    ConsentCloseRequired,
    MandatoryControlsMissing,
    EvidenceOverclaim,
    IdempotencyRequired,
    DelegationCycle,
    DuplicateMember,
    LearningDisabled,
    ForbiddenContent,
    LearningAuthorityForbidden,
}

impl FixtureDisposition {
    fn code(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::SchemaInvalid => "schema_invalid",
            Self::NumericDomain => "numeric_domain",
            Self::CancellationMismatch => "cancellation_mismatch",
            Self::DegradedAuthority => "degraded_authority",
            Self::SelfApproval => "self_approval",
            Self::ConsentOpenDenied => "consent_open_denied",
            Self::ConsentCloseRequired => "consent_close_required",
            Self::MandatoryControlsMissing => "mandatory_controls_missing",
            Self::EvidenceOverclaim => "evidence_overclaim",
            Self::IdempotencyRequired => "idempotency_required",
            Self::DelegationCycle => "delegation_cycle",
            Self::DuplicateMember => "duplicate_member",
            Self::LearningDisabled => "learning_disabled",
            Self::ForbiddenContent => "forbidden_content",
            Self::LearningAuthorityForbidden => "learning_authority_forbidden",
        }
    }
}

/// Native observation paired with the fixture's declared result.
pub struct FixtureResult {
    disposition: FixtureDisposition,
    expected: String,
    preserved_extensions: Option<Value>,
}

/// Data-only qualification state. No Abbey runtime path consumes this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FederationProfile {
    /// Corpus mismatch permits diagnostics only.
    DiagnosticOnly,
    /// Corpus bytes qualify for a future, separately integrated consequence gate.
    ConsequentiallyQualified,
}

/// Consequential classes that a diagnostic-only profile must block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consequence {
    GrantNegotiation,
    ApprovalDecision,
    ConsentOpen,
    ToolExecution,
    MemoryWrite,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    source_repository: String,
    source_revision: String,
    contract_major: u32,
    contract_revision: u32,
    aggregate_digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    contract_major: u32,
    contract_revision: u32,
    algorithm: String,
    redaction_profile: String,
    artifacts: Vec<ArtifactRow>,
    aggregate_digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRow {
    path: String,
    bytes: u64,
    media_type: String,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureWire {
    case_id: String,
    schema: String,
    expect: String,
    document: Value,
}

#[derive(Clone)]
struct LocalRetriever {
    schemas: HashMap<String, Value>,
}

impl Retrieve for LocalRetriever {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        self.schemas
            .get(uri.as_str())
            .cloned()
            .ok_or_else(|| "external schema resolution disabled".into())
    }
}

impl ContractCorpus {
    /// Qualify the exact corpus bundled with this repository checkout.
    pub fn qualified() -> Result<QualifiedCorpus, ContractMismatch> {
        Self::qualify_at(Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts/abbey"))
    }

    /// Qualify an explicitly selected managed vendor directory.
    pub fn qualify_at(root: impl AsRef<Path>) -> Result<QualifiedCorpus, ContractMismatch> {
        qualify(root.as_ref())
    }
}

impl QualifiedCorpus {
    #[must_use]
    pub fn source_revision(&self) -> &str {
        &self.source_revision
    }

    #[must_use]
    pub fn aggregate_digest(&self) -> &str {
        &self.aggregate_digest
    }

    #[must_use]
    pub fn artifact_count(&self) -> usize {
        self.artifact_count
    }

    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    #[must_use]
    pub fn schema_count(&self) -> usize {
        self.schemas.len()
    }

    #[must_use]
    pub fn fixtures(&self) -> &[Fixture] {
        &self.fixtures
    }

    #[must_use]
    pub fn fixture(&self, case_id: &str) -> Option<&Fixture> {
        self.fixtures
            .iter()
            .find(|fixture| fixture.case_id == case_id)
    }

    #[must_use]
    pub fn validate_fixture(&self, fixture: &Fixture) -> FixtureResult {
        let (disposition, extensions) = match parse_strict(&fixture.raw, &fixture.path) {
            Err(ParseFailure::Duplicate) => (FixtureDisposition::DuplicateMember, None),
            Err(ParseFailure::Invalid) => (FixtureDisposition::SchemaInvalid, None),
            Ok(value) => match serde_json::from_value::<FixtureWire>(value) {
                Err(_) => (FixtureDisposition::SchemaInvalid, None),
                Ok(wire) => {
                    let extensions = wire.document.get("extensions").cloned();
                    (validate_wire(&wire, &self.schemas), extensions)
                }
            },
        };
        FixtureResult {
            disposition,
            expected: fixture.expected.clone(),
            preserved_extensions: extensions,
        }
    }
}

impl Fixture {
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn category(&self) -> &str {
        &self.category
    }
}

impl FixtureResult {
    #[must_use]
    pub fn disposition(&self) -> FixtureDisposition {
        self.disposition
    }

    #[must_use]
    pub fn actual_code(&self) -> &'static str {
        self.disposition.code()
    }

    #[must_use]
    pub fn expected_code(&self) -> &str {
        &self.expected
    }

    #[must_use]
    pub fn matches_expected(&self) -> bool {
        self.actual_code() == self.expected
    }

    #[must_use]
    pub fn preserved_extensions(&self) -> Option<&Value> {
        self.preserved_extensions.as_ref()
    }
}

impl FederationProfile {
    #[must_use]
    pub fn from_corpus(
        qualification: &Result<QualifiedCorpus, ContractMismatch>,
    ) -> FederationProfile {
        if qualification.is_ok() {
            Self::ConsequentiallyQualified
        } else {
            Self::DiagnosticOnly
        }
    }

    /// Return whether this profile fails closed for the supplied consequence.
    #[must_use]
    pub fn blocks(self, _consequence: Consequence) -> bool {
        matches!(self, Self::DiagnosticOnly)
    }
}

fn qualify(root: &Path) -> Result<QualifiedCorpus, ContractMismatch> {
    ensure_directory(root, "vendor")?;
    let lock: Lock = parse_metadata(
        &read_bounded(
            &root.join("abbey-contracts.lock.json"),
            "abbey-contracts.lock.json",
        )?,
        "abbey-contracts.lock.json",
    )?;
    if lock.source_repository != SOURCE_REPOSITORY
        || lock.source_revision != SOURCE_REVISION
        || lock.contract_major != 1
        || lock.contract_revision != 1
        || lock.aggregate_digest != AGGREGATE_DIGEST
    {
        return Err(ContractMismatch::MetadataInvalid {
            path: "abbey-contracts.lock.json".to_owned(),
        });
    }

    let corpus = root.join("corpus");
    ensure_directory(&corpus, "corpus")?;
    let manifest_raw = read_bounded(&corpus.join("manifest.json"), "manifest.json")?;
    let manifest: Manifest = parse_metadata(&manifest_raw, "manifest.json")?;
    if manifest.contract_major != lock.contract_major
        || manifest.contract_revision != lock.contract_revision
        || manifest.algorithm != "abbey-contract-corpus-sha256-v1"
        || manifest.redaction_profile != "abbey-contract-redaction-v1"
        || manifest.aggregate_digest != lock.aggregate_digest
    {
        return Err(ContractMismatch::MetadataInvalid {
            path: "manifest.json".to_owned(),
        });
    }

    let actual = discover(&corpus)?;
    let actual_names: BTreeSet<String> = actual.iter().cloned().collect();
    let mut listed = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for row in &manifest.artifacts {
        validate_relative(&row.path)?;
        if !listed.insert(row.path.clone()) {
            return Err(ContractMismatch::InventoryMismatch {
                path: row.path.clone(),
            });
        }
        let bytes = read_bounded(&corpus.join(&row.path), &row.path)?;
        total_bytes = total_bytes.saturating_add(bytes.len() as u64);
        if row.bytes != bytes.len() as u64 || row.sha256 != sha256_hex(&bytes) {
            return Err(ContractMismatch::ArtifactDigestMismatch {
                path: row.path.clone(),
            });
        }
    }
    if listed != actual_names || total_bytes > MAX_CORPUS_BYTES {
        return Err(ContractMismatch::InventoryMismatch {
            path: "manifest.json".to_owned(),
        });
    }
    if aggregate_digest(&manifest)? != manifest.aggregate_digest {
        return Err(ContractMismatch::AggregateDigestMismatch {
            path: "manifest.json".to_owned(),
        });
    }

    let mut schemas = HashMap::new();
    for row in &manifest.artifacts {
        if let Some(schema_id) = &row.schema_id {
            let bytes = read_bounded(&corpus.join(&row.path), &row.path)?;
            let schema =
                parse_strict(&bytes, &row.path).map_err(|_| ContractMismatch::JsonInvalid {
                    path: row.path.clone(),
                })?;
            if schemas.insert(schema_id.clone(), schema).is_some() {
                return Err(ContractMismatch::SchemaInvalid {
                    path: row.path.clone(),
                });
            }
        }
    }
    for row in &manifest.artifacts {
        if let Some(schema_id) = &row.schema_id {
            compile_schema(
                schemas
                    .get(schema_id)
                    .expect("schema indexed from same rows"),
                &schemas,
            )
            .map_err(|()| ContractMismatch::SchemaInvalid {
                path: row.path.clone(),
            })?;
        }
    }

    let mut fixtures = Vec::new();
    for path in actual.iter().filter(|path| path.contains("/fixtures/")) {
        let raw = read_bounded(&corpus.join(path), path)?;
        let wire: FixtureWire = serde_json::from_slice(&raw)
            .map_err(|_| ContractMismatch::JsonInvalid { path: path.clone() })?;
        let category = path
            .split("/fixtures/")
            .nth(1)
            .and_then(|tail| tail.split('/').next())
            .ok_or_else(|| ContractMismatch::MetadataInvalid { path: path.clone() })?;
        fixtures.push(Fixture {
            path: path.clone(),
            category: category.to_owned(),
            case_id: wire.case_id,
            expected: wire.expect,
            raw,
        });
    }

    Ok(QualifiedCorpus {
        source_revision: lock.source_revision,
        aggregate_digest: manifest.aggregate_digest,
        artifact_count: manifest.artifacts.len(),
        total_bytes,
        schemas,
        fixtures,
    })
}

fn ensure_directory(path: &Path, label: &str) -> Result<(), ContractMismatch> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| ContractMismatch::ArtifactUnreadable {
            path: label.to_owned(),
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ContractMismatch::ArtifactInvalid {
            path: label.to_owned(),
        });
    }
    Ok(())
}

fn read_bounded(path: &Path, label: &str) -> Result<Vec<u8>, ContractMismatch> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| ContractMismatch::ArtifactUnreadable {
            path: label.to_owned(),
        })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_ARTIFACT_BYTES
    {
        return Err(ContractMismatch::ArtifactInvalid {
            path: label.to_owned(),
        });
    }
    fs::read(path).map_err(|_| ContractMismatch::ArtifactUnreadable {
        path: label.to_owned(),
    })
}

fn discover(root: &Path) -> Result<Vec<String>, ContractMismatch> {
    fn visit(
        root: &Path,
        relative: &Path,
        output: &mut Vec<String>,
    ) -> Result<(), ContractMismatch> {
        for entry in
            fs::read_dir(root.join(relative)).map_err(|_| ContractMismatch::ArtifactUnreadable {
                path: "corpus".to_owned(),
            })?
        {
            let entry = entry.map_err(|_| ContractMismatch::ArtifactUnreadable {
                path: "corpus".to_owned(),
            })?;
            let child = relative.join(entry.file_name());
            let label = normalize_relative(&child)?;
            let kind = entry
                .file_type()
                .map_err(|_| ContractMismatch::ArtifactInvalid {
                    path: label.clone(),
                })?;
            if kind.is_symlink() {
                return Err(ContractMismatch::ArtifactInvalid { path: label });
            }
            if kind.is_dir() {
                visit(root, &child, output)?;
            } else if kind.is_file() && child != Path::new("manifest.json") {
                output.push(label);
            } else if !kind.is_file() {
                return Err(ContractMismatch::ArtifactInvalid { path: label });
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, Path::new(""), &mut files)?;
    files.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    Ok(files)
}

fn normalize_relative(path: &Path) -> Result<String, ContractMismatch> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let text = part
                    .to_str()
                    .ok_or_else(|| ContractMismatch::MetadataInvalid {
                        path: "non_utf8".to_owned(),
                    })?;
                if text.contains('\\') {
                    return Err(ContractMismatch::MetadataInvalid {
                        path: "backslash".to_owned(),
                    });
                }
                parts.push(text);
            }
            _ => {
                return Err(ContractMismatch::MetadataInvalid {
                    path: "non_relative".to_owned(),
                });
            }
        }
    }
    Ok(parts.join("/"))
}

fn validate_relative(path: &str) -> Result<(), ContractMismatch> {
    if path.is_empty()
        || path.contains('\\')
        || Path::new(path).is_absolute()
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(ContractMismatch::MetadataInvalid {
            path: "manifest_entry".to_owned(),
        });
    }
    Ok(())
}

fn parse_metadata<T: for<'de> Deserialize<'de>>(
    bytes: &[u8],
    path: &str,
) -> Result<T, ContractMismatch> {
    let value = parse_strict(bytes, path).map_err(|_| ContractMismatch::JsonInvalid {
        path: path.to_owned(),
    })?;
    serde_json::from_value(value).map_err(|_| ContractMismatch::MetadataInvalid {
        path: path.to_owned(),
    })
}

fn aggregate_digest(manifest: &Manifest) -> Result<String, ContractMismatch> {
    let mut zeroed = manifest.clone();
    zeroed.aggregate_digest = "0".repeat(64);
    let mut manifest_bytes =
        serde_json::to_vec_pretty(&zeroed).map_err(|_| ContractMismatch::MetadataInvalid {
            path: "manifest.json".to_owned(),
        })?;
    manifest_bytes.push(b'\n');
    let mut entries: Vec<(String, u64, String)> = manifest
        .artifacts
        .iter()
        .map(|row| (row.path.clone(), row.bytes, row.sha256.clone()))
        .collect();
    entries.push((
        "manifest.json".to_owned(),
        manifest_bytes.len() as u64,
        sha256_hex(&manifest_bytes),
    ));
    entries.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut hasher = Sha256::new();
    hasher.update(CORPUS_DOMAIN);
    for (path, bytes, digest) in entries {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes.to_string().as_bytes());
        hasher.update([0]);
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn compile_schema(
    schema: &Value,
    schemas: &HashMap<String, Value>,
) -> Result<jsonschema::Validator, ()> {
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_retriever(LocalRetriever {
            schemas: schemas.clone(),
        })
        .build(schema)
        .map_err(|_| ())
}

#[derive(Debug, Clone, Copy)]
enum ParseFailure {
    Invalid,
    Duplicate,
}

fn parse_strict(bytes: &[u8], _path: &str) -> Result<Value, ParseFailure> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue
        .deserialize(&mut deserializer)
        .map_err(|error| {
            if error.to_string().contains("duplicate member") {
                ParseFailure::Duplicate
            } else {
                ParseFailure::Invalid
            }
        })?;
    deserializer.end().map_err(|_| ParseFailure::Invalid)?;
    Ok(value)
}

struct StrictValue;

impl<'de> DeserializeSeed<'de> for StrictValue {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate members")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictValue)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut map = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            if map.contains_key(&key) {
                return Err(de::Error::custom("duplicate member"));
            }
            let value = access.next_value_seed(StrictValue)?;
            map.insert(key, value);
        }
        Ok(Value::Object(map))
    }
}
