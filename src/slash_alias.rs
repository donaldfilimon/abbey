//! Cross-CLI slash aliases (Claude Code / Codex / Grok) onto Abbey catalog names.
//!
//! These are name mappings only: they do not reimplement vendor runtimes.
//! Unknown vendor verbs are not invented here.

/// One alias: the token the other CLI uses, the Abbey catalog name it maps to,
/// and which surface taught Abbey the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashAlias {
    pub alias: &'static str,
    pub target: &'static str,
    pub origin: &'static str,
}

/// Tokens that are not themselves catalog names. Catalog names stay canonical
/// even when another CLI uses the same spelling.
pub const SLASH_ALIASES: &[SlashAlias] = &[
    // Claude Code
    SlashAlias {
        alias: "plugin",
        target: "plugins",
        origin: "claude",
    },
    SlashAlias {
        alias: "reset",
        target: "clear",
        origin: "claude",
    },
    SlashAlias {
        alias: "context",
        target: "memory",
        origin: "claude",
    },
    SlashAlias {
        alias: "extra-usage",
        target: "cost",
        origin: "claude",
    },
    SlashAlias {
        alias: "hooks",
        target: "config",
        origin: "claude",
    },
    SlashAlias {
        alias: "login",
        target: "status",
        origin: "claude",
    },
    SlashAlias {
        alias: "logout",
        target: "status",
        origin: "claude",
    },
    SlashAlias {
        alias: "pull-request",
        target: "pr",
        origin: "claude",
    },
    SlashAlias {
        alias: "security",
        target: "security-review",
        origin: "claude",
    },
    // Codex
    SlashAlias {
        alias: "exec",
        target: "ask",
        origin: "codex",
    },
    SlashAlias {
        alias: "sandbox",
        target: "permissions",
        origin: "codex",
    },
    SlashAlias {
        alias: "undo",
        target: "rewind",
        origin: "codex",
    },
    SlashAlias {
        alias: "apply",
        target: "commit",
        origin: "codex",
    },
    // Grok Build
    SlashAlias {
        alias: "resume",
        target: "continue",
        origin: "grok",
    },
    SlashAlias {
        alias: "chat",
        target: "continue",
        origin: "grok",
    },
    SlashAlias {
        alias: "single",
        target: "ask",
        origin: "grok",
    },
    // Shared shorthand
    SlashAlias {
        alias: "h",
        target: "help",
        origin: "abbey",
    },
    SlashAlias {
        alias: "?",
        target: "help",
        origin: "abbey",
    },
    SlashAlias {
        alias: "fix",
        target: "please-fix",
        origin: "abbey",
    },
    SlashAlias {
        alias: "sec",
        target: "security-review",
        origin: "abbey",
    },
];

/// Resolve a typed token to a catalog name. `None` if it is neither a catalog
/// entry nor a known alias.
pub fn resolve_name(name: &str) -> Option<&'static str> {
    let lower = name.trim().trim_start_matches('/').to_ascii_lowercase();
    if lower.is_empty() {
        return Some("help");
    }
    if let Some(cmd) = crate::slash::SLASH_CATALOG
        .iter()
        .find(|c| c.name == lower.as_str())
    {
        return Some(cmd.name);
    }
    SLASH_ALIASES
        .iter()
        .find(|a| a.alias == lower.as_str())
        .map(|a| a.target)
}

/// Origin label for a catalog name or alias token (`claude` / `codex` / `grok` / `abbey`).
pub fn origin_for(token: &str) -> &'static str {
    let lower = token.trim().trim_start_matches('/').to_ascii_lowercase();
    if let Some(a) = SLASH_ALIASES.iter().find(|a| a.alias == lower.as_str()) {
        return a.origin;
    }
    if SLASH_ALIASES
        .iter()
        .any(|a| a.target == lower.as_str() && a.origin != "abbey")
    {
        return "shared";
    }
    "abbey"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_point_at_real_catalog_names() {
        for alias in SLASH_ALIASES {
            assert!(
                crate::slash::SLASH_CATALOG
                    .iter()
                    .any(|c| c.name == alias.target),
                "/{} maps to missing /{}",
                alias.alias,
                alias.target
            );
            assert_ne!(
                alias.alias, alias.target,
                "/{} aliases itself — put it in the catalog instead",
                alias.alias
            );
        }
    }

    #[test]
    fn claude_codex_grok_tokens_resolve() {
        assert_eq!(resolve_name("plugin"), Some("plugins"));
        assert_eq!(resolve_name("exec"), Some("ask"));
        assert_eq!(resolve_name("resume"), Some("continue"));
        assert_eq!(resolve_name("/Review"), Some("review"));
        assert_eq!(resolve_name("zzz"), None);
    }
}
