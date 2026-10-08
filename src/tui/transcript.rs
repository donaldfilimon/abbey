//! The conversation as cells, built from user input and [`StreamEvent`]s.

use super::markdown;
use super::theme::Theme;
use crate::stream::StreamEvent;
use crate::stream::ndjson::clip;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

pub(crate) const MAX_CELLS: usize = 2000;
const MAX_CELL_BYTES: usize = 4 * 1024 * 1024;
const MAX_TRANSCRIPT_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIFF_LINES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolStatus {
    Running,
    Ok,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolCell {
    pub id: String,
    pub name: String,
    pub input: Value,
    pub status: ToolStatus,
    pub output: String,
    pub expanded: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cell {
    User(String),
    Assistant(String),
    Thinking { text: String, expanded: bool },
    Tool(ToolCell),
    Notice(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiffLine {
    Removed(String),
    Added(String),
}

#[derive(Debug, Default)]
pub(crate) struct Transcript {
    pub cells: Vec<Cell>,
    pub usage: Option<(u64, u64)>,
    pub session: Option<String>,
    bytes: usize,
    evicted: usize,
    clipped: bool,
    active: bool,
    exhausted: bool,
}

impl Transcript {
    fn push(&mut self, mut cell: Cell) {
        self.clipped |= bound_cell(&mut cell);
        self.bytes += cell_bytes(&cell);
        self.cells.push(cell);
        self.retain();
    }

    pub(crate) fn begin_turn(&mut self) {
        self.active = true;
        self.exhausted = false;
        self.usage = None;
    }

    /// Evict completed cells first. If active tool cells alone exhaust the
    /// ceiling, terminate their presentation and tell the caller to cancel.
    fn retain(&mut self) -> bool {
        let mut accepted = true;
        while self.cells.len() > MAX_CELLS || self.bytes > MAX_TRANSCRIPT_BYTES {
            let last = self.cells.len().saturating_sub(1);
            let completed = self.cells.iter().enumerate().position(|(i, c)| {
                !matches!(c, Cell::Tool(t) if t.status == ToolStatus::Running)
                    && !(self.active
                        && i == last
                        && matches!(c, Cell::Assistant(_) | Cell::Thinking { .. }))
            });
            let index = match completed {
                Some(i) => i,
                None => {
                    accepted = false;
                    self.exhausted = true;
                    self.finish_turn();
                    0
                }
            };
            let removed = self.cells.remove(index);
            self.bytes -= cell_bytes(&removed);
            self.evicted += 1;
        }
        accepted
    }

    fn finish_turn(&mut self) {
        self.active = false;
        for cell in &mut self.cells {
            if let Cell::Tool(tool) = cell
                && tool.status == ToolStatus::Running
            {
                tool.status = ToolStatus::Failed;
            }
        }
    }

    pub(crate) fn push_user(&mut self, text: &str) {
        self.push(Cell::User(text.to_string()));
    }

    pub(crate) fn push_notice(&mut self, text: impl Into<String>) {
        self.push(Cell::Notice(text.into()));
    }

    pub(crate) fn push_error(&mut self, text: impl Into<String>) {
        self.push(Cell::Error(text.into()));
    }

    pub(crate) fn apply(&mut self, ev: StreamEvent) -> bool {
        match ev {
            StreamEvent::TextDelta(t) => match self.cells.last_mut() {
                Some(Cell::Assistant(s)) => {
                    let before = s.len();
                    self.clipped |= append_bounded(s, &t, MAX_CELL_BYTES);
                    self.bytes += s.len() - before;
                }
                _ => self.push(Cell::Assistant(t)),
            },
            StreamEvent::ThinkingDelta(t) => match self.cells.last_mut() {
                Some(Cell::Thinking { text, .. }) => {
                    let before = text.len();
                    self.clipped |= append_bounded(text, &t, MAX_CELL_BYTES);
                    self.bytes += text.len() - before;
                }
                _ => self.push(Cell::Thinking {
                    text: t,
                    expanded: false,
                }),
            },
            StreamEvent::ToolStart { id, name, input } => self.push(Cell::Tool(ToolCell {
                id,
                name,
                input,
                status: ToolStatus::Running,
                output: String::new(),
                expanded: false,
            })),
            StreamEvent::ToolEnd { id, ok, output } => {
                let found = self.cells.iter_mut().rev().find_map(|c| match c {
                    Cell::Tool(t) if t.id == id => Some(t),
                    _ => None,
                });
                if let Some(t) = found {
                    let before = tool_bytes(t);
                    t.status = if ok {
                        ToolStatus::Ok
                    } else {
                        ToolStatus::Failed
                    };
                    t.output = output;
                    self.clipped |= bound_tool(t);
                    self.bytes = self.bytes - before + tool_bytes(t);
                }
            }
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
            } => {
                self.usage = Some((input_tokens, output_tokens));
            }
            StreamEvent::SessionId(mut id) => {
                self.clipped |= truncate_bytes(&mut id, 256);
                self.session = Some(id);
            }
            StreamEvent::Notice(n) => self.push_notice(n),
            StreamEvent::Done { exit } => {
                self.finish_turn();
                match exit {
                    0 => {}
                    130 => self.push_notice("⏹ interrupted"),
                    _ => self.push_error(format!("executor exited {exit}")),
                }
            }
            StreamEvent::Failed(msg) => self.push_error(msg),
        }
        self.retain() && !self.exhausted
    }

    /// Expand/collapse the most recent tool or thinking cell (Ctrl-O).
    pub(crate) fn toggle_last_expandable(&mut self) {
        for c in self.cells.iter_mut().rev() {
            match c {
                Cell::Tool(t) => {
                    t.expanded = !t.expanded;
                    return;
                }
                Cell::Thinking { expanded, .. } => {
                    *expanded = !*expanded;
                    return;
                }
                _ => {}
            }
        }
    }

    pub(crate) fn lines(&self, theme: &Theme, width: u16) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        if self.evicted > 0 || self.clipped {
            out.push(Line::raw(format!(
                "retention: {} older cell(s) removed{}",
                self.evicted,
                if self.clipped {
                    " · oversized content clipped"
                } else {
                    ""
                }
            )));
        }
        for cell in &self.cells {
            cell_lines(cell, theme, width, &mut out);
            out.push(Line::default());
        }
        out
    }
}

fn truncate_bytes(text: &mut String, cap: usize) -> bool {
    if text.len() <= cap {
        return false;
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    true
}

fn append_bounded(text: &mut String, delta: &str, cap: usize) -> bool {
    let remaining = cap.saturating_sub(text.len());
    let mut end = delta.len().min(remaining);
    while !delta.is_char_boundary(end) {
        end -= 1;
    }
    text.push_str(&delta[..end]);
    end < delta.len()
}

fn tool_bytes(t: &ToolCell) -> usize {
    t.id.len() + t.name.len() + t.output.len() + t.input.to_string().len()
}

fn cell_bytes(c: &Cell) -> usize {
    match c {
        Cell::User(t)
        | Cell::Assistant(t)
        | Cell::Notice(t)
        | Cell::Error(t)
        | Cell::Thinking { text: t, .. } => t.len(),
        Cell::Tool(t) => tool_bytes(t),
    }
}

fn bound_tool(t: &mut ToolCell) -> bool {
    let mut clipped = false;
    if t.input.to_string().len() > MAX_CELL_BYTES / 2 {
        t.input = Value::Null;
        clipped = true;
    }
    clipped |= truncate_bytes(&mut t.id, MAX_CELL_BYTES / 4);
    clipped |= truncate_bytes(&mut t.name, MAX_CELL_BYTES / 4);
    let remaining = MAX_CELL_BYTES - t.id.len() - t.name.len() - t.input.to_string().len();
    clipped |= truncate_bytes(&mut t.output, remaining);
    clipped
}

fn bound_cell(c: &mut Cell) -> bool {
    match c {
        Cell::User(t)
        | Cell::Assistant(t)
        | Cell::Notice(t)
        | Cell::Error(t)
        | Cell::Thinking { text: t, .. } => truncate_bytes(t, MAX_CELL_BYTES),
        Cell::Tool(t) => bound_tool(t),
    }
}

fn cell_lines(cell: &Cell, theme: &Theme, width: u16, out: &mut Vec<Line<'static>>) {
    let dim = Style::default().fg(theme.fg_dim);
    let wrap_into = |line: Line<'static>, out: &mut Vec<Line<'static>>| {
        out.extend(markdown::wrap(line, width));
    };
    match cell {
        Cell::User(t) => {
            for (i, l) in t.lines().enumerate() {
                let marker = if i == 0 { "› " } else { "  " };
                wrap_into(
                    Line::from(vec![
                        Span::styled(
                            marker,
                            Style::default()
                                .fg(theme.accent)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(l.to_string()),
                    ]),
                    out,
                );
            }
        }
        Cell::Assistant(md) => out.extend(markdown::render(md, theme, width)),
        Cell::Thinking { text, expanded } => {
            if *expanded {
                for l in text.lines() {
                    wrap_into(
                        Line::styled(format!("┊ {l}"), dim.add_modifier(Modifier::ITALIC)),
                        out,
                    );
                }
            } else {
                wrap_into(
                    Line::styled(
                        format!("✻ thinking… ({} chars, Ctrl-O)", text.chars().count()),
                        dim,
                    ),
                    out,
                );
            }
        }
        Cell::Tool(t) => {
            let (glyph, color) = match t.status {
                ToolStatus::Running => ("●", theme.warn),
                ToolStatus::Ok => ("✓", theme.ok),
                ToolStatus::Failed => ("✗", theme.error),
            };
            wrap_into(
                Line::from(vec![
                    Span::styled(format!("{glyph} "), Style::default().fg(color)),
                    Span::styled(
                        t.name.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  {}", tool_summary(&t.name, &t.input)), dim),
                ]),
                out,
            );
            if let Some(items) = todo_items(&t.name, &t.input) {
                for (status, text) in items {
                    let mark = match status.as_str() {
                        "completed" => "☑",
                        "in_progress" => "◐",
                        _ => "☐",
                    };
                    wrap_into(Line::raw(format!("  {mark} {text}")), out);
                }
            } else if let Some(diff) = diff_lines(&t.name, &t.input) {
                let shown = if t.expanded {
                    diff.len()
                } else {
                    diff.len().min(12)
                };
                for d in &diff[..shown] {
                    let (s, c) = match d {
                        DiffLine::Removed(l) => (format!("  - {l}"), theme.error),
                        DiffLine::Added(l) => (format!("  + {l}"), theme.ok),
                    };
                    wrap_into(Line::styled(s, Style::default().fg(c)), out);
                }
                if shown < diff.len() {
                    wrap_into(
                        Line::styled(format!("  … {} more (Ctrl-O)", diff.len() - shown), dim),
                        out,
                    );
                }
            }
            if t.expanded && !t.output.is_empty() {
                for l in t.output.lines().take(200) {
                    wrap_into(Line::styled(format!("  │ {l}"), dim), out);
                }
            }
        }
        Cell::Notice(n) => {
            for line in n.lines() {
                wrap_into(Line::styled(format!("· {line}"), dim), out);
            }
        }
        Cell::Error(e) => {
            for line in e.lines() {
                wrap_into(
                    Line::styled(format!("✗ {line}"), Style::default().fg(theme.error)),
                    out,
                );
            }
        }
    }
}

pub(crate) fn tool_summary(_name: &str, input: &Value) -> String {
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
    ] {
        if let Some(s) = input[key].as_str() {
            return clip(s.lines().next().unwrap_or(s), 80);
        }
    }
    String::new()
}

pub(crate) fn diff_lines(name: &str, input: &Value) -> Option<Vec<DiffLine>> {
    let mut out = Vec::new();
    let mut add_pair = |old: &str, new: &str| {
        out.extend(old.lines().map(|l| DiffLine::Removed(l.to_string())));
        out.extend(new.lines().map(|l| DiffLine::Added(l.to_string())));
    };
    match name {
        "Edit" | "edit" | "StrReplace" | "search_replace" => {
            add_pair(input["old_string"].as_str()?, input["new_string"].as_str()?);
        }
        "MultiEdit" => {
            for e in input["edits"].as_array()? {
                add_pair(
                    e["old_string"].as_str().unwrap_or(""),
                    e["new_string"].as_str().unwrap_or(""),
                );
            }
        }
        "Write" | "write" => add_pair("", input["content"].as_str()?),
        _ => return None,
    }
    out.truncate(MAX_DIFF_LINES);
    Some(out)
}

pub(crate) fn todo_items(name: &str, input: &Value) -> Option<Vec<(String, String)>> {
    if name != "TodoWrite" {
        return None;
    }
    Some(
        input["todos"]
            .as_array()?
            .iter()
            .map(|t| {
                (
                    t["status"].as_str().unwrap_or("pending").to_string(),
                    t["content"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_notice_preserves_each_display_line() {
        let mut lines = Vec::new();
        cell_lines(
            &Cell::Notice("first line\nsecond line\nthird line".into()),
            &Theme::from_id(super::super::theme::ThemeId::Ink),
            100,
            &mut lines,
        );
        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["· first line", "· second line", "· third line"]
        );
    }

    #[test]
    fn multiline_error_preserves_each_display_line() {
        let mut lines = Vec::new();
        cell_lines(
            &Cell::Error("first error\nsecond error".into()),
            &Theme::from_id(super::super::theme::ThemeId::Ink),
            100,
            &mut lines,
        );
        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["✗ first error", "✗ second error"]
        );
    }
    use crate::stream::StreamEvent;
    use serde_json::json;

    #[test]
    fn retention_bounds_bytes_and_keeps_utf8_and_active_tools() {
        let mut t = Transcript::default();
        t.begin_turn();
        t.apply(StreamEvent::ToolStart {
            id: "active".into(),
            name: "Read".into(),
            input: json!({}),
        });
        t.push_notice("a".repeat(MAX_CELL_BYTES));
        t.push_notice("b".repeat(MAX_CELL_BYTES));
        assert!(t.bytes <= MAX_TRANSCRIPT_BYTES);
        assert!(matches!(&t.cells[0], Cell::Tool(tool) if tool.status == ToolStatus::Running));
        t.apply(StreamEvent::TextDelta("🌍".repeat(MAX_CELL_BYTES / 4 + 1)));
        assert!(t.bytes <= MAX_TRANSCRIPT_BYTES);
        assert!(t.clipped);
        assert!(t.evicted > 0);
        let Cell::Assistant(text) = t.cells.last().unwrap() else {
            panic!();
        };
        assert_eq!(text.len(), MAX_CELL_BYTES);
        assert!(text.ends_with('🌍'));
        assert!(
            t.lines(&Theme::from_id(super::super::theme::ThemeId::Ink), 80)[0]
                .to_string()
                .contains("retention")
        );
    }

    #[test]
    fn active_tool_flood_requests_cancellation_instead_of_unbounded_retention() {
        let mut t = Transcript::default();
        t.begin_turn();
        for i in 0..MAX_CELLS {
            assert!(t.apply(StreamEvent::ToolStart {
                id: i.to_string(),
                name: "Read".into(),
                input: json!({})
            }));
        }
        assert!(!t.apply(StreamEvent::ToolStart {
            id: "overflow".into(),
            name: "Read".into(),
            input: json!({})
        }));
        assert!(t.cells.len() <= MAX_CELLS);
        assert!(
            t.cells
                .iter()
                .all(|c| !matches!(c, Cell::Tool(tool) if tool.status == ToolStatus::Running))
        );
    }

    #[test]
    fn oversized_tool_cells_have_a_single_combined_budget() {
        let mut t = Transcript::default();
        t.apply(StreamEvent::ToolStart {
            id: "i".repeat(MAX_CELL_BYTES),
            name: "n".repeat(MAX_CELL_BYTES),
            input: json!({"data": "x".repeat(MAX_CELL_BYTES)}),
        });
        t.apply(StreamEvent::ToolEnd {
            id: "i".repeat(MAX_CELL_BYTES / 4),
            ok: true,
            output: "🌍".repeat(MAX_CELL_BYTES),
        });
        assert!(t.cells.iter().all(|c| cell_bytes(c) <= MAX_CELL_BYTES));
        assert!(t.clipped);
        assert!(t.bytes <= MAX_TRANSCRIPT_BYTES);
    }

    #[test]
    fn deltas_coalesce_into_one_assistant_cell() {
        let mut t = Transcript::default();
        t.push_user("hi");
        t.apply(StreamEvent::TextDelta("hel".into()));
        t.apply(StreamEvent::TextDelta("lo".into()));
        assert!(matches!(&t.cells[1], Cell::Assistant(s) if s == "hello"));
        assert_eq!(t.cells.len(), 2);
    }

    #[test]
    fn tool_end_updates_the_matching_start() {
        let mut t = Transcript::default();
        t.apply(StreamEvent::ToolStart {
            id: "a".into(),
            name: "Bash".into(),
            input: json!({"command": "ls"}),
        });
        t.apply(StreamEvent::TextDelta("between".into()));
        t.apply(StreamEvent::ToolEnd {
            id: "a".into(),
            ok: false,
            output: "boom".into(),
        });
        let Cell::Tool(tool) = &t.cells[0] else {
            panic!()
        };
        assert_eq!(tool.status, ToolStatus::Failed);
        assert_eq!(tool.output, "boom");
    }

    #[test]
    fn done_130_is_a_notice_and_nonzero_is_an_error() {
        let mut t = Transcript::default();
        t.apply(StreamEvent::Done { exit: 130 });
        t.apply(StreamEvent::Done { exit: 2 });
        t.apply(StreamEvent::Done { exit: 0 });
        assert!(matches!(&t.cells[0], Cell::Notice(n) if n.contains("interrupted")));
        assert!(matches!(&t.cells[1], Cell::Error(e) if e.contains('2')));
        assert_eq!(t.cells.len(), 2);
    }

    #[test]
    fn summaries_diffs_and_todos_come_from_tool_input() {
        assert_eq!(
            tool_summary("Bash", &json!({"command": "cargo test"})),
            "cargo test"
        );
        assert_eq!(
            tool_summary("Read", &json!({"file_path": "src/a.rs"})),
            "src/a.rs"
        );
        let d = diff_lines("Edit", &json!({"old_string": "a\nb", "new_string": "a\nc"})).unwrap();
        assert_eq!(
            d,
            vec![
                DiffLine::Removed("a".into()),
                DiffLine::Removed("b".into()),
                DiffLine::Added("a".into()),
                DiffLine::Added("c".into())
            ]
        );
        let todos = todo_items(
            "TodoWrite",
            &json!({"todos": [{"content": "x", "status": "completed"}]}),
        )
        .unwrap();
        assert_eq!(todos, vec![("completed".to_string(), "x".to_string())]);
        assert!(diff_lines("Read", &json!({})).is_none());
    }

    #[test]
    fn cell_count_is_bounded() {
        let mut t = Transcript::default();
        for i in 0..(MAX_CELLS + 50) {
            t.push_notice(format!("n{i}"));
        }
        assert_eq!(t.cells.len(), MAX_CELLS);
        assert!(matches!(&t.cells[0], Cell::Notice(n) if n == "n50"));
    }

    #[test]
    fn reported_usage_belongs_only_to_the_turn_that_emitted_it() {
        let mut transcript = Transcript::default();
        transcript.begin_turn();
        assert!(transcript.apply(StreamEvent::Usage {
            input_tokens: 7,
            output_tokens: 2,
        }));
        assert_eq!(transcript.usage, Some((7, 2)));
        assert!(transcript.apply(StreamEvent::Done { exit: 0 }));

        transcript.begin_turn();
        assert_eq!(transcript.usage, None, "new turn inherited old token usage");
        assert!(transcript.apply(StreamEvent::TextDelta("plain executor response".into())));
        assert_eq!(transcript.usage, None, "plain text fabricated usage");
        assert!(transcript.apply(StreamEvent::Done { exit: 0 }));
        assert_eq!(transcript.usage, None, "completion restored old usage");

        transcript.begin_turn();
        assert!(transcript.apply(StreamEvent::Usage {
            input_tokens: 19,
            output_tokens: 5,
        }));
        assert_eq!(transcript.usage, Some((19, 5)));
    }

    #[test]
    fn interrupted_turn_usage_is_not_reused_by_the_next_turn() {
        let mut transcript = Transcript::default();
        transcript.begin_turn();
        transcript.apply(StreamEvent::Usage {
            input_tokens: 31,
            output_tokens: 8,
        });
        transcript.apply(StreamEvent::Done { exit: 130 });
        transcript.begin_turn();
        assert_eq!(transcript.usage, None);
        transcript.apply(StreamEvent::Done { exit: 1 });
        assert_eq!(transcript.usage, None);
    }
}
