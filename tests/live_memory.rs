use std::path::PathBuf;
use std::process::{Command, Output};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("abbey-live-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        std::fs::write(p.join("consent"), "abbey-memory-consent: v1\n").unwrap();
        Self(p)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_abbey"))
            .env_clear()
            .env("HOME", &self.0)
            .env("ABBEY_CONFIG", self.0.join("config"))
            .env("ABBEY_PERSONAL_CONFIG", self.0.join("config"))
            .env(abbey::edition::ACTIVE.state_dir_env(), self.0.join("state"))
            .args(["knowledge"])
            .args(args)
            .args([
                "--project",
                self.0.to_str().unwrap(),
                "--consent",
                self.0.join("consent").to_str().unwrap(),
            ])
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> serde_json::Value {
        let o = self.run(args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn restart_correct_forget_and_project_scope() {
    let f = Fixture::new();
    let r = f.ok(&[
        "remember",
        "Juniper blue",
        "--source",
        "operator",
        "--owner",
        "tester",
    ]);
    let id = r["id"].as_str().unwrap();
    assert_eq!(
        f.ok(&["recall", "Juniper"])["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let r = f.ok(&["correct", id, "Juniper green"]);
    assert!(
        f.ok(&["recall", "blue"])["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let id = r["id"].as_str().unwrap();
    f.ok(&["forget", id]);
    assert!(f.ok(&["export"])["results"].as_array().unwrap().is_empty());
}
#[test]
fn remote_ask_refused_before_any_store() {
    let f = Fixture::new();
    let o = f.run(&["ask", "Juniper", "--route", "grok", "--model", "test"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("local"));
    assert!(!f.0.join("state/memory.sqlite").exists());
    let cloud = f.run(&["ask", "Juniper", "--model", "disguised:cloud"]);
    assert!(!cloud.status.success());
    assert!(String::from_utf8_lossy(&cloud.stderr).contains("local"));
    assert!(!f.0.join("state/memory.sqlite").exists());
}
#[test]
fn foreign_project_and_unapproved_data_are_excluded() {
    let f = Fixture::new();
    let r = f.ok(&[
        "remember", "violet", "--source", "operator", "--owner", "tester",
    ]);
    let foreign = Fixture::new();
    let o = Command::new(env!("CARGO_BIN_EXE_abbey"))
        .env_clear()
        .env("HOME", &f.0)
        .env("ABBEY_CONFIG", f.0.join("config"))
        .env("ABBEY_PERSONAL_CONFIG", f.0.join("config"))
        .env(abbey::edition::ACTIVE.state_dir_env(), f.0.join("state"))
        .args([
            "knowledge",
            "forget",
            r["id"].as_str().unwrap(),
            "--project",
            foreign.0.to_str().unwrap(),
            "--consent",
            foreign.0.join("consent").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert_eq!(
        f.ok(&["recall", "violet"])["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    std::fs::write(f.0.join("consent"), "abbey-train-consent: v1\n").unwrap();
    assert!(
        !f.run(&[
            "remember",
            "no approval",
            "--source",
            "operator",
            "--owner",
            "tester"
        ])
        .status
        .success()
    );
}
#[cfg(feature = "wdbx")]
#[test]
fn exact_wdbx_restart_and_deletion() {
    let f = Fixture::new();
    std::fs::write(f.0.join("config"), "memory_backend = 'wdbx'\n").unwrap();
    let r = f.ok(&[
        "remember",
        "WDBX violet",
        "--source",
        "operator",
        "--owner",
        "tester",
    ]);
    assert_eq!(
        f.ok(&["recall", "violet"])["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    f.ok(&["forget", r["id"].as_str().unwrap()]);
    assert!(
        f.ok(&["recall", "violet"])["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!f.0.join("state/memory.sqlite").exists());
}
#[cfg(not(feature = "wdbx"))]
#[test]
fn unavailable_backend_never_falls_back() {
    let f = Fixture::new();
    std::fs::write(f.0.join("config"), "memory_backend = 'wdbx'\n").unwrap();
    assert!(
        !f.run(&[
            "remember", "violet", "--source", "operator", "--owner", "tester"
        ])
        .status
        .success()
    );
    assert!(!f.0.join("state/memory.sqlite").exists());
}
#[test]
fn expired_and_low_confidence_are_not_recalled() {
    let f = Fixture::new();
    let r = f.ok(&[
        "remember", "amber", "--source", "operator", "--owner", "tester",
    ]);
    let id = r["id"].as_str().unwrap();
    {
        let db = rusqlite::Connection::open(f.0.join("state/memory.sqlite")).unwrap();
        db.execute("UPDATE memory SET confidence=0.8 WHERE id=?1", [id])
            .unwrap();
    }
    assert!(
        f.ok(&["recall", "amber"])["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    {
        let db = rusqlite::Connection::open(f.0.join("state/memory.sqlite")).unwrap();
        db.execute("UPDATE memory SET confidence=1.0, tags_json='[\"expires:2000-01-01T00:00:00Z\"]' WHERE id=?1",[id]).unwrap();
    }
    assert!(f.ok(&["export"])["results"].as_array().unwrap().is_empty());
    f.ok(&["forget", id]);
}
#[test]
fn live_mutation_waits_for_shared_command_lock() {
    use fs4::fs_std::FileExt;
    let f = Fixture::new();
    f.ok(&[
        "remember", "guarded", "--source", "operator", "--owner", "tester",
    ]);
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(f.0.join("state/live-memory-command.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    std::thread::scope(|scope| {
        let (send, recv) = std::sync::mpsc::channel();
        let fixture = &f;
        scope.spawn(move || {
            send.send(fixture.run(&[
                "remember",
                "serialized",
                "--source",
                "operator",
                "--owner",
                "tester",
            ]))
            .unwrap();
        });
        assert!(
            recv.recv_timeout(std::time::Duration::from_millis(250))
                .is_err(),
            "mutation ignored shared command lock"
        );
        lock.unlock().unwrap();
        let output = recv
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    });
}
#[test]
fn expired_records_count_toward_capture_cap_and_remain_forgettable() {
    let f = Fixture::new();
    let r = f.ok(&[
        "remember",
        "expired amber",
        "--source",
        "operator",
        "--owner",
        "tester",
    ]);
    let id = r["id"].as_str().unwrap();
    {
        let db = rusqlite::Connection::open(f.0.join("state/memory.sqlite")).unwrap();
        db.execute(
            "UPDATE memory SET tags_json='[\"expires:2000-01-01T00:00:00Z\"]'",
            [],
        )
        .unwrap();
        db.execute("WITH RECURSIVE numbers(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM numbers WHERE n<999) INSERT INTO memory SELECT 'expired-'||n,source_type,source_ref,timestamp,origin,payload,summary,tags_json,embedding_ref,confidence,provenance,retention,supersedes,classification,obsolete,project FROM memory,numbers WHERE id=?1",[id]).unwrap();
    }
    assert!(
        !f.run(&[
            "remember",
            "new amber",
            "--source",
            "operator",
            "--owner",
            "tester"
        ])
        .status
        .success()
    );
    assert!(f.ok(&["export"])["results"].as_array().unwrap().is_empty());
    f.ok(&["forget", id]);
    f.ok(&[
        "remember",
        "new amber",
        "--source",
        "operator",
        "--owner",
        "tester",
    ]);
}
#[test]
fn concurrent_correction_and_forget_cannot_revive_after_successful_forget() {
    let f = Fixture::new();
    for _ in 0..8 {
        let r = f.ok(&[
            "remember",
            "race amber",
            "--source",
            "operator",
            "--owner",
            "tester",
        ]);
        let id = r["id"].as_str().unwrap();
        let barrier = std::sync::Barrier::new(2);
        let (correct, forget) = std::thread::scope(|scope| {
            let c = scope.spawn(|| {
                barrier.wait();
                f.run(&["correct", id, "race replacement"])
            });
            let d = scope.spawn(|| {
                barrier.wait();
                f.run(&["forget", id])
            });
            (c.join().unwrap(), d.join().unwrap())
        });
        assert!(correct.status.success() || forget.status.success());
        let records = f.ok(&["export"])["results"].as_array().unwrap().clone();
        if forget.status.success() {
            assert!(records.is_empty(), "forgotten record revived");
        } else {
            assert_eq!(records.len(), 1);
            let replacement = records[0]["id"].as_str().unwrap();
            f.ok(&["forget", replacement]);
        }
    }
}
