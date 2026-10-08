//! Multiline prompt buffer and persisted prompt history.

use std::io::Write as _;
use std::path::{Path, PathBuf};

const HISTORY_FILE: &str = "tui-history.jsonl";
const HISTORY_MAX: usize = 500;

#[derive(Debug, Default, Clone)]
pub(crate) struct Composer {
    pub text: String,
    pub cursor: usize,
}

impl Composer {
    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(crate) fn set(&mut self, text: String) {
        self.cursor = text.len();
        self.text = text;
    }

    pub(crate) fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub(crate) fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub(crate) fn insert_str(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }

    pub(crate) fn newline(&mut self) {
        self.insert_char('\n');
    }

    fn prev_boundary(&self, i: usize) -> usize {
        self.text[..i]
            .char_indices()
            .next_back()
            .map_or(0, |(p, _)| p)
    }

    fn next_boundary(&self, i: usize) -> usize {
        self.text[i..]
            .chars()
            .next()
            .map_or(i, |c| i + c.len_utf8())
    }

    pub(crate) fn backspace(&mut self) {
        if self.cursor > 0 {
            let p = self.prev_boundary(self.cursor);
            self.text.replace_range(p..self.cursor, "");
            self.cursor = p;
        }
    }

    pub(crate) fn delete(&mut self) {
        if self.cursor < self.text.len() {
            let n = self.next_boundary(self.cursor);
            self.text.replace_range(self.cursor..n, "");
        }
    }

    pub(crate) fn left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor);
    }

    pub(crate) fn right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
    }

    pub(crate) fn word_left(&mut self) {
        let before = &self.text[..self.cursor];
        let trimmed = before.trim_end_matches(char::is_whitespace);
        self.cursor = trimmed.rfind(char::is_whitespace).map_or(0, |i| {
            i + trimmed[i..].chars().next().map_or(1, char::len_utf8)
        });
    }

    pub(crate) fn word_right(&mut self) {
        let after = &self.text[self.cursor..];
        let skip_ws = after.len() - after.trim_start_matches(char::is_whitespace).len();
        let rest = &after[skip_ws..];
        let word = rest.find(char::is_whitespace).unwrap_or(rest.len());
        self.cursor += skip_ws + word;
    }

    fn line_bounds(&self) -> (usize, usize) {
        let start = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i);
        (start, end)
    }

    pub(crate) fn line_start(&mut self) {
        self.cursor = self.line_bounds().0;
    }

    pub(crate) fn line_end(&mut self) {
        self.cursor = self.line_bounds().1;
    }

    pub(crate) fn kill_line_before(&mut self) {
        let (start, _) = self.line_bounds();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    pub(crate) fn kill_word_before(&mut self) {
        let end = self.cursor;
        self.word_left();
        self.text.replace_range(self.cursor..end, "");
    }

    pub(crate) fn cursor_row_col(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        let row = before.matches('\n').count();
        let col = before.rsplit('\n').next().unwrap_or("").chars().count();
        (row, col)
    }

    fn move_to_row(&mut self, row: usize, col: usize) {
        let mut offset = 0;
        for (i, line) in self.text.split('\n').enumerate() {
            if i == row {
                let byte = line.char_indices().nth(col).map_or(line.len(), |(b, _)| b);
                self.cursor = offset + byte;
                return;
            }
            offset += line.len() + 1;
        }
    }

    pub(crate) fn up(&mut self) -> bool {
        let (row, col) = self.cursor_row_col();
        if row == 0 {
            return false;
        }
        self.move_to_row(row - 1, col);
        true
    }

    pub(crate) fn down(&mut self) -> bool {
        let (row, col) = self.cursor_row_col();
        if row + 1 >= self.text.split('\n').count() {
            return false;
        }
        self.move_to_row(row + 1, col);
        true
    }
}

#[derive(Debug, Default)]
pub(crate) struct History {
    path: Option<PathBuf>,
    entries: Vec<String>,
    idx: Option<usize>,
    draft: String,
}

impl History {
    pub(crate) fn load(state_dir: &Path) -> Self {
        let path = state_dir.join(HISTORY_FILE);
        let entries = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<String>(l).ok())
            .collect();
        Self {
            path: Some(path),
            entries,
            idx: None,
            draft: String::new(),
        }
    }

    pub(crate) fn push(&mut self, entry: &str) {
        self.idx = None;
        if entry.trim().is_empty() || self.entries.last().is_some_and(|l| l == entry) {
            return;
        }
        self.entries.push(entry.to_string());
        if self.entries.len() > HISTORY_MAX {
            let excess = self.entries.len() - HISTORY_MAX;
            self.entries.drain(..excess);
        }
        if let Some(path) = &self.path {
            let body: String = self
                .entries
                .iter()
                .filter_map(|e| serde_json::to_string(e).ok())
                .map(|l| l + "\n")
                .collect();
            let tmp = path.with_extension("jsonl.tmp");
            if std::fs::File::create(&tmp)
                .and_then(|mut f| f.write_all(body.as_bytes()))
                .is_ok()
            {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub(crate) fn prev(&mut self, current: &str) -> Option<String> {
        let idx = match self.idx {
            None => {
                self.draft = current.to_string();
                self.entries.len().checked_sub(1)?
            }
            Some(0) => return None,
            Some(i) => i - 1,
        };
        self.idx = Some(idx);
        self.entries.get(idx).cloned()
    }

    pub(crate) fn next(&mut self) -> Option<String> {
        let i = self.idx?;
        if i + 1 >= self.entries.len() {
            self.idx = None;
            return Some(std::mem::take(&mut self.draft));
        }
        self.idx = Some(i + 1);
        self.entries.get(i + 1).cloned()
    }

    pub(crate) fn search(&self, query: &str) -> Option<&str> {
        if query.is_empty() {
            return None;
        }
        self.entries
            .iter()
            .rev()
            .find(|e| e.contains(query))
            .map(String::as_str)
    }

    pub(crate) fn entries(&self) -> &[String] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_editing_moves_between_rows_keeping_columns() {
        let mut c = Composer::default();
        c.insert_str("abc");
        c.newline();
        c.insert_str("de");
        assert_eq!(c.cursor_row_col(), (1, 2));
        assert!(c.up());
        assert_eq!(c.cursor_row_col(), (0, 2));
        assert!(!c.up());
        assert!(c.down());
        assert!(!c.down());
        assert_eq!(c.text, "abc\nde");
    }

    #[test]
    fn utf8_safe_at_every_motion() {
        let mut c = Composer::default();
        c.insert_str("héllo 🌍 wörld");
        for _ in 0..30 {
            c.left();
        }
        for _ in 0..3 {
            c.right();
        }
        c.backspace();
        c.word_right();
        c.kill_word_before();
        assert!(c.text.is_char_boundary(c.cursor));
    }

    #[test]
    fn kill_line_before_and_word_before() {
        let mut c = Composer::default();
        c.insert_str("one two three");
        c.kill_word_before();
        assert_eq!(c.text, "one two ");
        c.kill_line_before();
        assert_eq!(c.text, "");
    }

    #[test]
    fn history_persists_dedupes_and_searches() {
        let dir = std::env::temp_dir().join(format!(
            "abbey-hist-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = History::load(&dir);
        h.push("first\nline");
        h.push("second");
        h.push("second");
        let mut again = History::load(&dir);
        assert_eq!(again.prev("draft").as_deref(), Some("second"));
        assert_eq!(again.prev("").as_deref(), Some("first\nline"));
        assert_eq!(again.next().as_deref(), Some("second"));
        assert_eq!(again.next().as_deref(), Some("draft"));
        assert_eq!(again.search("fir"), Some("first\nline"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
