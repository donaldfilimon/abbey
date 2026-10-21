//! Unserved admin storage entry points must never fabricate success.
use super::*;
use crate::app_core::V4ScheduleKind;
fn fixture() -> (std::path::PathBuf, RuntimeStore) {
    let root = std::env::temp_dir().join(format!("abbey-unavailable-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(&root)).unwrap();
    (root, store)
}
fn query() -> V4ConversationQuery {
    V4ConversationQuery {
        search: None,
        after: None,
        limit: 10,
        archived: false,
    }
}
#[test]
fn v4_unavailable_admin_lists_refuse_instead_of_fabricating_empty_catalogs() {
    let (root, store) = fixture();
    assert!(store.v4_tasks(&query()).is_err());
    assert!(store.v4_automations(&query()).is_err());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn v4_unavailable_admin_effects_refuse_without_mutation_or_input_disclosure() {
    let (root, store) = fixture();
    let changes = store.conn.lock().unwrap().total_changes();
    let task = V4TaskCreate {
        title: "private task instruction".into(),
    };
    assert!(store.v4_create_task(&task, 1700000000000).is_err());
    let automation = V4AutomationUpsert {
        automation_id: "auto.private".into(),
        instruction: "private automation instruction".into(),
        schedule: V4ScheduleKind::Cron,
        cron: Some("0 * * * *".into()),
        timezone: "UTC".into(),
        route_id: "abi.local".into(),
        permission_profile_id: "desk.default".into(),
        budget_tokens: 100,
        enabled: true,
    };
    assert!(
        store
            .v4_upsert_automation(&automation, 1700000000000)
            .is_err()
    );
    for approved in [false, true] {
        let error = store
            .v4_decide_action(
                "private-action-id",
                "private-decision-id",
                approved,
                1700000000000,
            )
            .unwrap_err();
        assert!(!error.to_string().contains("private"));
    }
    assert_eq!(store.conn.lock().unwrap().total_changes(), changes);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
