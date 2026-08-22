use super::*;

fn request() -> V3ModelInferenceRequest {
    V3ModelInferenceRequest::new(
        "fixture-bigram",
        "0123456789abcdef0123456789abcdef01234567",
        "hello world",
        4,
    )
    .unwrap()
}

fn result(request: &V3ModelInferenceRequest) -> V3ModelInferenceResult {
    let mut result = V3ModelInferenceResult {
        model_id: request.model_id.clone(),
        revision: request.revision.clone(),
        request_digest: request.request_digest.clone(),
        output_digest: String::new(),
        output: "again".into(),
        requested_output_tokens: request.max_output_tokens,
        prompt_tokens: 2,
        output_tokens: 1,
        requested_device: V3ModelDevice::Cpu,
        executed_device: V3ModelDevice::Cpu,
        native_operations: 6,
        fallback_used: false,
        mixed_execution: false,
    };
    result.output_digest = result.computed_output_digest();
    result
}

#[test]
fn request_digest_binds_every_bounded_field() {
    let request = request();
    request.validate().unwrap();
    assert_eq!(request.request_digest, request.computed_digest());

    let mut changed = request.clone();
    changed.prompt.push_str(" again");
    assert!(changed.validate().is_err());
    let mut empty = request.clone();
    empty.prompt = " ".into();
    empty.request_digest = empty.computed_digest();
    assert!(empty.validate().is_err());
    assert!(V3ModelInferenceRequest::new("model", "revision", "prompt", 257).is_err());
}

#[test]
fn result_requires_exact_digests_native_single_device_evidence_and_bounds() {
    let request = request();
    let valid = result(&request);
    valid.validate_for(&request).unwrap();

    let mut fallback = valid.clone();
    fallback.fallback_used = true;
    assert!(fallback.validate_for(&request).is_err());
    let mut mixed = valid.clone();
    mixed.mixed_execution = true;
    assert!(mixed.validate_for(&request).is_err());
    let mut substituted = valid.clone();
    substituted.executed_device = V3ModelDevice::Metal;
    assert!(substituted.validate_for(&request).is_err());
    let mut oversized = valid;
    oversized.output = "x".repeat(MAX_V3_MODEL_OUTPUT_BYTES + 1);
    oversized.output_digest = oversized.computed_output_digest();
    assert!(oversized.validate_for(&request).is_err());
}
