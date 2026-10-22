//! Deterministic redaction of absolute paths and secret-shaped values.
//!
//! The rules are fixed, documented, and covered by table tests. The same
//! input always produces the same output; there is no RNG and no ordering
//! dependence.
//!
//! * Secrets are redacted **before** paths. A secret inside a path is
//!   removed by the secret rule, and a path that merely looks like a secret
//!   assignment is removed by the path rule.
//! * A prefixed secret shape (`sk-`, `ghp_`, `AKIA`) or a `Bearer` token needs
//!   at least [`MIN_SECRET_TAIL`] characters of secret material after its
//!   prefix. A shorter match is not a credential, and redacting it would
//!   silently corrupt the corpus.
//! * A `key=` / `token=` / `password=` / `secret=` assignment is redacted
//!   when its value is at least [`MIN_SECRET_TAIL`] characters long. The
//!   assignment name is kept so the corpus still reads sensibly.
//! * A PEM private-key block is always redacted — marker **and** body.
//!   Redacting only the `-----BEGIN …` line would leave the key material in
//!   the corpus.
//! * An absolute path is a run that starts with `/` (but not `//`) or with a
//!   Windows drive prefix, at a word boundary, and runs to the next
//!   whitespace or delimiter. It becomes `[path]`.
//! * URLs are **not** paths here: in `https://host/p` the `/` characters are
//!   not at a path boundary, so the URL keeps its text. This is a stated
//!   limitation, not an oversight.

/// Replacement for a redacted secret value.
pub const REDACTED: &str = "[redacted]";
/// Replacement for an absolute path.
pub const PATH_PLACEHOLDER: &str = "[path]";
/// Minimum number of secret characters a shape must carry to be redacted.
pub const MIN_SECRET_TAIL: usize = 8;

/// The result of redacting one string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    /// The redacted text.
    pub text: String,
    /// Whether anything was actually replaced.
    pub changed: bool,
}

/// Redact one string.
#[must_use]
pub fn redact(input: &str) -> Redaction {
    let secrets = redact_secrets(input);
    let text = redact_paths(&secrets);
    Redaction {
        changed: text != input,
        text,
    }
}

/// Characters that may appear inside a secret token.
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || c == '-'
        || c == '_'
        || c == '.'
        || c == '~'
        || c == '+'
        || c == '/'
}

/// Characters that may appear inside an assignment name.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `c` may sit directly before a secret prefix.
fn is_secret_boundary(c: char) -> bool {
    !(c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Whether `c` may sit directly before an absolute path.
fn is_path_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | '[' | '{' | '=' | ',' | '"' | '\'')
}

/// Whether `c` terminates an absolute path.
fn is_path_terminator(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            ',' | ';' | ')' | ']' | '}' | '(' | '"' | '\'' | '`' | '<' | '>' | '|'
        )
}

/// The character before byte offset `at`, if any.
fn prev_char(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

/// The secret-shaped spans, redacted left to right.
fn redact_secrets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut index = 0usize;
    while index < input.len() {
        if let Some(end) = pem_block_end(input, index) {
            out.push_str(REDACTED);
            index = end;
            continue;
        }
        if let Some(end) = bearer_end(input, index) {
            out.push_str(REDACTED);
            index = end;
            continue;
        }
        if let Some(end) = prefixed_secret_end(input, index) {
            out.push_str(REDACTED);
            index = end;
            continue;
        }
        if let Some(span) = assignment_span(input, index) {
            let name = &input[span.name_start..span.value_start];
            let quoted = span.quote;
            out.truncate(out.len() - (index - span.name_start));
            out.push_str(name);
            if let Some(q) = quoted {
                out.push(q);
            }
            out.push_str(REDACTED);
            if let Some(q) = quoted {
                out.push(q);
            }
            index = span.value_end;
            continue;
        }
        let ch = input[index..].chars().next().unwrap_or('\u{0}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

/// End of a PEM private-key block that starts at `index`, if one does.
fn pem_block_end(input: &str, index: usize) -> Option<usize> {
    const BEGIN: &str = "-----BEGIN ";
    const KEY_MARK: &str = "PRIVATE KEY-----";
    if !input[index..].starts_with(BEGIN) {
        return None;
    }
    // The marker must be close behind the BEGIN line, so ordinary prose that
    // merely mentions the words cannot swallow the rest of the corpus.
    let window_end = (index + 512).min(input.len());
    let window = input.get(index..window_end)?;
    let marker = window.find(KEY_MARK)?;
    let after_marker = index + marker + KEY_MARK.len();
    // Consume the body through the matching END marker when it is present.
    if let Some(end) = input[after_marker..].find("-----END ") {
        let tail = &input[after_marker + end..];
        if let Some(close) = tail.find(KEY_MARK) {
            return Some(after_marker + end + close + KEY_MARK.len());
        }
    }
    Some(input.len())
}

/// End of a `Bearer <token>` span that starts at `index`, if one does.
fn bearer_end(input: &str, index: usize) -> Option<usize> {
    const BEARER: &str = "Bearer";
    let rest = &input[index..];
    let head = rest.get(..BEARER.len())?;
    if !head.eq_ignore_ascii_case(BEARER) {
        return None;
    }
    if let Some(before) = prev_char(input, index)
        && !is_secret_boundary(before)
    {
        return None;
    }
    let after = index + BEARER.len();
    let tail = &input[after..];
    let spaces = tail.len() - tail.trim_start_matches([' ', '\t']).len();
    if spaces == 0 {
        return None;
    }
    let token_start = after + spaces;
    let token: String = input[token_start..]
        .chars()
        .take_while(|c| is_token_char(*c) || *c == '=')
        .collect();
    if token.chars().count() < MIN_SECRET_TAIL {
        return None;
    }
    Some(token_start + token.len())
}

/// End of a prefixed secret token (`sk-…`, `ghp_…`, `AKIA…`) at `index`.
fn prefixed_secret_end(input: &str, index: usize) -> Option<usize> {
    for prefix in ["sk-", "ghp_", "AKIA"] {
        if !input[index..].starts_with(prefix) {
            continue;
        }
        if let Some(before) = prev_char(input, index)
            && !is_secret_boundary(before)
        {
            continue;
        }
        let tail_start = index + prefix.len();
        let tail: String = input[tail_start..]
            .chars()
            .take_while(|c| is_token_char(*c))
            .collect();
        if tail.chars().count() < MIN_SECRET_TAIL {
            continue;
        }
        return Some(tail_start + tail.len());
    }
    None
}

/// One redacted assignment.
struct AssignmentSpan {
    /// Where the assignment name starts.
    name_start: usize,
    /// Where the value starts (just past the `=`).
    value_start: usize,
    /// Where the value ends.
    value_end: usize,
    /// The quote character the value was wrapped in, if any.
    quote: Option<char>,
}

/// The `key=`-style assignment whose keyword starts at `index`, if any.
fn assignment_span(input: &str, index: usize) -> Option<AssignmentSpan> {
    const KEYWORDS: [&str; 4] = ["key", "token", "password", "secret"];
    let keyword = KEYWORDS
        .iter()
        .find(|word| input[index..].to_ascii_lowercase().starts_with(**word))?;
    let after_keyword = index + keyword.len();
    if !input[after_keyword..].starts_with('=') {
        return None;
    }
    // The keyword must start a name: `api_key` and `my_secret` are secret
    // assignments, `monkey` and `mon-key` are ordinary words.
    if let Some(before) = prev_char(input, index)
        && (before.is_ascii_alphanumeric() || before == '-')
    {
        return None;
    }
    // Extend the name backwards over the identifier prefix, if any.
    let mut name_start = index;
    while name_start > 0 {
        let previous = input[..name_start].chars().next_back().unwrap_or('\u{0}');
        if !is_name_char(previous) {
            break;
        }
        name_start -= previous.len_utf8();
    }
    let value_start = after_keyword + 1;
    let rest = &input[value_start..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'');
    let (value_end, quote) = match quote {
        Some(q) => {
            let body = &rest[q.len_utf8()..];
            match body.find(q) {
                Some(close) => (value_start + q.len_utf8() + close + q.len_utf8(), Some(q)),
                None => (input.len(), Some(q)),
            }
        }
        None => {
            let end = rest
                .find(|c: char| {
                    c.is_whitespace()
                        || matches!(
                            c,
                            ',' | ';' | ')' | ']' | '}' | '"' | '\'' | '`' | '<' | '>'
                        )
                })
                .map(|offset| value_start + offset)
                .unwrap_or(input.len());
            (end, None)
        }
    };
    let value_len = input[value_start..value_end]
        .chars()
        .filter(|c| *c != '"' && *c != '\'')
        .count();
    if value_len < MIN_SECRET_TAIL {
        return None;
    }
    Some(AssignmentSpan {
        name_start,
        value_start,
        value_end,
        quote,
    })
}

/// The absolute paths, redacted left to right.
fn redact_paths(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut index = 0usize;
    while index < input.len() {
        if let Some(end) = path_end(input, index) {
            out.push_str(PATH_PLACEHOLDER);
            index = end;
            continue;
        }
        let ch = input[index..].chars().next().unwrap_or('\u{0}');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

/// End of the absolute path that starts at `index`, if one does.
fn path_end(input: &str, index: usize) -> Option<usize> {
    if let Some(before) = prev_char(input, index)
        && !is_path_boundary(before)
    {
        return None;
    }
    let rest = &input[index..];
    if rest.starts_with("//") {
        // A protocol-relative URL, not a path.
        return None;
    }
    if rest.starts_with('/') {
        let end = path_run_end(input, index + 1);
        if end - index < 2 {
            return None;
        }
        return Some(end);
    }
    let bytes = rest.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        let end = path_run_end(input, index + 3);
        if end - index < 3 {
            return None;
        }
        return Some(end);
    }
    None
}

/// The end of a path body that starts at `start`.
fn path_run_end(input: &str, start: usize) -> usize {
    let mut end = start;
    for ch in input[start..].chars() {
        if is_path_terminator(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redacted(input: &str) -> String {
        redact(input).text
    }

    #[test]
    fn a_clean_string_passes_through_unchanged() {
        let clean = "Rust is a systems programming language. Ask me anything.";
        assert_eq!(redacted(clean), clean);
        assert!(!redact(clean).changed);
    }

    #[test]
    fn absolute_paths_become_path_placeholders() {
        assert_eq!(
            redacted("the file is at /usr/local/bin/abbey now"),
            "the file is at [path] now"
        );
        assert_eq!(
            redacted(r"the file is at C:\Users\me\abbey.exe now"),
            "the file is at [path] now"
        );
        assert_eq!(redacted("see (/opt/data) here"), "see ([path]) here");
    }

    #[test]
    fn secret_shapes_become_redacted() {
        assert_eq!(
            redacted("key sk-abcdefghijklmnop here"),
            "key [redacted] here"
        );
        assert_eq!(
            redacted("token ghp_abcdefghijklmnop here"),
            "token [redacted] here"
        );
        assert_eq!(
            redacted("id AKIAIOSFODNN7EXAMPLE here"),
            "id [redacted] here"
        );
        assert_eq!(
            redacted("auth Bearer abcdefghijklmnop here"),
            "auth [redacted] here"
        );
        assert_eq!(
            redacted("password=hunter2secret here"),
            "password=[redacted] here"
        );
        assert_eq!(
            redacted("api_key=abcdefghij here"),
            "api_key=[redacted] here"
        );
    }

    #[test]
    fn a_pem_block_is_redacted_with_its_body() {
        let block = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----";
        let out = redacted(block);
        assert_eq!(out, "[redacted]");
        assert!(!out.contains("MIIEow"));
    }

    #[test]
    fn short_matches_and_ordinary_words_survive() {
        assert_eq!(
            redacted("ask-me about risk-driven work"),
            "ask-me about risk-driven work"
        );
        assert_eq!(redacted("password=short"), "password=short");
        assert_eq!(redacted("monkey=banana"), "monkey=banana");
    }

    #[test]
    fn urls_are_not_treated_as_paths() {
        let url = "https://example.com/docs/page";
        assert_eq!(redacted(url), url);
    }

    #[test]
    fn redaction_is_deterministic() {
        let input = "/tmp/x sk-abcdefghijklmnop password=abcdefghij";
        let first = redacted(input);
        for _ in 0..4 {
            assert_eq!(redacted(input), first);
        }
    }
}
