//! Process fixtures for both read-only Foundation Models CLI grammars.

use super::*;
use crate::agent::AgentConfig;
use std::os::unix::fs::PermissionsExt as _;

struct Probe {
    dir: PathBuf,
    executable: PathBuf,
}

impl Probe {
    fn new(body: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("abbey-fm-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let executable = dir.join("fm");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n{body}\n",
                dir.join("calls").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self { dir, executable }
    }

    fn config(&self) -> AgentConfig {
        AgentConfig::fixed_provider_recipe(
            self.executable.clone(),
            AgentBackend::Fm,
            "system".into(),
        )
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.join("calls")).unwrap_or_default()
    }

    /// The probe's answer, retried while it stays indeterminate.
    ///
    /// `fm_models_probe` answers `None` when the probe could not complete
    /// inside its bounded window. That is an absent observation, not the
    /// fact "the models are unavailable", and a loaded machine can push a
    /// trivial stub past that window. Each attempt starts from a clean call
    /// log, so the recorded invocations describe the attempt that produced
    /// the answer rather than the attempts that preceded it.
    fn probe_answer(&self) -> Option<FmModels> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let _ = std::fs::remove_file(self.dir.join("calls"));
            match fm_models_probe(&self.executable) {
                Some(observed) => return Some(observed),
                None => {
                    if std::time::Instant::now() >= deadline {
                        return None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }

    fn assert_both(&self, expected: FmModels, calls: &str) {
        assert_eq!(self.probe_answer(), Some(expected));
        assert_eq!(self.calls(), calls);
        std::fs::remove_file(self.dir.join("calls")).unwrap();
        assert_eq!(
            self.config().fm_availability_owned().unwrap(),
            format!(
                "on-device system model: {} · private cloud compute: {}",
                if expected.system {
                    "available"
                } else {
                    "unavailable"
                },
                if expected.pcc {
                    "available"
                } else {
                    "unavailable"
                }
            )
        );
        assert_eq!(self.calls(), calls);
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        for name in ["fm", "calls"] {
            let _ = std::fs::remove_file(self.dir.join(name));
        }
        let _ = std::fs::remove_dir(&self.dir);
    }
}

#[test]
fn fm_probe_current_grammar_accepts_partial_availability_without_fallback() {
    let probe = Probe::new(
        "printf '\\033[32m✓ system (AFM)\\033[0m\\n'\nprintf '✗ pcc (not available)\\n' >&2\nexit 1",
    );
    probe.assert_both(
        FmModels {
            system: true,
            pcc: false,
        },
        "models\n",
    );
}

#[test]
fn fm_probe_unsupported_models_falls_back_once_on_either_stream() {
    for diagnostic in [
        "printf \"\\033[31mError: Unknown command 'models'. Run fm --help\\033[0m\\n\" >&2",
        "printf \"error: unrecognized subcommand 'models'\\n\"",
    ] {
        let probe = Probe::new(&format!(
            "case \"$1\" in\nmodels) {diagnostic}; exit 1;;\navailable) printf 'System model available\\n' >&2;;\n*) exit 99;;\nesac"
        ));
        probe.assert_both(
            FmModels {
                system: true,
                pcc: false,
            },
            "models\navailable\n",
        );
    }
}

#[test]
fn fm_probe_negative_or_malformed_output_never_falls_back() {
    for body in [
        "printf '✗ system (unavailable)\\n✗ pcc\\n'; exit 1",
        "printf 'System model not available\\nPrivate Cloud Compute is not available\\n'; exit 1",
        "printf 'unexpected response\\n'; exit 0",
        "printf 'not system model available\\nPrivate Cloud Compute maybe\\n'; exit 0",
        "printf \"Error: Unknown command 'models-extra'\\n\"; exit 1",
        "printf \"Error: Unknown command 'models'\\n\"; exit 0",
    ] {
        Probe::new(body).assert_both(FmModels::default(), "models\n");
    }
}

#[test]
fn fm_probe_overflow_and_spawn_failure_never_fall_back() {
    let probe = Probe::new(
        "printf \"Error: Unknown command 'models'\\n\" >&2\n/usr/bin/awk 'BEGIN { for (i=0; i<70000; i++) printf \"x\" }'\nexit 1",
    );
    assert_eq!(fm_models_probe(&probe.executable), None);
    assert_eq!(probe.calls(), "models\n");
    std::fs::remove_file(probe.dir.join("calls")).unwrap();
    assert!(probe.config().fm_availability_owned().is_err());
    assert_eq!(probe.calls(), "models\n");
    let missing = probe.dir.join("missing");
    assert_eq!(fm_models_probe(&missing), None);
    let mut cfg = probe.config();
    cfg.agent_path = missing;
    assert!(cfg.fm_availability_owned().is_err());
}

#[test]
fn fm_probe_owned_cancellation_reaps_without_compatibility_fallback() {
    // The scenario proves one thing: cancelling an in-flight probe reports
    // the *cancellation* and never falls back to the compatibility grammar.
    //
    // Whether the probe is still in flight when the cancel arrives is a
    // scheduling fact, not the subject of the test: the probe's own capture
    // window is short, so on a loaded machine the stub can miss it and the
    // probe ends on its own with a timeout instead of a cancellation. That
    // is not a defect in the cancellation path, so the scenario is retried
    // until it observes the behaviour it is about, and fails only when it
    // never can.
    const ATTEMPTS: usize = 10;
    let mut last_outcome = String::from("the probe was never observed");
    for attempt in 1..=ATTEMPTS {
        let probe =
            Probe::new("printf \"Error: Unknown command 'models'\\n\" >&2\nexec /bin/sleep 10");
        let mut cfg = probe.config();
        let (sender, _receiver) = std::sync::mpsc::channel();
        let tap = crate::stream::StreamTap::new(sender);
        let cancel = tap.cancel.clone();
        cfg.stream = Some(tap);
        let observed = std::thread::scope(|scope| {
            let worker = scope.spawn(|| cfg.fm_availability_owned());
            // Wait for the probe to publish its receipt so the cancel lands
            // while it is in flight. This is a liveness bound, not the
            // subject of the test.
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while probe.calls().is_empty() {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            cancel.cancel();
            worker.join().unwrap()
        });
        // The probe writes its call log before it blocks, so an empty log
        // means the child never started — a different fact from a probe that
        // started and then reported something other than a cancellation.
        let calls = probe.calls();
        let started = !calls.is_empty();
        if started {
            // The owned grammar is the only one the probe may use. A
            // compatibility fallback would appear here on every outcome, not
            // only on the cancellation branch.
            assert_eq!(
                calls, "models\n",
                "attempt {attempt} used another grammar: {calls:?}"
            );
        }
        let cancelled = observed
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<crate::agent::CaptureCancelled>())
            .is_some();
        if cancelled {
            return;
        }
        last_outcome = match (&observed, started) {
            (Ok(_), _) => "the probe reported success".to_owned(),
            (Err(error), true) => format!("the probe reported {error:?}"),
            (Err(_), false) => "the probe never started".to_owned(),
        };
    }
    panic!("the cancellation path never reported a cancellation: {last_outcome}");
}

#[test]
fn fm_probe_timeout_never_falls_back() {
    let probe = Probe::new("printf \"Error: Unknown command 'models'\\n\" >&2\nexec /bin/sleep 10");
    assert_eq!(fm_models_probe(&probe.executable), None);
    assert_eq!(probe.calls(), "models\n");
    std::fs::remove_file(probe.dir.join("calls")).unwrap();
    assert!(probe.config().fm_availability_owned().is_err());
    assert_eq!(probe.calls(), "models\n");
}
