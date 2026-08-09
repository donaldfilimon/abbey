use abbey::app_core::{
    APP_PROTOCOL_VERSION, AppCapability, AppCommand, AppEvent, AppService, CapabilitySet,
    ClaimStatus, ClaimsQuery, ConversationId, Edition, RunId,
};

/// The compiled edition this test binary was built for. The safe build must
/// still report `standard`; the personal build must report itself rather than
/// impersonating the public edition.
#[cfg(not(feature = "personal-edition"))]
const EXPECTED_EDITION: Edition = Edition::Standard;
#[cfg(feature = "personal-edition")]
const EXPECTED_EDITION: Edition = Edition::Personal;

#[test]
fn public_identifiers_and_commands_have_stable_wire_shapes() {
    let run = RunId::new();
    let conversation = ConversationId::new();
    assert_eq!(
        serde_json::from_str::<RunId>(&serde_json::to_string(&run).unwrap()).unwrap(),
        run
    );
    assert_eq!(
        serde_json::from_str::<ConversationId>(&serde_json::to_string(&conversation).unwrap())
            .unwrap(),
        conversation
    );

    let command = AppCommand::Claims(ClaimsQuery {
        status: Some(ClaimStatus::Proposed),
        contains: Some("desktop".into()),
    });
    assert_eq!(
        serde_json::to_value(command).unwrap(),
        serde_json::json!({
            "type": "claims",
            "payload": {"status": "proposed", "contains": "desktop"}
        })
    );
    assert!(serde_json::from_str::<AppCommand>(r#"{"type":"unknown"}"#).is_err());
}

#[test]
fn standard_service_advertises_only_implemented_read_capabilities() {
    let event = AppService::default().handle(AppCommand::Status).unwrap();
    let AppEvent::Status(status) = event else {
        panic!("status command must return a status event");
    };

    assert_eq!(status.protocol_version, APP_PROTOCOL_VERSION);
    assert_eq!(status.edition, EXPECTED_EDITION);
    assert_eq!(
        status.capabilities.as_slice(),
        &[AppCapability::ReadStatus, AppCapability::ReadClaims]
    );
    assert!(status.capabilities.contains(AppCapability::ReadStatus));
    assert!(status.capabilities.contains(AppCapability::ReadClaims));
}

#[test]
fn capability_deserialization_fails_closed_on_duplicates_or_reordering() {
    let duplicate = serde_json::from_value::<CapabilitySet>(serde_json::json!({
        "capabilities": ["read_status", "read_status"]
    }))
    .unwrap();
    assert!(duplicate.validate().is_err());

    let reordered = serde_json::from_value::<CapabilitySet>(serde_json::json!({
        "capabilities": ["read_claims", "read_status"]
    }))
    .unwrap();
    assert!(reordered.validate().is_err());
}
