//! Bounded Unix executor metadata probes, separate from generation.
#[cfg(not(unix))]
use std::process::Command;

pub(super) fn version(path: &std::path::Path) -> String {
    #[cfg(unix)]
    let stdout = {
        use crate::runtime::supervisor::{
            ProcessSpec, SupervisorLimits, SupervisorOutcome, run_with_checkpoint,
        };
        use std::time::Duration;
        let spec = ProcessSpec::inherited(path.to_path_buf(), vec!["--version".into()]);
        let limits = SupervisorLimits {
            timeout: Duration::from_millis(500),
            terminate_grace: Duration::from_millis(100),
            stdout_bytes: 4096,
            stderr_bytes: 4096,
            poll_interval: Duration::from_millis(5),
        };
        match run_with_checkpoint(&spec, &limits, || false) {
            Ok(SupervisorOutcome::Exited { status, stdout, .. }) if status.success() => {
                Some(stdout)
            }
            _ => None,
        }
    };
    #[cfg(not(unix))]
    let stdout = Command::new(path)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| o.stdout);
    stdout
        .and_then(|o| String::from_utf8(o).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}
