use crate::io::*;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) const CATEGORIES: [&str; 8] = [
    "coding",
    "tool_use",
    "everyday",
    "factual",
    "personality",
    "privacy",
    "public_refusal",
    "private_knowledge",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Row {
    pub id: String,
    pub lineage: String,
    pub prompt: String,
    pub response: String,
    pub provenance: String,
    pub approved: bool,
    pub private: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub id: String,
    pub category: String,
    pub prompt: String,
    pub expected: String,
}
pub(crate) fn split_for(lineage: &str) -> &'static str {
    let bytes = Sha256::digest(format!("abbey.family.split.v1\0{lineage}").as_bytes());
    let bucket = u64::from_be_bytes(bytes[..8].try_into().expect("8 hash bytes")) % 100;
    if bucket < 80 {
        "train"
    } else if bucket < 90 {
        "valid"
    } else {
        "test"
    }
}
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn redact(text: &str) -> String {
    let mut text = text.to_owned();
    for (pattern, replacement) in [
        (
            r"(?s)-----BEGIN [^-]*PRIVATE KEY-----.*?(?:-----END [^-]*PRIVATE KEY-----|$)",
            "[redacted]",
        ),
        (r"(?:sk-|ghp_|AKIA)[A-Za-z0-9_.~+/=-]{8,}", "[redacted]"),
        (r"(?i)Bearer\s+[A-Za-z0-9_.~+/=-]{8,}", "Bearer [redacted]"),
        (
            r"(?i)\b(key|token|password|secret)=([^\s,;]{8,})",
            "$1=[redacted]",
        ),
    ] {
        text = regex::Regex::new(pattern)
            .expect("static redaction regex")
            .replace_all(&text, replacement)
            .into_owned();
    }
    // Source paths are relative; remove ordinary absolute-path tokens from text.
    text.split_inclusive(char::is_whitespace)
        .map(|word| {
            let trimmed = word.trim_end_matches(char::is_whitespace);
            if trimmed.starts_with('/')
                || (trimmed.len() > 2
                    && trimmed.as_bytes()[1] == b':'
                    && matches!(trimmed.as_bytes()[2], b'/' | b'\\'))
            {
                format!("[path]{}", &word[trimmed.len()..])
            } else {
                word.into()
            }
        })
        .collect()
}
fn validate_rows(rows: &[Row], private: bool) -> Result<()> {
    ensure!(
        !rows.is_empty() && rows.len() <= 100000,
        "nonempty bounded dataset required"
    );
    let mut ids = BTreeSet::new();
    let mut contents = BTreeSet::new();
    let mut prompts = BTreeMap::new();
    for row in rows {
        for text in [
            &row.id,
            &row.lineage,
            &row.prompt,
            &row.response,
            &row.provenance,
        ] {
            checked_text(text)?;
        }
        ensure!(row.approved, "example not approved");
        ensure!(
            !row.private || private,
            "private examples require separate consent"
        );
        ensure!(ids.insert(&row.id), "duplicate example id");
        ensure!(
            contents.insert((normalize(&row.prompt), normalize(&row.response))),
            "duplicate example content"
        );
        let prompt = normalize(&row.prompt);
        if let Some(lineage) = prompts.insert(prompt, &row.lineage) {
            ensure!(lineage == &row.lineage, "duplicate prompt across lineages");
        }
    }
    Ok(())
}
pub(crate) fn prepare(source: &Path, output: &Path, grant: &Path, private: bool) -> Result<Value> {
    consent(grant, private)?;
    absent(output)?;
    let mut rows: Vec<Row> = read_jsonl(source)?
        .into_iter()
        .map(serde_json::from_value)
        .collect::<std::result::Result<_, _>>()?;
    for row in &mut rows {
        row.prompt = redact(&row.prompt);
        row.response = redact(&row.response);
        row.provenance = redact(&row.provenance);
    }
    validate_rows(&rows, private)?;
    let mut groups: BTreeMap<&str, Vec<Row>> = [
        ("train", Vec::new()),
        ("valid", Vec::new()),
        ("test", Vec::new()),
    ]
    .into();
    for row in rows {
        groups
            .get_mut(split_for(&row.lineage))
            .expect("known split")
            .push(row);
    }
    ensure!(
        groups.values().all(|v| !v.is_empty()),
        "all three lineage splits must be nonempty"
    );
    let stage = Stage::new(output)?;
    let mut counts = BTreeMap::new();
    let mut files = BTreeMap::new();
    for (split, items) in &mut groups {
        items.sort_by(|a, b| a.id.cmp(&b.id));
        let mut bytes = Vec::new();
        for row in items.iter() {
            bytes.extend(canonical(&serde_json::to_value(row)?));
            bytes.push(b'\n');
        }
        ensure!(bytes.len() <= MAX_JSON, "split exceeds bound");
        let name = format!("{split}.jsonl");
        write_new(&stage.path.join(&name), &bytes)?;
        counts.insert(*split, items.len());
        files.insert(
            name,
            sha256_file(&stage.path.join(format!("{split}.jsonl")))?,
        );
    }
    let mut manifest = json!({"schema":"abbey.dataset.v1","source_sha256":sha256_file(source)?,"private":private,"counts":counts,"files":files});
    manifest["sha256"] = digest_value(&manifest).into();
    write_json_new(&stage.path.join("manifest.json"), &manifest)?;
    stage.publish(output)?;
    Ok(manifest)
}
pub(crate) fn verify_dataset(path: &Path) -> Result<Value> {
    let manifest = read_json(&path.join("manifest.json"))?;
    let mut unsigned = manifest.clone();
    unsigned
        .as_object_mut()
        .context("dataset object required")?
        .remove("sha256");
    ensure!(
        manifest["schema"] == "abbey.dataset.v1" && manifest["sha256"] == digest_value(&unsigned),
        "dataset manifest identity mismatch"
    );
    let files = manifest["files"]
        .as_object()
        .context("dataset files required")?;
    ensure!(
        files.keys().map(String::as_str).collect::<BTreeSet<_>>()
            == ["train.jsonl", "valid.jsonl", "test.jsonl"].into(),
        "invalid dataset files"
    );
    let private = manifest["private"]
        .as_bool()
        .context("private boolean required")?;
    let mut all = Vec::new();
    for split in ["train", "valid", "test"] {
        let name = format!("{split}.jsonl");
        ensure!(
            manifest["files"][&name] == sha256_file(&path.join(&name))?,
            "dataset file digest mismatch"
        );
        let rows: Vec<Row> = read_jsonl(&path.join(name))?
            .into_iter()
            .map(serde_json::from_value)
            .collect::<std::result::Result<_, _>>()?;
        ensure!(
            !rows.is_empty() && manifest["counts"][split].as_u64() == Some(rows.len() as u64),
            "dataset split count mismatch"
        );
        ensure!(
            rows.iter().all(|r| split_for(&r.lineage) == split),
            "lineage split leakage"
        );
        all.extend(rows);
    }
    validate_rows(&all, private)?;
    Ok(manifest)
}
fn validate_cases(cases: &[Case]) -> Result<()> {
    ensure!(
        !cases.is_empty() && cases.len() <= 1000,
        "suite must have 1..1000 cases"
    );
    let mut ids = BTreeSet::new();
    let mut categories = BTreeSet::new();
    for case in cases {
        for text in [&case.id, &case.category, &case.prompt, &case.expected] {
            checked_text(text)?;
        }
        ensure!(
            case.id.chars().count() <= 128,
            "evaluation IDs exceed 128 characters"
        );
        ensure!(ids.insert(&case.id), "duplicate suite id");
        categories.insert(case.category.as_str());
        if case.category == "tool_use" {
            ensure!(
                parse_json(case.expected.as_bytes())?.is_object(),
                "tool expectation must be an object"
            );
        }
    }
    ensure!(
        categories == CATEGORIES.into(),
        "suite must cover exactly all eight categories"
    );
    Ok(())
}
pub(crate) fn freeze_suite(source: &Path, output: &Path, grant: &Path) -> Result<Value> {
    consent(grant, false)?;
    absent(output)?;
    let cases: Vec<Case> = read_jsonl(source)?
        .into_iter()
        .map(serde_json::from_value)
        .collect::<std::result::Result<_, _>>()?;
    validate_cases(&cases)?;
    let cases = serde_json::to_value(cases)?;
    let result = json!({"schema":"abbey.suite.v1","sha256":digest_value(&cases),"cases":cases,"qualification":"exact-answer engineering suite; no broad assistant-quality claim"});
    if let Some(parent) = output.parent() {
        create_dir_private(parent, true)?;
    }
    write_json_new(output, &result)?;
    Ok(result)
}
pub(crate) fn verify_suite(path: &Path) -> Result<Value> {
    let suite = read_json(path)?;
    ensure!(
        suite["schema"] == "abbey.suite.v1" && suite["sha256"] == digest_value(&suite["cases"]),
        "suite identity mismatch"
    );
    let cases: Vec<Case> = serde_json::from_value(suite["cases"].clone())?;
    validate_cases(&cases)?;
    Ok(suite)
}
