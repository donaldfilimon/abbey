//! Regression coverage uses private executable fixtures and actual TUI APIs.
//! The App literal deliberately avoids ambient constructor refreshes.

use crate::agent::{AgentBackend, AgentConfig};
use crate::state::AbbeyState;
use crate::tui::app::{App, Overlay};
use crate::tui::completion::{Completion, FileIndex};
use crate::tui::composer::{Composer, History};
use crate::tui::theme::{Theme, ThemeId};
use crate::tui::transcript::Transcript;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::{Pid, getpgid};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const DRIVE_LIMIT: Duration = Duration::from_secs(8);
const ONCE_OBSERVATION: Duration = Duration::from_millis(1_200);

#[derive(Clone, Copy)]
enum Mode {
    Success,
    MissingTag,
    Nonzero,
    Unknown,
    Overflow,
    HeldList,
    HeldRun,
    DelayedFirst,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::MissingTag => "missing-tag",
            Self::Nonzero => "nonzero",
            Self::Unknown => "unknown",
            Self::Overflow => "overflow",
            Self::HeldList => "held-list",
            Self::HeldRun => "held-run",
            Self::DelayedFirst => "delayed-first",
        }
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
}

fn ctrl(app: &mut App, character: char) {
    app.handle_key(KeyEvent {
        code: KeyCode::Char(character),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
}

fn has_hint(app: &App, name: &str) -> bool {
    matches!(
        &app.completion,
        Some(Completion::Slash(rows))
            if rows.iter().any(|row| row.name == name && row.via == "llm")
    )
}

fn has_lexical(app: &App, name: &str) -> bool {
    matches!(
        &app.completion,
        Some(Completion::Slash(rows))
            if rows.iter().any(|row| row.name == name && row.via != "llm")
    )
}

fn tick(app: &mut App) {
    app.pump();
    app.tick = app.tick.wrapping_add(1);
    std::thread::sleep(Duration::from_millis(20));
}

fn drive_for(app: &mut App, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        tick(app);
    }
}

fn drive_until(app: &mut App, condition: impl Fn(&App) -> bool) -> bool {
    let deadline = Instant::now() + DRIVE_LIMIT;
    while Instant::now() < deadline {
        tick(app);
        if condition(app) {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
struct OwnedGroup {
    leader: Pid,
    group: Pid,
    descendant: Option<Pid>,
}

struct Fixture {
    app: App,
    root: PathBuf,
    nonce: String,
    groups: Vec<OwnedGroup>,
}

impl Fixture {
    fn new(mode: Mode, backend: AgentBackend) -> Self {
        let nonce = uuid::Uuid::new_v4().to_string();
        let root =
            std::env::temp_dir().join(format!("abbey-prediction-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).expect("owned fixture directory");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let state_dir = root.join("state");
        fs::create_dir(&state_dir).unwrap();
        fs::create_dir(state_dir.join("by-cwd")).unwrap();
        let state = AbbeyState {
            chat_file: state_dir.join("chat-id"),
            model_file: state_dir.join("model"),
            history_file: state_dir.join("history.log"),
            cwd_dir: state_dir.join("by-cwd"),
            state_dir: state_dir.clone(),
            per_cwd: false,
            cwd: root.clone(),
        };
        let executable = root.join("ollama");
        let script = format!(
            r#"#!/bin/sh
ROOT={root}
NONCE='{nonce}'
MODE='{mode}'
case "$1" in
  --version|version) printf 'owned-ollama-fixture\n'; exit 0 ;;
  list)
    printf 'list\n' >> "$ROOT/calls"
    if [ "$MODE" = held-list ]; then
      trap 'printf "%s %s\n" "$NONCE" "$$" > "$ROOT/list-term"' TERM
      printf '%s %s\n' "$NONCE" "$$" > "$ROOT/list-ready"
      while [ ! -f "$ROOT/release-list" ]; do /bin/sleep 0.02; done
    fi
    printf 'NAME ID SIZE MODIFIED\n'
    printf 'gemma4:26b-mlx generation-tag 1GB now\n'
    if [ "$MODE" = missing-tag ]; then
      printf 'gemma4:12b-mlx-other wrong-tag 1GB now\n'
    else
      printf 'gemma4:12b-mlx prediction-tag 1GB now\n'
    fi
    exit 0 ;;
  pull) printf 'forbidden-pull\n' >> "$ROOT/calls"; exit 91 ;;
  run)
    shift
    if [ "$#" -ne 5 ] || [ "$1" != --nowordwrap ] ||
       [ "$2" != --hidethinking ] || [ "$3" != gemma4:12b-mlx ] ||
       [ "$4" != -- ]; then
      printf 'unexpected-run\n' >> "$ROOT/calls"; exit 92
    fi
    printf 'run gemma4:12b-mlx\n' >> "$ROOT/calls"
    if [ "$MODE" = delayed-first ] && [ -f "$ROOT/run-ready" ]; then
      printf 'memory\n'; exit 0
    fi
    if [ "$MODE" = held-run ] || [ "$MODE" = delayed-first ]; then
      trap 'printf "%s %s\n" "$NONCE" "$$" > "$ROOT/run-term"; if [ -n "$descendant" ]; then kill "$descendant" 2>/dev/null || :; wait "$descendant" 2>/dev/null || :; fi' TERM
      /bin/sleep 30 & descendant=$!
      printf '%s %s\n' "$NONCE" "$descendant" > "$ROOT/run-child"
      printf '%s %s\n' "$NONCE" "$$" > "$ROOT/run-ready"
      while [ ! -f "$ROOT/release-run" ]; do /bin/sleep 0.02; done
      if [ "$MODE" = delayed-first ]; then
        kill "$descendant" 2>/dev/null || :
        wait "$descendant" 2>/dev/null || :
        printf 'doctor\n'; exit 0
      fi
    fi
    case "$MODE" in
      nonzero) printf 'review\n'; exit 17 ;;
      unknown) printf 'invented-not-in-the-catalog\n'; exit 0 ;;
      overflow)
        printf 'review\n'
        i=0; while [ "$i" -lt 4097 ]; do printf x; i=$((i + 1)); done
        exit 0 ;;
      *) printf 'review\n'; exit 0 ;;
    esac ;;
  *) printf 'unexpected-invocation\n' >> "$ROOT/calls"; exit 93 ;;
esac
"#,
            root = shell_quote(&root),
            nonce = nonce,
            mode = mode.name(),
        );
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        // No default resolver, App::new refresh, model-list startup probe,
        // ambient inventory/config read, or global environment mutation.
        // Root must initialize any later-added private owner field here once.
        let cfg = AgentConfig::fixed_provider_recipe(executable, backend, "gemma4:26b-mlx".into());
        let app = App {
            state,
            cfg,
            theme_id: ThemeId::Ink,
            theme: Theme::from_id(ThemeId::Ink),
            transcript: Transcript::default(),
            composer: Composer::default(),
            history_log: History::load(&state_dir),
            files: FileIndex::from_paths(Vec::new()),
            completion: None,
            completion_idx: 0,
            completion_basis: None,
            prediction: crate::tui::prediction_owner::PredictionOwner::default(),
            run: None,
            queued: Vec::new(),
            overlay: Overlay::None,
            panel_view: crate::tui::overlays::PanelView::default(),
            scroll_from_bottom: 0,
            status: String::new(),
            should_quit: false,
            ctrl_c_armed: false,
            last_esc: None,
            last_submitted: None,
            pending_suspend: None,
            tick: 0,
            claims_lines: Vec::new(),
            route_lines: Vec::new(),
            doctor_lines: Vec::new(),
            history: Vec::new(),
            aliases: Vec::new(),
            live_models: Vec::new(),
            persona_lines: Vec::new(),
            memory_lines: Vec::new(),
            skill_lines: Vec::new(),
        };
        Self {
            app,
            root,
            nonce,
            groups: Vec::new(),
        }
    }

    fn calls(&self) -> Vec<String> {
        let raw = fs::read_to_string(self.root.join("calls")).unwrap_or_default();
        assert!(
            raw.len() <= 4_096,
            "fixture invocation count exceeded its bound"
        );
        let lines: Vec<_> = raw.lines().map(str::to_string).collect();
        assert!(
            lines
                .iter()
                .all(|line| line == "list" || line == "run gemma4:12b-mlx"),
            "producer contacted an unexpected grammar/model or attempted a pull: {lines:?}"
        );
        lines
    }

    fn count(&self, prefix: &str) -> usize {
        self.calls()
            .iter()
            .filter(|line| line.starts_with(prefix))
            .count()
    }

    fn pid_receipt(&self, name: &str) -> Option<Pid> {
        let raw = fs::read_to_string(self.root.join(name)).ok()?;
        let mut parts = raw.split_whitespace();
        if parts.next()? != self.nonce {
            return None;
        }
        let pid = parts.next()?.parse::<i32>().ok()?;
        (pid > 1 && pid != i32::try_from(std::process::id()).ok()? && parts.next().is_none())
            .then_some(Pid::from_raw(pid))
    }

    fn ready(&mut self, phase: &str) -> OwnedGroup {
        let receipt = self.root.join(format!("{phase}-ready"));
        assert!(
            drive_until(&mut self.app, |_| receipt.is_file()),
            "eligible actual App producer did not start its owned {phase} process"
        );
        let leader = self
            .pid_receipt(&format!("{phase}-ready"))
            .expect("owned nonce/PID");
        let group = getpgid(Some(leader)).expect("held owned producer group");
        assert_eq!(
            group, leader,
            "supervisor must own a distinct process group"
        );
        assert_ne!(
            getpgid(None).unwrap(),
            group,
            "fixture must not signal the test runner"
        );
        let descendant = if phase == "run" {
            let descendant = self.pid_receipt("run-child").expect("owned descendant");
            assert_eq!(getpgid(Some(descendant)).unwrap(), group);
            Some(descendant)
        } else {
            None
        };
        let owned = OwnedGroup {
            leader,
            group,
            descendant,
        };
        self.groups.push(owned);
        owned
    }

    fn finish(&mut self) {
        self.app
            .shutdown()
            .expect("observed prediction/primary owner join");
    }

    fn assert_cancellation_started_before_timeout(&self, phase: &str, requested: SystemTime) {
        let receipt = format!("{phase}-term");
        assert!(
            self.pid_receipt(&receipt).is_some(),
            "actual supervisor SIGTERM not observed"
        );
        let when = fs::metadata(self.root.join(receipt))
            .unwrap()
            .modified()
            .unwrap();
        assert!(
            when <= requested + Duration::from_secs(1),
            "held {phase} survived cancellation until its natural work timeout"
        );
    }
}

fn assert_gone(pid: Pid) {
    // The production supervisor's successful join already observes group
    // disappearance and reaps its leader. Do not wait for cleanup to do it.
    assert_eq!(
        kill(pid, None),
        Err(Errno::ESRCH),
        "owned process survived: {pid}"
    );
}

fn assert_group_gone(owned: OwnedGroup) {
    assert_gone(owned.leader);
    if let Some(descendant) = owned.descendant {
        assert_gone(descendant);
    }
    assert_eq!(
        killpg(owned.group, None),
        Err(Errno::ESRCH),
        "owned group survived"
    );
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Emergency fixture cleanup is explicitly after behavioral assertions.
        let _ = self.app.shutdown();
        for phase in ["list", "run"] {
            if let Some(pid) = self.pid_receipt(&format!("{phase}-ready"))
                && let Ok(group) = getpgid(Some(pid))
                && group == pid
                && getpgid(None).is_ok_and(|ours| ours != group)
                && !self.groups.iter().any(|owned| owned.group == group)
            {
                self.groups.push(OwnedGroup {
                    leader: pid,
                    group,
                    descendant: None,
                });
            }
        }
        for owned in &self.groups {
            let _ = killpg(owned.group, Signal::SIGKILL);
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn eligible_selected_ollama_app_runs_one_prediction_without_submitting_a_turn() {
    let mut fixture = Fixture::new(Mode::Success, AgentBackend::Ollama);
    fixture.app.handle_paste("/rev");
    assert!(
        has_lexical(&fixture.app, "review"),
        "independent lexical control"
    );
    assert!(
        drive_until(&mut fixture.app, |app| has_hint(app, "review")),
        "App never applied a hint from the eligible selected owned Ollama"
    );
    assert_eq!(fixture.count("list"), 1);
    assert_eq!(fixture.count("run"), 1);
    assert!(fixture.app.run.is_none());
    assert!(fixture.app.transcript.cells.is_empty());
    assert!(fixture.app.queued.is_empty());
    assert_eq!(fixture.app.composer.text, "/rev");
    drive_for(&mut fixture.app, ONCE_OBSERVATION);
    press(&mut fixture.app, KeyCode::Left);
    press(&mut fixture.app, KeyCode::Right);
    drive_for(&mut fixture.app, ONCE_OBSERVATION);
    assert_eq!(
        fixture.count("list"),
        1,
        "unchanged draft re-probed metadata"
    );
    assert_eq!(
        fixture.count("run"),
        1,
        "unchanged draft repeated the rerank"
    );
    fixture.finish();
}

#[test]
fn optional_prediction_never_probes_an_unselected_backend() {
    for backend in [
        AgentBackend::Cursor,
        AgentBackend::Claude,
        AgentBackend::Grok,
        AgentBackend::Fm,
        AgentBackend::Abi,
    ] {
        let mut fixture = Fixture::new(Mode::Success, backend);
        fixture.app.handle_paste("/rev");
        assert!(has_lexical(&fixture.app, "review"));
        drive_for(&mut fixture.app, ONCE_OBSERVATION);
        assert!(
            fixture.calls().is_empty(),
            "prediction contacted unselected {backend:?}"
        );
        fixture.finish();
    }
}

#[test]
fn missing_exact_prediction_tag_consumes_one_attempt_and_never_runs_or_pulls() {
    let mut fixture = Fixture::new(Mode::MissingTag, AgentBackend::Ollama);
    fixture.app.handle_paste("/rev");
    let calls = fixture.root.join("calls");
    assert!(
        drive_until(&mut fixture.app, |_| calls.is_file()),
        "eligible model-list probe absent"
    );
    drive_for(&mut fixture.app, ONCE_OBSERVATION);
    assert_eq!(fixture.count("list"), 1);
    assert_eq!(fixture.count("run"), 0);
    assert!(has_lexical(&fixture.app, "review"));
    assert!(!has_hint(&fixture.app, "review"));
    press(&mut fixture.app, KeyCode::Left);
    press(&mut fixture.app, KeyCode::Right);
    drive_for(&mut fixture.app, ONCE_OBSERVATION);
    assert_eq!(
        fixture.count("list"),
        1,
        "negative probe was retried without an edit"
    );
    fixture.finish();
}

#[test]
fn invalid_owned_rerank_outcomes_keep_lexical_completion_without_repeating() {
    for mode in [Mode::Nonzero, Mode::Unknown, Mode::Overflow] {
        let mut fixture = Fixture::new(mode, AgentBackend::Ollama);
        fixture.app.handle_paste("/rev");
        let calls = fixture.root.join("calls");
        assert!(
            drive_until(&mut fixture.app, |_| {
                fs::read_to_string(&calls).is_ok_and(|text| text.contains("run gemma4:12b-mlx"))
            }),
            "actual owned rerank never ran"
        );
        drive_for(&mut fixture.app, ONCE_OBSERVATION);
        assert!(has_lexical(&fixture.app, "review"));
        assert!(!has_hint(&fixture.app, "review"));
        assert_eq!(fixture.count("list"), 1);
        assert_eq!(fixture.count("run"), 1);
        fixture.finish();
    }
}

#[test]
fn changing_the_draft_cancels_old_process_and_cannot_apply_its_old_hint() {
    let mut fixture = Fixture::new(Mode::DelayedFirst, AgentBackend::Ollama);
    fixture.app.handle_paste("/rev");
    let old = fixture.ready("run");
    ctrl(&mut fixture.app, 'u');
    fixture.app.handle_paste("/mem");
    fs::write(fixture.root.join("release-run"), b"release owned fixture").unwrap();
    let deadline = Instant::now() + DRIVE_LIMIT;
    while Instant::now() < deadline && !has_hint(&fixture.app, "memory") {
        tick(&mut fixture.app);
        assert!(
            !has_hint(&fixture.app, "doctor"),
            "old captured draft leaked its hint"
        );
    }
    assert!(
        has_hint(&fixture.app, "memory"),
        "new draft was not independently admitted"
    );
    assert_eq!(fixture.app.composer.text, "/mem");
    assert_eq!(fixture.count("run"), 2);
    assert_group_gone(old);
    fixture.finish();
}

#[test]
fn changing_the_selected_backend_invalidates_a_held_prediction() {
    let mut fixture = Fixture::new(Mode::HeldRun, AgentBackend::Ollama);
    fixture.app.handle_paste("/rev");
    let old = fixture.ready("run");
    // Direct current fact change; native Ctrl-B resolution is covered by the
    // separate isolated actual-binary PTY, not by an ambient host resolver.
    fixture.app.cfg.backend = AgentBackend::Cursor;
    let requested = SystemTime::now();
    drive_for(&mut fixture.app, ONCE_OBSERVATION);
    assert!(!has_hint(&fixture.app, "review"));
    assert_eq!(fixture.count("run"), 1);
    assert_group_gone(old);
    fixture.assert_cancellation_started_before_timeout("run", requested);
    fixture.finish();
}

#[test]
fn dismissing_completion_joins_the_held_prediction_and_does_not_rearm() {
    let mut fixture = Fixture::new(Mode::HeldRun, AgentBackend::Ollama);
    fixture.app.handle_paste("/rev");
    let old = fixture.ready("run");
    let requested = SystemTime::now();
    press(&mut fixture.app, KeyCode::Esc);
    drive_for(&mut fixture.app, ONCE_OBSERVATION);
    assert!(fixture.app.completion.is_none());
    assert_eq!(fixture.count("run"), 1);
    assert_group_gone(old);
    fixture.assert_cancellation_started_before_timeout("run", requested);
    fixture.finish();
}

#[test]
fn shutdown_observes_held_model_list_join_and_never_runs_after_cancellation() {
    let mut fixture = Fixture::new(Mode::HeldList, AgentBackend::Ollama);
    fixture.app.handle_paste("/rev");
    let owned = fixture.ready("list");
    let started = Instant::now();
    let requested = SystemTime::now();
    fixture.finish();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "probe cancellation lost its bound"
    );
    assert_group_gone(owned);
    fixture.assert_cancellation_started_before_timeout("list", requested);
    assert_eq!(fixture.count("list"), 1);
    assert_eq!(fixture.count("run"), 0);
}

#[test]
fn quit_shutdown_observes_held_run_and_descendant_joins() {
    let mut fixture = Fixture::new(Mode::HeldRun, AgentBackend::Ollama);
    fixture.app.handle_paste("/rev");
    let owned = fixture.ready("run");
    let requested = SystemTime::now();
    ctrl(&mut fixture.app, 'c');
    ctrl(&mut fixture.app, 'c');
    assert!(fixture.app.should_quit);
    let started = Instant::now();
    fixture.finish();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "run cancellation lost its bound"
    );
    assert_group_gone(owned);
    fixture.assert_cancellation_started_before_timeout("run", requested);
    assert_eq!(fixture.count("run"), 1);
    assert!(fixture.app.run.is_none());
    assert!(fixture.app.queued.is_empty());
}
