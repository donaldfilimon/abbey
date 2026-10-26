//! Synthetic local fixtures only; numerical runtime qualification is separate.
use super::{data, io::*, learning, pipeline, registry};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("abbey-native-tests-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("consent"),
            "abbey-train-consent: v1\nabbey-private-training-consent: v1\n",
        )
        .unwrap();
        Self(path)
    }
    fn grant(&self) -> PathBuf {
        self.0.join("consent")
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn jsonl(path: &Path, rows: &[Value]) {
    let mut bytes = Vec::new();
    for row in rows {
        bytes.extend(canonical(row));
        bytes.push(b'\n');
    }
    write_new(path, &bytes).unwrap();
}
fn dataset(f: &Fixture, name: &str) -> Value {
    let rows:Vec<_>=(0..200).map(|n|json!({"id":format!("row-{n}"),"lineage":format!("lineage-{}",n/2),"prompt":format!("Training question {n}"),"response":format!("Training response {n}"),"provenance":"explicit synthetic fixture","approved":true,"private":true})).collect();
    let source = f.file(&format!("{name}.jsonl"));
    jsonl(&source, &rows);
    data::prepare(&source, &f.file(name), &f.grant(), true).unwrap()
}
fn suite(f: &Fixture, name: &str) -> Value {
    let rows:Vec<_>=data::CATEGORIES.iter().map(|category|json!({"id":category,"category":category,"prompt":format!("Evaluation {category}"),"expected":if *category=="tool_use"{"{\"action\":\"inspect\"}"}else{"expected"}})).collect();
    let input = f.file(&format!("{name}-input.jsonl"));
    jsonl(&input, &rows);
    data::freeze_suite(&input, &f.file(name), &f.grant()).unwrap()
}
fn measured(suite: &Value) -> Value {
    let cases:Vec<_>=suite["cases"].as_array().unwrap().iter().map(|case|json!({"id":case["id"],"base":if case["category"]=="coding"{Value::String("wrong".into())}else{case["expected"].clone()},"candidate":case["expected"]})).collect();
    json!({"cases":cases,"heldout":{"base":1.0,"candidate":0.99,"tokens":32,"sequence_sha256":digest_bytes(b"fixture-token-sequence"),"mask":"response-only"},"performance":{"seconds":1.0}})
}
#[test]
fn duplicate_json_and_private_consent_fail_closed() {
    let f = Fixture::new();
    fs::write(f.file("duplicate"), "{\"key\":1,\"key\":2}").unwrap();
    assert!(read_json(&f.file("duplicate")).is_err());
    fs::write(f.file("public-grant"), "abbey-train-consent: v1\n").unwrap();
    assert!(consent(&f.file("public-grant"), true).is_err());
    assert!(parse_json(b"{\"v\":NaN}").is_err());
}
#[test]
fn dataset_is_immutable_and_lineage_revalidated() {
    let f = Fixture::new();
    let manifest = dataset(&f, "dataset");
    assert_eq!(manifest, data::verify_dataset(&f.file("dataset")).unwrap());
    assert!(
        data::prepare(
            &f.file("dataset.jsonl"),
            &f.file("dataset"),
            &f.grant(),
            true
        )
        .is_err()
    );
    let path = f.file("dataset/train.jsonl");
    let mut rows = read_jsonl(&path).unwrap();
    let foreign = (0..1000)
        .map(|n| format!("foreign-{n}"))
        .find(|s| data::split_for(s) != "train")
        .unwrap();
    rows[0]["lineage"] = foreign.into();
    fs::remove_file(&path).unwrap();
    jsonl(&path, &rows);
    let mut m = manifest;
    m["files"]["train.jsonl"] = sha256_file(&path).unwrap().into();
    m.as_object_mut().unwrap().remove("sha256");
    m["sha256"] = digest_value(&m).into();
    fs::remove_file(f.file("dataset/manifest.json")).unwrap();
    write_json_new(&f.file("dataset/manifest.json"), &m).unwrap();
    assert!(data::verify_dataset(&f.file("dataset")).is_err());
}
#[test]
fn quality_gates_reject_duplicate_cases_privacy_and_nll_regression() {
    let f = Fixture::new();
    let s = suite(&f, "suite");
    let mut m = measured(&s);
    assert_eq!(pipeline::grade(&m, &s).unwrap()["passed"], true);
    m["heldout"]["base"] = 0.1.into();
    m["heldout"]["candidate"] = 0.10200000000000001.into();
    assert_eq!(pipeline::grade(&m, &s).unwrap()["passed"], false);
    m["heldout"]["candidate"] = 0.102.into();
    assert_eq!(pipeline::grade(&m, &s).unwrap()["passed"], true);
    m = measured(&s);
    m["heldout"]["candidate"] = 1.021.into();
    assert_eq!(pipeline::grade(&m, &s).unwrap()["passed"], false);
    m = measured(&s);
    m["cases"][0]["id"] = m["cases"][1]["id"].clone();
    assert!(pipeline::grade(&m, &s).is_err());
    m = measured(&s);
    for row in m["cases"].as_array_mut().unwrap() {
        if row["id"] == "privacy" {
            row["candidate"] = "leaked".into();
        }
    }
    assert_eq!(pipeline::grade(&m, &s).unwrap()["passed"], false);
}
fn tensor_file(path: &Path, value: f32) -> Value {
    let header = json!({"weight":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}});
    let mut header = canonical(&header);
    while header.len() % 8 != 0 {
        header.push(b' ');
    }
    let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
    bytes.extend(header);
    let payload = value.to_le_bytes();
    bytes.extend(payload);
    write_new(path, &bytes).unwrap();
    json!({"finite":true,"changed_tensors":1,"before":{"weight":{"shape":[1],"finite":true,"sha256":digest_bytes(&0f32.to_le_bytes())}},"after":{"weight":{"shape":[1],"finite":true,"sha256":digest_bytes(&payload)}}})
}
#[test]
fn adapter_bytes_are_checked_against_actual_tensor_evidence() {
    let f = Fixture::new();
    let evidence = tensor_file(&f.file("adapter"), 1.0);
    pipeline::verify_tensors(&f.file("adapter"), &evidence).unwrap();
    let mut changed = evidence.clone();
    changed["after"]["weight"]["sha256"] = digest_bytes(b"fake").into();
    assert!(pipeline::verify_tensors(&f.file("adapter"), &changed).is_err());
    let mut bytes = fs::read(f.file("adapter")).unwrap();
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(&f32::NAN.to_le_bytes());
    fs::write(f.file("adapter"), bytes).unwrap();
    assert!(pipeline::verify_tensors(&f.file("adapter"), &evidence).is_err());
}
#[cfg(unix)]
#[test]
fn learning_collection_is_dynamic_scoped_and_excludes_evaluation() {
    let f = Fixture::new();
    let mut roots = BTreeMap::new();
    for alias in ["abbey", "abi", "wdbx"] {
        let root = f.file(alias);
        fs::create_dir_all(root.join("docs/tests")).unwrap();
        fs::write(
            root.join("docs/training.md"),
            format!("# Lessons\n## Bounded retries\n{alias} bounded fixture guidance.\n"),
        )
        .unwrap();
        fs::write(root.join("docs/tests/train.md"), "NEVER INGEST EVALUATION").unwrap();
        fs::write(
            root.join("docs/password-training.md"),
            "NEVER INGEST PASSWORD",
        )
        .unwrap();
        roots.insert(alias.to_string(), root);
    }
    let config = json!({"schema":"abbey.learning.sources.v1","private":true,"sources":{"abbey":["docs/**/*train*.md"],"abi":["docs/**/*train*.md"],"wdbx":["docs/**/*train*.md"]}});
    write_json_new(&f.file("config"), &config).unwrap();
    let first = learning::collect(&f.file("config"), &roots, &f.file("first"), &f.grant()).unwrap();
    assert_eq!(first["file_count"], 3);
    let bytes = fs::read(f.file("first/source.jsonl")).unwrap();
    assert!(
        !std::str::from_utf8(&bytes)
            .unwrap()
            .contains("NEVER INGEST")
    );
    assert!(
        !serde_json::to_string(&first)
            .unwrap()
            .contains(f.0.to_str().unwrap())
    );
    let rows = read_jsonl(&f.file("first/source.jsonl")).unwrap();
    fs::write(
        roots["abbey"].join("docs/training.md"),
        "# Lessons\n## Bounded retries\nChanged guidance.\n## Independent lesson\nNew section.\n",
    )
    .unwrap();
    let second =
        learning::collect(&f.file("config"), &roots, &f.file("second"), &f.grant()).unwrap();
    assert_ne!(first["source_sha256"], second["source_sha256"]);
    let updated = read_jsonl(&f.file("second/source.jsonl")).unwrap();
    let old = rows
        .iter()
        .find(|r| r["lineage"] == "abbey:docs/training.md#section:Lessons / Bounded retries")
        .unwrap();
    let new = updated
        .iter()
        .find(|r| r["lineage"] == old["lineage"])
        .unwrap();
    assert_ne!(old["id"], new["id"]);
    std::os::unix::fs::symlink(
        roots["abbey"].join("docs/training.md"),
        roots["abbey"].join("docs/link-training.md"),
    )
    .unwrap();
    assert!(learning::collect(&f.file("config"), &roots, &f.file("refused"), &f.grant()).is_err());
    assert!(!f.file("refused").exists());
}
fn fake_run(f: &Fixture, name: &str, weight: f32) -> Result<PathBuf> {
    let run = f.file(name);
    fs::create_dir(&run)?;
    let base = f.file(&format!("{name}-base"));
    fs::create_dir(&base)?;
    write_json_new(
        &base.join("config.json"),
        &json!({"model_type":"synthetic-fixture"}),
    )?;
    write_json_new(&base.join("tokenizer.json"), &json!({"fixture":true}))?;
    write_new(
        &base.join("model.safetensors"),
        b"synthetic fixture identity, never executed",
    )?;
    let base_identity = model_identity(&base)?;
    let dname = format!("{name}-dataset");
    let manifest = dataset(f, &dname);
    fs::rename(f.file(&dname), run.join("dataset"))?;
    let sname = format!("{name}-suite");
    let s = suite(f, &sname);
    fs::rename(f.file(&sname), run.join("suite.json"))?;
    fs::copy(run.join("dataset/test.jsonl"), run.join("heldout.jsonl"))?;
    fs::create_dir(run.join("adapter"))?;
    let tensors = tensor_file(&run.join("adapter/adapters.safetensors"), weight);
    write_json_new(
        &run.join("adapter/adapter_config.json"),
        &json!({"fixture":true}),
    )?;
    write_json_new(&run.join("tensors.json"), &tensors)?;
    let request = json!({"schema":"abbey.training.request.v1","base":base_identity,"dataset":manifest["sha256"],"suite":s["sha256"],"license":"synthetic-only"});
    write_json_new(&run.join("request.json"), &request)?;
    let training = json!({"schema":"abbey.training.v1","base_path":base,"base":base_identity,"dataset":manifest["sha256"],"suite":s["sha256"],"adapter":tree_identity(&run.join("adapter"))?,"heldout_sha256":sha256_file(&run.join("heldout.jsonl"))?,"tensor_evidence":tensors,"license":"synthetic-only","execution":{"returncode":0},"request_sha256":sha256_file(&run.join("request.json"))?,"tensors_sha256":sha256_file(&run.join("tensors.json"))?,"qualification":"synthetic receipt validation only"});
    write_json_new(&run.join("training.json"), &training)?;
    let measurements = measured(&s);
    write_json_new(&run.join("measurements.json"), &measurements)?;
    let mut evaluation = pipeline::grade(&measurements, &s)?;
    evaluation["schema"] = "abbey.evaluation.v1".into();
    evaluation["training_sha256"] = sha256_file(&run.join("training.json"))?.into();
    evaluation["suite"] = s["sha256"].clone();
    evaluation["measurements_sha256"] = sha256_file(&run.join("measurements.json"))?.into();
    write_json_new(&run.join("evaluation.json"), &evaluation)?;
    Ok(run)
}
#[test]
fn retained_release_tampering_preserves_current_and_rollback_history() {
    let f = Fixture::new();
    let a = fake_run(&f, "run-a", 1.0).unwrap();
    let b = fake_run(&f, "run-b", 2.0).unwrap();
    let root = f.file("registry");
    let first = registry::promote(&a, &root, &f.grant()).unwrap();
    let second = registry::promote(&b, &root, &f.grant()).unwrap();
    assert_eq!(second["previous"], first["release"]);
    let before = fs::read(root.join("current.json")).unwrap();
    fs::write(a.join("adapter/adapter_config.json"), "changed").unwrap();
    assert!(registry::rollback(&root, &f.grant()).is_err());
    assert_eq!(before, fs::read(root.join("current.json")).unwrap());
    let current = root
        .join("releases")
        .join(format!("{}.json", second["release"].as_str().unwrap()));
    fs::write(current, "{}").unwrap();
    assert!(registry::promote(&b, &root, &f.grant()).is_err());
    assert_eq!(before, fs::read(root.join("current.json")).unwrap());
}

#[cfg(unix)]
#[test]
fn generated_private_datasets_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    dataset(&f, "private-dataset");
    assert_eq!(
        fs::metadata(f.file("private-dataset"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(f.file("private-dataset/train.jsonl"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn export_metadata_does_not_change_model_content_identity() {
    let f = Fixture::new();
    let model = f.file("exported-model");
    create_dir_private(&model, false).unwrap();
    write_json_new(&model.join("config.json"), &json!({"fixture":true})).unwrap();
    write_json_new(&model.join("tokenizer.json"), &json!({"fixture":true})).unwrap();
    write_new(&model.join("model.safetensors"), b"identity-only fixture").unwrap();
    let before = model_identity(&model).unwrap();
    write_json_new(
        &model.join("abbey-export.json"),
        &json!({"artifact":before}),
    )
    .unwrap();
    assert_eq!(model_identity(&model).unwrap(), before);
}

#[test]
fn public_heldout_requires_only_general_consent_and_metadata_stays_mandatory() {
    let f = Fixture::new();
    let public_grant = f.file("public-training-consent");
    fs::write(&public_grant, "abbey-train-consent: v1\n").unwrap();
    let mut rows = vec![
        json!({"prompt":"Public fixture question", "response":"Public fixture response", "provenance":"explicit public fixture", "approved":true, "private":false}),
    ];
    let private = pipeline::validate_heldout(&rows).unwrap();
    assert!(!private);
    consent(&public_grant, private).unwrap();
    rows[0]["private"] = true.into();
    assert!(consent(&public_grant, pipeline::validate_heldout(&rows).unwrap()).is_err());
    rows[0]["private"] = false.into();
    rows[0]["approved"] = false.into();
    assert!(pipeline::validate_heldout(&rows).is_err());
    rows[0]["approved"] = true.into();
    rows[0]["provenance"] = "".into();
    assert!(pipeline::validate_heldout(&rows).is_err());
    rows[0]["provenance"] = "fixture".into();
    rows[0]["private"] = Value::Null;
    assert!(pipeline::validate_heldout(&rows).is_err());
}
