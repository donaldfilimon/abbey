//! Native local-terminal demonstration of governed encrypted tool delivery.
//! Run with an already enrolled tool.execute-only node directory. This example
//! creates scratch files; it is not a sandbox, general OS adapter, or trained model.
#[path = "governed_relay_worker/adapter.rs"]
mod adapter;
#[path = "governed_relay_worker/private.rs"]
#[allow(dead_code)]
mod private;
#[path = "governed_relay_worker/source.rs"]
mod source;
use abbey::runtime::{AbbeyLocalApprovalAuthority, RuntimeStore};
use abi_agent_host::{
    GovernedLiveExecutor,
    local_approval::{LocalApprovalState, LocalApprovalStore},
};
use abi_agent_runtime::CancellationToken;
use abi_capability::{Clock, Principal, PrincipalKind};
use abi_control_plane::{
    Dispatch, DurableRelayRuntime, FileRelayOutbox, GovernedToolContext, GovernedToolOutcome,
    NodeAction, NodeCertificate, OutboxLimits, RelayClient, RelayKey, RelayRuntime,
    SignedDeliveryPolicy,
};
use abi_worker::{FileControlStore, FileControlStoreLimits};
use adapter::{Evidence, FileAdapter, FileFacts, WallClock};
use private::{Result, now, read_private};
use std::{
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
};
const REVISION: &str = "scratch-create-v1";
const USAGE: &str = "governed_relay_worker NODE_DIRECTORY [--initialize | --initialize-outbox] [--loopback-development]\nRequires existing private enrollment files, a tool.execute-only certificate, and an interactive local terminal.\nProposals: tool=create_scratch, revision=scratch-create-v1, parameters={\"content\":\"text\"}.\nThe first mode explicitly creates fresh local journals and a private scratch directory; later runs reopen them.\nThis is a create_new scratch-file demonstration, not an OS sandbox or a trained model.";
fn main() {
    if let Err(error) = run() {
        eprintln!(
            "Governed worker refused: {error}. Retain all local journals; no state was reset."
        );
        std::process::exit(1);
    }
}
fn private_directory(path: &Path) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path)?;
        if !m.is_dir() || m.uid() != nix::unistd::Uid::effective().as_raw() || m.mode() & 0o077 != 0
        {
            return Err("owner-only nonsymlink directory required".into());
        }
        Ok(path.canonicalize()?)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err("platform private-directory ACL validation unavailable".into())
    }
}
fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.as_slice() == ["--help"] {
        println!("{USAGE}");
        return Ok(());
    }
    if args.is_empty()
        || args.len() > 3
        || args.iter().skip(1).any(|a| {
            !matches!(
                a.as_str(),
                "--initialize" | "--initialize-outbox" | "--loopback-development"
            )
        })
    {
        return Err(USAGE.into());
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("local approval requires interactive stdin and stdout; remote/nonterminal decisions refused".into());
    }
    let initialize = args.iter().any(|a| a == "--initialize");
    let initialize_outbox = args.iter().any(|a| a == "--initialize-outbox");
    if initialize && initialize_outbox {
        return Err("initialization modes are mutually exclusive".into());
    }
    let directory = private_directory(Path::new(&args[0]))?;
    let cert: NodeCertificate = read_private(&directory.join("node-certificate.json"))?;
    if cert.claim.capabilities != ["tool.execute"] {
        return Err("tool.execute-only certificate required".into());
    }
    let key =
        ed25519_dalek::SigningKey::from_bytes(&read_private(&directory.join("node-key.json"))?);
    let encryption = RelayKey::from_seed(&read_private(&directory.join("encryption-key.json"))?)?;
    let authority = read_private(&directory.join("authority-key.json"))?;
    let history = directory.join("tool-worker-control.log");
    let outbox_path = directory.join("tool-relay-outbox.sqlite");
    let approval_path = directory.join("tool-approvals.sqlite");
    let scratch = directory.join("governed-scratch");
    if initialize {
        for path in [&history, &outbox_path, &approval_path, &scratch] {
            if std::fs::symlink_metadata(path).is_ok() {
                return Err("initialization refuses existing state".into());
            }
        }
        private::directory(&scratch)?;
    } else if !approval_path.is_file() || !history.is_file() {
        return Err(
            "missing approval/history journal; explicit --initialize is required for a fresh node"
                .into(),
        );
    }
    let scratch = private_directory(&scratch)?;
    let store = if initialize {
        FileControlStore::create(&history, FileControlStoreLimits::default())?
    } else {
        FileControlStore::open(&history, FileControlStoreLimits::default())?
    };
    let outbox = if initialize || initialize_outbox {
        FileRelayOutbox::create(&outbox_path, OutboxLimits::default())?
    } else {
        FileRelayOutbox::open(&outbox_path, OutboxLimits::default())?
    };
    let runtime = RelayRuntime::new_tools(
        cert.clone(),
        key.clone(),
        encryption,
        authority,
        REVISION,
        store,
        now()?,
    )?;
    let mut runtime = DurableRelayRuntime::new(runtime, outbox)?;
    #[cfg(unix)]
    let user_id = format!("uid-{}", nix::unistd::Uid::effective());
    #[cfg(not(unix))]
    let user_id = String::new();
    let mut approvals = AbbeyLocalApprovalAuthority::new(
        RuntimeStore::open(&approval_path)?,
        cert.claim.tenant_id.clone(),
        cert.claim.account_id.clone(),
        cert.node_id.clone(),
        user_id.clone(),
        Principal::new(&cert.claim.account_id, PrincipalKind::HumanSubject)?,
    )?;
    let client = RelayClient::new(
        cert.clone(),
        key,
        args.iter().any(|a| a == "--loopback-development"),
    )?;
    let net = tokio::runtime::Runtime::new()?;
    println!(
        "Local create_new scratch demonstration ready. Approval previews remain in this terminal and private local storage. Ctrl-C stops; restart reopens the same journals."
    );
    loop {
        let recovered = net.block_on(client.recover_pending(&mut runtime, now()?))?;
        if recovered.refused > 0 {
            return Err("recovery authority refused; inspect retained state".into());
        }
        // Reopen pending proposals before polling. A proposal already delivered
        // by the service is never obtained again from Poll after a process exit.
        let pending = runtime
            .outbox
            .entries(now()?)?
            .into_iter()
            .filter(|e| {
                e.manifest.is_none()
                    && !e.abandoned
                    && !e.cancelled
                    && !e.expired
                    && e.packets.is_empty()
            })
            .map(|e| (e.dispatch, e.policy))
            .collect::<Vec<_>>();
        for (dispatch, policy) in pending {
            execute(
                &net,
                &client,
                &cert,
                &scratch,
                &user_id,
                &mut runtime,
                &mut approvals,
                &dispatch,
                &policy,
            )?;
        }
        if recovered.pending == 0 {
            let reply = net.block_on(client.exchange(NodeAction::Poll, now()?))?;
            if let Some(dispatch) = reply.dispatch {
                let policy = reply.delivery.ok_or("missing signed delivery policy")?;
                execute(
                    &net,
                    &client,
                    &cert,
                    &scratch,
                    &user_id,
                    &mut runtime,
                    &mut approvals,
                    &dispatch,
                    &policy,
                )?;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
#[allow(clippy::too_many_arguments)]
fn execute(
    net: &tokio::runtime::Runtime,
    client: &RelayClient,
    cert: &NodeCertificate,
    scratch: &Path,
    user_id: &str,
    runtime: &mut DurableRelayRuntime<FileControlStore>,
    approvals: &mut AbbeyLocalApprovalAuthority,
    dispatch: &Dispatch,
    policy: &SignedDeliveryPolicy,
) -> Result<()> {
    let job_id = uuid::Uuid::parse_str(&dispatch.submission.binding.job_id)?;
    let target = scratch.join(format!("effect-{job_id}.txt"));
    let (sources, registration, value) = source::compile(cert, &target)?;
    let evidence = Evidence::default();
    let mut facts = FileFacts {
        value,
        path: target.clone(),
        evidence: evidence.clone(),
    };
    let mut executor = FileAdapter {
        path: target.clone(),
        evidence,
    };
    let expected = vec![
        "Create this previously absent file with exactly the approved content; refuse overwrite."
            .into(),
    ];
    let mut refresh = || {
        let reply = net.block_on(client.exchange(
            NodeAction::Inspect {
                job_id: job_id.to_string(),
            },
            now().map_err(|_| abi_control_plane::Error::Denied)?,
        ))?;
        reply.permit.ok_or(abi_control_plane::Error::Denied)
    };
    let resource = target.to_str().ok_or("non-UTF8 scratch path")?;
    let mut invoke = |approvals: &mut AbbeyLocalApprovalAuthority,
                      executor: &mut dyn GovernedLiveExecutor| {
        let mut context = GovernedToolContext {
            user_id,
            resource,
            expected_effects: &expected,
            sources: &sources,
            registration: registration.clone(),
            facts: &mut facts,
            approvals,
            executor,
            clock: &WallClock,
            refresh: &mut refresh,
        };
        runtime.execute_governed(dispatch, policy, &mut context, &CancellationToken::new())
    };
    let outcome = invoke(approvals, &mut executor)?;
    match outcome {
        GovernedToolOutcome::Pending(status) if status.state == LocalApprovalState::Pending => {
            let preview = approvals
                .read_local_approval(&status.call_id)?
                .ok_or("missing exact local preview")?;
            println!("{}", serde_json::to_string_pretty(&preview)?);
            println!(
                "Type exactly: approve {}\nOr: deny {}",
                status.preview_digest, status.preview_digest
            );
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            let approved = line.trim() == format!("approve {}", status.preview_digest);
            if !approved && line.trim() != format!("deny {}", status.preview_digest) {
                return Err("no exact local decision; proposal remains pending".into());
            }
            approvals.decide_local_approval(
                &status.call_id,
                status.preview_digest,
                &format!("decision-{}", uuid::Uuid::new_v4()),
                approved,
                WallClock.now_ms(),
            )?;
            if approved {
                let outcome = invoke(approvals, &mut executor)?;
                log_outcome(job_id, &outcome);
                if matches!(outcome, GovernedToolOutcome::Unresolved) {
                    return Err("uncertain effect; never retry automatically".into());
                }
            } else {
                // Sticky local denial precedes the best-effort central metadata update.
                runtime
                    .outbox
                    .mark_unresolved(&job_id.to_string(), now()?)?;
                net.block_on(client.exchange(
                    NodeAction::Fail {
                        job_id: job_id.to_string(),
                    },
                    now()?,
                ))?;
            }
        }
        GovernedToolOutcome::Unresolved => {
            return Err("uncertain prior admission; never retry automatically".into());
        }
        other => log_outcome(job_id, &other),
    }
    // This republishes only durable artifacts. No adapter or approval is passed.
    net.block_on(client.recover_pending(runtime, now()?))?;
    Ok(())
}

fn log_outcome(job_id: uuid::Uuid, outcome: &GovernedToolOutcome) {
    match outcome {
        GovernedToolOutcome::Recorded(result) => {
            println!("job={job_id} outcome=Recorded status={:?}", result.status)
        }
        GovernedToolOutcome::Pending(status) => {
            println!("job={job_id} outcome=Pending state={:?}", status.state)
        }
        GovernedToolOutcome::Unresolved => println!("job={job_id} outcome=Unresolved"),
    }
}

#[cfg(test)]
#[path = "governed_relay_worker/tests.rs"]
mod tests;
