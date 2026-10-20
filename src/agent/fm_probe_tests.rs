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
        AgentConfig::fixed_provider_recipe(self.executable.clone(), AgentBackend::Fm, "system".into())
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.join("calls")).unwrap_or_default()
    }

    fn assert_both(&self, expected: FmModels, calls: &str) {
        assert_eq!(fm_models_probe(&self.executable), Some(expected));
        assert_eq!(self.calls(), calls);
        std::fs::remove_file(self.dir.join("calls")).unwrap();
        assert_eq!(
            self.config().fm_availability_owned().unwrap(),
            format!(
                "on-device system model: {} · private cloud compute: {}",
                if expected.system { "available" } else { "unavailable" },
                if expected.pcc { "available" } else { "unavailable" }
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
    probe.assert_both(FmModels { system: true, pcc: false }, "models\n");
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
        probe.assert_both(FmModels { system: true, pcc: false }, "models\navailable\n");
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
    let probe = Probe::new(
        "printf \"Error: Unknown command 'models'\\n\" >&2\nexec /bin/sleep 10",
    );
    let mut cfg = probe.config();
    let (sender, _receiver) = std::sync::mpsc::channel();
    let tap = crate::stream::StreamTap::new(sender);
    let cancel = tap.cancel.clone();
    cfg.stream = Some(tap);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| cfg.fm_availability_owned());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while probe.calls().is_empty() {
            assert!(std::time::Instant::now() < deadline, "probe never started");
            std::thread::sleep(Duration::from_millis(2));
        }
        cancel.cancel();
        let error = worker.join().unwrap().unwrap_err();
        assert!(error.downcast_ref::<crate::agent::CaptureCancelled>().is_some());
    });
    assert_eq!(probe.calls(), "models\n");
}

#[test]
fn fm_probe_timeout_never_falls_back() {
    let probe = Probe::new(
        "printf \"Error: Unknown command 'models'\\n\" >&2\nexec /bin/sleep 10",
    );
    assert_eq!(fm_models_probe(&probe.executable), None);
    assert_eq!(probe.calls(), "models\n");
    std::fs::remove_file(probe.dir.join("calls")).unwrap();
    assert!(probe.config().fm_availability_owned().is_err());
    assert_eq!(probe.calls(), "models\n");
}
