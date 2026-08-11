use super::*;
use crate::memory::MemoryRecord;

fn scratch(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "abbey-v3-memory-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ))
}

fn authority(label: &str) -> (PathBuf, MemoryAuthority) {
    let root = scratch(label);
    let authority = MemoryAuthority::new(MemoryEffectRoute::new(root.clone(), "sqlite".to_owned()));
    assert!(authority.readable());
    (root, authority)
}

fn store(root: &std::path::Path, id: &str, summary: &str, payload: &str) {
    let memory = crate::memory::open_backend_exact(root, "sqlite").unwrap();
    let mut record = MemoryRecord::new_stm(summary, payload);
    record.id = id.to_owned();
    record.source_ref = "/private/source/canary".to_owned();
    record.provenance = "secret provenance canary".to_owned();
    memory.store(record).unwrap();
}

#[test]
fn summary_space_is_exact_and_fixed() {
    let (root, authority) = authority("space");
    let V3Event::MemorySpaces(page) = authority.list_spaces(V3PageQuery::default()).unwrap() else {
        panic!("expected memory spaces");
    };
    assert_eq!(page.after, 0);
    assert_eq!(page.through, 1);
    assert_eq!(page.records[0].id, SUMMARY_SPACE_ID);
    assert!(matches!(
        authority.list_spaces(V3PageQuery {
            after: 0,
            through: Some(2),
            limit: 1,
        }),
        Err(error) if error.code() == "invalid_command"
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_and_metadata_expose_only_opaque_ids_and_sanitized_summaries() {
    let (root, authority) = authority("projection");
    store(
        &root,
        "raw-record-secret",
        "deploy from cwd=/Users/alice/private\u{7} file=C:\\Users\\Alice\\secret safely",
        "RAW_PAYLOAD_CANARY",
    );

    let V3Event::MemorySearchResults(page) = authority
        .search(V3SearchRequest {
            space_id: SUMMARY_SPACE_ID.to_owned(),
            query: "deploy".to_owned(),
            page: V3PageQuery::default(),
        })
        .unwrap()
    else {
        panic!("expected memory results");
    };
    assert_eq!(page.records.len(), 1);
    let record = &page.records[0];
    assert!(record.id.starts_with("memory-"));
    assert_eq!(record.id.len(), 71);
    assert!(!record.id.contains("raw-record-secret"));
    assert_eq!(record.label, "deploy from [path] [path] safely");
    let rendered = serde_json::to_string(&page).unwrap();
    for secret in [
        "RAW_PAYLOAD_CANARY",
        "secret provenance canary",
        "/private/source/canary",
        "/Users/alice/private",
        "raw-record-secret",
    ] {
        assert!(!rendered.contains(secret), "leaked {secret}");
    }

    let V3Event::MemoryMetadata(metadata) = authority
        .metadata(V3ResourceQuery {
            resource_id: record.id.clone(),
        })
        .unwrap()
    else {
        panic!("expected memory metadata");
    };
    assert_eq!(metadata, *record);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_watermarks_are_query_bound_and_snapshot_consistent() {
    let (root, authority) = authority("snapshot");
    store(&root, "one", "needle one", "private one");
    store(&root, "two", "needle two", "private two");
    let first_request = V3SearchRequest {
        space_id: SUMMARY_SPACE_ID.to_owned(),
        query: "needle".to_owned(),
        page: V3PageQuery {
            after: 0,
            through: None,
            limit: 1,
        },
    };
    let V3Event::MemorySearchResults(first) = authority.search(first_request).unwrap() else {
        panic!("expected first page");
    };
    assert_eq!(first.records.len(), 1);

    store(&root, "three", "needle three", "private three");
    let V3Event::MemorySearchResults(second) = authority
        .search(V3SearchRequest {
            space_id: SUMMARY_SPACE_ID.to_owned(),
            query: "needle".to_owned(),
            page: V3PageQuery {
                after: 1,
                through: Some(first.through),
                limit: 2,
            },
        })
        .unwrap()
    else {
        panic!("expected second page");
    };
    assert_eq!(second.records.len(), 1);
    assert!(!second.records[0].label.contains("three"));

    let wrong_query = authority
        .search(V3SearchRequest {
            space_id: SUMMARY_SPACE_ID.to_owned(),
            query: "different".to_owned(),
            page: V3PageQuery {
                after: 0,
                through: Some(first.through),
                limit: 1,
            },
        })
        .unwrap_err();
    assert_eq!(wrong_query.code(), "invalid_command");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_space_record_and_unavailable_backend_fail_closed() {
    let (root, authority) = authority("refusal");
    assert_eq!(
        authority
            .search(V3SearchRequest {
                space_id: "different-space".to_owned(),
                query: "query".to_owned(),
                page: V3PageQuery::default(),
            })
            .unwrap_err()
            .code(),
        "not_found"
    );
    assert_eq!(
        authority
            .metadata(V3ResourceQuery {
                resource_id: format!("memory-{}", "0".repeat(64)),
            })
            .unwrap_err()
            .code(),
        "not_found"
    );
    std::fs::remove_dir_all(root).unwrap();

    let unavailable = MemoryAuthority::new(MemoryEffectRoute::new(
        scratch("unavailable"),
        "invalid".to_owned(),
    ));
    assert!(!unavailable.readable());
    assert_eq!(
        unavailable
            .search(V3SearchRequest {
                space_id: SUMMARY_SPACE_ID.to_owned(),
                query: "query".to_owned(),
                page: V3PageQuery::default(),
            })
            .unwrap_err()
            .code(),
        "runtime_unavailable"
    );
}
