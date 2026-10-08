//! Process-level proof for `abbey learn distill` / `abbey learn sft` against
//! stub `fm` and `ollama` executables on an isolated PATH. No real model,
//! network, or Private Cloud Compute call is ever made here.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_abbey");

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "abbey-distill-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).expect("create scratch");
        Self(dir)
    }

    fn stub(&self, name: &str, body: &str) {
        let path = self.0.join("bin").join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o700);
        std::fs::set_permissions(&path, perm).unwrap();
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        self.run_with_backend(args, "sqlite")
    }

    fn run_with_backend(&self, args: &[&str], backend: &str) -> (i32, String, String) {
        let out = Command::new(BIN)
            .args(args)
            .env_clear()
            .env(abbey::edition::ACTIVE.state_dir_env(), &self.0)
            .env("ABBEY_MEMORY_BACKEND", backend)
            .env("HOME", &self.0)
            .env("PATH", self.0.join("bin"))
            .env("ABBEY_TEST_HOME_AGENTS_ONLY", "1")
            .output()
            .expect("spawn abbey");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `fm`: the on-device `system` model is unavailable, `pcc` answers.
const FM: &str = r#"case "$*" in
  *"--model pcc"*) echo "pcc says: $(eval echo \${$#})" ;;
  *) echo "system model unavailable" >&2; exit 3 ;;
esac"#;

/// `ollama`: lists the default model and answers `run`.
const OLLAMA: &str = r#"case "$1" in
  list) printf 'NAME ID SIZE MODIFIED\ngemma4:26b-mlx abc 17GB now\n' ;;
  run) echo "local says: $(eval echo \${$#})" ;;
  *) exit 9 ;;
esac"#;

fn sft_lines(s: &Scratch) -> Vec<serde_json::Value> {
    let (code, out, err) = s.run(&["learn", "sft"]);
    assert_eq!(code, 0, "sft export failed: {err}");
    out.lines()
        .map(|l| serde_json::from_str(l).expect("sft line is JSON"))
        .collect()
}

#[test]
fn default_chain_falls_through_to_the_local_model_before_pcc() {
    let s = Scratch::new("local-first");
    s.stub("fm", FM);
    s.stub("ollama", OLLAMA);

    let (code, out, err) = s.run(&["learn", "distill", "what is swift"]);
    assert_eq!(code, 0, "distill failed: {err}");
    assert!(out.contains("ollama:gemma4:26b-mlx"), "stdout: {out}");
    assert!(
        !out.contains("fm:pcc"),
        "pcc must not be asked once local answered"
    );
    assert!(
        err.contains("teacher fm:system"),
        "the fm miss is reported: {err}"
    );

    let lines = sft_lines(&s);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["messages"][0]["content"], "what is swift");
    assert_eq!(
        lines[0]["messages"][1]["content"],
        "local says: what is swift"
    );
    assert_eq!(lines[0]["on_device"], true);
}

#[test]
fn explicit_pcc_teacher_is_marked_off_device() {
    let s = Scratch::new("pcc");
    s.stub("fm", FM);

    let (code, out, err) = s.run(&["learn", "distill", "--teacher", "pcc", "hello"]);
    assert_eq!(code, 0, "distill failed: {err}");
    assert!(out.contains("fm:pcc"), "stdout: {out}");
    let lines = sft_lines(&s);
    assert_eq!(lines[0]["teacher"], "fm:pcc");
    assert_eq!(lines[0]["on_device"], false);
}

#[test]
fn no_teacher_available_fails_closed_and_stores_nothing() {
    let s = Scratch::new("none");
    let (code, _, err) = s.run(&["learn", "distill", "--teacher", "local", "hello"]);
    assert_eq!(code, 1, "an unanswered distill must exit non-zero: {err}");
    assert!(err.contains("0 train_candidate record(s) stored"), "{err}");
    assert!(sft_lines(&s).is_empty());
}

#[test]
fn ollama_teacher_never_pulls_a_missing_model() {
    let s = Scratch::new("no-pull");
    // `list` shows nothing; `run` would leave a marker if it were ever called.
    let marker = s.0.join("pulled");
    s.stub(
        "ollama",
        &format!(
            "case \"$1\" in list) echo NAME ;; *) touch {} ;; esac",
            marker.display()
        ),
    );
    let (code, _, err) = s.run(&["learn", "distill", "--teacher", "ollama", "hi"]);
    assert_eq!(code, 1);
    assert!(err.contains("not pulled locally"), "{err}");
    assert!(
        !Path::new(&marker).exists(),
        "ollama run must not be invoked"
    );
}

const LARGE_SFT_COUNT: usize = 10_037;

// Test-local persisted record fixture; Abbey's memory/distill modules are private.
#[derive(serde::Serialize)]
struct SyntheticSftRecord {
    id: String,
    source_type: String,
    source_ref: String,
    project: String,
    timestamp: String,
    origin: String,
    payload: String,
    summary: String,
    tags: Vec<String>,
    embedding_ref: Option<String>,
    confidence: f32,
    provenance: String,
    retention: String,
    supersedes: Option<String>,
    classification: String,
    obsolete: bool,
}

fn synthetic_sft_record(index: usize) -> SyntheticSftRecord {
    let on_device = index.is_multiple_of(2);
    let pair = serde_json::json!({
        "prompt": format!("synthetic prompt {index}\nsecond line"),
        "response": format!("synthetic response {index}"),
        "teacher": if on_device { "fm:system" } else { "fm:pcc" },
        "on_device": on_device,
    });
    SyntheticSftRecord {
        id: format!("synthetic-{index:05}"),
        source_type: "distill".into(),
        source_ref: "offline synthetic fixture".into(),
        project: String::new(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        origin: "test".into(),
        payload: serde_json::to_string(&pair).unwrap(),
        summary: "synthetic distill pair".into(),
        tags: vec!["train_candidate".into()],
        embedding_ref: None,
        confidence: 0.9,
        provenance: "offline synthetic teacher fixture".into(),
        retention: "train_candidate".into(),
        supersedes: None,
        classification: "internal".into(),
        obsolete: false,
    }
}

fn large_sft_fixture() -> Vec<SyntheticSftRecord> {
    let mut rows: Vec<_> = (0..LARGE_SFT_COUNT).map(synthetic_sft_record).collect();
    // Newer exclusions also consume the old candidate limit when their
    // retention matches, ensuring the regression checks filtering after scan.
    for (offset, retention) in ["stm", "ltm", "activity"].into_iter().enumerate() {
        let mut row = synthetic_sft_record(LARGE_SFT_COUNT + offset);
        row.timestamp = "2026-01-02T00:00:00Z".into();
        row.retention = retention.into();
        rows.push(row);
    }
    let mut wrong_type = synthetic_sft_record(LARGE_SFT_COUNT + 3);
    wrong_type.timestamp = "2026-01-02T00:00:00Z".into();
    wrong_type.source_type = "correction".into();
    rows.push(wrong_type);
    let mut malformed = synthetic_sft_record(LARGE_SFT_COUNT + 4);
    malformed.timestamp = "2026-01-02T00:00:00Z".into();
    malformed.payload = "not a distill pair".into();
    rows.push(malformed);
    let mut obsolete = synthetic_sft_record(LARGE_SFT_COUNT + 5);
    obsolete.timestamp = "2026-01-02T00:00:00Z".into();
    obsolete.obsolete = true;
    rows.push(obsolete);
    rows
}

fn seed_sqlite_sft(s: &Scratch, rows: &[SyntheticSftRecord]) {
    // The existing CLI initializes the schema inside this empty scratch state.
    let (code, out, err) = s.run_with_backend(&["learn", "sft"], "sqlite");
    assert_eq!(code, 0, "initialize fixture schema: {err}");
    assert!(out.is_empty(), "fresh fixture must have no exported rows");
    let path = s.0.join("memory.sqlite");
    let mut conn = rusqlite::Connection::open(path).unwrap();
    let tx = conn.transaction().unwrap();
    {
        // One transaction avoids a durable write per synthetic candidate.
        let mut insert = tx
            .prepare(
                "INSERT INTO memory (id, source_type, source_ref, project, timestamp,
                origin, payload, summary, tags_json, confidence, provenance,
                retention, classification, obsolete)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )
            .unwrap();
        for row in rows {
            insert
                .execute(rusqlite::params![
                    row.id,
                    row.source_type,
                    row.source_ref,
                    row.project,
                    row.timestamp,
                    row.origin,
                    row.payload,
                    row.summary,
                    serde_json::to_string(&row.tags).unwrap(),
                    row.confidence,
                    row.provenance,
                    row.retention,
                    row.classification,
                    row.obsolete,
                ])
                .unwrap();
        }
    }
    tx.commit().unwrap();
}

#[cfg(feature = "wdbx")]
fn seed_wdbx_sft(s: &Scratch, rows: &[SyntheticSftRecord]) {
    let dir = s.0.join("wdbx");
    std::fs::create_dir_all(&dir).unwrap();
    let mut snapshot = abi_wdbx::Snapshot::new();
    for row in rows {
        snapshot.kv.insert(
            format!("mem/{}", row.id),
            serde_json::to_string(row).unwrap(),
        );
    }
    snapshot.recount();
    // Publish one complete test-owned checkpoint before any reader opens it,
    // rather than issuing 10,000 independently synced WAL puts.
    abi_wdbx::wal::checkpoint(&abi_wdbx::StorePaths::new(dir), &snapshot).unwrap();
}

fn assert_large_sft_export(s: &Scratch, backend: &str) {
    let (code, out, err) = s.run_with_backend(&["learn", "sft"], backend);
    assert_eq!(code, 0, "synthetic sft export failed: {err}");
    let mut seen = std::collections::HashSet::new();
    for line in out.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("one JSON value per line");
        let id = value["id"].as_str().expect("exported record id");
        let index: usize = id.strip_prefix("synthetic-").unwrap().parse().unwrap();
        assert!(index < LARGE_SFT_COUNT, "excluded record exported: {id}");
        assert!(seen.insert(index), "duplicate record exported: {id}");
        assert_eq!(value.as_object().unwrap().len(), 4, "JSONL fields for {id}");
        assert_eq!(value["messages"].as_array().unwrap().len(), 2);
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][1]["role"], "assistant");
        assert_eq!(
            value["messages"][0]["content"],
            format!("synthetic prompt {index}\nsecond line")
        );
        assert_eq!(
            value["messages"][1]["content"],
            format!("synthetic response {index}")
        );
        let on_device = index.is_multiple_of(2);
        assert_eq!(
            value["teacher"],
            if on_device { "fm:system" } else { "fm:pcc" }
        );
        assert_eq!(value["on_device"], on_device);
    }
    assert_eq!(
        seen.len(),
        LARGE_SFT_COUNT,
        "every live valid pair must export"
    );
}

#[test]
fn sqlite_sft_exports_all_candidates_beyond_ten_thousand() {
    let s = Scratch::new("large-sqlite");
    seed_sqlite_sft(&s, &large_sft_fixture());
    assert_large_sft_export(&s, "sqlite");
}

#[cfg(feature = "wdbx")]
#[test]
fn wdbx_sft_exports_all_candidates_beyond_ten_thousand() {
    let s = Scratch::new("large-wdbx");
    seed_wdbx_sft(&s, &large_sft_fixture());
    assert_large_sft_export(&s, "wdbx");
    assert!(
        !s.0.join("memory.sqlite").exists(),
        "WDBX regression must not silently use SQLite"
    );
}
