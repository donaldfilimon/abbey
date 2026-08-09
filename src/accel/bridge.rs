//! Kernel execution and CPU-oracle parity evidence (`--features accel`).
//!
//! # Why the evidence is read from the adapter, not from the return value
//!
//! `MetalAccelerator` returns the CPU oracle whenever the native kernel is
//! unavailable, so comparing its return value to the oracle is *vacuous* unless
//! a native kernel actually ran. Execution is therefore read from the adapter's
//! own `CapabilityState` evidence ladder, and each kernel gets a **fresh**
//! adapter because `executed` is sticky once set — one shared adapter could not
//! tell you which kernel ran natively. Parity is recorded only when the
//! transition to `executed` is observed for that call; otherwise it stays
//! `None` and the report says the CPU produced the value.

use abi_compute::{Accelerator, CpuBackend, ScoredIndex};
use abi_gpu::{MetalAccelerator, metal_kernels};
use anyhow::Result;
use serde::Serialize;
use std::fmt::Write as _;

/// Relative tolerance for float parity between a native and an oracle value.
///
/// Compared as `|native - oracle| <= TOL * max(|native|, |oracle|, 1.0)`, i.e.
/// relative for large magnitudes and absolute near zero. `1e-3` matches the
/// tolerance ABI's own adapter uses internally and is deliberately not made
/// tighter: GPU reductions accumulate in a different order than the CPU SIMD
/// oracle, and that divergence grows with element count.
pub const PARITY_RELATIVE_TOLERANCE: f32 = 1e-3;

/// Elements per vector in the kernel fixtures.
pub const KERNEL_ELEMENTS: usize = 1024;

/// Candidate vectors ranked by the `top_k` fixture.
pub const TOP_K_CANDIDATES: usize = 8;

/// Ranked positions requested from the `top_k` fixture.
pub const TOP_K_LIMIT: usize = 4;

/// Claims a green `verify` explicitly does not establish.
pub const NOT_PROOF_OF: [&str; 4] = [
    "gpu_npu_tpu_compilation",
    "training_or_model_inference",
    "device_residency_or_placement",
    "speedup_or_performance",
];

/// Exit code when the feature is compiled but nothing was verified.
pub const EXIT_NOT_VERIFIED: i32 = 1;

/// Which backend actually produced the reported results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendUsed {
    /// Every kernel executed on the native Metal path.
    GpuMetal,
    /// No kernel executed natively; the deterministic CPU oracle served all.
    Cpu,
    /// Some kernels executed natively and some fell back.
    Mixed,
}

impl BackendUsed {
    /// Stable lowercase label, matching `abi_compute::Backend::name` spelling.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::GpuMetal => "gpu-metal",
            Self::Cpu => "cpu",
            Self::Mixed => "mixed",
        }
    }
}

/// One kernel's execution and parity evidence.
#[derive(Debug, Clone, Serialize)]
pub struct KernelCheck {
    /// Kernel name as exposed by the adapter (`dot`, `cosine`, `top_k`).
    pub kernel: &'static str,
    /// Elements per input vector.
    pub elements: usize,
    /// Whether the native Metal kernel ran for *this* call.
    pub executed_natively: bool,
    /// Parity against the CPU oracle, or `None` when nothing native ran.
    ///
    /// `None` rather than `true` on the fallback path is deliberate: the
    /// adapter returns the oracle itself there, so a comparison would pass
    /// without proving anything.
    pub oracle_parity: Option<bool>,
    /// Observed values plus the adapter's own evidence message.
    pub detail: String,
}

impl KernelCheck {
    /// Whether this kernel both ran natively and matched the oracle.
    #[must_use]
    pub fn verified(&self) -> bool {
        self.executed_natively && self.oracle_parity == Some(true)
    }
}

/// Result of one `abbey accel verify` run.
#[derive(Debug, Clone, Serialize)]
pub struct AccelReport {
    /// Cargo feature that compiled this bridge.
    pub feature: &'static str,
    /// Adapter driven by the bridge.
    pub adapter: &'static str,
    /// Whether the Metal kernel dylib is linked into this build.
    pub kernels_linked: bool,
    /// Whether a Metal device and pipeline initialized at runtime.
    pub device_initialized: bool,
    /// Backend the bridge asked for.
    pub backend_requested: &'static str,
    /// Backend that actually produced the results.
    pub backend_used: BackendUsed,
    /// Relative tolerance applied to float parity.
    pub relative_tolerance: f32,
    /// Per-kernel evidence.
    pub kernels: Vec<KernelCheck>,
    /// Every kernel executed natively *and* matched the oracle.
    pub verified: bool,
    /// Claims this run does not establish.
    pub not_proof_of: [&'static str; 4],
}

impl AccelReport {
    /// Summarize execution and parity across a kernel set.
    #[must_use]
    pub fn summarize(
        kernels_linked: bool,
        device_initialized: bool,
        kernels: Vec<KernelCheck>,
    ) -> Self {
        let native = kernels.iter().filter(|k| k.executed_natively).count();
        let backend_used = if kernels.is_empty() || native == 0 {
            BackendUsed::Cpu
        } else if native == kernels.len() {
            BackendUsed::GpuMetal
        } else {
            BackendUsed::Mixed
        };
        let verified = !kernels.is_empty() && kernels.iter().all(KernelCheck::verified);
        Self {
            feature: "accel",
            adapter: "abi-gpu MetalAccelerator",
            kernels_linked,
            device_initialized,
            backend_requested: "gpu-metal",
            backend_used,
            relative_tolerance: PARITY_RELATIVE_TOLERANCE,
            kernels,
            verified,
            not_proof_of: NOT_PROOF_OF,
        }
    }

    /// Process exit code for this report.
    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        if self.verified { 0 } else { EXIT_NOT_VERIFIED }
    }
}

/// Relative-with-absolute-floor float comparison.
#[must_use]
pub fn relative_close(left: f32, right: f32, tolerance: f32) -> bool {
    (left - right).abs() <= tolerance * left.abs().max(right.abs()).max(1.0)
}

/// Deterministic query vector.
///
/// Values are exact binary fractions, so the only native/oracle divergence is
/// reduction order rather than input rounding.
#[must_use]
pub fn fixture_query(elements: usize) -> Vec<f32> {
    (0..elements)
        .map(|i| (i % 17) as f32 * 0.125 - 1.0)
        .collect()
}

/// Deterministic second operand, on a different period than the query.
#[must_use]
pub fn fixture_operand(elements: usize) -> Vec<f32> {
    (0..elements)
        .map(|i| (i % 23) as f32 * 0.0625 - 0.5)
        .collect()
}

/// Candidate `rank` for the `top_k` fixture.
///
/// Each candidate blends the query with a fixed unrelated vector, with the
/// query's weight decreasing as `rank` grows. Cosine scores are therefore
/// strictly separated and the expected ranking is unambiguous — a tie would
/// make the index comparison depend on tie-break order rather than on the
/// kernel.
#[must_use]
pub fn fixture_candidate(query: &[f32], operand: &[f32], rank: usize) -> Vec<f32> {
    let weight = 1.0 - (rank as f32) * 0.1;
    query
        .iter()
        .zip(operand)
        .map(|(q, o)| q * weight + o * (1.0 - weight))
        .collect()
}

/// Execute the kernel set and collect parity evidence.
pub fn verify() -> Result<AccelReport> {
    let cpu = CpuBackend::default();
    let query = fixture_query(KERNEL_ELEMENTS);
    let operand = fixture_operand(KERNEL_ELEMENTS);
    let mut kernels = Vec::with_capacity(3);

    {
        let metal = MetalAccelerator::new();
        let before = metal.capability().executed();
        let native = metal.dot(&query, &operand)?;
        let executed = metal.capability().executed() && !before;
        let oracle = cpu.dot(&query, &operand)?;
        kernels.push(scalar_check(
            "dot",
            executed,
            native,
            oracle,
            metal.capability().message(),
        ));
    }

    {
        let metal = MetalAccelerator::new();
        let before = metal.capability().executed();
        let native = metal.cosine(&query, &operand)?;
        let executed = metal.capability().executed() && !before;
        let oracle = cpu.cosine(&query, &operand)?;
        kernels.push(scalar_check(
            "cosine",
            executed,
            native,
            oracle,
            metal.capability().message(),
        ));
    }

    kernels.push(top_k_check(&cpu, &query, &operand)?);

    Ok(AccelReport::summarize(
        metal_kernels::kernels_linked(),
        metal_kernels::kernels_active(),
        kernels,
    ))
}

/// Run the batch ranking kernel. Index parity is exact; only scores use the
/// float tolerance.
fn top_k_check(cpu: &CpuBackend, query: &[f32], operand: &[f32]) -> Result<KernelCheck> {
    let candidates: Vec<Vec<f32>> = (0..TOP_K_CANDIDATES)
        .map(|rank| fixture_candidate(query, operand, rank))
        .collect();
    let refs: Vec<&[f32]> = candidates.iter().map(Vec::as_slice).collect();

    let metal = MetalAccelerator::new();
    let before = metal.capability().executed();
    let native = metal.top_k(query, &refs, TOP_K_LIMIT)?;
    let executed = metal.capability().executed() && !before;
    let oracle = cpu.top_k(query, &refs, TOP_K_LIMIT)?;

    let parity = executed.then(|| {
        native.len() == oracle.len()
            && native.iter().zip(&oracle).all(|(n, o)| {
                n.index == o.index && relative_close(n.score, o.score, PARITY_RELATIVE_TOLERANCE)
            })
    });
    let message = metal.capability().message();
    let detail = if executed {
        format!(
            "native ranking=[{}] oracle ranking=[{}] · {message}",
            ranking(&native),
            ranking(&oracle),
        )
    } else {
        format!(
            "no native kernel ran; adapter returned the CPU oracle ranking [{}] · {message}",
            ranking(&oracle),
        )
    };

    Ok(KernelCheck {
        kernel: "top_k",
        elements: KERNEL_ELEMENTS,
        executed_natively: executed,
        oracle_parity: parity,
        detail,
    })
}

fn ranking(scored: &[ScoredIndex]) -> String {
    scored
        .iter()
        .map(|s| s.index.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Build a [`KernelCheck`] for a scalar-valued kernel.
fn scalar_check(
    kernel: &'static str,
    executed: bool,
    native: f32,
    oracle: f32,
    message: &str,
) -> KernelCheck {
    let parity = executed.then(|| relative_close(native, oracle, PARITY_RELATIVE_TOLERANCE));
    let detail = if executed {
        format!("native={native:.6} oracle={oracle:.6} · {message}")
    } else {
        format!("no native kernel ran; adapter returned the CPU oracle ({oracle:.6}) · {message}")
    };
    KernelCheck {
        kernel,
        elements: KERNEL_ELEMENTS,
        executed_natively: executed,
        oracle_parity: parity,
        detail,
    }
}

/// One-line verdict for a report.
#[must_use]
pub fn verdict(report: &AccelReport) -> &'static str {
    if report.verified {
        "VERIFIED — every kernel executed on Metal and matched the CPU oracle"
    } else if report
        .kernels
        .iter()
        .any(|k| k.oracle_parity == Some(false))
    {
        "MISMATCH — a native kernel disagreed with the CPU oracle (see above)"
    } else {
        "NOT VERIFIED — no native Metal execution; the CPU oracle served every kernel"
    }
}

/// Render a report for a terminal.
#[must_use]
pub fn render(report: &AccelReport) -> String {
    let mut text =
        String::from("abbey accel verify — numerical kernels via ABI MetalAccelerator\n\n");
    let _ = writeln!(text, "adapter:        {}", report.adapter);
    let _ = writeln!(text, "kernels linked: {}", yes_no(report.kernels_linked));
    let _ = writeln!(
        text,
        "device init:    {}",
        yes_no(report.device_initialized)
    );
    let _ = writeln!(text, "backend wanted: {}", report.backend_requested);
    let _ = writeln!(text, "backend used:   {}", report.backend_used.label());
    let _ = writeln!(
        text,
        "tolerance:      relative {} vs max(|native|, |oracle|, 1.0)\n",
        report.relative_tolerance
    );
    let _ = writeln!(
        text,
        "{:<8} {:>6}  {:<7} {:<9}",
        "kernel", "elems", "native", "parity"
    );
    for k in &report.kernels {
        let parity = match k.oracle_parity {
            Some(true) => "match",
            Some(false) => "MISMATCH",
            None => "n/a",
        };
        let _ = writeln!(
            text,
            "{:<8} {:>6}  {:<7} {:<9}\n         {}",
            k.kernel,
            k.elements,
            yes_no(k.executed_natively),
            parity,
            k.detail
        );
    }
    let _ = writeln!(text, "\nresult: {}", verdict(report));
    let _ = writeln!(text, "\nnot proof of: {}", report.not_proof_of.join(" · "));
    text
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
