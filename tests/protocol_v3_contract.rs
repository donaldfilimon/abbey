use abbey::app_core::{
    APP_PROTOCOL_V1, APP_PROTOCOL_V3, APP_PROTOCOL_VERSION, AppCommand, V3CapabilitySet, V3Command,
    V3GrantRequest,
};
use abbey::daemon::{CURRENT_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS};

#[test]
fn v3_is_additive_and_not_daemon_authority() {
    assert_eq!(APP_PROTOCOL_V1, 1);
    assert_eq!(APP_PROTOCOL_VERSION, 2);
    assert_eq!(APP_PROTOCOL_V3, 3);
    assert_eq!(CURRENT_PROTOCOL_VERSION, 2);
    assert_eq!(SUPPORTED_PROTOCOL_VERSIONS, &[1, 2]);
    assert!(!SUPPORTED_PROTOCOL_VERSIONS.contains(&APP_PROTOCOL_V3));

    assert_eq!(
        serde_json::to_value(AppCommand::Status).unwrap(),
        serde_json::json!({"type": "status"})
    );

    let negotiation = V3Command::Negotiate(V3GrantRequest {
        supported_versions: vec![1, 2, 3],
        requested: V3CapabilitySet::deny_all(),
    });
    assert_eq!(negotiation.minimum_protocol_version(), 3);
    negotiation.validate().unwrap();
    assert_eq!(
        serde_json::to_value(negotiation).unwrap(),
        serde_json::json!({
            "type": "negotiate",
            "payload": {
                "supported_versions": [1, 2, 3],
                "requested": {"capabilities": []}
            }
        })
    );
}
