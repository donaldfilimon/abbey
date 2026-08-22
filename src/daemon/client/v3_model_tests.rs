//! Typed send-once exact-model inference client tests.

use super::*;
use crate::app_core::{V3ModelDevice, V3ModelInferenceRequest, V3ModelInferenceResult};

fn request() -> V3ModelInferenceRequest {
    V3ModelInferenceRequest::new(
        "fixture-bigram",
        "0123456789abcdef0123456789abcdef01234567",
        "hello",
        4,
    )
    .unwrap()
}

#[test]
fn inference_echoes_the_exact_grant_and_recomputes_all_correlation_digests() {
    let root = scratch_dir("v3-model-infer");
    let config = test_config(root.join("abbeyd.sock"));
    let listener = UnixListener::bind(&config.socket_path).unwrap();
    let thread = thread::spawn(move || {
        let (mut negotiation_stream, _) = listener.accept().unwrap();
        let negotiation_request = read_v3_request(&mut negotiation_stream);
        let grants = V3CapabilitySet::from_sorted(vec![V3Capability::InferModels]).unwrap();
        negotiation_stream
            .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                negotiation_request.request_id,
                V3Event::Negotiated(V3GrantNegotiation {
                    protocol_version: crate::app_core::APP_PROTOCOL_V3,
                    schema_version: crate::app_core::APP_SCHEMA_V3,
                    granted: grants.clone(),
                }),
            )))
            .unwrap();

        for valid_digest in [true, false] {
            let (mut stream, _) = listener.accept().unwrap();
            let envelope = read_v3_request(&mut stream);
            assert_eq!(envelope.grants, grants);
            let V3Command::InferModel(request) = envelope.command else {
                panic!("expected exact-model inference request")
            };
            let mut result = V3ModelInferenceResult {
                model_id: request.model_id,
                revision: request.revision,
                request_digest: request.request_digest,
                output_digest: String::new(),
                output: "world again".into(),
                requested_output_tokens: request.max_output_tokens,
                prompt_tokens: 1,
                output_tokens: 2,
                requested_device: V3ModelDevice::Cpu,
                executed_device: V3ModelDevice::Cpu,
                native_operations: 9,
                fallback_used: false,
                mixed_execution: false,
            };
            result.output_digest = if valid_digest {
                result.computed_output_digest()
            } else {
                "0".repeat(64)
            };
            stream
                .write_all(&encoded_v3_response(V3ResponseEnvelope::ok(
                    envelope.request_id,
                    V3Event::ModelInference(result),
                )))
                .unwrap();
        }
    });

    let grants = V3CapabilitySet::from_sorted(vec![V3Capability::InferModels]).unwrap();
    let session = DaemonClient::new(config).negotiate_v3(grants).unwrap();
    let result = session.infer_model(request()).unwrap();
    assert_eq!(result.output, "world again");
    assert!(matches!(
        session.infer_model(request()),
        Err(ClientError::InvalidV3Response)
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn inference_stops_locally_when_its_separate_grant_is_denied() {
    let requested = V3CapabilitySet::from_sorted(vec![V3Capability::InferModels]).unwrap();
    let (config, thread, root) = fake_v3_server(|request| {
        encoded_v3_response(V3ResponseEnvelope::ok(
            request.request_id,
            V3Event::Negotiated(V3GrantNegotiation {
                protocol_version: crate::app_core::APP_PROTOCOL_V3,
                schema_version: crate::app_core::APP_SCHEMA_V3,
                granted: V3CapabilitySet::deny_all(),
            }),
        ))
    });
    let session = DaemonClient::new(config).negotiate_v3(requested).unwrap();
    assert!(matches!(
        session.infer_model(request()),
        Err(ClientError::V3CapabilityNotGranted {
            capability: V3Capability::InferModels
        })
    ));
    thread.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
