//! Synchronous bounded inference for one exact loaded model revision.

use std::time::Duration;

use abi_agent_runtime::{
    CancellationToken, EventSink, ModelEvent, ModelRequest, RunBudget, StopReason, run_provider,
};
use abi_model_runtime::{DevicePreference, ExecutionPath};
use serde_json::json;

use crate::app_core::{
    MAX_V3_MODEL_OUTPUT_BYTES, V3Event, V3ModelDevice, V3ModelInferenceRequest,
    V3ModelInferenceResult,
};
use crate::runtime::{AuditMetadata, NewAuditEvent, StoreError};

use super::{
    HandlerFailure, ModelKey, ModelLifecycleAuthority, invalid_command_failure, not_found_failure,
    runtime_failure,
};

impl ModelLifecycleAuthority {
    pub(in crate::daemon) fn infer(
        &self,
        request: V3ModelInferenceRequest,
    ) -> Result<V3Event, HandlerFailure> {
        request.validate().map_err(|_| invalid_command_failure())?;
        self.validate_model_revision(&request.model_id, &request.revision)?;
        let active = self.acquire_exact(&request.model_id, &request.revision)?;
        let result = self.infer_locked(&request);
        self.release_active(&active);
        result
    }

    fn infer_locked(&self, request: &V3ModelInferenceRequest) -> Result<V3Event, HandlerFailure> {
        let provider = self
            .inner
            .loaded
            .lock()
            .map_err(|_| runtime_failure())?
            .get(&ModelKey {
                model_id: request.model_id.clone(),
                revision: request.revision.clone(),
            })
            .cloned()
            .ok_or_else(not_found_failure)?;
        let model_request = ModelRequest::new(&request.model_id)
            .with_user(request.prompt.clone())
            .with_max_output_tokens(u64::from(request.max_output_tokens));
        let mut sink = ExactModelSink::new(&request.model_id);
        let cancellation = CancellationToken::new();
        let budget = RunBudget::unlimited()
            .with_max_events(u64::from(request.max_output_tokens) + 2)
            .with_max_output_tokens(u64::from(request.max_output_tokens))
            .with_max_duration(Duration::from_secs(1));
        let run = run_provider(
            provider.as_ref(),
            &model_request,
            &mut sink,
            &cancellation,
            budget,
        )
        .map_err(|_| runtime_failure())?;
        if run.stop != StopReason::Completed
            || run.output.truncated()
            || run.text().len() > MAX_V3_MODEL_OUTPUT_BYTES
            || !sink.valid()
        {
            return Err(runtime_failure());
        }
        let evidence = provider
            .last_inference_report()
            .map_err(|_| runtime_failure())?
            .ok_or_else(runtime_failure)?;
        let load = provider.load_report();
        if load.model_id != request.model_id
            || load.revision != request.revision
            || evidence.model_id != request.model_id
            || evidence.prompt_tokens
                != usize::try_from(run.usage.input_tokens).unwrap_or(usize::MAX)
            || evidence.generated_tokens
                != usize::try_from(run.usage.output_tokens).unwrap_or(usize::MAX)
            || !evidence.capability.executed()
            || !evidence.capability.runtime_verified()
        {
            return Err(runtime_failure());
        }
        let mut result = V3ModelInferenceResult {
            model_id: request.model_id.clone(),
            revision: request.revision.clone(),
            request_digest: request.request_digest.clone(),
            output_digest: String::new(),
            output: run.output.into_text(),
            requested_output_tokens: request.max_output_tokens,
            prompt_tokens: u32::try_from(evidence.prompt_tokens).map_err(|_| runtime_failure())?,
            output_tokens: u32::try_from(evidence.generated_tokens)
                .map_err(|_| runtime_failure())?,
            requested_device: model_device(evidence.requested_device),
            executed_device: executed_device(evidence.executed_path),
            native_operations: evidence.native_operations,
            fallback_used: evidence.fallback_used,
            mixed_execution: evidence.mixed_execution,
        };
        result.output_digest = result.computed_output_digest();
        result
            .validate_for(request)
            .map_err(|_| runtime_failure())?;
        self.audit_inference(&result)
            .map_err(|_| runtime_failure())?;
        Ok(V3Event::ModelInference(result))
    }

    fn audit_inference(&self, result: &V3ModelInferenceResult) -> Result<(), StoreError> {
        self.inner.store.record_audit(NewAuditEvent {
            run_id: None,
            action: "v3_model_inference".to_owned(),
            outcome: "succeeded".to_owned(),
            metadata: AuditMetadata::new(json!({
                "model_id": result.model_id,
                "revision": result.revision,
                "request_digest": result.request_digest,
                "output_digest": result.output_digest,
                "output_bytes": result.output.len(),
                "requested_output_tokens": result.requested_output_tokens,
                "prompt_tokens": result.prompt_tokens,
                "output_tokens": result.output_tokens,
                "requested_device": device_name(result.requested_device),
                "executed_device": device_name(result.executed_device),
                "native_operations": result.native_operations,
                "fallback_used": result.fallback_used,
                "mixed_execution": result.mixed_execution
            }))?,
        })?;
        Ok(())
    }
}

struct ExactModelSink<'a> {
    expected_model: &'a str,
    started: bool,
    terminal: bool,
    invalid: bool,
}

impl<'a> ExactModelSink<'a> {
    const fn new(expected_model: &'a str) -> Self {
        Self {
            expected_model,
            started: false,
            terminal: false,
            invalid: false,
        }
    }

    const fn valid(&self) -> bool {
        self.started && self.terminal && !self.invalid
    }
}

impl EventSink for ExactModelSink<'_> {
    fn emit(&mut self, event: &ModelEvent) {
        match event {
            ModelEvent::Started { model }
                if !self.started && !self.terminal && model == self.expected_model =>
            {
                self.started = true;
            }
            ModelEvent::TextDelta { .. } if self.started && !self.terminal => {}
            ModelEvent::Finished {
                stop: StopReason::Completed,
            } if self.started && !self.terminal => self.terminal = true,
            _ => self.invalid = true,
        }
    }
}

const fn model_device(device: DevicePreference) -> V3ModelDevice {
    match device {
        DevicePreference::Cpu => V3ModelDevice::Cpu,
        DevicePreference::Metal => V3ModelDevice::Metal,
        DevicePreference::Cuda => V3ModelDevice::Cuda,
    }
}

const fn executed_device(device: ExecutionPath) -> V3ModelDevice {
    match device {
        ExecutionPath::Cpu => V3ModelDevice::Cpu,
        ExecutionPath::Metal => V3ModelDevice::Metal,
        ExecutionPath::Cuda => V3ModelDevice::Cuda,
    }
}

const fn device_name(device: V3ModelDevice) -> &'static str {
    match device {
        V3ModelDevice::Cpu => "cpu",
        V3ModelDevice::Metal => "metal",
        V3ModelDevice::Cuda => "cuda",
    }
}
