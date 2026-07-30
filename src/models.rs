//! Friendly model aliases → cursor-agent model ids.

/// Resolve a user-facing alias or pass through a full model id.
pub fn resolve_model(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "auto" | "smart" | "default" => "auto".into(),

        // Fable 5
        "fable"
        | "fable5"
        | "fable-5"
        | "claude-fable"
        | "claude-fable-5"
        | "fable-thinking"
        | "fable-thinking-high"
        | "fable5-thinking"
        | "fable5-thinking-high" => "claude-fable-5-thinking-high".into(),
        "fable-thinking-xhigh" | "fable-xhigh-thinking" => "claude-fable-5-thinking-xhigh".into(),
        "fable-thinking-low" => "claude-fable-5-thinking-low".into(),
        "fable-thinking-medium" => "claude-fable-5-thinking-medium".into(),
        "fable-thinking-max" => "claude-fable-5-thinking-max".into(),
        "fable-low" => "claude-fable-5-low".into(),
        "fable-medium" => "claude-fable-5-medium".into(),
        "fable-high" => "claude-fable-5-high".into(),
        "fable-xhigh" => "claude-fable-5-xhigh".into(),
        "fable-max" => "claude-fable-5-max".into(),

        // Opus 5
        "opus" | "opus5" | "opus-5" | "claude-opus" | "claude-opus-5" | "opus-thinking"
        | "opus-thinking-high" => "claude-opus-5-thinking-high".into(),
        "opus-thinking-fast" | "opus-fast" => "claude-opus-5-thinking-high-fast".into(),
        "opus-thinking-xhigh" | "opus-xhigh-thinking" | "opus-xhigh" => {
            "claude-opus-5-thinking-xhigh".into()
        }
        "opus-thinking-max" | "opus-max-thinking" | "opus-max" => {
            "claude-opus-5-thinking-max".into()
        }
        "opus-low" => "claude-opus-5-low".into(),
        "opus-medium" => "claude-opus-5-medium".into(),
        "opus-high" => "claude-opus-5-high".into(),

        // Opus 4.8
        "opus48" | "opus-4.8" | "opus-4-8" | "claude-opus-4-8" => {
            "claude-opus-4-8-thinking-high".into()
        }
        "opus48-fast" | "opus-4.8-fast" => "claude-opus-4-8-thinking-high-fast".into(),

        // GPT / Codex / Sol
        "gpt" | "gpt5" | "gpt-5.2" | "gpt5.2" => "gpt-5.2".into(),
        "gpt55" | "gpt-5.5" | "gpt5.5" => "gpt-5.5-high".into(),
        "gpt55-fast" => "gpt-5.5-high-fast".into(),
        "sol" | "gpt56" | "gpt-5.6" => "gpt-5.6-sol-high".into(),
        "sol-xhigh" | "gpt56-xhigh" => "gpt-5.6-sol-xhigh".into(),
        "sol-fast" => "gpt-5.6-sol-high-fast".into(),
        "terra" | "gpt-5.6-terra" => "gpt-5.6-terra-medium".into(),
        "terra-high" => "gpt-5.6-terra-high".into(),
        "codex" | "codex53" | "gpt-5.3-codex" => "gpt-5.3-codex".into(),
        "codex-high" => "gpt-5.3-codex-high".into(),
        "codex-xhigh" => "gpt-5.3-codex-xhigh".into(),
        "codex-fast" => "gpt-5.3-codex-fast".into(),
        "codex-low" => "gpt-5.3-codex-low".into(),

        // Grok (Cursor-hosted)
        "grok" | "grok45" | "grok-4.5" | "cursor-grok" => "cursor-grok-4.5-high".into(),
        "grok-fast" => "cursor-grok-4.5-high-fast".into(),
        "grok-low" => "cursor-grok-4.5-low".into(),
        "grok-medium" => "cursor-grok-4.5-medium".into(),

        // Composer / Kimi / role aliases (bindings — not local weights)
        "composer" | "composer2" | "composer-2.5" => "composer-2.5".into(),
        "composer-fast" => "composer-2.5-fast".into(),
        "kimi" | "kimi-k3" | "k3" => "kimi-k3-high".into(),
        "max" | "qwen" | "qwen3" | "qwen-3.5" => "claude-fable-5-thinking-high".into(),
        "gemma" | "gemma4" | "gemma-4" => "composer-2.5".into(),

        other => {
            if let Some(rest) = other
                .strip_prefix("fable-5-")
                .or_else(|| other.strip_prefix("fable5-"))
            {
                format!("claude-fable-5-{rest}")
            } else if let Some(rest) = other
                .strip_prefix("opus-5-")
                .or_else(|| other.strip_prefix("opus5-"))
            {
                format!("claude-opus-5-{rest}")
            } else {
                raw.trim().to_string()
            }
        }
    }
}

/// Picker entries for the TUI model browser.
pub fn alias_table() -> &'static [(&'static str, &'static str)] {
    &[
        ("auto", "auto"),
        ("fable", "claude-fable-5-thinking-high"),
        ("fable-xhigh", "claude-fable-5-thinking-xhigh"),
        ("opus", "claude-opus-5-thinking-high"),
        ("opus-fast", "claude-opus-5-thinking-high-fast"),
        ("opus48", "claude-opus-4-8-thinking-high"),
        ("gpt", "gpt-5.2"),
        ("gpt55", "gpt-5.5-high"),
        ("sol", "gpt-5.6-sol-high"),
        ("terra", "gpt-5.6-terra-medium"),
        ("codex", "gpt-5.3-codex"),
        ("codex-high", "gpt-5.3-codex-high"),
        ("grok", "cursor-grok-4.5-high"),
        ("grok-fast", "cursor-grok-4.5-high-fast"),
        ("max", "claude-fable-5-thinking-high"),
        ("gemma", "composer-2.5"),
        ("composer", "composer-2.5"),
        ("kimi", "kimi-k3-high"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_resolve() {
        assert_eq!(resolve_model("Fable"), "claude-fable-5-thinking-high");
        assert_eq!(resolve_model("opus"), "claude-opus-5-thinking-high");
        assert_eq!(resolve_model("auto"), "auto");
        assert_eq!(resolve_model("max"), "claude-fable-5-thinking-high");
        assert_eq!(resolve_model("gemma"), "composer-2.5");
        assert_eq!(resolve_model("claude-opus-5-high"), "claude-opus-5-high");
    }
}
