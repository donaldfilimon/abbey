//! Run report for `abbey improve` — timeline accumulator + on-disk markdown.
//!
//! Split out of `mod.rs` for file size. Self-contained: the report owns its own
//! fields and only reaches `AbbeyState` for `state_dir`, so nothing here shares
//! mutable state with the diagnose/implement loop.
//!
//! The rendered footer restates the loop's honesty constraints, so a report read
//! later cannot be mistaken for evidence of a multi-node run or a goal close.

use super::ImproveOpts;
use super::gate::GateReport;
use super::ledger::WorkFocus;
use crate::state::AbbeyState;
use crate::subagents::LaneResult;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) struct RunReport {
    correlation: String,
    focus: String,
    confirm: bool,
    max_rounds: usize,
    max_minutes: u64,
    lines: Vec<String>,
    /// Set by the loop at each terminating branch; rendered into the header.
    pub(super) outcome: String,
}

impl RunReport {
    pub(super) fn new(correlation: &str, focus: &WorkFocus, opts: &ImproveOpts) -> Self {
        Self {
            correlation: correlation.into(),
            focus: focus.summary(),
            confirm: opts.confirm,
            max_rounds: opts.max_rounds,
            max_minutes: opts.max_minutes,
            lines: Vec::new(),
            outcome: String::new(),
        }
    }

    pub(super) fn note_gate(&mut self, round: usize, g: &GateReport) {
        self.lines.push(format!(
            "- round {round} gate: ok={} exit={} kinds={} ({} ms)",
            g.ok,
            g.exit,
            g.kinds_csv(),
            g.elapsed_ms
        ));
    }

    pub(super) fn note_diagnose(&mut self, round: usize, results: &[LaneResult]) {
        let names: Vec<_> = results.iter().map(|r| r.name.as_str()).collect();
        self.lines
            .push(format!("- round {round} diagnose: {}", names.join(", ")));
    }

    pub(super) fn note_implement(&mut self, round: usize, r: &LaneResult) {
        self.lines.push(format!(
            "- round {round} implement: {} exit={}",
            r.name, r.exit
        ));
    }

    fn render(&self) -> String {
        let mut s = format!(
            "# Abbey improve report\n\n\
             correlation: {}\n\
             focus: {}\n\
             confirm: {}\n\
             budget: {} rounds / {} minutes\n\
             outcome: {}\n\n\
             ## Timeline\n\n",
            self.correlation,
            self.focus,
            self.confirm,
            self.max_rounds,
            self.max_minutes,
            self.outcome
        );
        for line in &self.lines {
            s.push_str(line);
            s.push('\n');
        }
        s.push_str(
            "\n## Honesty\n\n\
             - Local subagents / PATH peers only — not a multi-node mesh.\n\
             - `tasks/goals.md` was not auto-marked done.\n\
             - OS allowlist execute was not invoked by this loop.\n",
        );
        s
    }
}

pub(super) fn report_path(state_dir: &Path, correlation: &str) -> PathBuf {
    state_dir.join("improve").join(format!("{correlation}.md"))
}

pub(super) fn write_report(state: &AbbeyState, report: &RunReport) -> Result<()> {
    let dir = state.state_dir.join("improve");
    fs::create_dir_all(&dir)?;
    let path = report_path(&state.state_dir, &report.correlation);
    fs::write(&path, report.render())?;
    eprintln!("abbey: improve report → {}", path.display());
    Ok(())
}

/// Most recently modified report, for `improve status`'s "last report" line.
pub(super) fn latest_report_path(state_dir: &Path) -> Option<PathBuf> {
    let dir = state_dir.join("improve");
    let rd = fs::read_dir(dir).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for ent in rd.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let modified = ent.metadata().ok()?.modified().ok()?;
        if best.as_ref().is_none_or(|(t, _)| modified > *t) {
            best = Some((modified, path));
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> ImproveOpts {
        ImproveOpts::default()
    }

    #[test]
    fn render_carries_outcome_and_honesty_footer() {
        let mut r = RunReport::new("corr-1", &WorkFocus::Stabilize, &opts());
        r.outcome = "gate green".into();
        let out = r.render();
        assert!(out.contains("correlation: corr-1"));
        assert!(out.contains("outcome: gate green"));
        assert!(out.contains("not a multi-node mesh"));
        assert!(out.contains("was not auto-marked done"));
    }

    #[test]
    fn report_path_is_correlation_scoped() {
        let p = report_path(Path::new("/state"), "abc");
        assert!(p.ends_with("improve/abc.md"));
    }
}
