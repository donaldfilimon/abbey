//! Closed synthetic regressions; no recording, transport, or effect authority.
use abbey::app_core::{FindingCode, GuildIntelligenceError, RecordingGuildSource};
use serde_json::{Value, json};

const FIXTURE: &str = include_str!("fixtures/guild_intelligence/community-risk.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

fn source(value: &Value) -> RecordingGuildSource {
    RecordingGuildSource::from_json(&value.to_string()).unwrap()
}

#[test]
fn alternatives_only_offer_permission_plans_with_an_actual_change() {
    let mut value = fixture();
    value["roles"][0]["permissions"] = json!(1024);
    let mut recording = source(&value);
    let preview = recording.replay(None).unwrap();
    assert_eq!(preview.alternatives.len(), 1);
    assert_eq!(preview.alternatives[0].id, "do-nothing");
    for id in ["least-privilege", "focused-overwrite"] {
        assert_eq!(
            recording.replay(Some(id)),
            Err(GuildIntelligenceError::UnknownSelection)
        );
    }
    value["roles"][0]["permissions"] = json!(3072);
    value["channels"] = json!([]);
    value["active_threads"] = json!([]);
    let preview = source(&value).replay(None).unwrap();
    assert_eq!(
        preview
            .alternatives
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        ["least-privilege", "do-nothing"]
    );

    let mut value = fixture();
    for channel in value["channels"].as_array_mut().unwrap() {
        channel["overwrites"] = json!([{
            "target": {"kind": "role", "ref_id": "role-everyone"},
            "allow": 0, "deny": 2048
        }]);
    }
    let mut recording = source(&value);
    assert_eq!(
        recording.replay(Some("focused-overwrite")),
        Err(GuildIntelligenceError::UnknownSelection)
    );
    value["channels"][0]["overwrites"] = json!([]);
    let plan = source(&value)
        .replay(Some("focused-overwrite"))
        .unwrap()
        .plan
        .unwrap();
    assert_eq!(plan.desired_states.len(), 1);
    assert_eq!(plan.desired_states[0].scope_ref, "channel-public");
}

#[test]
fn rollback_distinguishes_absent_overwrites_from_explicit_zero_entries() {
    let mut value = fixture();
    for (present, overwrites) in [
        (false, json!([])),
        (
            true,
            json!([{"target": {"kind": "role", "ref_id": "role-everyone"}, "allow": 0, "deny": 0}]),
        ),
    ] {
        value["channels"][0]["overwrites"] = overwrites;
        let replay = source(&value).replay(Some("focused-overwrite")).unwrap();
        assert_eq!(replay.twin.schema_version, 2);
        let plan = serde_json::to_value(replay.plan.unwrap()).unwrap();
        assert_eq!(plan["preconditions"][0]["present"], json!(present));
        assert_eq!(plan["rollback_preview"][0]["present"], json!(present));
        assert_eq!(plan["postconditions"][0]["present"], json!(true));
    }
}

#[test]
fn visibility_evidence_binds_bot_authority_even_when_effective_bits_do_not_change() {
    let value = fixture();
    let before = source(&value).replay(None).unwrap();
    let mut changed = value;
    changed["roles"][1]["permissions"] = json!(3072);
    let after = source(&changed).replay(None).unwrap();
    assert_eq!(
        before.twin.effective_channel_permissions,
        after.twin.effective_channel_permissions
    );
    let evidence = |replay: &abbey::app_core::ReplayArtifact| {
        replay
            .findings
            .iter()
            .find(|f| f.code == FindingCode::BotChannelVisibilityRestored)
            .unwrap()
            .evidence_digests
            .clone()
    };
    assert_ne!(evidence(&before), evidence(&after));
}

#[test]
fn recording_byte_limit_is_checked_before_parsing_and_allocation() {
    const LIMIT: usize = 4 * 1024 * 1024;
    let mut exact = FIXTURE.to_string();
    exact.extend(std::iter::repeat_n(' ', LIMIT - exact.len()));
    assert!(RecordingGuildSource::from_json(&exact).is_ok());
    exact.push(' ');
    let error = RecordingGuildSource::from_json(&exact).unwrap_err();
    assert!(error.to_string().contains("byte limit"), "{error}");
}

#[test]
fn parent_cycles_are_refused_while_acyclic_parent_chains_remain_valid() {
    let mut value = fixture();
    value["channels"][0]["parent_ref"] = json!("channel-staff");
    assert!(RecordingGuildSource::from_json(&value.to_string()).is_ok());
    value["channels"][1]["parent_ref"] = json!("channel-public");
    assert!(matches!(
        RecordingGuildSource::from_json(&value.to_string()),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));
    value["channels"][0]["parent_ref"] = json!("channel-public");
    assert!(matches!(
        RecordingGuildSource::from_json(&value.to_string()),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));
}
