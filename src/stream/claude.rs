//! `claude -p --output-format stream-json --verbose --include-partial-messages`.

use super::ndjson::{LineBuffer, clip};
use super::{StreamDecoder, StreamEvent};
use serde_json::Value;

#[derive(Default)]
pub(crate) struct ClaudeDecoder {
    lines: LineBuffer,
    /// Text deltas already streamed for the current message; the final
    /// `assistant` message repeats them and must be skipped.
    streamed_text: bool,
    unknown: u64,
}

pub(crate) fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

impl ClaudeDecoder {
    fn decode_line(&mut self, line: &str, out: &mut Vec<StreamEvent>) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            out.push(StreamEvent::Notice(format!(
                "claude: unparsed line: {}",
                clip(line, 160)
            )));
            return;
        };
        match v["type"].as_str() {
            Some("system") => {
                if v["subtype"] == "init"
                    && let Some(id) = v["session_id"].as_str()
                {
                    out.push(StreamEvent::SessionId(id.to_string()));
                }
            }
            Some("stream_event") => {
                let ev = &v["event"];
                match ev["type"].as_str() {
                    Some("message_start") => self.streamed_text = false,
                    Some("content_block_delta") => match ev["delta"]["type"].as_str() {
                        Some("text_delta") => {
                            self.streamed_text = true;
                            let t = ev["delta"]["text"].as_str().unwrap_or_default();
                            out.push(StreamEvent::TextDelta(t.to_string()));
                        }
                        Some("thinking_delta") => {
                            let t = ev["delta"]["thinking"].as_str().unwrap_or_default();
                            out.push(StreamEvent::ThinkingDelta(t.to_string()));
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            Some("assistant") => {
                for block in v["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") if !self.streamed_text => {
                            let t = block["text"].as_str().unwrap_or_default();
                            out.push(StreamEvent::TextDelta(t.to_string()));
                        }
                        Some("tool_use") => out.push(StreamEvent::ToolStart {
                            id: block["id"].as_str().unwrap_or_default().to_string(),
                            name: block["name"].as_str().unwrap_or("tool").to_string(),
                            input: block["input"].clone(),
                        }),
                        _ => {}
                    }
                }
                self.streamed_text = false;
            }
            Some("user") => {
                for block in v["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_result" {
                        out.push(StreamEvent::ToolEnd {
                            id: block["tool_use_id"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            ok: !block["is_error"].as_bool().unwrap_or(false),
                            output: tool_result_text(&block["content"]),
                        });
                    }
                }
            }
            Some("result") => {
                if let (Some(i), Some(o)) = (
                    v["usage"]["input_tokens"].as_u64(),
                    v["usage"]["output_tokens"].as_u64(),
                ) {
                    out.push(StreamEvent::Usage {
                        input_tokens: i,
                        output_tokens: o,
                    });
                }
                if v["is_error"].as_bool() == Some(true) {
                    let msg = v["result"]
                        .as_str()
                        .or(v["subtype"].as_str())
                        .unwrap_or("error");
                    out.push(StreamEvent::Failed(msg.to_string()));
                }
            }
            _ => self.unknown += 1,
        }
    }
}

impl StreamDecoder for ClaudeDecoder {
    fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        let (lines, overflow) = self.lines.push(chunk);
        let mut out = Vec::new();
        if overflow {
            out.push(StreamEvent::Notice(
                "claude: dropped an over-long stream line".into(),
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

    fn decode(all: &str) -> Vec<StreamEvent> {
        let mut d = ClaudeDecoder::default();
        let mut out = d.feed(all.as_bytes());
        out.extend(d.finish());
        out
    }

    #[test]
    fn partial_text_is_not_duplicated_by_the_final_assistant_message() {
        let s = concat!(
            r#"{"type":"system","subtype":"init","session_id":"s1"}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"message_start"}}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"he"}}}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"llo"}}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#,
            "\n",
            r#"{"type":"result","is_error":false,"usage":{"input_tokens":7,"output_tokens":2}}"#,
            "\n",
        );
        assert_eq!(
            decode(s),
            vec![
                StreamEvent::SessionId("s1".into()),
                StreamEvent::TextDelta("he".into()),
                StreamEvent::TextDelta("llo".into()),
                StreamEvent::Usage {
                    input_tokens: 7,
                    output_tokens: 2
                },
            ]
        );
    }

    #[test]
    fn tool_use_and_result_pair_by_id() {
        let s = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"a.rs"}}]}}"#,
            "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"fn a(){}"}],"is_error":false}]}}"#,
            "\n",
        );
        assert_eq!(
            decode(s),
            vec![
                StreamEvent::ToolStart {
                    id: "t1".into(),
                    name: "Read".into(),
                    input: serde_json::json!({"file_path": "a.rs"})
                },
                StreamEvent::ToolEnd {
                    id: "t1".into(),
                    ok: true,
                    output: "fn a(){}".into()
                },
            ]
        );
    }

    #[test]
    fn malformed_and_unknown_lines_never_panic() {
        let mut d = ClaudeDecoder::default();
        let out = d.feed(b"not json\n{\"type\":\"brand_new\"}\n");
        assert!(matches!(out.as_slice(), [StreamEvent::Notice(_)]));
        assert_eq!(d.unknown_events(), 1);
    }

    #[test]
    fn error_result_becomes_failed() {
        let s = r#"{"type":"result","is_error":true,"result":"quota exceeded"}"#;
        assert_eq!(
            decode(&format!("{s}\n")),
            vec![StreamEvent::Failed("quota exceeded".into())]
        );
    }

    #[test]
    fn live_fixture_decodes_text_and_a_read_tool() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/streams/claude-live.ndjson"
        );
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let mut d = ClaudeDecoder::default();
        let mut events = Vec::new();
        for chunk in bytes.chunks(7) {
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
        assert!(text.to_lowercase().contains("done"), "text was {text:?}");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ToolStart { name, .. } if name == "Read"))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::ToolEnd { .. }))
        );
        assert!(!events.iter().any(|e| matches!(e, StreamEvent::Notice(_))));
    }
}
