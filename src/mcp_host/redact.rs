//! Outbound secret redaction for the stdio MCP server.
//!
//! No registered tool reads the environment, so this is defence in depth rather
//! than the primary control: it runs over the *serialized* response, after the
//! handler and after JSON encoding, so a future tool cannot leak a credential
//! by smuggling it through a field nobody thought to audit.
//!
//! The pass is value-based, not name-based. It collects the values of
//! environment variables whose *names* look credential-bearing and replaces any
//! literal occurrence of those values in the outgoing frame.

use std::collections::BTreeSet;

/// What a redacted value is replaced with.
pub const REDACTION_PLACEHOLDER: &str = "[REDACTED]";

/// Shortest value that will ever be substring-replaced.
///
/// Without a floor, a variable that happens to hold `1`, `true`, or `en_US`
/// would rewrite unrelated text and corrupt every response. A credential
/// shorter than this is not protected by this pass — that is a deliberate,
/// documented boundary, not an oversight.
pub const MIN_REDACTABLE_SECRET_BYTES: usize = 8;

/// Case-insensitive fragments that mark an environment variable name as
/// credential-bearing.
pub const SENSITIVE_NAME_FRAGMENTS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "APIKEY",
    "API_KEY",
    "_KEY",
    "KEY_",
    "CREDENTIAL",
    "AUTH",
    "BEARER",
    "SESSION",
    "COOKIE",
    "PRIVATE",
    "SIGNATURE",
];

/// True when a variable name looks like it carries a credential.
#[must_use]
pub fn name_is_sensitive(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    SENSITIVE_NAME_FRAGMENTS
        .iter()
        .any(|fragment| upper.contains(fragment))
}

/// Values of every credential-bearing environment variable of this process,
/// longest first so a secret that contains another is masked whole.
#[must_use]
pub fn sensitive_env_values() -> Vec<String> {
    let mut values: BTreeSet<String> = BTreeSet::new();
    for (name, value) in std::env::vars_os() {
        let (Some(name), Some(value)) = (name.to_str(), value.to_str()) else {
            continue;
        };
        if !name_is_sensitive(name) {
            continue;
        }
        let value = value.trim();
        if value.len() >= MIN_REDACTABLE_SECRET_BYTES {
            values.insert(value.to_owned());
        }
    }
    let mut values: Vec<String> = values.into_iter().collect();
    values.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    values
}

/// Replace every occurrence of `secrets` in `text` with the placeholder.
#[must_use]
pub fn redact_with(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        if secret.len() < MIN_REDACTABLE_SECRET_BYTES {
            continue;
        }
        if out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), REDACTION_PLACEHOLDER);
        }
    }
    out
}
