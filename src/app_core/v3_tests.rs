use super::*;

#[test]
fn deny_all_allows_only_negotiation() {
    let grants = V3CapabilitySet::deny_all();
    let negotiation = V3Command::Negotiate(V3GrantRequest {
        supported_versions: vec![1, 2, 3],
        requested: V3CapabilitySet::deny_all(),
    });
    assert!(grants.permits(&negotiation));
    assert!(!grants.permits(&V3Command::ListTools(V3PageQuery::default())));
}

#[test]
fn grants_are_ordered_duplicate_free_and_exact() {
    let grants =
        V3CapabilitySet::from_sorted(vec![V3Capability::ListTools, V3Capability::InvokeTools])
            .unwrap();
    assert!(grants.permits(&V3Command::ListTools(V3PageQuery::default())));
    assert!(!grants.permits(&V3Command::CancelTool(V3Action {
        resource_id: "call-1".into(),
        operation_id: "cancel-1".into(),
    })));
    assert!(
        V3CapabilitySet::from_sorted(vec![V3Capability::InvokeTools, V3Capability::ListTools])
            .is_err()
    );
    assert!(
        serde_json::from_value::<V3CapabilitySet>(serde_json::json!({
            "capabilities": ["list_tools", "list_tools"]
        }))
        .is_err()
    );
}

#[test]
fn approvals_are_bound_to_an_exact_lowercase_digest() {
    let valid = V3Command::ApproveTool(V3ToolDecision {
        call_id: "call-1".into(),
        call_digest: "a".repeat(64),
        decision_id: "decision-1".into(),
    });
    valid.validate().unwrap();

    let invalid = V3Command::DenyTool(V3ToolDecision {
        call_id: "call-1".into(),
        call_digest: "A".repeat(64),
        decision_id: "decision-1".into(),
    });
    assert!(invalid.validate().is_err());
}

#[test]
fn command_wire_shape_is_separate_and_strict() {
    let command = V3Command::ClaimById(V3ResourceQuery {
        resource_id: "local-model-inference".into(),
    });
    assert_eq!(
        serde_json::to_value(command).unwrap(),
        serde_json::json!({
            "type": "claim_by_id",
                "payload": {
                    "resource_id": "local-model-inference"
            }
        })
    );
    assert!(
        serde_json::from_value::<V3Command>(serde_json::json!({
            "type": "list_tools",
            "payload": {"after": 0, "through": null, "limit": 32, "extra": true}
        }))
        .is_err()
    );
}

#[test]
fn tool_input_is_object_bounded_and_depth_limited() {
    let descriptor = V3ToolDescriptor {
        tool_id: "abbey.status".into(),
        description: "Read bounded Abbey status".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {"verbose": {"type": "boolean"}},
            "additionalProperties": false
        }),
    };
    V3Event::Tools(V3ToolPage {
        after: 0,
        through: 1,
        tools: vec![descriptor],
    })
    .validate()
    .unwrap();

    let valid = V3ToolCall {
        tool_id: "abbey.status".into(),
        call_id: "call-1".into(),
        input: serde_json::json!({"verbose": false}),
    };
    valid.validate().unwrap();
    let mut invalid = valid.clone();
    invalid.input = serde_json::json!(["not-an-object"]);
    assert!(invalid.validate().is_err());

    let mut nested = serde_json::json!({});
    for _ in 0..=32 {
        nested = serde_json::json!({"nested": nested});
    }
    let mut invalid = valid;
    invalid.input = nested;
    assert!(invalid.validate().is_err());
}

#[test]
fn tool_results_are_terminal_correlated_and_bounded() {
    let valid = V3ToolResult {
        tool_id: "abbey_status".into(),
        call_id: "call-1".into(),
        state: V3OperationState::Succeeded,
        output: serde_json::json!({"edition": "standard"}),
    };
    V3Event::ToolResult(valid.clone()).validate().unwrap();

    let mut running = valid.clone();
    running.state = V3OperationState::Running;
    assert!(running.validate().is_err());

    let mut oversized = valid;
    oversized.output = serde_json::json!({"text": "x".repeat(33 * 1_024)});
    assert!(oversized.validate().is_err());
}

#[test]
fn pages_reject_future_cursors_and_invalid_event_order() {
    assert!(
        V3PageQuery {
            after: 2,
            through: Some(1),
            limit: 1,
        }
        .validate()
        .is_err()
    );
    assert!(
        V3MetricQuery {
            operation_id: "train-1".into(),
            page: V3PageQuery {
                after: 3,
                through: Some(2),
                limit: 1,
            },
        }
        .validate()
        .is_err()
    );
    let status = |sequence| V3EventRecord {
        sequence,
        operation: V3OperationStatus {
            operation_id: "op-1".into(),
            resource_id: "resource-1".into(),
            state: V3OperationState::Running,
            progress_basis_points: 500,
        },
    };
    assert!(
        V3EventPage {
            after: 0,
            through: 2,
            events: vec![status(2), status(1)],
        }
        .validate()
        .is_err()
    );
}

#[test]
fn negotiation_and_events_are_versioned_and_bounded() {
    let request = V3GrantRequest {
        supported_versions: vec![1, 2, 3],
        requested: V3CapabilitySet::deny_all(),
    };
    let negotiation = V3GrantNegotiation {
        protocol_version: APP_PROTOCOL_V3,
        schema_version: APP_SCHEMA_V3,
        granted: V3CapabilitySet::deny_all(),
    };
    negotiation.validate_for(&request).unwrap();
    let negotiated = V3Event::Negotiated(negotiation);
    negotiated.validate().unwrap();

    let unexpected_grant = V3GrantNegotiation {
        protocol_version: APP_PROTOCOL_V3,
        schema_version: APP_SCHEMA_V3,
        granted: V3CapabilitySet::from_sorted(vec![V3Capability::ListTools]).unwrap(),
    };
    assert!(unexpected_grant.validate_for(&request).is_err());

    let non_finite = V3Event::TrainingMetrics(V3MetricPage {
        operation_id: "train-1".into(),
        after: 0,
        through: 1,
        metrics: vec![V3Metric {
            name: "loss".into(),
            step: 1,
            value: f64::NAN,
        }],
    });
    assert!(non_finite.validate().is_err());
}
