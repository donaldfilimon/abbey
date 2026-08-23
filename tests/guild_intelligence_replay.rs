use abbey::app_core::{
    EvidenceLevel, FindingCode, GuildIntelligenceError, RecordingGuildSource, ViewKind,
};

const FIXTURE: &str = include_str!("fixtures/guild_intelligence/community-risk.json");

#[test]
fn synthetic_replay_is_closed_deterministic_and_non_executable() {
    let mut first = RecordingGuildSource::from_json(FIXTURE).expect("closed synthetic fixture");
    let preview = first.replay(None).expect("read-only analysis");

    assert_eq!(preview.twin.views.len(), 5);
    assert_eq!(
        preview
            .twin
            .views
            .iter()
            .map(|view| view.kind)
            .collect::<Vec<_>>(),
        vec![
            ViewKind::Structure,
            ViewKind::Authority,
            ViewKind::Workflow,
            ViewKind::Goal,
            ViewKind::Health,
        ]
    );
    assert!(
        preview.twin.views.iter().all(|view| view.metadata.stale),
        "closed synthetic replay cannot prove temporal freshness"
    );
    assert_eq!(preview.twin.watermarks.len(), 7);
    assert!(preview.twin.coverage.content_excluded);
    assert!(preview.twin.coverage.member_enumeration_excluded);
    assert_eq!(
        preview.twin.effective_channel_permissions["channel-public"],
        3072
    );
    assert_eq!(
        preview.twin.effective_channel_permissions["channel-staff"], 3072,
        "role overwrite restores view after the everyone deny"
    );
    assert!(
        preview
            .findings
            .iter()
            .all(|finding| !finding.evidence_digests.is_empty())
    );
    assert_eq!(
        preview
            .findings
            .iter()
            .map(|finding| finding.code)
            .collect::<Vec<_>>(),
        vec![
            FindingCode::EveryoneCanSend,
            FindingCode::BotChannelVisibilityRestored,
            FindingCode::CoverageLimited,
        ]
    );
    assert_eq!(preview.alternatives.len(), 3);
    assert_eq!(preview.alternatives.last().unwrap().id, "do-nothing");
    assert!(preview.plan.is_none(), "selection must be explicit");
    assert_eq!(
        preview.status.evidence_level,
        EvidenceLevel::C1LocalSyntheticContract
    );
    assert!(preview.status.read_only);
    assert!(!preview.status.fresh);
    assert!(!preview.status.plan_present);
    let status_json = serde_json::to_string(&preview.status).unwrap();
    assert!(!status_json.contains("synthetic-guild-a"));
    assert!(!status_json.contains("synthetic-operator-a"));
    assert!(!status_json.contains("channel-public"));
    assert_eq!(first.read_operations().len(), 1);

    let selected_id = preview.alternatives[0].id.clone();
    let selected = RecordingGuildSource::from_json(FIXTURE)
        .unwrap()
        .replay(Some(&selected_id))
        .expect("explicit selection");
    let plan = selected.plan.as_ref().expect("desired state plan");
    assert_eq!(plan.selected_option_id, selected_id);
    assert!(!plan.desired_states.is_empty());
    assert_eq!(plan.desired_states.len(), plan.rollback_preview.len());
    assert!(selected.status.plan_present);

    let focused = RecordingGuildSource::from_json(FIXTURE)
        .unwrap()
        .replay(Some("focused-overwrite"))
        .unwrap();
    let focused_plan = focused.plan.unwrap();
    assert_eq!(focused_plan.desired_states.len(), 2);
    assert!(
        focused_plan
            .desired_states
            .iter()
            .all(|state| state.scope_ref.starts_with("channel-") && state.deny & 2048 != 0)
    );
    assert_ne!(
        &plan.desired_states, &focused_plan.desired_states,
        "operator alternatives must not collapse to the same desired state"
    );

    let again = RecordingGuildSource::from_json(FIXTURE)
        .unwrap()
        .replay(Some(&selected_id))
        .unwrap();
    assert_eq!(
        selected.canonical_json().unwrap(),
        again.canonical_json().unwrap()
    );

    let unchanged = RecordingGuildSource::from_json(FIXTURE)
        .unwrap()
        .replay(Some("do-nothing"))
        .unwrap();
    assert!(unchanged.plan.unwrap().desired_states.is_empty());
}

#[test]
fn authorization_and_schema_fail_closed() {
    let member = FIXTURE.replace("\"owner\"", "\"member\"");
    assert!(matches!(
        RecordingGuildSource::from_json(&member)
            .unwrap()
            .replay(None),
        Err(GuildIntelligenceError::Unauthorized)
    ));

    let unknown = FIXTURE.replacen("{", "{\"unexpected\":true,", 1);
    assert!(matches!(
        RecordingGuildSource::from_json(&unknown),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));

    let real = FIXTURE.replace("\"synthetic\": true", "\"synthetic\": false");
    assert!(matches!(
        RecordingGuildSource::from_json(&real),
        Err(GuildIntelligenceError::NonSyntheticRecording)
    ));

    let false_owner = FIXTURE.replace(
        "\"owner_ref\": \"synthetic-operator-a\"",
        "\"owner_ref\": \"synthetic-other-owner\"",
    );
    assert!(matches!(
        RecordingGuildSource::from_json(&false_owner),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));
}

#[test]
fn invalid_selection_is_rejected() {
    let error = RecordingGuildSource::from_json(FIXTURE)
        .unwrap()
        .replay(Some("not-an-option"))
        .unwrap_err();
    assert_eq!(error, GuildIntelligenceError::UnknownSelection);
}

#[test]
fn object_order_is_normalized_before_digest_and_analysis() {
    let expected = RecordingGuildSource::from_json(FIXTURE)
        .unwrap()
        .replay(None)
        .unwrap();
    let mut reordered: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    reordered["roles"].as_array_mut().unwrap().reverse();
    reordered["channels"].as_array_mut().unwrap().reverse();
    reordered["active_threads"]
        .as_array_mut()
        .unwrap()
        .reverse();
    for channel in reordered["channels"].as_array_mut().unwrap() {
        channel["overwrites"].as_array_mut().unwrap().reverse();
    }

    let actual = RecordingGuildSource::from_json(&serde_json::to_string(&reordered).unwrap())
        .unwrap()
        .replay(None)
        .unwrap();
    assert_eq!(
        expected.canonical_json().unwrap(),
        actual.canonical_json().unwrap()
    );
}

#[test]
fn duplicate_and_dangling_metadata_relationships_fail_closed() {
    let mut duplicate: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    duplicate["roles"][1]["ref_id"] = duplicate["roles"][0]["ref_id"].clone();
    assert!(matches!(
        RecordingGuildSource::from_json(&serde_json::to_string(&duplicate).unwrap()),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));

    let mut dangling_parent: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    dangling_parent["active_threads"][0]["parent_ref"] = "missing-channel".into();
    assert!(matches!(
        RecordingGuildSource::from_json(&serde_json::to_string(&dangling_parent).unwrap()),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));

    let mut dangling_role: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    dangling_role["channels"][1]["overwrites"][1]["target"]["ref_id"] = "missing-role".into();
    assert!(matches!(
        RecordingGuildSource::from_json(&serde_json::to_string(&dangling_role).unwrap()),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));
}
