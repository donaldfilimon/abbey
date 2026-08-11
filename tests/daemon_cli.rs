//! Real-binary proof for Abbey's authenticated read-only daemon CLI.

#![cfg(unix)]

use abbey::app_core::{
    AppEvent, ClaimStatus, Edition, RuntimeState, V3Capability, V3CapabilitySet, V3ErrorCode,
    V3Event, V3OperationState, V3PageQuery, V3ResourceQuery, V3ToolCall, V3ToolDecision,
    V3ToolEffect, V3ToolInvocation,
};
use abbey::daemon::{BearerSecret, ClientError, DaemonClient, DaemonConfig};
use abbey::edition;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// The compiled edition this test binary was built for. The safe build must
/// still report `standard`; the personal build must report itself rather than
/// impersonating the public edition.
#[cfg(not(feature = "personal-edition"))]
const EXPECTED_EDITION: Edition = Edition::Standard;
#[cfg(feature = "personal-edition")]
const EXPECTED_EDITION: Edition = Edition::Personal;

const ABBEY_BIN: &str = env!("CARGO_BIN_EXE_abbey");
const ABBEYD_BIN: &str = env!("CARGO_BIN_EXE_abbeyd");
const BEARER: &str = "abbey-daemon-cli-test-bearer-0001";

struct Harness {
    root: PathBuf,
    socket: PathBuf,
    child: Child,
}

impl Harness {
    fn start() -> Self {
        Self::start_with_abi(false)
    }

    fn start_with_abi(bind_abi: bool) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "abbey-dcli-{}-{}",
            std::process::id(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir(&root).expect("create daemon CLI scratch directory");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("make daemon CLI scratch directory private");
        let socket = root.join("abbeyd.sock");
        let abi_provider = root.join("abi-provider");
        if bind_abi {
            std::fs::write(&abi_provider, "#!/bin/sh\nexit 0\n")
                .expect("write fixed ABI provider fixture");
            std::fs::set_permissions(&abi_provider, std::fs::Permissions::from_mode(0o700))
                .expect("make ABI provider fixture executable");
        }
        let mut daemon = Command::new(ABBEYD_BIN);
        daemon
            .env(edition::ACTIVE.state_dir_env(), &root)
            .env(edition::ACTIVE.daemon_socket_env(), &socket)
            .env(edition::ACTIVE.daemon_bearer_env(), BEARER);
        if bind_abi {
            daemon.env(edition::ACTIVE.scoped_env("ABI_BIN"), &abi_provider);
        }
        let child = daemon.spawn().expect("start abbeyd");

        let deadline = Instant::now() + Duration::from_secs(3);
        while !socket.exists() {
            assert!(Instant::now() < deadline, "abbeyd socket was not created");
            std::thread::sleep(Duration::from_millis(10));
        }
        Self {
            root,
            socket,
            child,
        }
    }

    fn abbey(&self, bearer: &str, args: &[&str]) -> std::process::Output {
        self.command(args)
            .env(edition::ACTIVE.daemon_bearer_env(), bearer)
            .output()
            .expect("run abbey daemon command")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(ABBEY_BIN);
        command
            .args(args)
            .env(edition::ACTIVE.state_dir_env(), &self.root)
            .env(edition::ACTIVE.daemon_socket_env(), &self.socket)
            .env_remove(edition::ACTIVE.daemon_bearer_env())
            .env_remove(edition::ACTIVE.daemon_bearer_file_env())
            // A local control-plane query must not resolve a model executor.
            .env("ABBEY_AGENT_BIN", self.root.join("missing-agent"));
        command
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn status_and_filtered_claims_round_trip_through_real_binaries() {
    let harness = Harness::start();

    let human = harness.abbey(BEARER, &["daemon", "status"]);
    assert_success(&human, "daemon human status");
    let human = String::from_utf8(human.stdout).unwrap();
    // The daemon names the edition it was actually compiled as; a personal
    // build must never present itself as the standard one.
    let expected_edition_word = match EXPECTED_EDITION {
        Edition::Standard => "standard",
        Edition::Personal => "personal",
    };
    assert!(
        human.contains(&format!("abbeyd: ready ({expected_edition_word} edition)")),
        "{human}"
    );
    // A prefix match: `abbeyd` runs the protocol-v2 handler, so the advertised
    // set continues past these two. `read_routes` is asserted separately
    // because it is declared last and a prefix check would never see it.
    assert!(
        human.contains("capabilities: read_status, read_claims"),
        "{human}"
    );
    assert!(human.contains("read_routes"), "{human}");

    let status = harness.abbey(BEARER, &["daemon", "status", "--json"]);
    assert_success(&status, "daemon status");
    let status_event: AppEvent = serde_json::from_slice(&status.stdout).unwrap();
    let AppEvent::Status(status) = status_event else {
        panic!("daemon status returned the wrong event");
    };
    assert_eq!(status.edition, EXPECTED_EDITION);
    assert_eq!(status.state, RuntimeState::Ready);

    let claims = harness.abbey(
        BEARER,
        &[
            "daemon",
            "claims",
            "--status",
            "blocked",
            "--contains",
            "linux",
            "--json",
        ],
    );
    assert_success(&claims, "daemon claims");
    let claims_event: AppEvent = serde_json::from_slice(&claims.stdout).unwrap();
    let AppEvent::Claims(snapshot) = claims_event else {
        panic!("daemon claims returned the wrong event");
    };
    assert_eq!(snapshot.matched, 1);
    assert_eq!(snapshot.claims[0].status, ClaimStatus::Blocked);

    let proposed = harness.abbey(
        BEARER,
        &[
            "daemon",
            "claims",
            "--status",
            "proposed",
            "--contains",
            "desktop",
            "--json",
        ],
    );
    assert_success(&proposed, "daemon proposed claims");
    let event: AppEvent = serde_json::from_slice(&proposed.stdout).unwrap();
    let AppEvent::Claims(snapshot) = event else {
        panic!("daemon proposed claims returned the wrong event");
    };
    assert_eq!(snapshot.matched, 1);
    assert_eq!(snapshot.claims[0].status, ClaimStatus::Proposed);
}

#[test]
fn protocol_v3_negotiation_and_model_inventory_round_trip_through_real_binaries() {
    let harness = Harness::start_with_abi(true);

    let negotiation = harness.abbey(BEARER, &["daemon", "negotiate", "--json"]);
    assert_success(&negotiation, "daemon protocol-v3 negotiation");
    let V3Event::Negotiated(negotiated) =
        serde_json::from_slice::<V3Event>(&negotiation.stdout).unwrap()
    else {
        panic!("daemon negotiate returned the wrong event");
    };
    assert_eq!(negotiated.granted.as_slice(), &[V3Capability::ReadModels]);

    let models = harness.abbey(
        BEARER,
        &[
            "daemon",
            "models",
            "--after",
            "0",
            "--through",
            "1",
            "--limit",
            "1",
            "--json",
        ],
    );
    assert_success(&models, "daemon protocol-v3 model inventory");
    let json_text = String::from_utf8(models.stdout).unwrap();
    let V3Event::Models(page) = serde_json::from_str::<V3Event>(&json_text).unwrap() else {
        panic!("daemon models returned the wrong event");
    };
    assert_eq!(page.after, 0);
    assert_eq!(page.through, 1);
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].id, "abi-local:local");
    assert_eq!(page.records[0].state, V3OperationState::Available);

    let human = harness.abbey(BEARER, &["daemon", "models"]);
    assert_success(&human, "daemon protocol-v3 model inventory text");
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("abbeyd models: 1 record(s)"), "{human}");
    assert!(human.contains("[available] abi-local:local"), "{human}");
    for private in [BEARER, path_text(&harness.root), path_text(&harness.socket)] {
        assert!(!json_text.contains(private), "model JSON leaked {private}");
        assert!(!human.contains(private), "model text leaked {private}");
    }

    for rejected in ["0", "33"] {
        let output = harness.abbey(BEARER, &["daemon", "models", "--limit", rejected]);
        assert!(
            !output.status.success(),
            "--limit {rejected} must be rejected"
        );
    }
}

#[test]
fn protocol_v3_stable_claim_lookup_round_trips_through_real_daemon_and_typed_client() {
    let harness = Harness::start();
    let config = DaemonConfig::local(
        harness.socket.clone(),
        BearerSecret::parse(BEARER).expect("valid bearer"),
    );
    let requested =
        V3CapabilitySet::from_sorted(vec![V3Capability::ReadModels, V3Capability::ReadClaimsById])
            .unwrap();
    let session = DaemonClient::new(config)
        .negotiate_v3(requested)
        .expect("negotiate stable claim authority");
    assert_eq!(
        session.negotiation().granted.as_slice(),
        &[V3Capability::ReadClaimsById]
    );

    let claim = session
        .claim_by_id(V3ResourceQuery {
            resource_id: "ci-self-hosted-linux-proof".into(),
        })
        .expect("read canonical claim by stable id");
    assert_eq!(claim.id, "ci-self-hosted-linux-proof");
    assert_eq!(claim.status, ClaimStatus::Blocked);
    assert!(claim.note.contains("Linux ARM64"));

    let error = session
        .claim_by_id(V3ResourceQuery {
            resource_id: "ci-self-hosted-linux".into(),
        })
        .unwrap_err();
    assert!(matches!(
        error,
        ClientError::DaemonV3 {
            code: V3ErrorCode::NotFound,
            ..
        }
    ));
}

#[test]
fn protocol_v3_safe_tool_inventory_and_audited_status_invocation_round_trip() {
    let harness = Harness::start();
    let config = DaemonConfig::local(
        harness.socket.clone(),
        BearerSecret::parse(BEARER).expect("valid bearer"),
    );
    let requested = V3CapabilitySet::from_sorted(vec![
        V3Capability::ListTools,
        V3Capability::InvokeTools,
        V3Capability::DecideToolApprovals,
    ])
    .unwrap();
    let session = DaemonClient::new(config)
        .negotiate_v3(requested)
        .expect("negotiate safe tool inventory");
    let expected_grants = if EXPECTED_EDITION == Edition::Standard {
        vec![
            V3Capability::ListTools,
            V3Capability::InvokeTools,
            V3Capability::DecideToolApprovals,
        ]
    } else {
        vec![V3Capability::ListTools, V3Capability::InvokeTools]
    };
    assert_eq!(session.negotiation().granted.as_slice(), expected_grants);

    let page = session
        .list_tools(V3PageQuery::default())
        .expect("read canonical safe tool inventory");
    assert_eq!(page.after, 0);
    let expected_tool_ids = if EXPECTED_EDITION == Edition::Standard {
        vec![
            "abbey_status",
            "abbey_claims",
            "abbey_platform",
            "abbey_memory_mark_obsolete",
        ]
    } else {
        vec!["abbey_status", "abbey_claims", "abbey_platform"]
    };
    assert_eq!(page.through, expected_tool_ids.len() as u64);
    assert_eq!(
        page.tools
            .iter()
            .map(|tool| tool.tool_id.as_str())
            .collect::<Vec<_>>(),
        expected_tool_ids
    );
    assert!(
        page.tools
            .iter()
            .take(3)
            .all(|tool| tool.effect == V3ToolEffect::ReadOnly)
    );
    assert!(page.tools.iter().all(|tool| tool.input_schema.is_object()));
    assert!(page.tools.iter().all(|tool| {
        !tool.tool_id.contains("shell")
            && !tool.tool_id.contains("exec")
            && !tool.tool_id.contains("process")
    }));

    let second = session
        .list_tools(V3PageQuery {
            after: 1,
            through: Some(page.through),
            limit: 1,
        })
        .expect("read one fixed-watermark tool page");
    assert_eq!(second.tools.len(), 1);
    assert_eq!(second.tools[0].tool_id, "abbey_claims");

    let result = session
        .invoke_tool(V3ToolCall {
            tool_id: "abbey_status".into(),
            call_id: "real-safe-call-1".into(),
            input: serde_json::json!({}),
        })
        .expect("invoke the schema-validated read-only status tool");
    assert_eq!(result.tool_id, "abbey_status");
    assert_eq!(result.call_id, "real-safe-call-1");
    assert_eq!(result.state, V3OperationState::Succeeded);
    assert_eq!(
        result.output["edition"],
        serde_json::json!(EXPECTED_EDITION)
    );

    let duplicate = session
        .invoke_tool(V3ToolCall {
            tool_id: "abbey_status".into(),
            call_id: "real-safe-call-1".into(),
            input: serde_json::json!({}),
        })
        .unwrap_err();
    assert!(matches!(
        duplicate,
        ClientError::DaemonV3 {
            code: V3ErrorCode::Conflict,
            ..
        }
    ));

    if EXPECTED_EDITION == Edition::Standard {
        let mut put = harness.command(&[
            "memory",
            "put",
            "pending approval must not mutate this record",
        ]);
        let put = put.output().expect("seed one memory record");
        assert_success(&put, "memory put before pending tool request");
        let record_id = String::from_utf8(put.stdout).unwrap().trim().to_owned();
        let call = V3ToolCall {
            tool_id: "abbey_memory_mark_obsolete".into(),
            call_id: "real-pending-call-1".into(),
            input: serde_json::json!({"record_id": record_id}),
        };
        let expected_digest = call.approval_digest().unwrap();
        let V3ToolInvocation::ApprovalRequired(status) = session
            .request_tool(call)
            .expect("persist one exact pending approval")
        else {
            panic!("mutating safe tool must not execute before approval");
        };
        assert_eq!(status.call_digest, expected_digest);
        let approved = session
            .approve_tool(V3ToolDecision {
                call_id: status.call_id,
                call_digest: status.call_digest,
                decision_id: "real-decision-approve-1".into(),
            })
            .expect("approve exact pending record without executing it");
        assert_eq!(
            approved.state,
            abbey::app_core::V3ToolApprovalState::Approved
        );

        let deny_call = V3ToolCall {
            tool_id: "abbey_memory_mark_obsolete".into(),
            call_id: "real-pending-call-2".into(),
            input: serde_json::json!({"record_id": record_id}),
        };
        let V3ToolInvocation::ApprovalRequired(deny_pending) = session
            .request_tool(deny_call)
            .expect("persist second exact pending approval")
        else {
            panic!("second mutating safe tool must await approval");
        };
        let denied = session
            .deny_tool(V3ToolDecision {
                call_id: deny_pending.call_id,
                call_digest: deny_pending.call_digest,
                decision_id: "real-decision-deny-1".into(),
            })
            .expect("deny exact pending record");
        assert_eq!(denied.state, abbey::app_core::V3ToolApprovalState::Denied);

        let mut get = harness.command(&["memory", "get", &record_id]);
        let get = get.output().expect("read memory after pending request");
        assert_success(&get, "memory get after pending tool request");
        let memory: serde_json::Value = serde_json::from_slice(&get.stdout).unwrap();
        assert_eq!(memory["obsolete"], false, "pending must not execute");
    }

    let database = harness.root.join("daemon/runtime.sqlite");
    let connection = rusqlite::Connection::open(database).expect("open daemon audit database");
    let audit = connection
        .prepare(
            "SELECT action, outcome, metadata_json FROM audit_events \
             WHERE action IN ('v3_tool_authorization', 'v3_tool_execution') ORDER BY id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(audit.len(), 2);
    assert_eq!(&audit[0].0, "v3_tool_authorization");
    assert_eq!(&audit[0].1, "allow");
    assert_eq!(&audit[1].0, "v3_tool_execution");
    assert_eq!(&audit[1].1, "succeeded");
    assert!(audit.iter().all(|(_, _, metadata)| {
        metadata.contains("input_digest")
            && !metadata.contains("protocol_version")
            && !metadata.contains("capabilities")
    }));
    if EXPECTED_EDITION == Edition::Standard {
        let approval = connection
            .query_row(
                "SELECT tool_id, call_digest, state FROM tool_approvals WHERE call_id=?1",
                ["real-pending-call-1"],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(approval.0, "abbey_memory_mark_obsolete");
        assert_eq!(approval.1.len(), 64);
        assert_eq!(approval.2, "approved");
        let events: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tool_approval_events WHERE call_id=?1",
                ["real-pending-call-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(events, 2);
        let denied = connection
            .query_row(
                "SELECT state, decision_id FROM tool_approvals WHERE call_id=?1",
                ["real-pending-call-2"],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(denied.0, "denied");
        assert_eq!(denied.1, "real-decision-deny-1");
        let denied_events: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tool_approval_events WHERE call_id=?1",
                ["real-pending-call-2"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(denied_events, 2);
    }
}

/// Real-binary proof for the route audit, including that the human and JSON
/// surfaces describe the *same* daemon answer and that neither carries the raw
/// working directory the log stores on disk.
#[test]
fn route_audit_reads_the_scratch_log_with_cli_and_json_parity() {
    let harness = Harness::start();

    // A workspace path that must never appear on either surface. It is under
    // the scratch root, so it is a genuine absolute path on this machine.
    let workspace = harness.root.join("secret-workspace");
    let workspace_text = path_text(&workspace).to_owned();
    let records = [
        format!(
            r#"{{"ts":"2026-08-08T12:00:00Z","cwd":{workspace},"persona":"Abbey","role":"max","model":"fable","reason":"persona=Abbey role=max class=Code","confidence":0.82,"tools":["mcp"]}}"#,
            workspace = serde_json::to_string(&workspace_text).unwrap()
        ),
        // A malformed line between two good ones must not poison the read.
        "{ not json at all".to_owned(),
        format!(
            r#"{{"ts":"2026-08-08T12:01:00Z","cwd":{workspace},"persona":"Abbey","role":"gemma","model":"gemma-3","reason":"hybrid-loop stage=interpret log={workspace_text}/route.jsonl","confidence":0.85,"stage":"interpret","correlation":"corr-1"}}"#,
            workspace = serde_json::to_string(&workspace_text).unwrap()
        ),
    ];
    std::fs::write(
        harness.root.join("route.jsonl"),
        format!("{}\n", records.join("\n")),
    )
    .expect("seed the scratch route log");

    let json = harness.abbey(BEARER, &["daemon", "routes", "--json"]);
    assert_success(&json, "daemon routes --json");
    let json_text = String::from_utf8(json.stdout).expect("routes JSON is UTF-8");
    let AppEvent::RouteAudit(page) =
        serde_json::from_str::<AppEvent>(&json_text).expect("routes returns a typed event")
    else {
        panic!("daemon routes returned the wrong event");
    };
    assert_eq!(page.returned, 2, "the malformed line must be isolated");
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].persona, "Abbey");
    assert_eq!(page.entries[0].confidence_percent, 82);
    assert_eq!(page.entries[1].stage.as_deref(), Some("interpret"));
    assert_eq!(page.entries[1].correlation.as_deref(), Some("corr-1"));

    let human = harness.abbey(BEARER, &["daemon", "routes"]);
    assert_success(&human, "daemon routes");
    let human_text = String::from_utf8(human.stdout).expect("routes text is UTF-8");

    // Parity: every fact the human surface prints comes from the same page the
    // JSON surface returned, and the counts agree.
    assert!(
        human_text.contains(&format!("abbeyd routes: {} of at most", page.returned)),
        "{human_text}"
    );
    for entry in &page.entries {
        let workspace_label = entry.workspace.as_deref().expect("a workspace digest");
        assert!(workspace_label.starts_with("ws-"), "{workspace_label}");
        for fragment in [
            entry.recorded_at.as_str(),
            entry.model.as_str(),
            workspace_label,
        ] {
            assert!(
                human_text.contains(fragment),
                "human surface omitted {fragment}: {human_text}"
            );
        }
        assert!(human_text.contains(&format!("{}%", entry.confidence_percent)));
    }

    // Redaction holds across the socket on both surfaces.
    for surface in [&json_text, &human_text] {
        assert!(
            !surface.contains(&workspace_text),
            "route audit leaked the raw workspace path: {surface}"
        );
        assert!(
            !surface.contains(path_text(&harness.root)),
            "route audit leaked the state root: {surface}"
        );
    }
    assert!(json_text.contains("[path]"), "{json_text}");

    // The limit is honoured end to end and its bound is enforced by Clap.
    let narrow = harness.abbey(BEARER, &["daemon", "routes", "--limit", "1", "--json"]);
    assert_success(&narrow, "daemon routes --limit 1");
    let AppEvent::RouteAudit(page) = serde_json::from_slice::<AppEvent>(&narrow.stdout).unwrap()
    else {
        panic!("daemon routes returned the wrong event");
    };
    assert_eq!(page.returned, 1);
    assert_eq!(page.limit, 1);
    // The tail keeps the newest decision.
    assert_eq!(page.entries[0].role, "gemma");

    for rejected in ["0", "51"] {
        let output = harness.abbey(BEARER, &["daemon", "routes", "--limit", rejected]);
        assert!(
            !output.status.success(),
            "--limit {rejected} must be rejected"
        );
    }
}

#[test]
fn authentication_failure_does_not_disclose_local_secrets() {
    let harness = Harness::start();
    let wrong_bearer = "abbey-daemon-cli-test-bearer-wrong";
    for args in [&["daemon", "status"][..], &["daemon", "negotiate"][..]] {
        let output = harness.abbey(wrong_bearer, args);
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains(BEARER), "correct bearer leaked: {stderr}");
        assert!(
            !stderr.contains(wrong_bearer),
            "supplied bearer leaked: {stderr}"
        );
        assert!(
            !stderr.contains(path_text(&harness.socket)),
            "socket path leaked: {stderr}"
        );
        assert!(stderr.contains("authentication failed"), "stderr: {stderr}");
    }

    let missing = harness
        .command(&["daemon", "status"])
        .output()
        .expect("run daemon command without bearer");
    assert!(!missing.status.success());
    let missing_stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(missing_stderr.contains("set exactly one"));
    assert!(!missing_stderr.contains(path_text(&harness.socket)));

    let bearer_file = harness.root.join("bearer");
    std::fs::write(&bearer_file, BEARER).unwrap();
    std::fs::set_permissions(&bearer_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let conflicting = harness
        .command(&["daemon", "status"])
        .env(edition::ACTIVE.daemon_bearer_env(), wrong_bearer)
        .env(edition::ACTIVE.daemon_bearer_file_env(), &bearer_file)
        .output()
        .expect("run daemon command with conflicting bearer sources");
    assert!(!conflicting.status.success());
    let conflicting_stderr = String::from_utf8_lossy(&conflicting.stderr);
    assert!(conflicting_stderr.contains("cannot both be set"));
    assert!(!conflicting_stderr.contains(BEARER));
    assert!(!conflicting_stderr.contains(wrong_bearer));
}

fn assert_success(output: &std::process::Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("scratch socket path must be UTF-8")
}
