//! Plain-text streams (ollama, fm, abi): UTF-8 boundary carry plus ANSI CSI
//! stripping, so a split code point or a spinner escape never reaches the UI.

use super::{StreamDecoder, StreamEvent};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Esc {
    #[default]
    None,
    Start,
    Csi,
}

#[derive(Default)]
pub(crate) struct PlainDecoder {
    carry: Vec<u8>,
    esc: Esc,
}

impl PlainDecoder {
    fn strip(&mut self, s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match self.esc {
                Esc::None if c == '\u{1b}' => self.esc = Esc::Start,
                Esc::None => out.push(c),
                Esc::Start => self.esc = if c == '[' { Esc::Csi } else { Esc::None },
                Esc::Csi => {
                    if ('@'..='~').contains(&c) {
                        self.esc = Esc::None;
                    }
                }
            }
        }
        out
    }

    fn emit(&mut self, text: &str) -> Vec<StreamEvent> {
        let cleaned = self.strip(text);
        if cleaned.is_empty() {
            Vec::new()
        } else {
            vec![StreamEvent::TextDelta(cleaned)]
        }
    }
}

impl StreamDecoder for PlainDecoder {
    fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        self.carry.extend_from_slice(chunk);
        let valid = match std::str::from_utf8(&self.carry) {
            Ok(s) => s.len(),
            Err(e) if e.error_len().is_some() => {
                let text = String::from_utf8_lossy(&self.carry).into_owned();
                self.carry.clear();
                return self.emit(&text);
            }
            Err(e) => e.valid_up_to(),
        };
        if valid == 0 {
            return Vec::new();
        }
        let bytes: Vec<u8> = self.carry.drain(..valid).collect();
        let text = String::from_utf8(bytes).expect("prefix validated as UTF-8");
        self.emit(&text)
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        if self.carry.is_empty() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&self.carry).into_owned();
        self.carry.clear();
        self.emit(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{StreamDecoder, StreamEvent};

    fn text(events: Vec<StreamEvent>) -> String {
        events
            .into_iter()
            .map(|e| match e {
                StreamEvent::TextDelta(t) => t,
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    #[test]
    fn plain_decoder_joins_split_utf8() {
        let bytes = "héllo 🌍".as_bytes();
        let mut d = PlainDecoder::default();
        let mut out = String::new();
        for b in bytes {
            out.push_str(&text(d.feed(std::slice::from_ref(b))));
        }
        out.push_str(&text(d.finish()));
        assert_eq!(out, "héllo 🌍");
        assert!(!out.contains('\u{FFFD}'));
    }

    #[test]
    fn plain_decoder_strips_ansi_csi_across_chunks() {
        let mut d = PlainDecoder::default();
        let mut out = text(d.feed(b"a\x1b[3"));
        out.push_str(&text(d.feed(b"2mb\x1b[?25lc")));
        out.push_str(&text(d.finish()));
        assert_eq!(out, "abc");
    }

    #[test]
    fn plain_decoder_replaces_invalid_bytes_instead_of_stalling() {
        let mut d = PlainDecoder::default();
        let mut out = text(d.feed(b"ok\xffok"));
        out.push_str(&text(d.finish()));
        assert_eq!(out, "ok\u{FFFD}ok");
    }
}
