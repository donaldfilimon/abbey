//! Explicit local Rust model-family collection, training and release pipeline.
mod cli;
mod data;
pub(crate) mod io;
mod learning;
mod native;
mod pipeline;
mod registry;

/// Execute an explicitly selected operation. Worker prefixes name the embedding
/// executable's command route; no interpreter or alternate model is substituted.
pub fn run(args: &[String], worker_prefix: &[String]) -> anyhow::Result<serde_json::Value> {
    cli::run(args, worker_prefix)
}

#[cfg(test)]
mod tests;
