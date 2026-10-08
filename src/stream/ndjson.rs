//! Newline framing shared by the NDJSON decoders.

pub(crate) const MAX_LINE_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(crate) struct LineBuffer {
    pending: Vec<u8>,
    discarding: bool,
}

impl LineBuffer {
    /// Complete, non-empty, trimmed lines in order, and whether an over-long
    /// line was dropped (its remainder is skipped up to the next newline).
    pub(crate) fn push(&mut self, chunk: &[u8]) -> (Vec<String>, bool) {
        let mut lines = Vec::new();
        let mut overflow = false;
        for &byte in chunk {
            if byte == b'\n' {
                if self.discarding {
                    self.discarding = false;
                } else {
                    let text = String::from_utf8_lossy(&self.pending).trim().to_string();
                    if !text.is_empty() {
                        lines.push(text);
                    }
                }
                self.pending.clear();
                continue;
            }
            if self.discarding {
                continue;
            }
            self.pending.push(byte);
            if self.pending.len() > MAX_LINE_BYTES {
                self.pending.clear();
                self.discarding = true;
                overflow = true;
            }
        }
        (lines, overflow)
    }

    pub(crate) fn finish(&mut self) -> Option<String> {
        let text = String::from_utf8_lossy(&self.pending).trim().to_string();
        self.pending.clear();
        (!self.discarding && !text.is_empty()).then_some(text)
    }
}

/// At most `max_chars` characters, with `…` when cut.
pub(crate) fn clip(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_buffer_splits_across_chunks() {
        let mut b = LineBuffer::default();
        let (l1, o1) = b.push(b"{\"a\":1}\n{\"b\"");
        assert_eq!(l1, vec!["{\"a\":1}".to_string()]);
        assert!(!o1);
        let (l2, _) = b.push(b":2}\n\n");
        assert_eq!(l2, vec!["{\"b\":2}".to_string()]);
        assert_eq!(b.finish(), None);
    }

    #[test]
    fn line_buffer_joins_split_utf8_inside_json() {
        let line = "{\"t\":\"🌍\"}\n".as_bytes();
        let mut b = LineBuffer::default();
        let mut got = Vec::new();
        for byte in line {
            got.extend(b.push(std::slice::from_ref(byte)).0);
        }
        assert_eq!(got, vec!["{\"t\":\"🌍\"}".to_string()]);
    }

    #[test]
    fn line_buffer_drops_overlong_line_and_reports_it() {
        let mut b = LineBuffer::default();
        let big = vec![b'x'; MAX_LINE_BYTES + 1];
        let (lines, overflow) = b.push(&big);
        assert!(lines.is_empty());
        assert!(overflow);
        let (lines, _) = b.push(b"ok\n{}\n");
        assert_eq!(lines, vec!["{}".to_string()]);
    }
}
