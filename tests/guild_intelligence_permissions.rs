use abbey::app_core::{GuildIntelligenceError, RecordingGuildSource};
use serde_json::json;

const FIXTURE: &str = include_str!("fixtures/guild_intelligence/community-risk.json");

fn recording() -> serde_json::Value {
    serde_json::from_str(FIXTURE).expect("checked-in recording fixture")
}

fn recording_json(recording: &serde_json::Value) -> String {
    serde_json::to_string(recording).expect("synthetic recording JSON")
}

fn assert_invalid(recording: &serde_json::Value) {
    assert!(matches!(
        RecordingGuildSource::from_json(&recording_json(recording)),
        Err(GuildIntelligenceError::InvalidRecording(_))
    ));
}

fn overwrite_order_recording(overwrites: Vec<serde_json::Value>) -> serde_json::Value {
    let mut ordered = recording();
    ordered["roles"] = json!([
        {
            "ref_id": "role-everyone",
            "position": 0,
            "permissions": 1_024,
            "managed": false
        },
        {
            "ref_id": "role-send",
            "position": 10,
            "permissions": 2_048,
            "managed": true
        },
        {
            "ref_id": "role-other",
            "position": 5,
            "permissions": 4_096,
            "managed": false
        }
    ]);
    ordered["channels"] = json!([{
        "ref_id": "channel-order",
        "parent_ref": null,
        "kind": "text",
        "overwrites": overwrites
    }]);
    ordered["active_threads"] = json!([]);
    ordered["bot_self"]["role_refs"] = json!(["role-send", "role-other"]);
    ordered
}

fn channel_order_permission(recording: &serde_json::Value) -> u64 {
    RecordingGuildSource::from_json(&recording_json(recording))
        .expect("overwrite-order recording is valid")
        .replay(None)
        .expect("overwrite-order replay succeeds")
        .twin
        .effective_channel_permissions["channel-order"]
}

#[test]
fn owner_override_grants_every_effective_channel_permission() {
    let mut owner = recording();
    owner["bot_self"]["ref_id"] = owner["owner_ref"].clone();

    let replay = RecordingGuildSource::from_json(&recording_json(&owner))
        .expect("owner recording is valid")
        .replay(None)
        .expect("owner replay succeeds");

    assert_eq!(replay.twin.effective_channel_permissions.len(), 2);
    assert_eq!(
        replay.twin.effective_channel_permissions["channel-public"],
        u64::MAX
    );
    assert_eq!(
        replay.twin.effective_channel_permissions["channel-staff"],
        u64::MAX
    );
}

#[test]
fn administrator_role_override_grants_every_effective_channel_permission() {
    let mut administrator = recording();
    administrator["roles"][1]["permissions"] = (1_024_u64 | 8).into();

    let replay = RecordingGuildSource::from_json(&recording_json(&administrator))
        .expect("administrator recording is valid")
        .replay(None)
        .expect("administrator replay succeeds");

    assert_eq!(replay.twin.effective_channel_permissions.len(), 2);
    assert_eq!(
        replay.twin.effective_channel_permissions["channel-public"],
        u64::MAX
    );
    assert_eq!(
        replay.twin.effective_channel_permissions["channel-staff"],
        u64::MAX
    );
}

#[test]
fn base_union_without_overwrites_is_literal_union() {
    let ordered = overwrite_order_recording(Vec::new());

    assert_eq!(channel_order_permission(&ordered), 7_168);
}

#[test]
fn everyone_overwrite_applies_after_base_union() {
    let ordered = overwrite_order_recording(vec![json!({
        "target": { "kind": "role", "ref_id": "role-everyone" },
        "allow": 0,
        "deny": 1_024
    })]);

    assert_eq!(channel_order_permission(&ordered), 6_144);
}

#[test]
fn aggregate_role_overwrites_apply_after_everyone_overwrite() {
    let ordered = overwrite_order_recording(vec![
        json!({
            "target": { "kind": "role", "ref_id": "role-everyone" },
            "allow": 0,
            "deny": 1_024
        }),
        json!({
            "target": { "kind": "role", "ref_id": "role-send" },
            "allow": 0,
            "deny": 2_048
        }),
        json!({
            "target": { "kind": "role", "ref_id": "role-other" },
            "allow": 1_024,
            "deny": 0
        }),
    ]);

    assert_eq!(channel_order_permission(&ordered), 5_120);
}

#[test]
fn aggregate_role_allow_wins_same_bit_even_when_deny_target_sorts_later() {
    let ordered = overwrite_order_recording(vec![
        json!({
            "target": { "kind": "role", "ref_id": "role-everyone" },
            "allow": 0,
            "deny": 1_024
        }),
        json!({
            "target": { "kind": "role", "ref_id": "role-other" },
            "allow": 4_096,
            "deny": 0
        }),
        json!({
            "target": { "kind": "role", "ref_id": "role-send" },
            "allow": 0,
            "deny": 4_096
        }),
    ]);

    assert_eq!(channel_order_permission(&ordered), 6_144);
}

#[test]
fn base_union_and_overwrites_follow_discord_precedence() {
    let ordered = overwrite_order_recording(vec![
        json!({
            "target": { "kind": "role", "ref_id": "role-everyone" },
            "allow": 0,
            "deny": 1_024
        }),
        json!({
            "target": { "kind": "role", "ref_id": "role-send" },
            "allow": 0,
            "deny": 2_048
        }),
        json!({
            "target": { "kind": "role", "ref_id": "role-other" },
            "allow": 1_024,
            "deny": 0
        }),
        json!({
            "target": { "kind": "member", "ref_id": "synthetic-bot" },
            "allow": 0,
            "deny": 1_024
        }),
    ]);
    let permissions = channel_order_permission(&ordered);

    assert_eq!(permissions, 4_096);
    assert_eq!(permissions & 4_096, 4_096);
    assert_eq!(permissions & 1_024, 0);
    assert_eq!(permissions & 2_048, 0);
}

#[test]
fn missing_role_overwrite_target_fails_before_replay() {
    let mut missing_role = recording();
    missing_role["channels"][1]["overwrites"][1]["target"]["ref_id"] = "role-missing".into();

    assert_invalid(&missing_role);
}

#[test]
fn non_bot_member_overwrite_target_fails_before_replay() {
    let mut other_member = recording();
    other_member["channels"][0]["overwrites"] = json!([{
        "target": { "kind": "member", "ref_id": "synthetic-other-member" },
        "allow": 0,
        "deny": 1_024
    }]);

    assert_invalid(&other_member);
}

#[test]
fn unrecognized_overwrite_target_kind_fails_before_replay() {
    let mut unrecognized = recording();
    unrecognized["channels"][0]["overwrites"] = json!([{
        "target": { "kind": "unrecognized", "ref_id": "synthetic-unknown-target" },
        "allow": 0,
        "deny": 1_024
    }]);

    assert_invalid(&unrecognized);
}

#[test]
fn semantic_duplicate_everyone_targets_fail_before_replay() {
    let role_form = recording();
    assert!(RecordingGuildSource::from_json(&recording_json(&role_form)).is_ok());

    let mut everyone_form = recording();
    everyone_form["channels"][1]["overwrites"][0]["target"]["kind"] = "everyone".into();
    assert!(RecordingGuildSource::from_json(&recording_json(&everyone_form)).is_ok());

    let mut duplicate_everyone = recording();
    duplicate_everyone["channels"][1]["overwrites"]
        .as_array_mut()
        .expect("fixture overwrite array")
        .push(json!({
            "target": { "kind": "everyone", "ref_id": "role-everyone" },
            "allow": 0,
            "deny": 0
        }));

    assert_invalid(&duplicate_everyone);
}
