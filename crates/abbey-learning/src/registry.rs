use crate::{io::*, pipeline};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};
fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn lock(root: &Path) -> Result<File> {
    use fs4::fs_std::FileExt;
    create_dir_private(root, true)?;
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .create(true)
        .write(true)
        .truncate(false)
        .open(root.join(".registry.lock"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if file.try_lock_exclusive()? {
            return Ok(file);
        }
        ensure!(
            Instant::now() < deadline,
            "registry is busy; pointer preserved"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(crate) fn release_record(run: &Path) -> Result<Value> {
    let run = run.canonicalize()?;
    let training = pipeline::verify_training(&run)?;
    let evaluation = pipeline::validated_evaluation(&run, &training)?;
    ensure!(
        evaluation["passed"] == true,
        "quality gate failed; current release preserved"
    );
    Ok(
        json!({"schema":"abbey.release.v1","run":run,"training_sha256":sha256_file(&run.join("training.json"))?,"evaluation_sha256":sha256_file(&run.join("evaluation.json"))?,"base_sha256":training["base"]["sha256"],"adapter_sha256":training["adapter"]["sha256"],"qualification":evaluation["qualification"]}),
    )
}
fn retained(root: &Path, id: &str) -> Result<Value> {
    ensure!(valid_id(id), "invalid retained release id");
    let record = read_json(&root.join("releases").join(format!("{id}.json")))?;
    ensure!(
        digest_value(&record) == id,
        "retained release record corrupt"
    );
    let run = Path::new(record["run"].as_str().context("retained run required")?);
    ensure!(
        release_record(run)? == record,
        "retained release identity changed"
    );
    Ok(record)
}
fn pointer(root: &Path) -> Result<Option<Value>> {
    let path = root.join("current.json");
    if !path.exists() {
        return Ok(None);
    }
    let value = read_json(&path)?;
    ensure!(
        value["release"].as_str().is_some_and(valid_id),
        "invalid current release pointer"
    );
    ensure!(
        value["previous"].is_null() || value["previous"].as_str().is_some_and(valid_id),
        "invalid previous release pointer"
    );
    Ok(Some(value))
}
pub(crate) fn promote(run: &Path, root: &Path, grant: &Path) -> Result<Value> {
    consent(grant, false)?;
    let record = release_record(run)?;
    let id = digest_value(&record);
    let _lock = lock(root)?;
    create_dir_private(&root.join("releases"), true)?;
    let path = root.join("releases").join(format!("{id}.json"));
    if path.exists() {
        ensure!(
            read_json(&path)? == record && retained(root, &id)? == record,
            "retained release corrupt; pointer preserved"
        );
    } else {
        write_json_new(&path, &record)?;
    }
    let current = pointer(root)?;
    if let Some(current) = &current {
        retained(
            root,
            current["release"]
                .as_str()
                .context("current release id required")?,
        )?;
        if current["release"] == id {
            return Ok(current.clone());
        }
    }
    let result = json!({"release":id,"previous":current.as_ref().map(|p|p["release"].clone())});
    atomic_json(&root.join("current.json"), &result)?;
    Ok(result)
}
pub(crate) fn rollback(root: &Path, grant: &Path) -> Result<Value> {
    consent(grant, false)?;
    let _lock = lock(root)?;
    let current = pointer(root)?.context("no current release")?;
    let previous = current["previous"]
        .as_str()
        .context("no previous release")?;
    retained(root, previous)?;
    let result = json!({"release":previous,"previous":current["release"]});
    atomic_json(&root.join("current.json"), &result)?;
    Ok(result)
}
pub(crate) fn compact_receipt(run: &Path) -> Result<Value> {
    let training = pipeline::verify_training(run)?;
    let mut result = json!({"schema":"abbey.model.receipt.v1","training_sha256":sha256_file(&run.join("training.json"))?,"base_sha256":training["base"]["sha256"],"adapter_sha256":training["adapter"]["sha256"],"dataset_sha256":training["dataset"],"suite_sha256":training["suite"],"qualification":training["qualification"]});
    if run.join("evaluation.json").exists() {
        let evaluation = pipeline::validated_evaluation(run, &training)?;
        result["evaluation_sha256"] = sha256_file(&run.join("evaluation.json"))?.into();
        result["passed"] = evaluation["passed"].clone();
    }
    Ok(result)
}
