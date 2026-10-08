//! Composer completion: `/slash` via the existing ranker, `@path` fuzzy files.

use super::predict::{self, Prediction};
use std::path::Path;
use std::process::Command;

const MAX_FILES: usize = 20_000;
const MAX_ITEMS: usize = 8;
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".build",
    "zig-out",
    ".zig-cache",
];

#[derive(Debug, Default, Clone)]
pub(crate) struct FileIndex {
    paths: Vec<String>,
}

impl FileIndex {
    #[cfg(test)]
    pub(crate) fn from_paths(paths: Vec<String>) -> Self {
        Self { paths }
    }

    pub(crate) fn load(root: &Path) -> Self {
        let git = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "-co", "--exclude-standard"])
            .output();
        if let Ok(out) = git
            && out.status.success()
        {
            let paths = String::from_utf8_lossy(&out.stdout)
                .lines()
                .take(MAX_FILES)
                .map(str::to_string)
                .collect();
            return Self { paths };
        }
        let mut paths = Vec::new();
        let mut stack = vec![(root.to_path_buf(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in rd.flatten() {
                if paths.len() >= MAX_FILES {
                    return Self { paths };
                }
                let p = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if p.is_dir() {
                    if depth < 6 && !SKIP_DIRS.contains(&name.as_str()) {
                        stack.push((p, depth + 1));
                    }
                } else if let Ok(rel) = p.strip_prefix(root) {
                    paths.push(rel.to_string_lossy().into_owned());
                }
            }
        }
        Self { paths }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Completion {
    Slash(Vec<Prediction>),
    File {
        token_start: usize,
        items: Vec<String>,
    },
}

pub(crate) fn fuzzy_score(candidate: &str, query: &str) -> Option<i32> {
    let cand: Vec<char> = candidate.to_lowercase().chars().collect();
    let base_start = candidate
        .rfind('/')
        .map_or(0, |i| candidate[..=i].chars().count());
    let mut score = 0i32;
    let mut pos = 0usize;
    let mut prev: Option<usize> = None;
    for q in query.to_lowercase().chars() {
        let found = cand[pos..].iter().position(|c| *c == q)? + pos;
        score += 1;
        if prev == Some(found.wrapping_sub(1)) {
            score += 5;
        }
        if found >= base_start {
            score += 2;
        }
        prev = Some(found);
        pos = found + 1;
    }
    Some(score * 10 - i32::try_from(cand.len()).unwrap_or(i32::MAX / 20))
}

fn at_token(text: &str, cursor: usize) -> Option<(usize, &str)> {
    let before = text.get(..cursor)?;
    let start = before
        .char_indices()
        .rfind(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8());
    let token = &before[start..];
    token.strip_prefix('@').map(|q| (start, q))
}

pub(crate) fn complete(
    text: &str,
    cursor: usize,
    history: &[String],
    files: &FileIndex,
    llm_boost: Option<&str>,
) -> Option<Completion> {
    if !text.contains('@')
        && text.trim_start().starts_with('/')
        && text.trim_start().contains(char::is_whitespace)
    {
        return None;
    }
    if cursor == text.len() && !text.contains('\n') && !text.contains('@') {
        let preds = predict::rank(text, history, llm_boost);
        return (!preds.is_empty()).then_some(Completion::Slash(preds));
    }
    let (token_start, query) = at_token(text, cursor)?;
    let mut scored: Vec<(i32, &String)> = files
        .paths
        .iter()
        .filter_map(|p| fuzzy_score(p, query).map(|s| (s, p)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let items: Vec<String> = scored
        .into_iter()
        .take(MAX_ITEMS)
        .map(|(_, p)| p.clone())
        .collect();
    (!items.is_empty()).then_some(Completion::File { token_start, items })
}

pub(crate) fn accept(text: &str, cursor: usize, c: &Completion, idx: usize) -> (String, usize) {
    match c {
        Completion::Slash(preds) => {
            let Some(p) = preds.get(idx) else {
                return (text.to_string(), cursor);
            };
            let new = predict::accept_text(text, p.name);
            let len = new.len();
            (new, len)
        }
        Completion::File { token_start, items } => {
            let Some(path) = items.get(idx) else {
                return (text.to_string(), cursor);
            };
            if at_token(text, cursor).is_none_or(|(start, query)| {
                start != *token_start || fuzzy_score(path, query).is_none()
            }) {
                return (text.to_string(), cursor);
            }
            let insert = format!("@{path} ");
            let new = format!("{}{insert}{}", &text[..*token_start], &text[cursor..]);
            (new, token_start + insert.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> FileIndex {
        FileIndex::from_paths(vec![
            "src/tui/app.rs".into(),
            "src/stream/mod.rs".into(),
            "README.md".into(),
        ])
    }

    #[test]
    fn at_token_under_cursor_completes_paths() {
        let text = "look at @strmo please";
        let cursor = "look at @strmo".len();
        let Some(Completion::File { token_start, items }) =
            complete(text, cursor, &[], &idx(), None)
        else {
            panic!()
        };
        assert_eq!(token_start, "look at ".len());
        assert_eq!(items[0], "src/stream/mod.rs");
        let (new, cur) = accept(text, cursor, &Completion::File { token_start, items }, 0);
        assert_eq!(new, "look at @src/stream/mod.rs  please");
        assert_eq!(cur, "look at @src/stream/mod.rs ".len());
    }

    #[test]
    fn slash_prefix_uses_the_catalog_ranker() {
        let Some(Completion::Slash(p)) = complete("/hel", 4, &[], &idx(), None) else {
            panic!()
        };
        assert_eq!(p[0].name, "help");
    }

    #[test]
    fn plain_text_has_no_completion() {
        assert!(complete("hello world", 11, &[], &idx(), None).is_none());
    }

    #[test]
    fn fuzzy_prefers_contiguous_and_basename_matches() {
        assert!(
            fuzzy_score("src/tui/app.rs", "app").unwrap()
                > fuzzy_score("src/tui/app.rs", "srs").unwrap()
        );
        assert!(fuzzy_score("README.md", "zz").is_none());
    }

    #[test]
    fn file_completion_handles_full_unicode_whitespace_boundaries() {
        let files = FileIndex::from_paths(vec!["src/main.rs".into()]);
        for separator in [" ", "\u{00a0}", "\u{3000}"] {
            let prefix = format!("hello{separator}");
            let text = format!("{prefix}@src");
            let Some(Completion::File { token_start, items }) =
                complete(&text, text.len(), &[], &files, None)
            else {
                panic!("file completion missing after {separator:?}");
            };
            assert_eq!(token_start, prefix.len());
            assert!(text.is_char_boundary(token_start));
            assert_eq!(items, vec!["src/main.rs"]);
        }
    }

    #[test]
    fn file_accept_refuses_ranges_from_a_different_composer_state() {
        for (text, cursor, token_start) in [
            ("", 0, 5),
            ("é@", "é@".len(), 1),
            ("look @app", 0, 5),
            ("look plain", "look plain".len(), 5),
            ("", 0, 0),
        ] {
            let cached = Completion::File {
                token_start,
                items: vec!["apple.rs".into()],
            };
            let accepted = accept(text, cursor, &cached, 0);
            assert_eq!(
                accepted,
                (text.to_string(), cursor),
                "stale completion modified {text:?} at cursor {cursor} / token {token_start}"
            );
        }
    }

    #[test]
    fn file_accept_preserves_a_current_utf8_prefix_and_suffix() {
        let text = "é look @app keep 🌍";
        let cursor = "é look @app".len();
        let cached = Completion::File {
            token_start: "é look ".len(),
            items: vec!["apple.rs".into()],
        };
        let (accepted, next_cursor) = accept(text, cursor, &cached, 0);
        assert_eq!(accepted, "é look @apple.rs  keep 🌍");
        assert_eq!(next_cursor, "é look @apple.rs ".len());
        assert!(accepted.is_char_boundary(next_cursor));
    }

    #[test]
    fn natural_language_intent_reuses_the_existing_ranker() {
        let text = "review the auth diff";
        let Some(Completion::Slash(predictions)) =
            complete(text, text.len(), &[], &FileIndex::default(), None)
        else {
            panic!("approved natural-language intent completion was dropped");
        };
        let selected = predictions
            .iter()
            .position(|prediction| prediction.name == "review")
            .expect("review intent remains in the catalog predictions");
        let (accepted, cursor) =
            accept(text, text.len(), &Completion::Slash(predictions), selected);
        assert!(accepted.starts_with("/review "));
        assert!(accepted.contains("auth diff"));
        assert_eq!(cursor, accepted.len());
    }
}
