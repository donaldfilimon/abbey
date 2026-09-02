//! Backend argv construction and prompt/argv safety.
//!
//! Everything that shapes what the backend binary receives on its command line
//! lives here: the per-backend argv grammars (`build_args` / `build_args_fm` /
//! `build_args_abi`), OS argv-limit clamping, flag-shaped-prompt detection, and
//! the E2BIG exec error mapping. Second `impl AgentConfig` block — state and
//! process execution stay in `agent/mod.rs`.

use super::{AgentBackend, AgentConfig, Worktree, max_prompt_argv_bytes};
use std::path::Path;

/// Map an Abbey model/alias onto `fm`'s two-model vocabulary (`system` | `pcc`).
///
/// Exact aliases only — substring matching (`cloud`, `private`) would mis-route
/// ordinary cursor-agent ids. Under `fm` the Max/Gemma distinction is carried by
/// the prompt alone, not by the model.
pub fn fm_model(requested: &str) -> &'static str {
    match requested.trim().to_ascii_lowercase().as_str() {
        "pcc" | "private-cloud-compute" | "private_cloud_compute" => "pcc",
        _ => "system",
    }
}

/// Which `abi complete` transport an Abbey model/alias selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiTransport {
    /// Deterministic persona-template completion in-process — no network.
    Local,
    /// Anthropic live transport (`--live`), with an explicit model id or
    /// abi's own default when `None`.
    Live(Option<String>),
}

/// Cursor role/thinking bindings look like Anthropic ids (`claude-*-thinking-*`,
/// `claude-*-high`, …). Under `ABBEY_BACKEND=abi` those must stay Local —
/// otherwise every Max-role leftover in state would silently select `--live`.
fn cursor_style_binding(lower: &str) -> bool {
    lower.contains("thinking")
        || lower.ends_with("-fast")
        || lower.ends_with("-high")
        || lower.ends_with("-xhigh")
        || lower.ends_with("-medium")
        || lower.ends_with("-low")
        || lower.ends_with("-max")
}

/// Normalize an Abbey model string for the abi backend.
///
/// Does **not** run cursor `resolve_model` expansion: `fable` must stay a local
/// tag, not become `claude-fable-5-thinking-high` (which would look like live).
/// State leftovers that are already cursor-expanded collapse to `local`.
pub fn abi_normalize_model(requested: &str) -> String {
    let t = requested.trim();
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "live" | "anthropic" => "live".into(),
        "local" | "auto" | "smart" | "default" | "abi" | "" => "local".into(),
        s if s.starts_with("claude-") => {
            if cursor_style_binding(s) {
                "local".into()
            } else {
                t.to_string()
            }
        }
        "fable" | "fable5" | "fable-5" | "max" | "composer" | "composer2" | "composer-2.5"
        | "gemma" | "gemma4" | "qwen" | "kimi" | "opus" | "opus5" | "grok" | "codex" | "sol"
        | "terra" => "local".into(),
        other
            if other.starts_with("cursor-")
                || other.starts_with("gpt-")
                || other.starts_with("composer-")
                || other.starts_with("kimi-")
                || other.contains("thinking") =>
        {
            "local".into()
        }
        other => other.to_string(),
    }
}

/// Map an Abbey model/alias onto `abi complete`'s transports.
///
/// Live is opt-in and explicit — only a bare `claude-*` catalog id (not a
/// Cursor thinking/speed binding) or the exact aliases `live` / `anthropic`
/// select it. Everything else stays local, so no role alias or state leftover
/// can silently turn a deterministic run into a network call.
pub fn abi_transport(requested: &str) -> AbiTransport {
    let normalized = abi_normalize_model(requested);
    let t = normalized.to_ascii_lowercase();
    if t == "live" || t == "anthropic" {
        return AbiTransport::Live(None);
    }
    if t.starts_with("claude-") {
        return AbiTransport::Live(Some(normalized));
    }
    AbiTransport::Local
}

/// Strip cursor-agent's thinking/effort decorations off a `claude-*` id so it
/// becomes a catalog id the Claude Code CLI accepts:
/// `claude-opus-5-thinking-high` → `claude-opus-5`.
fn claude_strip_cursor_binding(lower: &str) -> String {
    let mut s = lower;
    loop {
        let mut stripped = false;
        for suf in [
            "-fast",
            "-high",
            "-xhigh",
            "-medium",
            "-low",
            "-max",
            "-thinking",
        ] {
            if let Some(rest) = s.strip_suffix(suf) {
                s = rest;
                stripped = true;
            }
        }
        if !stripped {
            break;
        }
    }
    s.to_string()
}

/// Map an Abbey model/alias onto the Claude Code CLI's vocabulary.
///
/// `None` means omit `--model` entirely and let the user's Claude plan pick
/// its own default — that is what keeps `auto` working on plans that reject
/// named models. Foreign executors' ids (gpt/sol/kimi/grok/composer bindings)
/// cannot be served by Claude Code, so they clamp to Abbey's flagship default
/// (`opus`) rather than reaching claude as an id it will reject.
pub fn claude_model(requested: &str) -> Option<String> {
    let t = requested.trim();
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "" | "auto" | "smart" | "default" => None,
        "opus" | "opus5" | "opus-5" | "max" => Some("opus".into()),
        "sonnet" | "sonnet5" | "sonnet-5" => Some("sonnet".into()),
        "haiku" => Some("haiku".into()),
        "fable" | "fable5" | "fable-5" => Some("fable".into()),
        // Gemma role bindings are conversational/visual — Sonnet's lane.
        "gemma" | "gemma4" | "gemma-4" | "composer" | "composer2" | "composer-2.5" => {
            Some("sonnet".into())
        }
        s if s.starts_with("claude-") => Some(claude_strip_cursor_binding(s)),
        _ => Some("opus".into()),
    }
}

/// How much transcript tail rides into an abi/ollama turn as context.
pub const ABI_CONTEXT_TAIL_BYTES: usize = 8 * 1024;

/// Map an Abbey model/alias onto a local Ollama tag.
///
/// Cursor leftovers (`claude-*thinking*`, `composer-2.5`, `auto`) collapse to
/// the default local Gemma 4 26.2B MLX tag. `gemma:27b-mlx` is accepted as an
/// alias for that same installed weight — Ollama has no `gemma:27b-mlx` tag
/// on this host. Any other non-empty string is passed through so a user can
/// select `gemma4:12b-mlx` without Abbey rewriting it.
pub fn ollama_normalize_model(requested: &str) -> String {
    let t = requested.trim();
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "" | "auto" | "smart" | "default" | "local" | "ollama" | "gemma" | "gemma4" | "gemma-4"
        | "gemma:27b-mlx" | "gemma4:26b-mlx" | "gemma4:26b" | "max" | "composer" | "composer2"
        | "composer-2.5" | "opus" | "opus5" | "fable" | "fable5" | "qwen" | "kimi" | "grok"
        | "codex" | "sol" | "terra" => crate::models::OLLAMA_DEFAULT_MODEL.into(),
        other
            if other.starts_with("cursor-")
                || other.starts_with("claude-")
                || other.starts_with("gpt-")
                || other.starts_with("composer-")
                || other.starts_with("kimi-")
                || other.contains("thinking") =>
        {
            crate::models::OLLAMA_DEFAULT_MODEL.into()
        }
        _ => t.to_string(),
    }
}

/// The trailing ≤ `max_bytes` of `s`, cut on a UTF-8 boundary.
pub fn utf8_tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

/// Truncate `s` on a UTF-8 boundary so its byte length is ≤ `max_bytes`.
pub fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let marker = format!(
        "\n\n… [truncated for OS argv limit; kept {max_bytes} of {} bytes]",
        s.len()
    );
    let budget = max_bytes.saturating_sub(marker.len());
    let mut end = budget.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push_str(&marker);
    if out.len() > max_bytes {
        let mut e = max_bytes;
        while e > 0 && !out.is_char_boundary(e) {
            e -= 1;
        }
        out.truncate(e);
    }
    out
}

/// Clamp every trailing prompt string so the child argv stays under the OS limit.
/// Whether a prompt would reach the backend in option position.
///
/// Prompt words are appended to the backend's argv with no `--` separator, so a
/// leading-dash prompt is parsed as flags rather than text: `abbey print --
/// "--force …"` does not *ask about* `--force`, it hands cursor-agent its real
/// `--force` (always-approve) flag.
///
/// Abbey's own generated prompts are unaffected — `please-fix` and the hybrid
/// loop embed captured errors and model output mid-string, so that argv element
/// always begins with prose. Only a user's own leading-dash prompt trips this.
pub fn looks_like_flags(prompt_and_rest: &[String]) -> bool {
    prompt_and_rest
        .iter()
        .find(|p| !p.trim().is_empty())
        .is_some_and(|first| first.trim_start().starts_with('-'))
}

/// Warn once, at the spawn point, rather than rewriting the argv: whether the
/// backend honours a `--` separator is unverified, and silently reshaping every
/// invocation to find out is not worth the risk to the primary path.
pub(super) fn warn_if_prompt_looks_like_flags(prompt_and_rest: &[String]) {
    if looks_like_flags(prompt_and_rest) {
        eprintln!(
            "abbey: prompt begins with `-` — the backend will read it as options, \
             not text (e.g. `--force` enables always-approve)"
        );
    }
}

pub fn clamp_prompt_args(prompt_and_rest: &[String]) -> Vec<String> {
    let cap = max_prompt_argv_bytes();
    prompt_and_rest
        .iter()
        .map(|s| truncate_utf8_bytes(s, cap))
        .collect()
}

pub(crate) fn map_exec_err(err: std::io::Error, agent_path: &Path) -> anyhow::Error {
    let cap = max_prompt_argv_bytes();
    // E2BIG — Darwin/Linux typically use errno 7; Windows CreateProcess uses other codes.
    let too_long = err.raw_os_error() == Some(7)
        || err.kind() == std::io::ErrorKind::InvalidInput
        || err.to_string().to_ascii_lowercase().contains("too long");
    if too_long {
        return anyhow::anyhow!(
            "exec {}: argument list / command line too long ({err}).\n\
             Abbey clamps prompts to {cap} bytes on this host; if this still \
             fires, shrink CURSOR_AGENT_COMPLETED_PATH / please-fix input or unset \
             oversized environment variables.",
            agent_path.display()
        );
    }
    anyhow::Error::new(err).context(format!("exec {}", agent_path.display()))
}

impl AgentConfig {
    /// `fm respond` argv. Deliberately built from scratch rather than filtered
    /// from the cursor grammar: `fm` shares none of those flags, and a single
    /// leaked `--force`/`--sandbox` would break the backend the first time a
    /// user passed one.
    fn build_args_fm(&self, resume_id: Option<&str>, prompt_and_rest: &[String]) -> Vec<String> {
        let prompts = clamp_prompt_args(prompt_and_rest);
        let mut args = vec![
            "respond".to_string(),
            "--model".into(),
            fm_model(&self.model).into(),
        ];
        if let Some(mode) = &self.mode {
            // Abbey's ask/plan modes have no `fm` equivalent; express them as
            // instructions so the behaviour survives the backend switch.
            args.push("--instructions".into());
            args.push(match mode.as_str() {
                "ask" => "Answer the question. Do not modify files.".into(),
                "plan" => "Produce a plan only. Do not write the implementation.".into(),
                other => format!("Mode: {other}."),
            });
        }
        if self.print {
            // Streaming interleaves partial output; capture wants one clean body.
            args.push("--no-stream".into());
        }
        if let Some(id) = resume_id.filter(|i| !i.is_empty())
            && let Some(path) = self.transcript_path(id)
        {
            if path.is_file() {
                args.push("--resume".into());
                args.push(path.display().to_string());
            }
            args.push("--save-transcript".into());
            args.push(path.display().to_string());
        }
        args.extend(prompts);
        args
    }

    /// `abi complete` argv. Built from scratch like the `fm` grammar: `abi`
    /// shares no flags with cursor-agent, and a leaked `--force`/`--sandbox`
    /// would be parsed as (or joined into) completion input.
    ///
    /// `abi complete` is a stateless one-shot with no instruction channel and
    /// no `--resume` flag, so the mode note rides in the input text and the
    /// prompt always follows a real `--` separator. `resume_id` is **not**
    /// forwarded to abi — it names the transcript whose bounded tail is
    /// injected as a context prefix (Abbey-side continuity).
    fn build_args_abi(&self, resume_id: Option<&str>, prompt_and_rest: &[String]) -> Vec<String> {
        let prompts = clamp_prompt_args(prompt_and_rest);
        let mut args = vec!["complete".to_string()];
        // Normalize at the argv choke point so every call site (hybrid, TUI,
        // subagents) inherits the no-silent-live guarantee.
        match abi_transport(&self.model) {
            AbiTransport::Local => {
                args.push("--model".into());
                args.push(abi_normalize_model(&self.model));
            }
            AbiTransport::Live(None) => args.push("--live".into()),
            AbiTransport::Live(Some(id)) => {
                args.push("--live".into());
                args.push("--model".into());
                args.push(id);
            }
        }
        args.push("--".into());
        // Abbey-side continuity: `abi complete` has no resume surface, so the
        // previous turns ride in as a bounded context prefix read from the
        // transcript this chat id maps to. Best-effort — a missing or
        // unreadable transcript simply means a context-free turn.
        if let Some(id) = resume_id.filter(|i| !i.is_empty())
            && let Some(path) = self.transcript_path(id)
            && let Ok(prev) = std::fs::read_to_string(&path)
        {
            let tail = utf8_tail(&prev, ABI_CONTEXT_TAIL_BYTES);
            if !tail.trim().is_empty() {
                args.push(format!(
                    "Previous conversation (context, oldest first, may be truncated):\n\
                     {tail}\n--- end of context; answer the next message ---"
                ));
            }
        }
        if let Some(mode) = &self.mode {
            args.push(match mode.as_str() {
                "ask" => "Answer the question. Do not modify files.".into(),
                "plan" => "Produce a plan only. Do not write the implementation.".into(),
                other => format!("Mode: {other}."),
            });
        }
        args.extend(prompts);
        args
    }

    /// `claude` (Claude Code CLI) argv. Built from scratch like the `fm`/`abi`
    /// grammars: claude shares almost none of cursor-agent's flags, and a
    /// leaked `--trust`/`--auto-review` would abort every invocation.
    ///
    /// Continuity is Claude's own session store: the first turn mints Abbey's
    /// chat id as the session (`--session-id`), and once a run has succeeded
    /// (marker file — see `touch_claude_session_marker`) later turns
    /// `--resume` it. `extra_args` are cursor-shaped and never forwarded.
    fn build_args_claude(
        &self,
        resume_id: Option<&str>,
        prompt_and_rest: &[String],
    ) -> Vec<String> {
        let prompts = clamp_prompt_args(prompt_and_rest);
        let mut args: Vec<String> = Vec::new();
        if let Some(m) = claude_model(&self.model) {
            args.push("--model".into());
            args.push(m);
        }
        if self.print {
            args.push("--print".into());
            if let Some(fmt) = &self.output_format {
                args.push("--output-format".into());
                args.push(fmt.clone());
            }
        }
        // One permission mode: Abbey's explicit --force (always-approve) wins;
        // otherwise plan mode maps onto claude's own plan permission mode.
        if self.force {
            args.push("--permission-mode".into());
            args.push("bypassPermissions".into());
        } else if self.mode.as_deref() == Some("plan") {
            args.push("--permission-mode".into());
            args.push("plan".into());
        }
        if let Some(mode) = &self.mode {
            // ask has no claude flag; the note rides as an appended system prompt.
            args.push("--append-system-prompt".into());
            args.push(match mode.as_str() {
                "ask" => "Answer the question. Do not modify files.".into(),
                "plan" => "Produce a plan only. Do not write the implementation.".into(),
                other => format!("Mode: {other}."),
            });
        }
        for d in &self.add_dirs {
            args.push("--add-dir".into());
            args.push(d.display().to_string());
        }
        if let Some(id) = resume_id.filter(|i| !i.is_empty()) {
            let established = self.transcript_path(id).is_some_and(|p| p.is_file());
            if established {
                args.push("--resume".into());
            } else {
                args.push("--session-id".into());
            }
            args.push(id.to_string());
        }
        args.extend(prompts);
        args
    }

    /// `ollama run` argv. Built from scratch like the `abi` grammar: ollama
    /// shares no flags with cursor-agent, and a leaked `--force`/`--sandbox`
    /// would be parsed as (or joined into) the prompt.
    ///
    /// `ollama run` is a stateless one-shot with no `--resume` flag, so the
    /// mode note rides in the input text and the prompt always follows a real
    /// `--` separator. `resume_id` names the transcript whose bounded tail is
    /// injected as a context prefix (Abbey-side continuity).
    fn build_args_ollama(
        &self,
        resume_id: Option<&str>,
        prompt_and_rest: &[String],
    ) -> Vec<String> {
        let prompts = clamp_prompt_args(prompt_and_rest);
        let mut args = vec![
            "run".into(),
            "--nowordwrap".into(),
            ollama_normalize_model(&self.model),
            "--".into(),
        ];
        if let Some(id) = resume_id.filter(|i| !i.is_empty())
            && let Some(path) = self.transcript_path(id)
            && let Ok(prev) = std::fs::read_to_string(&path)
        {
            let tail = utf8_tail(&prev, ABI_CONTEXT_TAIL_BYTES);
            if !tail.trim().is_empty() {
                args.push(format!(
                    "Previous conversation (context, oldest first, may be truncated):\n\
                     {tail}\n--- end of context; answer the next message ---"
                ));
            }
        }
        if let Some(mode) = &self.mode {
            args.push(match mode.as_str() {
                "ask" => "Answer the question. Do not modify files.".into(),
                "plan" => "Produce a plan only. Do not write the implementation.".into(),
                other => format!("Mode: {other}."),
            });
        }
        args.extend(prompts);
        args
    }

    pub fn build_args(&self, resume_id: Option<&str>, prompt_and_rest: &[String]) -> Vec<String> {
        if self.backend == AgentBackend::Fm {
            return self.build_args_fm(resume_id, prompt_and_rest);
        }
        if self.backend == AgentBackend::Abi {
            return self.build_args_abi(resume_id, prompt_and_rest);
        }
        if self.backend == AgentBackend::Claude {
            return self.build_args_claude(resume_id, prompt_and_rest);
        }
        if self.backend == AgentBackend::Ollama {
            return self.build_args_ollama(resume_id, prompt_and_rest);
        }
        let prompts = clamp_prompt_args(prompt_and_rest);
        let mut args = Vec::new();
        args.push("--model".into());
        args.push(self.model.clone());
        if self.auto_review {
            args.push("--auto-review".into());
        }
        if self.trust {
            args.push("--trust".into());
        }
        if self.force {
            args.push("--force".into());
        }
        if let Some(mode) = &self.mode {
            args.push("--mode".into());
            args.push(mode.clone());
        }
        if self.print {
            args.push("--print".into());
            if let Some(fmt) = &self.output_format {
                args.push("--output-format".into());
                args.push(fmt.clone());
            }
        }
        if let Some(wt) = &self.worktree {
            args.push("--worktree".into());
            if let Worktree::Named(name) = wt {
                args.push(name.clone());
            }
        }
        if let Some(ws) = &self.workspace {
            args.push("--workspace".into());
            args.push(ws.display().to_string());
        }
        for d in &self.add_dirs {
            args.push("--add-dir".into());
            args.push(d.display().to_string());
        }
        if let Some(sb) = &self.sandbox {
            args.push("--sandbox".into());
            args.push(sb.clone());
        }
        args.extend(self.extra_args.iter().cloned());
        if let Some(id) = resume_id
            && !id.is_empty()
        {
            args.push("--resume".into());
            args.push(id.to_string());
        }
        args.extend(prompts);
        args
    }
}

#[cfg(test)]
mod tests;
