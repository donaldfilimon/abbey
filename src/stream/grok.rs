//! `grok --output-format streaming-json`: one ACP session update per line.

use super::ndjson::{LineBuffer, clip};
use super::{StreamDecoder, StreamEvent};
use serde_json::Value;

#[derive(Default)]
pub(crate) struct GrokDecoder {
    lines: LineBuffer,
    unknown: u64,
}

fn update(v: &Value) -> Option<&Value> {
    [v, &v["update"], &v["params"]["update"]]
        .into_iter()
        .find(|u| u.get("sessionUpdate").is_some())
}

impl GrokDecoder {
    fn decode_line(&mut self, line: &str, out: &mut Vec<StreamEvent>) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            out.push(StreamEvent::Notice(format!(
                "grok: unparsed line: {}",
                clip(line, 160)
            )));
            return;
        };
        let Some(u) = update(&v) else {
            self.unknown += 1;
            return;
        };
        let text = || {
            u["content"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        match u["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => out.push(StreamEvent::TextDelta(text())),
            Some("agent_thought_chunk") => out.push(StreamEvent::ThinkingDelta(text())),
            Some("tool_call") => out.push(StreamEvent::ToolStart {
                id: u["toolCallId"].as_str().unwrap_or_default().to_string(),
                name: u["title"]
                    .as_str()
                    .or(u["kind"].as_str())
                    .unwrap_or("tool")
                    .to_string(),
                input: u["rawInput"].clone(),
            }),
            Some("tool_call_update") => {
                if let Some(status @ ("completed" | "failed")) = u["status"].as_str() {
                    out.push(StreamEvent::ToolEnd {
                        id: u["toolCallId"].as_str().unwrap_or_default().to_string(),
                        ok: status == "completed",
                        output: clip(&u["content"].to_string(), 400),
                    });
                }
            }
            _ => self.unknown += 1,
        }
    }
}

impl StreamDecoder for GrokDecoder {
    fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        let (lines, overflow) = self.lines.push(chunk);
        let mut out = Vec::new();
        if overflow {
            out.push(StreamEvent::Notice(
                "grok: dropped an over-long stream line".into(),
            ));
        }
        for line in lines {
            self.decode_line(&line, &mut out);
        }
        out
    }

    fn finish(&mut self) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if let Some(line) = self.lines.finish() {
            self.decode_line(&line, &mut out);
        }
        out
    }

    fn unknown_events(&self) -> u64 {
        self.unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{StreamDecoder, StreamEvent};

    #[test]
    fn acp_updates_map_to_events_at_any_nesting() {
        let s = concat!(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}"#,
            "\n",
            r#"{"update":{"sessionUpdate":"tool_call","toolCallId":"g1","title":"Read note.txt","rawInput":{"path":"note.txt"}}}"#,
            "\n",
            r#"{"params":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"g1","status":"failed"}}}"#,
            "\n",
        );
        let mut d = GrokDecoder::default();
        let ev = d.feed(s.as_bytes());
        assert_eq!(ev[0], StreamEvent::TextDelta("hi".into()));
        assert!(
            matches!(&ev[1], StreamEvent::ToolStart { id, name, .. } if id == "g1" && name == "Read note.txt")
        );
        assert!(matches!(&ev[2], StreamEvent::ToolEnd { ok: false, .. }));
    }

    #[test]
    fn live_fixture_yields_text() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/streams/grok-live.ndjson"
        );
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let mut d = GrokDecoder::default();
        let mut ev = d.feed(&bytes);
        ev.extend(d.finish());
        assert!(ev.iter().any(|e| matches!(e, StreamEvent::TextDelta(_))));
    }
}
