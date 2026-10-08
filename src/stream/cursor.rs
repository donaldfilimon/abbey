//! `cursor-agent -p --output-format stream-json --stream-partial-output`.

use super::ndjson::{LineBuffer, clip};
use super::{StreamDecoder, StreamEvent};
use serde_json::Value;

#[derive(Default)]
pub(crate) struct CursorDecoder {
    lines: LineBuffer,
    /// Text streamed so far this turn; a final full message equal to it is a repeat.
    text: String,
    unknown: u64,
}

fn tool_entry(v: &Value) -> Option<(String, &Value)> {
    let (key, body) = v["tool_call"].as_object()?.iter().next()?;
    let name = key.strip_suffix("ToolCall").unwrap_or(key).to_string();
    Some((name, body))
}

impl CursorDecoder {
    fn decode_line(&mut self, line: &str, out: &mut Vec<StreamEvent>) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            out.push(StreamEvent::Notice(format!(
                "cursor: unparsed line: {}",
                clip(line, 160)
            )));
            return;
        };
        match v["type"].as_str() {
            Some("system") => {
                if let Some(id) = v["session_id"].as_str() {
                    out.push(StreamEvent::SessionId(id.to_string()));
                }
            }
            Some("assistant") => {
                for block in v["message"]["content"].as_array().into_iter().flatten() {
                    let Some(t) = block["text"].as_str() else {
                        continue;
                    };
                    if !self.text.is_empty() && t == self.text {
                        continue;
                    }
                    self.text.push_str(t);
                    out.push(StreamEvent::TextDelta(t.to_string()));
                }
            }
            Some("thinking") => {
                if let Some(t) = v["text"].as_str() {
                    out.push(StreamEvent::ThinkingDelta(t.to_string()));
                }
            }
            Some("tool_call") => {
                let id = v["call_id"].as_str().unwrap_or_default().to_string();
                let Some((name, body)) = tool_entry(&v) else {
                    self.unknown += 1;
                    return;
                };
                match v["subtype"].as_str() {
                    Some("started") => out.push(StreamEvent::ToolStart {
                        id,
                        name,
                        input: body["args"].clone(),
                    }),
                    Some("completed") => {
                        let result = &body["result"];
                        let ok = result.get("success").is_some();
                        let detail = if ok {
                            &result["success"]
                        } else {
                            &result["error"]
                        };
                        let output = detail
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| clip(&detail.to_string(), 400));
                        out.push(StreamEvent::ToolEnd { id, ok, output });
                    }
                    _ => self.unknown += 1,
                }
            }
            Some("result") => {
                if v["is_error"].as_bool() == Some(true) {
                    let msg = v["result"].as_str().unwrap_or("error");
                    out.push(StreamEvent::Failed(msg.to_string()));
                }
            }
            Some("user") => {}
            _ => self.unknown += 1,
        }
    }
}

impl StreamDecoder for CursorDecoder {
    fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        let (lines, overflow) = self.lines.push(chunk);
        let mut out = Vec::new();
        if overflow {
            out.push(StreamEvent::Notice(
                "cursor: dropped an over-long stream line".into(),
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

    fn decode(s: &str) -> Vec<StreamEvent> {
        let mut d = CursorDecoder::default();
        let mut out = d.feed(s.as_bytes());
        out.extend(d.finish());
        out
    }

    #[test]
    fn tool_call_name_comes_from_the_single_key() {
        let s = concat!(
            r#"{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"readToolCall":{"args":{"path":"a.rs"}}}}"#,
            "\n",
            r#"{"type":"tool_call","subtype":"completed","call_id":"c1","tool_call":{"readToolCall":{"args":{"path":"a.rs"},"result":{"success":{"content":"x"}}}}}"#,
            "\n",
        );
        let ev = decode(s);
        assert_eq!(
            ev[0],
            StreamEvent::ToolStart {
                id: "c1".into(),
                name: "read".into(),
                input: serde_json::json!({"path": "a.rs"})
            }
        );
        assert!(matches!(&ev[1], StreamEvent::ToolEnd { id, ok: true, .. } if id == "c1"));
    }

    #[test]
    fn a_repeated_full_message_after_deltas_is_skipped() {
        let s = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"do"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ne"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"done"}]}}"#,
            "\n",
        );
        assert_eq!(
            decode(s),
            vec![
                StreamEvent::TextDelta("do".into()),
                StreamEvent::TextDelta("ne".into())
            ]
        );
    }

    #[test]
    fn error_tool_result_is_not_ok() {
        let s = r#"{"type":"tool_call","subtype":"completed","call_id":"c2","tool_call":{"shellToolCall":{"result":{"error":{"message":"denied"}}}}}"#;
        assert!(matches!(
            decode(&format!("{s}\n")).as_slice(),
            [StreamEvent::ToolEnd { ok: false, .. }]
        ));
    }

    #[test]
    fn live_fixture_text_matches_the_result_field() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/streams/cursor-live.ndjson"
        );
        let Ok(raw) = std::fs::read_to_string(path) else {
            return;
        };
        let mut d = CursorDecoder::default();
        let mut events = Vec::new();
        for chunk in raw.as_bytes().chunks(5) {
            events.extend(d.feed(chunk));
        }
        events.extend(d.finish());
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        let result = raw
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["type"] == "result")
            .and_then(|v| v["result"].as_str().map(str::to_string));
        if let Some(result) = result {
            assert_eq!(text.trim(), result.trim());
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ToolStart { .. }))
        );
    }
}
