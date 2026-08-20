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

/// How much transcript tail rides into an abi turn as context.
pub const ABI_CONTEXT_TAIL_BYTES: usize = 8 * 1024;

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

pub(super) fn map_exec_err(err: std::io::Error, agent_path: &Path) -> anyhow::Error {
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
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn flag_shaped_prompts_are_detected() {
        // Only the first non-empty word matters — that is the one the backend
        // sees in option position.
        assert!(looks_like_flags(&["--force".to_string()]));
        assert!(looks_like_flags(&["  ".to_string(), "-p".to_string()]));
        assert!(!looks_like_flags(&["explain --force".to_string()]));
        assert!(!looks_like_flags(&[]));
    }

    #[test]
    fn abbey_generated_prompts_are_not_flagged() {
        // please-fix and the hybrid loop wrap untrusted text mid-string, so the
        // argv element begins with prose even when the text itself has flags.
        assert!(!looks_like_flags(&[
            "Please fix this failure:\n\n--force --yolo".to_string()
        ]));
        assert!(!looks_like_flags(&[
            "Stage 2 of 2 (implement).\n--- interpretation ---\n--force".to_string()
        ]));
    }

    /// A config with every cursor-agent knob turned on — the `fm` builder must
    /// still emit only flags `fm respond` actually accepts.
    fn maximal_cursor_config() -> AgentConfig {
        AgentConfig {
            agent_path: PathBuf::from("/usr/bin/fm"),
            model: "claude-fable-5-thinking-high".into(),
            auto_review: true,
            trust: true,
            force: true,
            no_resume: true,
            mode: Some("plan".into()),
            print: true,
            output_format: Some("json".into()),
            worktree: Some(Worktree::Named("wt".into())),
            workspace: Some(PathBuf::from("/ws")),
            add_dirs: vec![PathBuf::from("/extra")],
            sandbox: Some("enabled".into()),
            extra_args: vec!["--debug".into(), "--max-turns".into(), "7".into()],
            backend: AgentBackend::Fm,
            transcript_dir: None,
            media_note: None,
            media_prefers_gemma: false,
            force_capture: false,
            cot_path: None,
        }
    }

    #[test]
    fn fm_argv_never_leaks_cursor_flags() {
        let argv = maximal_cursor_config().build_args(None, &["hello".into()]);
        // Every flag `fm respond` knows about.
        let allowed = [
            "respond",
            "--model",
            "system",
            "pcc",
            "--instructions",
            "--no-stream",
            "--resume",
            "--save-transcript",
            "hello",
        ];
        for a in &argv {
            if a.starts_with("--") {
                assert!(
                    allowed.contains(&a.as_str()),
                    "leaked non-fm flag {a:?} into argv {argv:?}"
                );
            }
        }
        for banned in [
            "--auto-review",
            "--trust",
            "--force",
            "--mode",
            "--worktree",
            "--workspace",
            "--add-dir",
            "--sandbox",
            "--output-format",
            "--print",
            "--debug",
            "--max-turns",
        ] {
            assert!(!argv.contains(&banned.to_string()), "{banned} leaked");
        }
    }

    #[test]
    fn fm_collapses_any_cursor_model_id_to_system() {
        assert_eq!(fm_model("claude-fable-5-thinking-high"), "system");
        assert_eq!(fm_model("composer-2.5"), "system");
        assert_eq!(fm_model("auto"), "system");
        assert_eq!(fm_model("pcc"), "pcc");
        assert_eq!(fm_model("private-cloud-compute"), "pcc");
        assert_eq!(fm_model("private_cloud_compute"), "pcc");
        // Substrings must not hijack unrelated aliases.
        assert_eq!(fm_model("Private Cloud Compute"), "system");
        assert_eq!(fm_model("my-cloud-model"), "system");
    }

    #[test]
    fn fm_mode_becomes_instructions() {
        let mut cfg = maximal_cursor_config();
        cfg.mode = Some("ask".into());
        let argv = cfg.build_args(None, &["q".into()]);
        let i = argv.iter().position(|a| a == "--instructions").unwrap();
        assert!(argv[i + 1].contains("Do not modify files"));
    }

    #[test]
    fn fm_resume_maps_a_chat_id_onto_a_transcript_file() {
        let dir = std::env::temp_dir().join(format!("abbey-fm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = maximal_cursor_config();
        cfg.transcript_dir = Some(dir.clone());

        // No transcript yet: save but do not resume from a file that isn't there.
        let argv = cfg.build_args(Some("chat-1"), &["hi".into()]);
        assert!(!argv.contains(&"--resume".to_string()));
        let save = argv.iter().position(|a| a == "--save-transcript").unwrap();
        assert!(argv[save + 1].ends_with("chat-1.transcript"));

        // Once it exists, resume from it.
        std::fs::write(dir.join("chat-1.transcript"), "{}").unwrap();
        let argv = cfg.build_args(Some("chat-1"), &["hi".into()]);
        let r = argv.iter().position(|a| a == "--resume").unwrap();
        assert!(argv[r + 1].ends_with("chat-1.transcript"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn abi_argv_never_leaks_cursor_or_fm_flags() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Abi;
        // resume_id must be ignored: `abi complete` has no resume surface.
        let argv = cfg.build_args(Some("chat-1"), &["hello".into()]);
        let dashdash = argv.iter().position(|a| a == "--").expect("-- separator");
        for a in &argv[..dashdash] {
            assert!(
                ["complete", "--live", "--model"].contains(&a.as_str()) || !a.starts_with("--"),
                "leaked non-abi flag {a:?} into argv {argv:?}"
            );
        }
        for banned in [
            "--auto-review",
            "--trust",
            "--force",
            "--mode",
            "--worktree",
            "--workspace",
            "--add-dir",
            "--sandbox",
            "--output-format",
            "--print",
            "--debug",
            "--max-turns",
            "--instructions",
            "--no-stream",
            "--save-transcript",
            "--resume",
        ] {
            assert!(!argv.contains(&banned.to_string()), "{banned} leaked");
        }
        assert!(!argv.contains(&"chat-1".to_string()), "resume id leaked");
    }

    #[test]
    fn abi_prompt_always_follows_a_double_dash() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Abi;
        cfg.mode = None;
        cfg.model = "auto".into();
        // A flag-shaped prompt must land after `--`, i.e. as text.
        let argv = cfg.build_args(None, &["--force".into()]);
        let dashdash = argv.iter().position(|a| a == "--").expect("-- separator");
        let prompt = argv.iter().position(|a| a == "--force").expect("prompt");
        assert!(prompt > dashdash, "prompt reached abi in option position");
    }

    #[test]
    fn abi_transport_is_live_only_for_explicit_aliases() {
        assert_eq!(abi_transport("auto"), AbiTransport::Local);
        assert_eq!(abi_transport("fable"), AbiTransport::Local);
        assert_eq!(abi_transport("composer-2.5"), AbiTransport::Local);
        // Cursor leftovers / Max bindings must not silently select --live.
        assert_eq!(
            abi_transport("claude-fable-5-thinking-high"),
            AbiTransport::Local
        );
        assert_eq!(abi_normalize_model("claude-fable-5-thinking-high"), "local");
        assert_eq!(abi_normalize_model("fable"), "local");
        // Substrings must not hijack unrelated ids into a network call.
        assert_eq!(abi_transport("my-live-model"), AbiTransport::Local);
        assert_eq!(abi_transport("anthropic-ish"), AbiTransport::Local);
        assert_eq!(abi_transport("live"), AbiTransport::Live(None));
        assert_eq!(abi_transport("anthropic"), AbiTransport::Live(None));
        assert_eq!(
            abi_transport("claude-fable-5"),
            AbiTransport::Live(Some("claude-fable-5".into()))
        );
    }

    #[test]
    fn abi_local_argv_uses_normalized_model_not_cursor_binding() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Abi;
        cfg.mode = None;
        cfg.model = "claude-fable-5-thinking-high".into();
        let argv = cfg.build_args(None, &["hi".into()]);
        assert!(
            !argv.contains(&"--live".to_string()),
            "silent live: {argv:?}"
        );
        let model_i = argv.iter().position(|a| a == "--model").unwrap();
        assert_eq!(argv[model_i + 1], "local");
    }

    #[test]
    fn abi_resume_carries_bounded_transcript_context() {
        let dir = std::env::temp_dir().join(format!("abbey-abi-ctx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Abi;
        cfg.mode = None;
        cfg.model = "auto".into();
        cfg.transcript_dir = Some(dir.clone());

        // No transcript yet → context-free turn, prompt still after `--`.
        let argv = cfg.build_args(Some("chat-9"), &["next".into()]);
        assert!(!argv.iter().any(|a| a.contains("Previous conversation")));

        // With a transcript, the tail rides in as a context element before
        // the prompt — and stays bounded for oversized histories.
        std::fs::write(
            dir.join("chat-9.transcript"),
            format!(
                "### user\nremember the word xyzzy\n### abbey\nnoted\n{}",
                "pad ".repeat(4000)
            ),
        )
        .unwrap();
        let argv = cfg.build_args(Some("chat-9"), &["next".into()]);
        let ctx = argv
            .iter()
            .find(|a| a.contains("Previous conversation"))
            .expect("context element");
        assert!(
            ctx.len() <= ABI_CONTEXT_TAIL_BYTES + 200,
            "context unbounded: {}",
            ctx.len()
        );
        let dashdash = argv.iter().position(|a| a == "--").unwrap();
        let ctx_pos = argv
            .iter()
            .position(|a| a.contains("Previous conversation"))
            .unwrap();
        assert!(
            ctx_pos > dashdash,
            "context must be input text, not options"
        );
        assert_eq!(argv.last().unwrap(), "next");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn utf8_tail_respects_char_boundaries() {
        let s = "é".repeat(100); // 2 bytes each
        let tail = utf8_tail(&s, 5);
        assert!(tail.len() <= 5);
        assert!(tail.chars().all(|c| c == 'é'));
        assert_eq!(utf8_tail("short", 100), "short");
    }

    #[test]
    fn abi_mode_rides_in_the_input_text() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Abi;
        cfg.mode = Some("ask".into());
        let argv = cfg.build_args(None, &["q".into()]);
        let dashdash = argv.iter().position(|a| a == "--").unwrap();
        // No --instructions flag exists on abi; the note is input text.
        assert!(argv[dashdash + 1].contains("Do not modify files"));
        assert_eq!(argv.last().unwrap(), "q");
    }

    #[test]
    fn claude_argv_never_leaks_cursor_or_fm_flags() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Claude;
        let argv = cfg.build_args(None, &["hello".into()]);
        let allowed = [
            "--model",
            "--print",
            "--output-format",
            "--permission-mode",
            "--append-system-prompt",
            "--add-dir",
            "--resume",
            "--session-id",
        ];
        for a in &argv {
            if a.starts_with("--") {
                assert!(
                    allowed.contains(&a.as_str()),
                    "leaked non-claude flag {a:?} into argv {argv:?}"
                );
            }
        }
        for banned in [
            "--auto-review",
            "--trust",
            "--force",
            "--mode",
            "--worktree",
            "--workspace",
            "--sandbox",
            "--debug",
            "--max-turns",
            "--instructions",
            "--no-stream",
            "--save-transcript",
            "--live",
        ] {
            assert!(!argv.contains(&banned.to_string()), "{banned} leaked");
        }
        // Cursor thinking binding collapses to a real catalog id.
        let m = argv.iter().position(|a| a == "--model").unwrap();
        assert_eq!(argv[m + 1], "claude-fable-5");
        // --force (always-approve) becomes claude's bypassPermissions and wins
        // over plan mode — exactly one permission mode is emitted.
        let pm = argv.iter().position(|a| a == "--permission-mode").unwrap();
        assert_eq!(argv[pm + 1], "bypassPermissions");
        assert_eq!(argv.iter().filter(|a| *a == "--permission-mode").count(), 1);
        assert_eq!(argv.last().unwrap(), "hello");
    }

    #[test]
    fn claude_model_vocabulary_clamp() {
        // Abbey's flagship default and role aliases.
        assert_eq!(claude_model("opus").as_deref(), Some("opus"));
        assert_eq!(claude_model("max").as_deref(), Some("opus"));
        assert_eq!(claude_model("gemma").as_deref(), Some("sonnet"));
        assert_eq!(claude_model("composer-2.5").as_deref(), Some("sonnet"));
        // auto must stay claude's own plan default — plans that reject named
        // models only work when --model is omitted entirely.
        assert_eq!(claude_model("auto"), None);
        assert_eq!(claude_model(""), None);
        // Cursor bindings collapse to catalog ids; bare ids pass through.
        assert_eq!(
            claude_model("claude-opus-5-thinking-high").as_deref(),
            Some("claude-opus-5")
        );
        assert_eq!(
            claude_model("claude-opus-5").as_deref(),
            Some("claude-opus-5")
        );
        // Foreign executors' ids clamp to the flagship default.
        assert_eq!(claude_model("gpt-5.6-sol-high").as_deref(), Some("opus"));
        assert_eq!(
            claude_model("cursor-grok-4.5-high").as_deref(),
            Some("opus")
        );
        assert_eq!(claude_model("kimi-k3-high").as_deref(), Some("opus"));
    }

    #[test]
    fn claude_session_mints_then_resumes() {
        let dir = std::env::temp_dir().join(format!("abbey-claude-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Claude;
        cfg.force = false;
        cfg.mode = None;
        cfg.transcript_dir = Some(dir.clone());

        // No marker yet: the id is minted as a fresh claude session.
        let argv = cfg.build_args(Some("chat-7"), &["hi".into()]);
        let s = argv.iter().position(|a| a == "--session-id").unwrap();
        assert_eq!(argv[s + 1], "chat-7");
        assert!(!argv.contains(&"--resume".to_string()));

        // After a successful run recorded the marker, later turns resume.
        cfg.touch_claude_session_marker("chat-7");
        let argv = cfg.build_args(Some("chat-7"), &["hi".into()]);
        let r = argv.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(argv[r + 1], "chat-7");
        assert!(!argv.contains(&"--session-id".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn claude_plan_mode_maps_to_permission_mode() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Claude;
        cfg.force = false;
        cfg.mode = Some("plan".into());
        let argv = cfg.build_args(None, &["q".into()]);
        let pm = argv.iter().position(|a| a == "--permission-mode").unwrap();
        assert_eq!(argv[pm + 1], "plan");
        let sp = argv
            .iter()
            .position(|a| a == "--append-system-prompt")
            .unwrap();
        assert!(argv[sp + 1].contains("plan only"));
    }

    #[test]
    fn cursor_backend_argv_is_unchanged_by_the_fm_split() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Cursor;
        let argv = cfg.build_args(Some("abc"), &["hi".into()]);
        for expected in [
            "--auto-review",
            "--trust",
            "--force",
            "--sandbox",
            "--resume",
        ] {
            assert!(argv.contains(&expected.to_string()), "{expected} missing");
        }
    }

    #[test]
    fn truncate_utf8_bytes_respects_char_boundaries_and_cap() {
        let cap = max_prompt_argv_bytes();
        let s = "a".repeat(cap + 50_000);
        let out = truncate_utf8_bytes(&s, cap);
        assert!(out.len() <= cap);
        assert!(out.contains("truncated for OS argv limit"));
    }

    #[test]
    fn clamp_prompt_args_caps_each_trailing_string() {
        let cap = max_prompt_argv_bytes();
        let huge = "x".repeat(cap + 10_000);
        let argv = clamp_prompt_args(&[huge, "ok".into()]);
        assert!(argv[0].len() <= cap);
        assert_eq!(argv[1], "ok");
    }

    #[test]
    fn build_args_clamps_a_please_fix_sized_prompt() {
        let mut cfg = maximal_cursor_config();
        cfg.backend = AgentBackend::Cursor;
        let cap = max_prompt_argv_bytes();
        let huge = "y".repeat(cap + 80_000);
        let argv = cfg.build_args(None, &[huge]);
        let last = argv.last().expect("prompt");
        assert!(
            last.len() <= cap,
            "prompt argv still too long: {}",
            last.len()
        );
    }
}
