//! Actual captured executor output must be admitted before durable turn writes.

use super::{AgentBackend, AgentConfig, MAX_CAPTURE_BYTES, run_resilient};
use crate::state::AbbeyState;
use crate::stream::{StreamEvent, StreamTap};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

const CHAT: &str = "fixture-budget-turn";
const TRANSCRIPT_SEED: &[u8] = b"unchanged-transcript-seed\n";
const COT_SEED: &[u8] = b"unchanged-cot-seed\n";

struct Fixture {
    root: PathBuf,
    cfg: AgentConfig,
    state: AbbeyState,
    transcript: PathBuf,
    cot: PathBuf,
    tap: StreamTap,
    events: Receiver<StreamEvent>,
    body: String,
}

impl Fixture {
    fn new(label: &str, output_bytes: usize) -> Self {
        let root = std::env::temp_dir().join(format!(
            "abbey-owned-budget-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("state/by-cwd")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let body = "q".repeat(output_bytes);
        let output = root.join("synthetic-output");
        fs::write(&output, &body).unwrap();
        fs::set_permissions(&output, fs::Permissions::from_mode(0o600)).unwrap();
        let quoted = format!("'{}'", output.to_string_lossy().replace('\'', "'\\''"));
        let program = root.join("executor");
        fs::write(&program, format!("#!/bin/sh\nexec /bin/cat {quoted}\n")).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let state = AbbeyState {
            state_dir: root.join("state"),
            chat_file: root.join("state/chat-id"),
            model_file: root.join("state/model"),
            history_file: root.join("state/history.log"),
            cwd_dir: root.join("state/by-cwd"),
            per_cwd: false,
            cwd: root.clone(),
        };
        state.save_chat(CHAT).unwrap();
        let (tx, events) = mpsc::channel();
        let tap = StreamTap::new(tx);
        let mut cfg =
            AgentConfig::fixed_provider_recipe(program, AgentBackend::Abi, "local".into());
        cfg.no_resume = false;
        cfg.force_capture = true;
        cfg.transcript_dir = Some(root.join("transcripts"));
        cfg.stream = Some(tap.clone());
        fs::create_dir_all(cfg.transcript_dir.as_ref().unwrap()).unwrap();
        let transcript = cfg.transcript_path(CHAT).unwrap();
        let cot = root.join("cot/latest.md");
        fs::create_dir_all(cot.parent().unwrap()).unwrap();
        fs::write(&transcript, TRANSCRIPT_SEED).unwrap();
        fs::write(&cot, COT_SEED).unwrap();
        cfg.cot_path = Some(cot.clone());
        Self {
            root,
            cfg,
            state,
            transcript,
            cot,
            tap,
            events,
            body,
        }
    }

    fn run_turn(&self) -> anyhow::Result<i32> {
        run_resilient(
            &self.cfg,
            &self.state,
            false,
            &["private synthetic prompt".into()],
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn captured_cot_control_delivers_then_preserves_successful_continuity() {
    let fixture = Fixture::new("cot-control", 2048);
    assert_eq!(fixture.run_turn().unwrap(), 0);
    assert!(!fixture.tap.cancel.is_cancelled());
    assert!(
        fs::read_to_string(&fixture.transcript)
            .unwrap()
            .contains(&fixture.body)
    );
    assert!(
        fs::read_to_string(&fixture.cot)
            .unwrap()
            .contains(&fixture.body)
    );
    assert!(
        fixture
            .events
            .try_iter()
            .any(|event| matches!(event, StreamEvent::TextDelta(text) if text == fixture.body))
    );
    assert_eq!(
        fixture
            .state
            .resolve_chat_for(AgentBackend::Abi)
            .unwrap()
            .as_deref(),
        Some(CHAT)
    );
}

#[test]
fn captured_cot_budget_rejection_preserves_prior_transcript_and_cot() {
    let fixture = Fixture::new("cot-overflow", 2048);
    assert!(
        fixture
            .tap
            .emit(StreamEvent::TextDelta("x".repeat(MAX_CAPTURE_BYTES - 128)))
    );
    assert!(!fixture.tap.cancel.is_cancelled());
    let outcome = fixture.run_turn();
    let transcript_unchanged = fs::read(&fixture.transcript).unwrap() == TRANSCRIPT_SEED;
    let cot_unchanged = fs::read(&fixture.cot).unwrap() == COT_SEED;
    let rejected = !matches!(outcome, Ok(0));
    let cancelled = fixture.tap.cancel.is_cancelled();
    let failure_visible = fixture
        .events
        .try_iter()
        .any(|event| matches!(event, StreamEvent::Failed(_)));
    assert!(
        rejected && cancelled && failure_visible && transcript_unchanged && cot_unchanged,
        "captured rejection was too late: outcome={outcome:?}, cancelled={cancelled}, failed_event={failure_visible}, transcript_unchanged={transcript_unchanged}, cot_unchanged={cot_unchanged}"
    );
    assert_eq!(
        fixture
            .state
            .resolve_chat_for(AgentBackend::Abi)
            .unwrap()
            .as_deref(),
        Some(CHAT)
    );
}

#[test]
fn headless_capture_control_returns_exact_owned_output_without_cancellation() {
    let fixture = Fixture::new("capture-control", 2048);
    let (status, output, errors) = fixture
        .cfg
        .run_capture(None, &["private synthetic prompt".into()])
        .unwrap();
    assert!(status.success());
    assert_eq!(output, fixture.body);
    assert!(errors.is_empty());
    assert!(!fixture.tap.cancel.is_cancelled());
    assert_eq!(fs::read(&fixture.transcript).unwrap(), TRANSCRIPT_SEED);
    assert_eq!(fs::read(&fixture.cot).unwrap(), COT_SEED);
}

#[test]
fn successive_headless_captures_share_the_original_tap_raw_budget() {
    let fixture = Fixture::new("capture-cumulative", MAX_CAPTURE_BYTES / 2 + 1);
    let first = fixture
        .cfg
        .run_capture(None, &["first private synthetic prompt".into()]);
    assert!(
        matches!(&first, Ok((status, output, errors)) if status.success() && output.len() == fixture.body.len() && errors.is_empty())
    );
    let second = fixture
        .cfg
        .clone()
        .run_capture(None, &["second private synthetic prompt".into()]);
    let accepted_second = matches!(&second, Ok((status, _, _)) if status.success());
    let cancelled = fixture.tap.cancel.is_cancelled();
    let failure_visible = fixture
        .events
        .try_iter()
        .any(|event| matches!(event, StreamEvent::Failed(_)));
    assert!(
        !accepted_second && cancelled && failure_visible,
        "successive headless captures bypassed the shared raw ceiling: accepted_second={accepted_second}, cancelled={cancelled}, failed_event={failure_visible}, remaining_raw={}",
        fixture.tap.raw_remaining()
    );
    assert_eq!(fs::read(&fixture.transcript).unwrap(), TRANSCRIPT_SEED);
    assert_eq!(fs::read(&fixture.cot).unwrap(), COT_SEED);
}

mod identity_tests;
