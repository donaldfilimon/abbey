//! Background-data refresh for the Personas/Memory/Skills/Doctor panels.
//!
//! Split out of `app.rs` purely for file-size — these four methods only read
//! `self` via pub fields and write back a `Vec<String>` per panel, no shared
//! state with the input/event-handling half of `App`.

use crate::agent::AgentBackend;
use crate::config;
use crate::inventory;
use crate::memory;
use crate::persona;
use crate::roles;
use std::time::Duration;

use super::app::App;

/// Doctor line naming the active executor backend.
pub fn backend_doctor_line(backend: AgentBackend) -> String {
    match backend {
        AgentBackend::Cursor => "backend:   cursor-agent (default)".into(),
        AgentBackend::Grok => "backend:   grok (ABBEY_BACKEND=grok)".into(),
        AgentBackend::Fm => {
            "backend:   fm — on-device Apple Foundation Models (ABBEY_BACKEND=fm)".into()
        }
        AgentBackend::Abi => {
            "backend:   abi — `abi complete`, local persona-template; claude-*/live \
             opt into Anthropic transport (ABBEY_BACKEND=abi)"
                .into()
        }
        AgentBackend::Claude => {
            "backend:   claude — Claude Code CLI, no cursor-agent (ABBEY_BACKEND=claude)".into()
        }
    }
}

/// Doctor line for Max/Gemma — bindings only exist where cursor model ids do.
pub fn roles_doctor_line(backend: AgentBackend) -> String {
    if matches!(backend, AgentBackend::Fm | AgentBackend::Abi) {
        format!(
            "roles:      Max/Gemma are prompt-only under `{}` (no per-role model ids)",
            backend.label()
        )
    } else {
        format!(
            "roles:      Max→technical · Gemma→visual (model bindings via {})",
            backend.label()
        )
    }
}

impl App {
    pub fn refresh_personas(&mut self) {
        let ac = config::AbbeyConfig::load().unwrap_or_default();
        let mut lines = persona::persona_status_lines("");
        lines.extend(roles::role_status_lines(&ac.roles.max, &ac.roles.gemma));
        lines.push(format!("default_role: {}", ac.default_role));
        lines.push(
            "routing: route_decision → route.jsonl (alt/fb audit only; no auto second agent)"
                .into(),
        );
        lines.push("Tip: /max · /gemma · /routes · hybrid-loop · /persona aviva".into());
        self.persona_lines = lines;
    }

    pub fn refresh_memory(&mut self) {
        let abbey_cfg = config::AbbeyConfig::load().unwrap_or_default();
        let backend = abbey_cfg.memory_backend.clone();
        let mut lines = vec![memory::backend_status(&self.state.state_dir, &backend)];
        let opened = memory::open_backend_with_timeout(
            &self.state.state_dir,
            &backend,
            Duration::from_millis(250),
        );
        match opened {
            Ok(mem) => {
                match memory::build_embedder(&abbey_cfg.embeddings) {
                    Ok(embedder) if embedder.space().provider == "none" => {
                        lines.push("semantic disabled (lexical search remains available)".into());
                    }
                    Ok(embedder) => match memory::embedding_status(mem.as_ref(), embedder.as_ref())
                    {
                        Ok(status) => lines.push(format!(
                            "semantic {} {}d ready={} pending={}",
                            embedder.space().provider,
                            embedder.space().dimension,
                            status.ready,
                            status.pending()
                        )),
                        Err(error) => lines.push(format!("semantic index unavailable: {error}")),
                    },
                    Err(error) => lines.push(format!("semantic provider unavailable: {error}")),
                }
                for layer in ["stm", "ltm", "activity", "train_candidate"] {
                    let n = mem
                        .filter(Some(layer), None, 500)
                        .map(|v| v.len())
                        .unwrap_or(0);
                    lines.push(format!("{layer:<16} {n}"));
                }
                if let Ok(report) = mem.reflect() {
                    lines.push(format!(
                        "reflect low={} dups={} superseded={}",
                        report.low_confidence.len(),
                        report.duplicate_summaries.len(),
                        report.superseded.len()
                    ));
                }
                if let Ok(prefs) = mem.filter(Some("ltm"), Some("preference"), 10) {
                    for p in prefs.into_iter().take(5) {
                        lines.push(format!("pref: {}", p.summary));
                    }
                }
                if let Ok(records) = mem.filter(None, None, 12) {
                    if records.is_empty() {
                        lines.push(
                            "map: empty — teach with `abbey memory put --tag <subject>`".into(),
                        );
                    } else {
                        lines.push("map  topic×recency×depth (CLI: abbey memory map|near)".into());
                        for r in &records {
                            let p = memory::coordinates(r);
                            lines.push(format!(
                                "  {:>5.0} {:>6.2} {:>5.2}  {:<14} {}",
                                p.x,
                                p.y,
                                p.z,
                                memory::primary_topic(r),
                                r.summary
                            ));
                        }
                    }
                }
            }
            Err(e) => lines.push(format!("unavailable: {e}")),
        }
        lines.push("CLI: abbey learn correction|preference|digest|review|stats".into());
        self.memory_lines = lines;
    }

    pub fn refresh_skills(&mut self) {
        let mut lines = Vec::new();
        if let Ok(skills) = inventory::list_skills() {
            for s in skills.into_iter().take(80) {
                if s.description.is_empty() {
                    lines.push(s.name);
                } else {
                    lines.push(format!("{} — {}", s.name, s.description));
                }
            }
        }
        for t in inventory::list_agent_tools() {
            let mark = if t.path.is_some() { "✓" } else { "·" };
            lines.push(format!("{mark} tool:{:<12} {}", t.name, t.kind));
        }
        if lines.is_empty() {
            lines.push("(no skills/tools found)".into());
        }
        self.skill_lines = lines;
    }

    pub fn refresh_doctor(&mut self) {
        let chat = self
            .state
            .read_chat_for(self.cfg.backend)
            .unwrap_or_else(|| "(none)".into());
        let ac = config::AbbeyConfig::load().unwrap_or_default();
        let mut lines = crate::build_info::lines();
        lines.extend([
            backend_doctor_line(self.cfg.backend),
            format!("agent:     {}", self.cfg.agent_path.display()),
            format!("agent ver: {}", self.cfg.agent_version()),
            format!("model:     {}", self.cfg.model),
            format!("chat:      {chat}"),
            format!("chat file: {}", self.state.active_chat_file().display()),
            format!("per-cwd:   {}", self.state.per_cwd),
            format!("cwd:       {}", self.state.cwd.display()),
            format!("state:     {}", self.state.state_dir.display()),
            format!("auto-review: {}", self.cfg.auto_review),
            format!("trust:     {}", self.cfg.trust),
            format!("force:     {}", self.cfg.force),
            format!("no-resume: {}", self.cfg.no_resume),
            "personas:   Abbey · Aviva · Abi (abi-ai)".into(),
            roles_doctor_line(self.cfg.backend),
            memory::backend_status(&self.state.state_dir, &ac.memory_backend),
            memory::feature_status(),
            config::wdbx_cli_status(&ac),
            "os-control: abbey os dry-run|execute --confirm (cross-platform allowlist)".into(),
            "subagents:  abbey subagents run --lanes max,reviewer [--peers gemini]".into(),
            "parallel:   alias of subagents with Max+Gemma+Aviva defaults".into(),
            "learn:      abbey learn correction|preference|routes|digest|review|stats".into(),
        ]);
        self.doctor_lines = lines;
        self.history = self.state.history(40);
        // Routes pane rides the doctor refresh: it re-runs after every agent
        // run, so the audit tail is always current when the Home tab redraws.
        self.route_lines = crate::route_log::recent_routes(&self.state.state_dir, 8)
            .map(|v| {
                v.iter()
                    .rev()
                    .map(crate::route_log::compact_route_line)
                    .collect()
            })
            .unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_names_every_backend() {
        assert!(backend_doctor_line(AgentBackend::Cursor).contains("cursor-agent"));
        assert!(backend_doctor_line(AgentBackend::Grok).contains("grok"));
        assert!(backend_doctor_line(AgentBackend::Fm).contains("on-device"));
        let abi = backend_doctor_line(AgentBackend::Abi);
        assert!(abi.contains("abi complete") && abi.contains("ABBEY_BACKEND=abi"));
    }

    #[test]
    fn roles_line_claims_bindings_only_under_cursor_model_ids() {
        for b in [AgentBackend::Cursor, AgentBackend::Grok] {
            assert!(roles_doctor_line(b).contains("bindings"));
        }
        for b in [AgentBackend::Fm, AgentBackend::Abi] {
            let line = roles_doctor_line(b);
            assert!(
                line.contains("prompt-only") && !line.contains("bindings"),
                "under {} the TUI must not claim per-role model bindings: {line}",
                b.label()
            );
        }
    }
}
