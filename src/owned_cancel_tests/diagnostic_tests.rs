//! Current owned generation and metadata APIs retain truthful diagnostics.

use super::{Fixture, tap};
use crate::agent::AgentBackend;
use crate::generate::{GenKind, run_generate};
use crate::stream::StreamEvent;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;

#[test]
fn generation_refusal_control_keeps_unsupported_backend_exit_and_no_output_file() {
    for backend in [AgentBackend::Abi, AgentBackend::Fm, AgentBackend::Ollama] {
        let fixture = Fixture::new("generation-refusal-control", false);
        let (owner, _events) = tap();
        let mut cfg = fixture.cfg(backend, owner);
        cfg.stream = None;
        let output = fixture.root.join("never-generated.png");
        let code = run_generate(
            &mut cfg,
            &fixture.state(),
            GenKind::Image,
            &["synthetic image description".into()],
            Some(output.clone()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(code, 2);
        assert!(!output.exists());
        assert!(!cfg.force);
    }
}

#[test]
fn owned_generation_refusal_retains_backend_and_tool_advice_in_the_tap() {
    for backend in [AgentBackend::Abi, AgentBackend::Fm, AgentBackend::Ollama] {
        let fixture = Fixture::new("generation-refusal-notice", false);
        let (owner, events) = tap();
        let mut cfg = fixture.cfg(backend, owner);
        let code = run_generate(
            &mut cfg,
            &fixture.state(),
            GenKind::Image,
            &["synthetic image description".into()],
            Some(fixture.root.join("never-generated.png")),
            None,
            None,
        )
        .unwrap();
        assert_eq!(code, 2);
        let notices: Vec<String> = events
            .try_iter()
            .filter_map(|event| match event {
                StreamEvent::Notice(text) => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            notices.iter().any(|text| text
                .contains("needs an executor with image/video tool access")
                && text.contains(backend.label())
                && text.contains("Select ABBEY_BACKEND=cursor")),
            "unsupported generation refused but its advice bypassed the owned notice sink"
        );
    }
}

#[test]
fn owned_generation_progress_retains_selected_output_destination_before_capture() {
    let fixture = Fixture::new("generation-progress", false);
    let (owner, events) = tap();
    let mut cfg = fixture.cfg(AgentBackend::Cursor, owner);
    cfg.force_capture = true;
    let output = fixture.root.join("synthetic-output.png");
    let code = run_generate(
        &mut cfg,
        &fixture.state(),
        GenKind::Image,
        &["synthetic image description".into()],
        Some(output.clone()),
        None,
        None,
    )
    .unwrap();
    // The owned response fixture is not an image tool; this is argv/result
    // and diagnostic proof only, never a generated-artifact claim.
    assert_eq!(code, 0);
    let destination = output.to_string_lossy();
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, StreamEvent::Notice(text)
        if text.contains(destination.as_ref()) && text.contains("not a local model"))),
        "successful canonical capture lost its generation progress destination"
    );
    assert!(!output.exists());
}

fn metadata_body(fixture: &Fixture, bytes: usize) {
    let body = fixture.root.join("metadata-body");
    fs::write(&body, "m".repeat(bytes)).unwrap();
    fs::set_permissions(&body, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(
        &fixture.program,
        format!("#!/bin/sh\nexec /bin/cat {}\n", super::shell_quote(&body)),
    )
    .unwrap();
}

#[test]
fn owned_metadata_exact_cap_control_returns_and_charges_4096_bytes() {
    let fixture = Fixture::new("metadata-exact-cap", false);
    metadata_body(&fixture, 4096);
    let (owner, _events) = tap();
    let cfg = fixture.cfg(AgentBackend::Abi, owner.clone());
    let before = owner.raw_remaining();
    let (status, stdout, stderr) = cfg.metadata_owned(&["--version".into()]).unwrap();
    assert!(status.success());
    assert_eq!(stdout.len(), 4096);
    assert!(stderr.is_empty());
    assert_eq!(owner.raw_remaining(), before - 4096);
    assert!(!owner.cancel.is_cancelled());
}

fn reports_metadata_cap(text: &str) -> bool {
    (text.contains("4096") || text.contains("4 KiB"))
        && !text.contains("4 MiB")
        && !text.contains("4194304")
}

#[test]
fn owned_metadata_overflow_reports_its_effective_cap_in_error_and_notice() {
    let fixture = Fixture::new("metadata-overflow-cap", false);
    metadata_body(&fixture, 4097);
    let (owner, events) = tap();
    let cfg = fixture.cfg(AgentBackend::Abi, owner.clone());
    assert!(owner.raw_remaining() > 4097);
    let error = cfg.metadata_owned(&["--version".into()]).unwrap_err();
    let error = format!("{error:#}");
    let failures: Vec<String> = events
        .try_iter()
        .filter_map(|event| match event {
            StreamEvent::Failed(text) => Some(text),
            _ => None,
        })
        .collect();
    assert!(owner.cancel.is_cancelled());
    assert!(
        reports_metadata_cap(&error) && failures.iter().any(|text| reports_metadata_cap(text)),
        "metadata refusal reported the wrong ceiling: error={error:?}, failure_notice={failures:?}"
    );
}

#[test]
fn owned_doctor_active_abi_line_identifies_the_selected_live_executable() {
    let fixture = Fixture::new("doctor-live-selection", false);
    let (owner, events) = tap();
    let cfg = fixture.cfg(AgentBackend::Abi, owner);
    assert_eq!(
        crate::doctor::cmd_doctor(&fixture.state(), &cfg).unwrap(),
        0
    );
    let notices: Vec<String> = events
        .try_iter()
        .filter_map(|event| match event {
            StreamEvent::Notice(text) => Some(text),
            _ => None,
        })
        .collect();
    let active = notices
        .iter()
        .find(|text| text.starts_with("abi backend:"))
        .unwrap();
    assert!(
        active.contains(&fixture.program.to_string_lossy().to_string())
            && active.contains("live TUI configuration")
            && !active.contains("ACTIVE BUT NO BINARY"),
        "owned doctor did not identify its selected ABI context"
    );
}
