//! Accelerator **kernel execution** bridge — `abbey accel verify`.
//!
//! Behind `--features accel` this runs a fixed set of numerical kernels through
//! ABI's `MetalAccelerator` (`abi-gpu`) and checks every natively produced value
//! against the deterministic `CpuBackend` oracle (`abi-compute`) in the same
//! process and the same run. Without the feature the verb refuses with exit
//! code 2 — it never silently no-ops.
//!
//! The reporting vocabulary lives in the `bridge` submodule and is compiled
//! only with the feature, because without a bridge there is nothing to
//! describe.
//!
//! # What a green run does NOT prove
//!
//! It is evidence of exactly one thing: a numerical kernel executed on Metal
//! and its result matched a CPU oracle. It is not GPU/NPU/TPU compilation,
//! training, or model inference; not device residency or placement (Metal does
//! not expose that here, and neither does `CoreML` — see this project's
//! recorded lesson); and not any speedup, because nothing here is timed. See
//! the `accelerator-kernel-execution-metal` claim.

use anyhow::Result;

/// Exit code when this binary was built without the `accel` feature.
///
/// Only defined in builds that can actually return it, so a build carrying the
/// bridge cannot accidentally reference an "unavailable" code.
#[cfg(not(feature = "accel"))]
pub const EXIT_UNAVAILABLE: i32 = 2;

#[cfg(feature = "accel")]
mod bridge;
#[cfg(feature = "accel")]
pub use bridge::{render, verify};

/// `abbey accel verify [--json]`.
///
/// Exit codes: `0` verified · `1` compiled but not verified · `2` built
/// without the feature.
pub fn verify_command(args: &[String]) -> Result<i32> {
    let json = args.iter().any(|a| a == "--json");
    run_verify(json)
}

#[cfg(feature = "accel")]
fn run_verify(json: bool) -> Result<i32> {
    let report = verify()?;
    if json {
        crate::output::print(serde_json::to_string_pretty(&report)?)?;
    } else {
        crate::output::print(render(&report))?;
    }
    Ok(report.exit_code())
}

#[cfg(not(feature = "accel"))]
fn run_verify(_json: bool) -> Result<i32> {
    eprintln!(
        "abbey: refused — `abbey accel verify` executes numerical kernels through ABI's\n\
         MetalAccelerator, and this binary was built without that bridge.\n\
         Rebuild with `--features accel` (macOS + Xcode toolchain: abi-gpu's build\n\
         script compiles a Metal/CoreML dylib with `xcrun swiftc`).\n\
         \n\
         Still available without the feature:\n\
         \x20 abbey accel detect   — host accelerator presence inventory (report-only)\n\
         \x20 abbey accel status   — what remains Proposed\n\
         \n\
         Note: even with the feature, this verifies numerical kernel execution and\n\
         CPU-oracle parity only — never compilation, training, inference, device\n\
         placement, or speedup."
    );
    Ok(EXIT_UNAVAILABLE)
}

#[cfg(test)]
mod tests;
