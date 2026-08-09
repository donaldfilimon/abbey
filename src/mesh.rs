//! Claim-bounded bridge to ABI's authenticated local multi-process WDBX proof.
//!
//! This module never invokes a shell, so a shell alias named `abi` cannot be
//! mistaken for an executable. The proof is exact local process evidence on
//! one Unix host, where Abbey can terminate the complete ABI process group;
//! it is not production multi-host deployment or shared compute.

use crate::config::{self, AbbeyConfig};
#[cfg(unix)]
use crate::runtime::CancellationToken;
#[cfg(unix)]
use crate::runtime::supervisor::{
    ProcessSpec, SupervisorLimits, SupervisorOutcome, run as run_supervised,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::ffi::OsString;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MIN_NODES: usize = 3;
pub const MAX_NODES: usize = 9;
pub const PROOF_LABEL: &str = "authenticated_local_multi_process";
pub const STORAGE_PROOF_SCOPE: &str = "isolated_child_process_exact_transaction_replicas";
pub const ABBEY_SCOPE: &str = "single_host_authenticated_local_multi_process_only";
pub const EXCLUDED_CLAIMS: [&str; 2] = ["production_multi_host", "shared_compute"];

#[cfg(unix)]
const MAX_PROOF_JSON_BYTES: usize = 1024 * 1024;
#[cfg(unix)]
const MAX_PROOF_STDERR_BYTES: usize = 1024 * 1024;
const LOCAL_DEMO_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(unix)]
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(unix)]
const CHILD_TERMINATE_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MeshStatus {
    pub available: bool,
    pub abi_bin: Option<String>,
    pub nodes_min: usize,
    pub nodes_max: usize,
    pub proof: &'static str,
    pub abbey_scope: &'static str,
    pub not_proof_of: [&'static str; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ElectionProof {
    pub leader: u64,
    pub term: u64,
    pub votes: usize,
    pub quorum: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplicatedWriteProof {
    pub acknowledgements: usize,
    pub quorum: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MeshProof {
    pub proof: String,
    pub storage_proof_scope: String,
    pub nodes: usize,
    pub election: ElectionProof,
    pub replicated_write: ReplicatedWriteProof,
    pub shard_placement_verified: bool,
    pub failover: ElectionProof,
    pub conflicts_observed: bool,
    pub read_repair_completed: bool,
    pub children_reaped: bool,
    pub abbey_scope: &'static str,
    pub not_proof_of: [&'static str; 2],
}

#[derive(Debug, Deserialize)]
struct AbiMeshProof {
    proof: String,
    storage_proof_scope: String,
    nodes: usize,
    election: ElectionProof,
    replicated_write: ReplicatedWriteProof,
    shard_placement_verified: bool,
    failover: ElectionProof,
    conflicts_observed: bool,
    read_repair_completed: bool,
    children_reaped: bool,
}

/// Resolve the configured executable path and report the exact claim boundary.
///
/// `AbbeyConfig::load()` applies `ABBEY_ABI_BIN` over `abi_bin`; the existing
/// resolver then falls back to a real executable on PATH. No shell is involved.
pub fn status(cfg: &AbbeyConfig) -> MeshStatus {
    let abi_bin = resolve_abi_bin(cfg);
    MeshStatus {
        available: cfg!(unix) && abi_bin.is_some(),
        abi_bin: abi_bin.map(|path| path.display().to_string()),
        nodes_min: MIN_NODES,
        nodes_max: MAX_NODES,
        proof: PROOF_LABEL,
        abbey_scope: ABBEY_SCOPE,
        not_proof_of: EXCLUDED_CLAIMS,
    }
}

/// Supported local process counts.
pub fn nodes() -> RangeInclusive<usize> {
    MIN_NODES..=MAX_NODES
}

/// Run `abi wdbx cluster local-demo <nodes> --json` and validate its evidence.
pub fn local_demo(cfg: &AbbeyConfig, node_count: usize) -> Result<MeshProof> {
    let _ = build_local_demo_argv(node_count)?;
    ensure_local_demo_platform()?;
    let Some(bin) = resolve_abi_bin(cfg) else {
        bail!(
            "a real `abi` binary is required for the local mesh proof; set \
             ABBEY_ABI_BIN or `abi_bin` in {}",
            AbbeyConfig::config_path().display()
        );
    };
    run_local_demo_with_bin(&bin, node_count)
}

/// Convenience dispatcher for root CLI/slash wiring.
pub fn dispatch(cfg: &AbbeyConfig, args: &[String], json: bool) -> Result<i32> {
    match args {
        [] => emit_status(cfg, json),
        [command] if command == "status" => emit_status(cfg, json),
        [command] if command == "nodes" => {
            if json {
                println!(r#"{{"min":{MIN_NODES},"max":{MAX_NODES}}}"#);
            } else {
                println!("mesh local-demo nodes: {MIN_NODES}..={MAX_NODES}");
            }
            Ok(0)
        }
        [command] if command == "local-demo" => emit_demo(cfg, MIN_NODES, json),
        [command, count] if command == "local-demo" => {
            let node_count = count
                .parse::<usize>()
                .with_context(|| format!("mesh local-demo nodes `{count}`"))?;
            emit_demo(cfg, node_count, json)
        }
        _ => bail!("usage: abbey mesh <status|nodes|local-demo [3..=9]> [--json]"),
    }
}

fn emit_status(cfg: &AbbeyConfig, json: bool) -> Result<i32> {
    let status = status(cfg);
    if json {
        println!("{}", serde_json::to_string(&status)?);
    } else {
        println!(
            "mesh: {}\nabi: {}\nnodes: {}..={}\nproof: {}\nscope: {}\nnot proof of: {}",
            if status.available {
                "available"
            } else {
                "unavailable"
            },
            status
                .abi_bin
                .as_deref()
                .unwrap_or("(real abi binary not found)"),
            status.nodes_min,
            status.nodes_max,
            status.proof,
            status.abbey_scope,
            status.not_proof_of.join(", ")
        );
    }
    Ok(i32::from(!status.available))
}

fn emit_demo(cfg: &AbbeyConfig, node_count: usize, json: bool) -> Result<i32> {
    let proof = local_demo(cfg, node_count)?;
    if json {
        println!("{}", serde_json::to_string(&proof)?);
    } else {
        println!(
            "proof: {}\nnodes: {}\nwrite acknowledgements: {}\nread repair: {}\nchildren reaped: {}\nscope: {}\nnot proof of: {}",
            proof.proof,
            proof.nodes,
            proof.replicated_write.acknowledgements,
            proof.read_repair_completed,
            proof.children_reaped,
            proof.abbey_scope,
            proof.not_proof_of.join(", ")
        );
    }
    Ok(0)
}

fn resolve_abi_bin(cfg: &AbbeyConfig) -> Option<PathBuf> {
    config::resolve_abi_bin(cfg).filter(|path| path.is_file())
}

fn build_local_demo_argv(node_count: usize) -> Result<Vec<String>> {
    if !nodes().contains(&node_count) {
        bail!("mesh local-demo supports {MIN_NODES}..={MAX_NODES} nodes");
    }
    Ok(vec![
        "wdbx".into(),
        "cluster".into(),
        "local-demo".into(),
        node_count.to_string(),
        "--json".into(),
    ])
}

fn run_local_demo_with_bin(bin: &Path, node_count: usize) -> Result<MeshProof> {
    run_local_demo_with_bin_timeout(bin, node_count, LOCAL_DEMO_TIMEOUT)
}

#[cfg(unix)]
fn run_local_demo_with_bin_timeout(
    bin: &Path,
    node_count: usize,
    timeout: Duration,
) -> Result<MeshProof> {
    let argv = build_local_demo_argv(node_count)?;
    let spec = ProcessSpec::inherited(bin.to_path_buf(), argv.iter().map(OsString::from).collect());
    let limits = SupervisorLimits {
        timeout,
        terminate_grace: CHILD_TERMINATE_GRACE,
        stdout_bytes: MAX_PROOF_JSON_BYTES,
        stderr_bytes: MAX_PROOF_STDERR_BYTES,
        poll_interval: CHILD_POLL_INTERVAL,
    };
    let outcome =
        run_supervised(&spec, &limits, &CancellationToken::default()).map_err(|error| {
            if error.is_teardown() {
                anyhow::anyhow!("ABI local mesh proof teardown failed: {error}")
            } else {
                anyhow::anyhow!("run {} {}: {error}", bin.display(), argv.join(" "))
            }
        })?;
    match outcome {
        SupervisorOutcome::Exited {
            status,
            stdout,
            stderr,
        } => {
            if !status.success() {
                let stderr = String::from_utf8_lossy(&stderr);
                bail!(
                    "ABI local mesh proof failed (exit {}): {}",
                    status.code().unwrap_or(1),
                    stderr.trim()
                );
            }
            parse_proof(&stdout, node_count)
        }
        SupervisorOutcome::Cancelled => bail!("ABI local mesh proof was cancelled"),
        SupervisorOutcome::TimedOut => bail!(
            "ABI local mesh proof timed out after {} ms",
            timeout.as_millis()
        ),
        SupervisorOutcome::StdoutLimit => {
            bail!("ABI local mesh proof stdout exceeds {MAX_PROOF_JSON_BYTES} bytes")
        }
        SupervisorOutcome::StderrLimit => {
            bail!("ABI local mesh proof stderr exceeds {MAX_PROOF_STDERR_BYTES} bytes")
        }
    }
}

#[cfg(not(unix))]
fn run_local_demo_with_bin_timeout(
    _bin: &Path,
    _node_count: usize,
    _timeout: Duration,
) -> Result<MeshProof> {
    ensure_local_demo_platform()?;
    unreachable!("unsupported platforms return an error")
}

#[cfg(unix)]
fn ensure_local_demo_platform() -> Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn ensure_local_demo_platform() -> Result<()> {
    bail!(
        "mesh local-demo is supported only on Unix hosts because safe descendant-process teardown requires process groups"
    )
}

fn parse_proof(json: &[u8], requested_nodes: usize) -> Result<MeshProof> {
    let proof: AbiMeshProof =
        serde_json::from_slice(json).context("parse ABI local mesh proof JSON")?;
    if proof.proof != PROOF_LABEL {
        bail!("unexpected ABI mesh proof label `{}`", proof.proof);
    }
    if proof.storage_proof_scope != STORAGE_PROOF_SCOPE {
        bail!(
            "unexpected ABI storage proof scope `{}`",
            proof.storage_proof_scope
        );
    }
    if proof.nodes != requested_nodes {
        bail!(
            "ABI mesh proof node mismatch: requested {requested_nodes}, reported {}",
            proof.nodes
        );
    }
    let election_quorum = proof.nodes / 2 + 1;
    let write_replicas = proof.nodes.min(3);
    let write_quorum = write_replicas / 2 + 1;
    let leaders_are_valid = proof.election.leader < proof.nodes as u64
        && proof.failover.leader < proof.nodes as u64
        && proof.election.leader != proof.failover.leader;
    let election_is_valid = proof.election.term > 0
        && proof.election.quorum == election_quorum
        && proof.election.votes >= proof.election.quorum
        && proof.election.votes <= proof.nodes;
    let failover_is_valid = proof.failover.term > proof.election.term
        && proof.failover.quorum == election_quorum
        && proof.failover.votes >= proof.failover.quorum
        && proof.failover.votes <= proof.nodes;
    let write_is_valid = proof.replicated_write.quorum == write_quorum
        && proof.replicated_write.acknowledgements >= proof.replicated_write.quorum
        && proof.replicated_write.acknowledgements <= write_replicas;
    if !leaders_are_valid
        || !election_is_valid
        || !failover_is_valid
        || !write_is_valid
        || proof.election.votes < proof.election.quorum
        || proof.replicated_write.acknowledgements < proof.replicated_write.quorum
        || proof.failover.votes < proof.failover.quorum
        || !proof.shard_placement_verified
        || !proof.conflicts_observed
        || !proof.read_repair_completed
        || !proof.children_reaped
    {
        bail!("ABI local mesh proof reported incomplete evidence");
    }
    Ok(MeshProof {
        proof: proof.proof,
        storage_proof_scope: proof.storage_proof_scope,
        nodes: proof.nodes,
        election: proof.election,
        replicated_write: proof.replicated_write,
        shard_placement_verified: proof.shard_placement_verified,
        failover: proof.failover,
        conflicts_observed: proof.conflicts_observed,
        read_repair_completed: proof.read_repair_completed,
        children_reaped: proof.children_reaped,
        abbey_scope: ABBEY_SCOPE,
        not_proof_of: EXCLUDED_CLAIMS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::thread;
    #[cfg(unix)]
    use std::time::Instant;

    #[cfg(unix)]
    fn write_fake_abi(tag: &str, script: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = std::env::temp_dir().join(format!(
            "abbey-mesh-{tag}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("abi");
        std::fs::write(&bin, script).unwrap();
        let mut permissions = std::fs::metadata(&bin).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&bin, permissions).unwrap();
        (dir, bin)
    }

    fn proof_json(nodes: usize) -> String {
        let election_quorum = nodes / 2 + 1;
        let write_quorum = nodes.min(3) / 2 + 1;
        format!(
            r#"{{"proof":"{PROOF_LABEL}","storage_proof_scope":"{STORAGE_PROOF_SCOPE}","nodes":{nodes},"election":{{"leader":0,"term":1,"votes":{nodes},"quorum":{election_quorum}}},"replicated_write":{{"acknowledgements":{},"quorum":{write_quorum}}},"shard_placement_verified":true,"failover":{{"leader":1,"term":2,"votes":{nodes},"quorum":{election_quorum}}},"conflicts_observed":true,"read_repair_completed":true,"children_reaped":true}}"#,
            nodes.min(3)
        )
    }

    #[test]
    fn node_range_and_argv_are_exact() {
        assert_eq!(nodes(), MIN_NODES..=MAX_NODES);
        assert_eq!(
            build_local_demo_argv(3).unwrap(),
            ["wdbx", "cluster", "local-demo", "3", "--json"]
        );
        for invalid in [0, 2, 10, usize::MAX] {
            assert!(build_local_demo_argv(invalid).is_err());
        }
    }

    #[cfg(not(unix))]
    #[test]
    fn local_demo_fails_closed_before_resolving_or_spawning_abi() {
        let error = local_demo(&AbbeyConfig::default(), 3)
            .unwrap_err()
            .to_string();
        assert!(error.contains("supported only on Unix hosts"), "{error}");
    }

    #[test]
    fn parser_requires_the_authenticated_complete_local_proof() {
        let proof = parse_proof(proof_json(3).as_bytes(), 3).unwrap();
        assert_eq!(proof.proof, PROOF_LABEL);
        assert_eq!(proof.abbey_scope, ABBEY_SCOPE);
        assert_eq!(proof.not_proof_of, EXCLUDED_CLAIMS);

        let wrong_label = proof_json(3).replace(PROOF_LABEL, "production_cluster");
        assert!(parse_proof(wrong_label.as_bytes(), 3).is_err());
        assert!(parse_proof(proof_json(3).as_bytes(), 4).is_err());
        let incomplete =
            proof_json(3).replace("\"children_reaped\":true", "\"children_reaped\":false");
        assert!(parse_proof(incomplete.as_bytes(), 3).is_err());

        for (path, invalid) in [
            (&["election", "quorum"][..], 0_u64),
            (&["election", "votes"][..], 4),
            (&["election", "leader"][..], 3),
            (&["failover", "leader"][..], 0),
            (&["failover", "term"][..], 1),
            (&["replicated_write", "acknowledgements"][..], 0),
        ] {
            let mut value: serde_json::Value = serde_json::from_str(&proof_json(3)).unwrap();
            value[path[0]][path[1]] = invalid.into();
            assert!(
                parse_proof(serde_json::to_string(&value).unwrap().as_bytes(), 3).is_err(),
                "accepted invalid proof field {}.{}={invalid}",
                path[0],
                path[1]
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn fake_abi_binary_receives_exact_argv_and_returns_typed_proof() {
        let script = format!(
            "#!/bin/sh\n[ \"$*\" = \"wdbx cluster local-demo 3 --json\" ] || exit 23\nprintf '%s\\n' '{}'\n",
            proof_json(3)
        );
        let (dir, bin) = write_fake_abi("success", &script);

        let cfg = AbbeyConfig {
            abi_bin: Some(bin.clone()),
            ..AbbeyConfig::default()
        };
        let resolved = status(&cfg);
        assert!(resolved.available);
        assert_eq!(resolved.abi_bin, Some(bin.display().to_string()));

        let proof = local_demo(&cfg, 3).unwrap();
        assert_eq!(proof.nodes, 3);
        assert_eq!(proof.proof, PROOF_LABEL);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn local_demo_timeout_kills_and_reaps_the_child() {
        let script = "#!/bin/sh\nexec sleep 5\n";
        let (dir, bin) = write_fake_abi("timeout", script);
        let started = Instant::now();
        let error = run_local_demo_with_bin_timeout(&bin, 3, Duration::from_millis(50))
            .unwrap_err()
            .to_string();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn local_demo_timeout_kills_descendants_holding_capture_pipes() {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;

        let (dir, bin) = write_fake_abi(
            "descendant-timeout",
            "#!/bin/sh\nsleep 30 &\necho $! > \"$0.child\"\nwait\n",
        );
        let started = Instant::now();
        // Parallel full-suite runs can briefly delay the shell before it writes
        // the descendant PID. Keep the total two-second bar while allowing a
        // bounded startup window that does not depend on scheduler speed.
        let error = run_local_demo_with_bin_timeout(&bin, 3, Duration::from_millis(500))
            .unwrap_err()
            .to_string();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
        let descendant = std::fs::read_to_string(format!("{}.child", bin.display()))
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        let reaped_deadline = Instant::now() + Duration::from_secs(1);
        while kill(Pid::from_raw(descendant), None).is_ok() && Instant::now() < reaped_deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            kill(Pid::from_raw(descendant), None).is_err(),
            "descendant process {descendant} survived the process-group kill"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn local_demo_fails_closed_when_either_stream_exceeds_its_cap() {
        for (tag, redirection, expected) in [
            ("large-stdout", "2>/dev/null", "stdout exceeds"),
            ("large-stderr", "1>&2 2>/dev/null", "stderr exceeds"),
        ] {
            let script = format!(
                "#!/bin/sh\n[ \"$*\" = \"wdbx cluster local-demo 3 --json\" ] || exit 23\ndd if=/dev/zero bs={} count=1 {redirection}\n",
                MAX_PROOF_JSON_BYTES + 1
            );
            let (dir, bin) = write_fake_abi(tag, &script);
            let error = run_local_demo_with_bin_timeout(&bin, 3, Duration::from_secs(2))
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{tag}: {error}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
