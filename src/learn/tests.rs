use super::*;
use crate::route_log::RouteRecord;
use std::fs;
use std::path::PathBuf;

fn temp_state(tag: &str) -> AbbeyState {
    let state_dir = std::env::temp_dir().join(format!("abbey-learn-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&state_dir);
    fs::create_dir_all(&state_dir).unwrap();
    let cwd_dir = state_dir.join("by-cwd");
    fs::create_dir_all(&cwd_dir).unwrap();
    AbbeyState {
        chat_file: state_dir.join("chat-id"),
        model_file: state_dir.join("model"),
        history_file: state_dir.join("history.log"),
        cwd_dir,
        per_cwd: false,
        cwd: PathBuf::from("."),
        state_dir,
    }
}

#[test]
fn learn_from_routes_keeps_alternate_and_fallback() {
    // Force SQLite so the test is independent of the user's config.toml.
    unsafe { std::env::set_var("ABBEY_MEMORY_BACKEND", "sqlite") };
    let state = temp_state("routes");
    let rec = RouteRecord::new(".", "abbey", "max", "fable", "hybrid", 0.7)
        .with_routing(Some("gemma".into()), Some("prefer hybrid-loop".into()));
    route_log::append_route_record(&state.state_dir, &rec).unwrap();
    let n = learn_from_routes(&state, 5).unwrap();
    assert_eq!(n, 1);
    let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();
    let rows = mem
        .filter(Some("activity"), Some("self-learn"), 10)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].payload.contains("alternate=gemma"));
    assert!(rows[0].payload.contains("fallback=prefer hybrid-loop"));
    assert!(rows[0].payload.contains("confidence=0.70"));
    let _ = fs::remove_dir_all(&state.state_dir);
}

#[test]
fn learn_from_routes_is_idempotent_over_the_same_route_tail() {
    unsafe { std::env::set_var("ABBEY_MEMORY_BACKEND", "sqlite") };
    let state = temp_state("routes-idem");
    for ts in ["t1", "t2", "t3"] {
        let mut rec = RouteRecord::new(".", "abbey", "max", "fable", "hybrid", 0.7);
        rec.ts = ts.into();
        route_log::append_route_record(&state.state_dir, &rec).unwrap();
    }
    let first = learn_from_routes(&state, 5).unwrap();
    assert_eq!(first, 3);
    let second = learn_from_routes(&state, 5).unwrap();
    assert_eq!(second, 0, "re-consuming the same tail must not re-store");
    let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();
    let rows = mem
        .filter(Some("activity"), Some("self-learn"), 100)
        .unwrap();
    assert_eq!(rows.len(), 3);
    let _ = fs::remove_dir_all(&state.state_dir);
}

#[test]
fn route_activity_payload_includes_routing_fields() {
    let r = RouteRecord::new(".", "abbey", "max", "fable", "other", 0.55)
        .with_routing(Some("gemma".into()), Some("low conf".into()));
    let p = route_activity_payload(&r);
    assert!(p.contains("alternate=gemma"));
    assert!(p.contains("fallback=low conf"));
    assert!(p.contains("confidence=0.55"));
}

#[test]
fn plan_improvements_flags_low_confidence_routes_for_review_only() {
    let hi = RouteRecord::new(".", "abbey", "max", "fable", "hybrid", 0.9);
    let lo = RouteRecord::new(".", "abbey", "max", "fable", "guess", 0.3);
    let reflect = memory::ReflectReport {
        duplicate_summaries: vec![("a".into(), "b".into())],
        low_confidence: vec!["c".into()],
        superseded: vec![],
    };
    let stats = TrainStats {
        total: 2,
        with_provenance: 1,
        high_confidence: 2,
    };
    let plan = plan_improvements(&reflect, &[hi, lo], Some(stats));
    let kinds: Vec<_> = plan.iter().map(|p| p.kind).collect();
    assert_eq!(
        kinds,
        [
            "promote-routes",
            "review-route",
            "dedupe",
            "low-confidence",
            "provenance"
        ]
    );
    // Only the additive step is auto-applied.
    assert!(
        plan.iter()
            .filter(|p| p.applies)
            .all(|p| p.kind == "promote-routes")
    );
    assert_eq!(plan.iter().filter(|p| p.applies).count(), 1);
}

#[test]
fn plan_improvements_groups_repeated_routes_and_duplicate_clusters() {
    let low = |ts: &str| {
        let mut r = RouteRecord::new(".", "abbey", "max", "fable", "Other", 0.55);
        r.ts = ts.into();
        r
    };
    let routes = [low("t1"), low("t2"), low("t3")];
    let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    let reflect = memory::ReflectReport {
        duplicate_summaries: vec![
            pair("a", "b"),
            pair("a", "c"),
            pair("b", "c"),
            pair("x", "y"),
        ],
        low_confidence: vec![],
        superseded: vec![],
    };
    let plan = plan_improvements(&reflect, &routes, None);
    let reviews: Vec<_> = plan.iter().filter(|p| p.kind == "review-route").collect();
    assert_eq!(reviews.len(), 1, "{plan:?}");
    assert!(
        reviews[0].detail.starts_with("x3 t1..t3 "),
        "{}",
        reviews[0].detail
    );
    let dedupes: Vec<_> = plan
        .iter()
        .filter(|p| p.kind == "dedupe")
        .map(|p| p.detail.as_str())
        .collect();
    assert_eq!(
        dedupes,
        [
            "3 memories share one summary: a, b, c",
            "2 memories share one summary: x, y"
        ]
    );
}

#[test]
fn duplicate_clusters_merges_chains() {
    let pairs: Vec<(String, String)> = (0..5)
        .map(|i| (format!("m{i}"), format!("m{}", i + 1)))
        .collect();
    let clusters = duplicate_clusters(&pairs);
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0].len(), 6);
    assert!(duplicate_clusters(&[]).is_empty());
}

#[test]
fn plan_improvements_is_empty_without_signals() {
    let plan = plan_improvements(&memory::ReflectReport::default(), &[], None);
    assert!(plan.is_empty());
}

#[test]
fn improve_dry_run_does_not_create_a_store() {
    unsafe { std::env::set_var("ABBEY_MEMORY_BACKEND", "sqlite") };
    let state = temp_state("improve-dry");
    let rec = RouteRecord::new(".", "abbey", "max", "fable", "hybrid", 0.4);
    route_log::append_route_record(&state.state_dir, &rec).unwrap();
    let applied = improve(&state, 5, false, None).unwrap();
    assert_eq!(applied, 0);
    assert!(!memory::backend_path(&state.state_dir, "sqlite").exists());
    let applied = improve(&state, 5, true, None).unwrap();
    assert_eq!(applied, 1);
    let _ = fs::remove_dir_all(&state.state_dir);
}

#[test]
fn train_stats_counts_provenance_and_ready() {
    unsafe { std::env::set_var("ABBEY_MEMORY_BACKEND", "sqlite") };
    let state = temp_state("stats");
    let id = capture_correction(&state, "train candidate", "prefer small diffs", true).unwrap();
    assert!(!id.is_empty());
    let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();
    let cur = collect_train_curation(mem.as_ref()).unwrap();
    assert_eq!(cur.stats.total, 1);
    assert_eq!(cur.stats.with_provenance, 1);
    assert_eq!(cur.stats.missing_provenance(), 0);
    assert_eq!(cur.ready, 1); // capture_correction uses conf 0.95
    let _ = fs::remove_dir_all(&state.state_dir);
}

/// A distilled candidate: a real prompt/response pair at teacher confidence.
fn distilled_candidate(state: &AbbeyState, id: &str, confidence: f32) -> String {
    let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();
    let mut record = MemoryRecord::new_stm(
        "distilled pair",
        serde_json::to_string(&crate::distill::DistillPair {
            prompt: "what is the gate?".into(),
            response: "./check.sh".into(),
            teacher: "fm:system".into(),
            on_device: true,
        })
        .unwrap(),
    );
    record.id = id.to_string();
    record.retention = "train_candidate".into();
    record.source_type = "distill".into();
    record.provenance = "teacher fm:system @ 2026-10-22T00:00:00Z".into();
    record.confidence = confidence;
    mem.store(record).unwrap();
    id.to_string()
}

#[test]
fn promote_raises_a_distilled_candidate_to_the_ready_bar() {
    unsafe { std::env::set_var("ABBEY_MEMORY_BACKEND", "sqlite") };
    let state = temp_state("promote");
    let id = distilled_candidate(&state, "pair-1", 0.6);

    let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();
    let before = mem.get(&id).unwrap().unwrap();
    assert_eq!(before.confidence, 0.6);
    assert_eq!(
        collect_train_curation(mem.as_ref()).unwrap().ready,
        0,
        "a distilled pair is not curated until a human promotes it"
    );

    let message = promote_candidate(&state, &id).unwrap();
    assert!(message.contains("promoted"), "{message}");

    let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();
    let after = mem.get(&id).unwrap().unwrap();
    assert_eq!(after.confidence, READY_CONFIDENCE);
    assert!(after.tags.iter().any(|t| t == "promoted"));
    // The teacher provenance survives; the promotion is added to it.
    assert!(
        after.provenance.contains("teacher fm:system"),
        "the teacher must still be named: {}",
        after.provenance
    );
    assert!(after.provenance.contains("promoted"));
    // The payload — the pair itself — is untouched.
    assert_eq!(before.payload, after.payload);
    assert_eq!(
        collect_train_curation(mem.as_ref()).unwrap().ready,
        1,
        "a promoted pair is curated"
    );
    let _ = fs::remove_dir_all(&state.state_dir);
}

#[test]
fn promote_refuses_what_it_may_not_change() {
    unsafe { std::env::set_var("ABBEY_MEMORY_BACKEND", "sqlite") };
    let state = temp_state("promote-refuse");
    let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();

    // Unknown id.
    let error = promote_candidate(&state, "nope").unwrap_err().to_string();
    assert!(error.contains("no train candidate"), "{error}");

    // Not a candidate.
    let mut ltm = MemoryRecord::new_stm("ltm", "body");
    ltm.retention = "ltm".into();
    ltm.provenance = "user correction @ .".into();
    mem.store(ltm.clone()).unwrap();
    let error = promote_candidate(&state, &ltm.id).unwrap_err().to_string();
    assert!(error.contains("not a train candidate"), "{error}");

    // No provenance: the store refuses to hold such a candidate at all, so
    // the promotion path never sees one. The rule is enforced at the
    // boundary, not only at the promotion.
    let mut orphan = MemoryRecord::new_stm("orphan", "body");
    orphan.retention = "train_candidate".into();
    orphan.provenance = "   ".into();
    let error = mem.store(orphan.clone()).unwrap_err().to_string();
    assert!(error.contains("provenance"), "{error}");

    // Obsolete.
    let mut obsolete = MemoryRecord::new_stm("obsolete", "body");
    obsolete.retention = "train_candidate".into();
    obsolete.provenance = "teacher fm:system @ now".into();
    obsolete.confidence = 0.6;
    obsolete.obsolete = true;
    mem.store(obsolete.clone()).unwrap();
    let error = promote_candidate(&state, &obsolete.id)
        .unwrap_err()
        .to_string();
    assert!(error.contains("obsolete"), "{error}");

    let _ = fs::remove_dir_all(&state.state_dir);
}

#[test]
fn promote_is_a_no_op_for_an_already_curated_candidate() {
    unsafe { std::env::set_var("ABBEY_MEMORY_BACKEND", "sqlite") };
    let state = temp_state("promote-noop");
    let id = distilled_candidate(&state, "pair-2", READY_CONFIDENCE);
    let message = promote_candidate(&state, &id).unwrap();
    assert!(message.contains("already curated"), "{message}");
    let mem = memory::open_backend(&state.state_dir, "sqlite").unwrap();
    let after = mem.get(&id).unwrap().unwrap();
    assert_eq!(after.confidence, READY_CONFIDENCE);
    assert!(
        !after.provenance.contains("promoted"),
        "a no-op must not rewrite the provenance: {}",
        after.provenance
    );
    let _ = fs::remove_dir_all(&state.state_dir);
}
