//! Host platform + compute inventory.
//!
//! **Current:** report OS/arch, thread budget, and best-effort GPU/NPU/TPU *host*
//! presence; document which Abbey surfaces are portable vs macOS-only.
//!
//! **Not claimed:** Abbey does not compile for, train on, or run inference on
//! GPU/NPU/TPU. Multi-threaded fan-out is process/thread parallelism in
//! `subagents`, not accelerator kernels. "100% supported" here means primary
//! *host targets* for the portable CLI surfaces — not accelerator runtimes.

use crate::agent;
use crate::output;
use anyhow::Result;
use std::process::Command;
use std::thread;

#[derive(Debug, Clone)]
pub struct AcceleratorHint {
    pub kind: &'static str,
    pub status: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct SurfaceSupport {
    pub name: &'static str,
    pub linux: bool,
    pub macos: bool,
    pub windows: bool,
    pub note: &'static str,
}

/// Abbey surfaces vs primary host targets. `unix` ≈ linux ∪ macos ∪ *bsd.
pub fn surface_matrix() -> &'static [SurfaceSupport] {
    &[
        SurfaceSupport {
            name: "CLI core (run/print/doctor/memory/sqlite)",
            linux: true,
            macos: true,
            windows: true,
            note: "primary portable surface",
        },
        SurfaceSupport {
            name: "TUI (ratatui)",
            linux: true,
            macos: true,
            windows: true,
            note: "crossterm; needs a real terminal",
        },
        SurfaceSupport {
            name: "WDBX memory + cross-process lock",
            linux: true,
            macos: true,
            windows: true,
            note: "fs2 advisory lock (Unix flock / Win LockFileEx); --features wdbx",
        },
        SurfaceSupport {
            name: "multi-thread subagents (--jobs)",
            linux: true,
            macos: true,
            windows: true,
            note: "std::thread fan-out — not GPU kernels",
        },
        SurfaceSupport {
            name: "OS allowlist control",
            linux: true,
            macos: true,
            windows: true,
            note: "platform-specific allowlist entries",
        },
        SurfaceSupport {
            name: "voice I/O (say + Speech)",
            linux: false,
            macos: true,
            windows: false,
            note: "macOS only",
        },
        SurfaceSupport {
            name: "on-device fm backend",
            linux: false,
            macos: true,
            windows: false,
            note: "Apple Foundation Models CLI, macOS 26+",
        },
        SurfaceSupport {
            name: "GPU/NPU/TPU inference or training in Abbey",
            linux: false,
            macos: false,
            windows: false,
            note: "Out of scope — detect-only via `abbey platform`",
        },
    ]
}

pub fn thread_budget() -> usize {
    thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

pub fn default_subagent_jobs() -> usize {
    std::env::var("ABBEY_SUBAGENT_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n: &usize| *n > 0)
        .unwrap_or_else(|| thread_budget().clamp(1, 8))
}

fn which(name: &str) -> Option<std::path::PathBuf> {
    agent::which_bin(name)
}

fn run_ok(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() && out.stdout.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        Some("(present)".into())
    } else {
        Some(line.chars().take(120).collect())
    }
}

/// Best-effort accelerator discovery — presence only, not Abbey compute.
pub fn detect_accelerators() -> Vec<AcceleratorHint> {
    let mut out = Vec::new();

    // NVIDIA CUDA stack (driver/tools)
    if let Some(p) = which("nvidia-smi") {
        let detail = run_ok(
            p.to_str().unwrap_or("nvidia-smi"),
            &["--query-gpu=name,driver_version", "--format=csv,noheader"],
        )
        .unwrap_or_else(|| p.display().to_string());
        out.push(AcceleratorHint {
            kind: "GPU",
            status: "host",
            detail: format!("NVIDIA · {detail}"),
        });
    }

    // AMD ROCm
    if let Some(p) = which("rocm-smi").or_else(|| which("rocminfo")) {
        out.push(AcceleratorHint {
            kind: "GPU",
            status: "host",
            detail: format!("AMD ROCm tools · {}", p.display()),
        });
    }

    // Apple Silicon / Metal / ANE (report host, not Abbey Metal kernels)
    #[cfg(target_os = "macos")]
    {
        let arch = std::env::consts::ARCH;
        if arch == "aarch64" {
            out.push(AcceleratorHint {
                kind: "GPU",
                status: "host",
                detail:
                    "Apple Silicon · Metal available to the OS (Abbey does not run Metal kernels)"
                        .into(),
            });
            out.push(AcceleratorHint {
                kind: "NPU",
                status: "host",
                detail:
                    "Apple Neural Engine present on Apple Silicon (Abbey does not compile for ANE)"
                        .into(),
            });
        } else {
            out.push(AcceleratorHint {
                kind: "GPU",
                status: "host",
                detail: "macOS · Metal may be available (Intel/AMD GPU via system)".into(),
            });
        }
    }

    // Generic Linux DRM nodes
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new("/dev/dri").is_dir() && out.iter().all(|h| h.kind != "GPU") {
            out.push(AcceleratorHint {
                kind: "GPU",
                status: "host",
                detail: "/dev/dri present (DRM) — no vendor tool found on PATH".into(),
            });
        }
    }

    // Google TPU tooling / devices (best-effort)
    if let Some(p) = which("tpu-info").or_else(|| which("ctpu")) {
        out.push(AcceleratorHint {
            kind: "TPU",
            status: "host",
            detail: format!("TPU tooling · {}", p.display()),
        });
    }
    #[cfg(unix)]
    {
        if (std::path::Path::new("/dev/accel0").exists()
            || std::path::Path::new("/dev/vfio").is_dir())
            && out.iter().all(|h| h.kind != "TPU")
        {
            out.push(AcceleratorHint {
                kind: "TPU",
                status: "host",
                detail: "accel/vfio device nodes seen — not claimed as Abbey TPU runtime".into(),
            });
        }
    }

    if out.is_empty() {
        out.push(AcceleratorHint {
            kind: "GPU/NPU/TPU",
            status: "none",
            detail: "no host accelerator tools/devices detected".into(),
        });
    }

    // Always append the honesty line as a synthetic row? Better in print.
    out
}

fn mark(ok: bool) -> &'static str {
    if ok { "✓" } else { "·" }
}

pub fn print_platform() -> Result<i32> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let family = std::env::consts::FAMILY;
    let threads = thread_budget();
    let jobs = default_subagent_jobs();

    println!("abbey platform — host targets + compute inventory");
    println!("os:        {os}  arch: {arch}  family: {family}");
    println!("threads:   {threads} (std::thread::available_parallelism)");
    println!("subagents: default --jobs {jobs} (override ABBEY_SUBAGENT_JOBS)");
    println!();
    println!("primary targets (portable Abbey surfaces):");
    println!(
        "  {:<44} {:^5} {:^5} {:^7}  note",
        "surface", "linux", "macos", "windows"
    );
    for s in surface_matrix() {
        println!(
            "  {:<44} {:^5} {:^5} {:^7}  {}",
            s.name,
            mark(s.linux),
            mark(s.macos),
            mark(s.windows),
            s.note
        );
    }
    println!();
    println!("accelerators (host detect — Abbey does NOT run on these):");
    for h in detect_accelerators() {
        println!("  [{:<3}] {:<4} {}", h.status, h.kind, h.detail);
    }
    println!();
    println!(
        "honesty: GPU/NPU/TPU compilation, training, and inference inside Abbey \
         are Out of scope (`abbey claims oos`). Multi-threading = subagent jobs, \
         not accelerator kernels. Voice/fm remain macOS-only."
    );
    Ok(0)
}

pub fn print_compute() -> Result<i32> {
    println!("abbey compute — threads + host accelerators (report-only)\n");
    println!("threads:   {}", thread_budget());
    println!(
        "jobs:      {} (ABBEY_SUBAGENT_JOBS / clamp 1..=8)",
        default_subagent_jobs()
    );
    println!();
    for h in detect_accelerators() {
        println!("  [{:<3}] {:<4} {}", h.status, h.kind, h.detail);
    }
    println!();
    let _ = output::println(
        "Abbey does not schedule GPU/NPU/TPU work. For local second opinions use \
         `abbey subagents --peers`. For claims: `abbey claims refuse npu`.",
    );
    Ok(0)
}

pub fn dispatch(args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str) {
        None | Some("status") | Some("show") | Some("targets") => print_platform(),
        Some("compute") | Some("accel") | Some("gpu") | Some("npu") | Some("tpu") => {
            print_compute()
        }
        Some("threads") | Some("jobs") => {
            println!("threads: {}", thread_budget());
            println!("subagent_jobs_default: {}", default_subagent_jobs());
            Ok(0)
        }
        Some("refuse") | Some("runtime") | Some("kernels") => crate::claims::refuse("npu"),
        Some("-h") | Some("--help") => {
            println!(
                "abbey platform — host OS/arch + thread/accelerator inventory\n\
                 \n\
                 usage:\n\
                   abbey platform              # full matrix + accelerators\n\
                   abbey platform compute      # threads + GPU/NPU/TPU detect\n\
                   abbey platform threads      # parallelism only\n\
                   abbey platform refuse       # OOS: Abbey accelerator runtime\n\
                 \n\
                 aliases: abbey compute · abbey accel\n\
                 note: detection ≠ Abbey accelerator runtime (see claims oos)"
            );
            Ok(0)
        }
        Some(other) => {
            anyhow::bail!(
                "unknown platform subcommand `{other}` — try: status|compute|threads|refuse"
            );
        }
    }
}

pub fn status_line() -> String {
    let threads = thread_budget();
    let acc = detect_accelerators()
        .into_iter()
        .filter(|h| h.status == "host")
        .count();
    format!(
        "platform:  {}/{} · {threads} threads · {acc} host accelerator hint(s) — `abbey platform`",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_marks_core_portable() {
        let core = surface_matrix()
            .iter()
            .find(|s| s.name.starts_with("CLI core"))
            .unwrap();
        assert!(core.linux && core.macos && core.windows);
    }

    #[test]
    fn accelerator_compute_not_in_matrix_as_supported() {
        let gpu = surface_matrix()
            .iter()
            .find(|s| s.name.contains("GPU/NPU/TPU inference"))
            .unwrap();
        assert!(!gpu.linux && !gpu.macos && !gpu.windows);
    }

    #[test]
    fn thread_budget_at_least_one() {
        assert!(thread_budget() >= 1);
    }

    #[test]
    fn detect_returns_at_least_one_row() {
        assert!(!detect_accelerators().is_empty());
    }
}
