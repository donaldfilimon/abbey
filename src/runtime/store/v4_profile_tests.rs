use super::*;
use crate::app_core::V4SubmitTurn;
fn profile() -> V4PermissionProfile {
    V4PermissionProfile {
        profile_id: "desk.default".into(),
        tool_ids: vec!["memory.read".into()],
        application_ids: vec!["editor".into()],
        filesystem_roots: vec!["/private/example".into()],
        browser_domains: vec!["example.com".into()],
        max_runtime_ms: 12345,
        max_output_bytes: 54321,
    }
}
fn fixture() -> (std::path::PathBuf, RuntimeStore) {
    let root = std::env::temp_dir().join(format!("abbey-profile-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(&root)).unwrap();
    (root, store)
}
#[test]
fn v4_profile_update_preserves_referenced_identity_and_exact_reopen() {
    let (root, store) = fixture();
    let mut p = profile();
    store.v4_put_permission_profile(&p, 1700000000000).unwrap();
    let cid = store
        .v4_create_conversation("profile", 1700000000000)
        .unwrap()
        .conversation_id;
    store
        .v4_admit_turn(
            &V4SubmitTurn {
                conversation_id: cid,
                idempotency_key: "profile-turn".parse().unwrap(),
                route_id: "abi.local".into(),
                input: "hello".into(),
            },
            1700000000000,
        )
        .unwrap();
    p.max_runtime_ms = 23456;
    store.v4_put_permission_profile(&p, 1700000000001).unwrap();
    assert_eq!(store.v4_permission_profile(&p.profile_id).unwrap(), p);
    drop(store);
    let reopened = RuntimeStore::open(&RuntimeStore::path_for_state_dir(&root)).unwrap();
    assert_eq!(reopened.v4_permission_profile(&p.profile_id).unwrap(), p);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn v4_profile_missing_corrupt_invalid_and_debug_fail_closed() {
    let (root, store) = fixture();
    assert!(store.v4_permission_profile("missing").is_err());
    let p = profile();
    assert!(!format!("{p:?}").contains("/private/example"));
    let mut invalid = p.clone();
    invalid.filesystem_roots = vec!["relative".into()];
    assert!(
        store
            .v4_put_permission_profile(&invalid, 1700000000000)
            .is_err()
    );
    for document in [
        "{}".to_owned(),
        "[]".to_owned(),
        "{\"profile_id\":\"bad\"}".to_owned(),
        {
            let mut value = serde_json::to_value(&p).unwrap();
            value["unknown"] = true.into();
            value.to_string()
        },
        {
            let mut value = serde_json::to_value(&p).unwrap();
            value["profile_id"] = "different".into();
            value.to_string()
        },
    ] {
        store.conn.lock().unwrap().execute("INSERT INTO assistant_permission_profiles VALUES (?1,?2,0) ON CONFLICT(profile_id) DO UPDATE SET document_json=excluded.document_json",params![p.profile_id,document]).unwrap();
        assert!(store.v4_permission_profile(&p.profile_id).is_err());
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn v4_profile_limits_and_document_bounds() {
    let (root, store) = fixture();
    let base = profile();
    let mut invalid = base.clone();
    invalid.tool_ids = vec!["tool".into(); 65];
    assert!(invalid.validate().is_err());
    invalid = base.clone();
    invalid.max_runtime_ms = 3_600_001;
    assert!(invalid.validate().is_err());
    invalid = base.clone();
    invalid.max_output_bytes = 1_048_577;
    assert!(invalid.validate().is_err());
    invalid = base.clone();
    invalid.browser_domains = vec!["https://example.com".into()];
    assert!(invalid.validate().is_err());
    let mut large = base.clone();
    large.tool_ids = vec!["t".repeat(128); 64];
    large.application_ids = vec!["a".repeat(128); 64];
    large.filesystem_roots = vec![format!("/{}", "r".repeat(511)); 64];
    large.browser_domains = vec!["d".repeat(253); 64];
    large.validate().unwrap();
    assert!(
        store
            .v4_put_permission_profile(&large, 1700000000000)
            .is_err()
    );
    let oversized = serde_json::to_string(&"é".repeat(34000)).unwrap();
    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO assistant_permission_profiles VALUES ('oversized',?1,0)",
            [oversized],
        )
        .unwrap();
    assert!(store.v4_permission_profile("oversized").is_err());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
