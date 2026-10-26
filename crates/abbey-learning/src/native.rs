//! Native Rust causal-decoder execution, LoRA updates and model exports.
//! Operators and autograd use the pinned Candle Rust implementation. No
//! interpreter, remote code, model download or provider fallback is invoked.
mod model;
#[cfg(test)]
mod tests;
mod weights;
mod worker;

pub(crate) fn execute(request: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    worker::execute(request)
}
