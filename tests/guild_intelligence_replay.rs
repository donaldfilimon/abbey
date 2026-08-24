use abbey::app_core::{
    EvidenceLevel, FindingCode, GuildIntelligenceError, RecordingGuildSource, ViewKind,
};
use serde_json::json;

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
        EvidenceLevel::C2ClosedSyntheticReplay
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
    assert_eq!(plan.preconditions.len(), 1);
    assert_eq!(plan.preconditions[0].scope_ref, "synthetic-guild-a");
    assert_eq!(plan.preconditions[0].subject_ref, "role-everyone");
    assert_eq!(plan.preconditions[0].allow, 3_072);
    assert_eq!(plan.preconditions[0].deny, 0);
    assert!(!plan.desired_states.is_empty());
    assert_eq!(plan.postconditions.len(), 1);
    assert_eq!(plan.postconditions[0].allow, 1_024);
    assert_eq!(plan.postconditions[0].deny, 0);
    assert_eq!(
        plan.postconditions[0].source_observation_digest,
        plan.source_observation_digest
    );
    assert_eq!(plan.desired_states.len(), plan.rollback_preview.len());
    assert!(selected.status.plan_present);

    let focused = RecordingGuildSource::from_json(FIXTURE)
        .unwrap()
        .replay(Some("focused-overwrite"))
        .unwrap();
    let focused_plan = focused.plan.unwrap();
    assert_eq!(focused_plan.preconditions.len(), 2);
    assert_eq!(focused_plan.preconditions[0].scope_ref, "channel-public");
    assert_eq!(focused_plan.preconditions[0].subject_ref, "role-everyone");
    assert_eq!(focused_plan.preconditions[0].allow, 0);
    assert_eq!(focused_plan.preconditions[0].deny, 0);
    assert_eq!(focused_plan.preconditions[1].scope_ref, "channel-staff");
    assert_eq!(focused_plan.preconditions[1].subject_ref, "role-everyone");
    assert_eq!(focused_plan.preconditions[1].allow, 0);
    assert_eq!(focused_plan.preconditions[1].deny, 1_024);
    assert_eq!(focused_plan.desired_states.len(), 2);
    assert_eq!(focused_plan.postconditions.len(), 2);
    assert_eq!(focused_plan.postconditions[0].scope_ref, "channel-public");
    assert_eq!(focused_plan.postconditions[0].subject_ref, "role-everyone");
    assert_eq!(focused_plan.postconditions[0].allow, 0);
    assert_eq!(focused_plan.postconditions[0].deny, 2_048);
    assert_eq!(focused_plan.postconditions[1].scope_ref, "channel-staff");
    assert_eq!(focused_plan.postconditions[1].subject_ref, "role-everyone");
    assert_eq!(focused_plan.postconditions[1].allow, 0);
    assert_eq!(focused_plan.postconditions[1].deny, 3_072);
    assert!(
        focused_plan
            .preconditions
            .iter()
            .chain(&focused_plan.postconditions)
            .all(|condition| condition.source_observation_digest
                == focused_plan.source_observation_digest)
    );
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
    let unchanged_plan = unchanged.plan.unwrap();
    assert!(unchanged_plan.preconditions.is_empty());
    assert!(unchanged_plan.desired_states.is_empty());
    assert!(unchanged_plan.postconditions.is_empty());
    assert!(unchanged_plan.rollback_preview.is_empty());
}

#[test]
fn owner_and_administrator_closed_replays_are_deterministic_and_distinct() {
    let mut administrator: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    administrator["operator_authority"] = "administrator".into();
    administrator["operator_ref"] = "synthetic-admin-a".into();
    let administrator = serde_json::to_string(&administrator).unwrap();

    let replay_twice = |recording: &str| {
        let first = RecordingGuildSource::from_json(recording)
            .unwrap()
            .replay(Some("least-privilege"))
            .unwrap();
        let second = RecordingGuildSource::from_json(recording)
            .unwrap()
            .replay(Some("least-privilege"))
            .unwrap();

        assert_eq!(
            first.canonical_json().unwrap(),
            second.canonical_json().unwrap()
        );
        assert_eq!(
            first.status.evidence_level,
            EvidenceLevel::C2ClosedSyntheticReplay
        );
        assert_eq!(first.status.authorization_basis, "synthetic_fixture_claim");
        assert!(first.status.read_only);
        assert!(!first.status.fresh);
        first.canonical_json().unwrap()
    };

    let owner_artifact = replay_twice(FIXTURE);
    let administrator_artifact = replay_twice(&administrator);
    assert_ne!(owner_artifact, administrator_artifact);
}

#[test]
fn substantive_plans_are_empty_when_everyone_cannot_send() {
    let mut fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    fixture["roles"][0]["permissions"] = 1_024.into();
    let fixture = serde_json::to_string(&fixture).unwrap();

    for selected_option in ["least-privilege", "focused-overwrite"] {
        let replay = RecordingGuildSource::from_json(&fixture)
            .unwrap()
            .replay(Some(selected_option))
            .unwrap();
        let plan = replay.plan.expect("explicit selection produces a plan");
        assert!(plan.preconditions.is_empty(), "{selected_option}");
        assert!(plan.desired_states.is_empty(), "{selected_option}");
        assert!(plan.postconditions.is_empty(), "{selected_option}");
        assert!(plan.rollback_preview.is_empty(), "{selected_option}");
    }
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

#[test]
fn synthetic_administrator_authority_is_accepted_without_claiming_ownership() {
    let mut administrator: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    administrator["operator_authority"] = "administrator".into();
    administrator["operator_ref"] = "synthetic-admin-a".into();

    let replay = RecordingGuildSource::from_json(&serde_json::to_string(&administrator).unwrap())
        .unwrap()
        .replay(None)
        .expect("administrator synthetic fixture is accepted");
    assert_eq!(replay.status.authorization_basis, "synthetic_fixture_claim");
    assert!(replay.status.read_only);
    assert!(!replay.status.fresh);
}

#[test]
fn opaque_reference_limits_accept_the_boundary_and_reject_the_next_byte() {
    let mut boundary: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    boundary["guild_ref"] = "g".repeat(96).into();
    assert!(RecordingGuildSource::from_json(&serde_json::to_string(&boundary).unwrap()).is_ok());

    boundary["guild_ref"] = "g".repeat(97).into();
    assert!(matches!(
        RecordingGuildSource::from_json(&serde_json::to_string(&boundary).unwrap()),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));

    boundary["guild_ref"] = "guild\ncontrol".into();
    assert!(matches!(
        RecordingGuildSource::from_json(&serde_json::to_string(&boundary).unwrap()),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));
}

#[test]
fn collection_limits_accept_2048_and_reject_2049() {
    let mut threads: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    threads["active_threads"] = (0..2_048)
        .map(|index| json!({ "ref_id": format!("thread-{index:04}"), "parent_ref": "channel-public" }))
        .collect();
    assert!(RecordingGuildSource::from_json(&serde_json::to_string(&threads).unwrap()).is_ok());

    threads["active_threads"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "ref_id": "thread-2048", "parent_ref": "channel-public" }));
    assert!(matches!(
        RecordingGuildSource::from_json(&serde_json::to_string(&threads).unwrap()),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));

    let mut overwrites: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    overwrites["roles"] = std::iter::once(json!({
        "ref_id": "role-everyone",
        "position": 0,
        "permissions": 3072,
        "managed": false,
    }))
    .chain((1..=2_047).map(|index| {
        json!({
            "ref_id": format!("role-{index:04}"),
            "position": index,
            "permissions": 0,
            "managed": false,
        })
    }))
    .collect();
    overwrites["bot_self"]["role_refs"] = json!(["role-0001"]);
    overwrites["channels"] = json!([{
        "ref_id": "channel-public",
        "parent_ref": null,
        "kind": "text",
        "overwrites": (1..=2_047)
            .map(|index| json!({
                "target": { "kind": "role", "ref_id": format!("role-{index:04}") },
                "allow": 0,
                "deny": 0,
            }))
            .chain(std::iter::once(json!({
                "target": { "kind": "member", "ref_id": "synthetic-bot" },
                "allow": 0,
                "deny": 0,
            })))
            .collect::<Vec<_>>(),
    }]);
    assert!(RecordingGuildSource::from_json(&serde_json::to_string(&overwrites).unwrap()).is_ok());

    overwrites["channels"][0]["overwrites"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "target": { "kind": "everyone", "ref_id": "role-everyone" },
            "allow": 0,
            "deny": 0,
        }));
    assert!(matches!(
        RecordingGuildSource::from_json(&serde_json::to_string(&overwrites).unwrap()),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));
}
