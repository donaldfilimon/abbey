//! JSONL routing audit log for hybrid persona/role decisions.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRecord {
    pub ts: String,
    pub cwd: String,
    pub persona: String,
    pub role: String,
    pub model: String,
    pub reason: String,
    pub confidence: f32,
    #[serde(default)]
    pub tools: Vec<String>,
    /// Shared id linking the stages of one multi-stage run (e.g. the
    /// Gemma-interpret → Max-implement hybrid loop). `None` for single-shot runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<String>,
    /// Stage name within a correlated run (`interpret`, `implement`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// Alternate role Abi considered (not auto-invoked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate: Option<String>,
    /// Fallback note when confidence is low (audit only — no second agent path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

impl RouteRecord {
    pub fn new(
        cwd: impl Into<String>,
        persona: impl Into<String>,
        role: impl Into<String>,
        model: impl Into<String>,
        reason: impl Into<String>,
        confidence: f32,
    ) -> Self {
        Self {
            ts: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            cwd: cwd.into(),
            persona: persona.into(),
            role: role.into(),
            model: model.into(),
            reason: reason.into(),
            confidence,
            tools: Vec::new(),
            correlation: None,
            stage: None,
            alternate: None,
            fallback: None,
        }
    }

    /// Tag this record as one stage of a correlated multi-stage run.
    pub fn in_stage(mut self, correlation: impl Into<String>, stage: impl Into<String>) -> Self {
        self.correlation = Some(correlation.into());
        self.stage = Some(stage.into());
        self
    }

    pub fn with_routing(mut self, alternate: Option<String>, fallback: Option<String>) -> Self {
        self.alternate = alternate;
        self.fallback = fallback;
        self
    }
}

/// All records sharing `correlation`, oldest first — the linked view of one
/// multi-stage run.
pub fn correlated_routes(state_dir: &Path, correlation: &str) -> Result<Vec<RouteRecord>> {
    Ok(recent_routes(state_dir, usize::MAX)?
        .into_iter()
        .filter(|r| r.correlation.as_deref() == Some(correlation))
        .collect())
}

pub fn route_log_path(state_dir: &Path) -> std::path::PathBuf {
    state_dir.join("route.jsonl")
}

pub fn append_route_record(state_dir: &Path, rec: &RouteRecord) -> Result<()> {
    fs::create_dir_all(state_dir)?;
    let path = route_log_path(state_dir);
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    serde_json::to_writer(&mut f, rec)?;
    f.write_all(b"\n")?;
    Ok(())
}

pub fn recent_routes(state_dir: &Path, n: usize) -> Result<Vec<RouteRecord>> {
    let path = route_log_path(state_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    let mut out: Vec<RouteRecord> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if out.len() > n {
        out = out.split_off(out.len() - n);
    }
    Ok(out)
}

/// Compact one-liner for the TUI Routes pane — narrow columns, no reason text.
///
/// The full audit line stays [`format_route_line`]; this keeps only what fits
/// a half-width pane: clock time, persona/role, model, confidence, stage.
pub fn compact_route_line(r: &RouteRecord) -> String {
    // ts is RFC3339-ish ("2026-08-08T04:53:21…") — keep HH:MM when present.
    let clock = if r.ts.len() >= 16 && r.ts.is_char_boundary(11) && r.ts.is_char_boundary(16) {
        &r.ts[11..16]
    } else {
        r.ts.as_str()
    };
    let stage = r.stage.as_deref().unwrap_or("-");
    format!(
        "{clock} {}/{} {} {:.2} {stage}",
        r.persona, r.role, r.model, r.confidence
    )
}

/// One-line display for CLI / slash — keeps both surfaces in lockstep.
pub fn format_route_line(r: &RouteRecord) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{:.2}\t{}\talt={}\tfb={}\t{}",
        r.ts,
        r.persona,
        r.role,
        r.model,
        r.confidence,
        r.stage.as_deref().unwrap_or("-"),
        r.alternate.as_deref().unwrap_or("-"),
        r.fallback.as_deref().unwrap_or("-"),
        r.reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read() {
        let dir = std::env::temp_dir().join(format!("abbey-route-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let rec = RouteRecord::new(".", "abbey", "max", "fable", "code heuristic", 0.8);
        append_route_record(&dir, &rec).unwrap();
        let got = recent_routes(&dir, 5).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].role, "max");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn alternate_and_fallback_survive_jsonl_roundtrip() {
        let dir = std::env::temp_dir().join(format!("abbey-route-alt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let rec = RouteRecord::new(".", "abbey", "max", "fable", "hybrid", 0.7)
            .with_routing(Some("gemma".into()), Some("prefer hybrid-loop".into()));
        append_route_record(&dir, &rec).unwrap();
        let got = recent_routes(&dir, 1).unwrap();
        assert_eq!(got[0].alternate.as_deref(), Some("gemma"));
        assert_eq!(got[0].fallback.as_deref(), Some("prefer hybrid-loop"));
        assert!(format_route_line(&got[0]).contains("alt=gemma"));
        assert!(format_route_line(&got[0]).contains("fb=prefer hybrid-loop"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compact_line_keeps_route_fields_and_drops_reason() {
        let rec = RouteRecord::new(".", "abbey", "max", "fable", "a very long reason", 0.82);
        let line = compact_route_line(&rec);
        assert!(line.contains("abbey/max"), "{line}");
        assert!(line.contains("fable"), "{line}");
        assert!(line.contains("0.82"), "{line}");
        assert!(
            !line.contains("a very long reason"),
            "reason must not widen the pane: {line}"
        );
        // A short/odd timestamp must fall back rather than panic or slice
        // off a char boundary.
        let mut odd = RouteRecord::new(".", "abbey", "max", "fable", "r", 0.5);
        odd.ts = "now".into();
        assert!(compact_route_line(&odd).starts_with("now "));
    }
}
