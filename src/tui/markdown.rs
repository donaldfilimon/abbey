//! Markdown → pre-wrapped ratatui lines. Pre-wrapping makes transcript
//! scrolling exact (line counts are known before drawing).

use super::theme::Theme;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

pub(crate) fn render(md: &str, theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let mut r = Renderer {
        theme,
        lines: Vec::new(),
        cur: Vec::new(),
        styles: vec![Style::default()],
        prefix: String::new(),
        list: Vec::new(),
        code: None,
    };
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for ev in Parser::new_ext(md, opts) {
        r.event(ev);
    }
    if let Some((lang, body)) = r.code.take() {
        r.code_block(lang.as_deref(), &body);
    }
    r.flush();
    while r.lines.last().is_some_and(|l| l.width() == 0) {
        r.lines.pop();
    }
    r.lines
        .into_iter()
        .flat_map(|l| wrap(l, width.max(8)))
        .collect()
}

struct Renderer<'t> {
    theme: &'t Theme,
    lines: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    styles: Vec<Style>,
    prefix: String,
    list: Vec<Option<u64>>,
    code: Option<(Option<String>, String)>,
}

impl Renderer<'_> {
    fn style(&self) -> Style {
        *self.styles.last().unwrap_or(&Style::default())
    }

    fn push_style(&mut self, s: Style) {
        let next = self.style().patch(s);
        self.styles.push(next);
    }

    fn flush(&mut self) {
        if !self.cur.is_empty() {
            let spans = std::mem::take(&mut self.cur);
            self.lines.push(Line::from(spans));
        }
    }

    fn blank(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|l| l.width() > 0) {
            self.lines.push(Line::default());
        }
    }

    fn text(&mut self, t: &str) {
        if self.cur.is_empty() && !self.prefix.is_empty() {
            self.cur.push(Span::styled(
                self.prefix.clone(),
                Style::default().fg(self.theme.fg_dim),
            ));
        }
        let style = self.style();
        self.cur.push(Span::styled(t.to_string(), style));
    }

    fn code_block(&mut self, lang: Option<&str>, body: &str) {
        self.flush();
        for runs in crate::highlight::code_lines(body, lang) {
            let mut spans = vec![Span::styled("│ ", Style::default().fg(self.theme.fg_dim))];
            spans.extend(
                runs.into_iter().map(|((r, g, b), t)| {
                    Span::styled(t, Style::default().fg(Color::Rgb(r, g, b)))
                }),
            );
            self.lines.push(Line::from(spans));
        }
        self.blank();
    }

    fn event(&mut self, ev: Event<'_>) {
        if let Some((_, body)) = self.code.as_mut() {
            match ev {
                Event::Text(t) => body.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, body) = self.code.take().expect("in code block");
                    self.code_block(lang.as_deref(), &body);
                }
                _ => {}
            }
            return;
        }
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                self.blank();
                let s = Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD);
                self.push_style(if level == HeadingLevel::H1 {
                    s.add_modifier(Modifier::UNDERLINED)
                } else {
                    s
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                self.styles.pop();
                self.flush();
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => self.blank(),
            Event::Start(Tag::Emphasis) => {
                self.push_style(Style::default().add_modifier(Modifier::ITALIC));
            }
            Event::Start(Tag::Strong) => {
                self.push_style(Style::default().add_modifier(Modifier::BOLD));
            }
            Event::Start(Tag::Strikethrough) => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough) => {
                self.styles.pop();
            }
            Event::Start(Tag::BlockQuote(_)) => self.prefix.push_str("▎ "),
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush();
                let keep = self.prefix.len().saturating_sub("▎ ".len());
                self.prefix.truncate(keep);
            }
            Event::Start(Tag::List(start)) => {
                self.flush();
                self.list.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                self.list.pop();
                if self.list.is_empty() {
                    self.blank();
                }
            }
            Event::Start(Tag::Item) => {
                self.flush();
                let indent = "  ".repeat(self.list.len().saturating_sub(1));
                let bullet = match self.list.last_mut() {
                    Some(Some(n)) => {
                        let b = format!("{n}. ");
                        *n += 1;
                        b
                    }
                    _ => "• ".to_string(),
                };
                self.cur.push(Span::raw(format!("{indent}{bullet}")));
            }
            Event::End(TagEnd::Item) => self.flush(),
            Event::TaskListMarker(done) => self.text(if done { "[x] " } else { "[ ] " }),
            Event::Start(Tag::CodeBlock(kind)) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) if !l.is_empty() => Some(l.to_string()),
                    _ => None,
                };
                self.code = Some((lang, String::new()));
            }
            Event::Code(c) => {
                let s = self.style().fg(self.theme.accent);
                self.cur.push(Span::styled(c.to_string(), s));
            }
            Event::Text(t) => {
                let mut first = true;
                for part in t.split('\n') {
                    if !first {
                        self.flush();
                    }
                    first = false;
                    if !part.is_empty() {
                        self.text(part);
                    }
                }
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.lines.push(Line::styled(
                    "────────",
                    Style::default().fg(self.theme.fg_dim),
                ));
                self.blank();
            }
            Event::End(TagEnd::TableCell) => self.text(" │ "),
            Event::End(TagEnd::TableRow | TagEnd::TableHead) => self.flush(),
            _ => {}
        }
    }
}

/// Hard-wrap one styled line at `width` display columns (character wrap).
pub(crate) fn wrap(line: Line<'static>, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    let mut out = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for span in line.spans {
        let mut buf = String::new();
        for c in span.content.chars() {
            let w = c.width().unwrap_or(0);
            if used + w > width && used > 0 {
                if !buf.is_empty() {
                    cur.push(Span::styled(std::mem::take(&mut buf), span.style));
                }
                out.push(Line::from(std::mem::take(&mut cur)));
                used = 0;
            }
            buf.push(c);
            used += w;
        }
        if !buf.is_empty() {
            cur.push(Span::styled(buf, span.style));
        }
    }
    out.push(Line::from(cur));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Theme, ThemeId};

    fn plain(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn headings_lists_and_code_render_as_text() {
        let t = Theme::from_id(ThemeId::Ink);
        let out = plain(&render(
            "# Title\n\n- one\n- two\n\n```rust\nfn a() {}\n```\n",
            &t,
            80,
        ));
        assert!(out.contains(&"Title".to_string()));
        assert!(out.contains(&"• one".to_string()));
        assert!(out.iter().any(|l| l.contains("fn a() {}")));
    }

    #[test]
    fn long_lines_wrap_to_width_by_display_columns() {
        let t = Theme::from_id(ThemeId::Ink);
        let out = render(&"字".repeat(30), &t, 20);
        for line in &out {
            assert!(line.width() <= 20, "line too wide: {}", line.width());
        }
    }

    #[test]
    fn partial_streaming_markdown_never_panics() {
        let t = Theme::from_id(ThemeId::Ink);
        for md in [
            "```rust\nfn a(",
            "**bold",
            "- [ ] item\n  - nest",
            "| a | b |\n|--",
        ] {
            let _ = render(md, &t, 40);
        }
    }
}
