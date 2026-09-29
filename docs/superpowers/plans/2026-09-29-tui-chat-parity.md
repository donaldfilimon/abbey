# Chat-first TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Abbey's hand-off TUI with a chat-first ratatui TUI that
streams executor output, shows tool activity inline, supports interrupt,
queueing, a multiline composer with `@file` and slash completion, and keeps
today's dashboards as overlays.

**Architecture:** Streaming is an output sink (`StreamTap`) carried on
`AgentConfig` through the one canonical path
(`run_agent → hybrid_run → run_resilient → run_once`). With a tap present,
`run_once` runs the executor under the Unix supervisor with a stdout chunk tap,
and a per-backend decoder (`src/stream/`) turns bytes into `StreamEvent`s sent
to the TUI's run thread. The TUI (`src/tui/`, rewritten) owns a `Transcript` of
cells rendered with markdown and syntect, a `Composer`, overlays, and a
keymap state machine.

**Tech Stack:** Rust 2024 on `nightly-2026-09-01`; ratatui 0.30, crossterm
0.29, syntect 5 (existing); `pulldown-cmark` (new); serde_json (existing).

**Spec:** `docs/superpowers/specs/2026-09-29-tui-chat-parity-design.md`

## Global Constraints

- One canonical execution path: streaming is `AgentConfig.stream: Option<StreamTap>`, never a second runner. `route.jsonl` rows, persona/role wrap, resume/retry stay in `hybrid_run`/`run_resilient`.
- With `stream == None` every existing CLI output is byte-identical (no new flags, no new stderr lines).
- Per-backend argv grammars stay isolated in `src/agent/argv.rs`; stream flags are appended only inside the owning backend's builder.
- `os_control` never executes without `--confirm` and the allowlist; the TUI `!cmd` runs `abbey os execute --confirm <cmd…>` only after an explicit dialog "y".
- Token usage is displayed only when the executor reported it; otherwise `n/a`.
- Per-call behaviour reads the live `cfg.backend`, never `AgentBackend::from_env()`.
- File-size guard: every `.rs` < 800 lines (targets: `src/stream/*` < 400, `src/tui/*` < 600); `main.rs` < 200.
- One new dependency: `pulldown-cmark`. No async runtime, no HTTP client.
- `unsafe_code` is denied: no `dup2`/fd tricks.
- Commit only this plan's paths (another session's distill work may still be uncommitted). Every commit message ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012fjNUQG7H8Ye9htCwDtCfh
  ```
- Never run `./check.sh` concurrently with `./zig/tools/check.sh`.

## Precondition

- [ ] Before Task 1, `git --no-optional-locks status --short` must show no modified `src/agent/`, `src/capture.rs`, `src/lib.rs`, `src/claims/`, `CLAUDE.md`, `AGENTS.md` from the in-flight `learn distill` work (it edits the same files). If they are still dirty, stop and ask Donald; do not commit or stash someone else's work.

## Review Focus

1. **Esc during a server-backend run** (claude/cursor/grok) must not trigger `run_resilient`'s "resume failed → new chat" retry: cancel returns exit 130 and the chat id is unchanged. Pinned in Task 7 (`cancelled_stream_run_does_not_retry_or_mint_a_chat`).
2. **Multi-byte UTF-8 split across pipe chunks** (emoji, CJK from ollama/fm) must decode without U+FFFD. Pinned in Task 2 (`plain_decoder_joins_split_utf8`) and Task 3 (`line_buffer_joins_split_utf8_inside_json`).
3. **Long agentic turns > 4 MiB of stream-json** must not fail with `StdoutLimit`. Pinned in Task 1 (`tapped_run_keeps_only_a_tail_and_never_overflows`).
4. **stderr noise and `eprintln!` from the run path while the alternate screen is up** must not corrupt the display: routed as `Notice` events. Pinned in Task 7 (`streaming_notices_go_to_the_tap_not_stderr`).
5. **Pasting a multi-line block into the composer** must insert it, not submit line by line. Pinned in Task 13 (`bracketed_paste_inserts_without_submitting`).

## File Structure

**Create**
- `src/stream/mod.rs` — `StreamDecoder` trait, `decoder_for`, `stream_output_format`.
- `src/stream/event.rs` — `StreamEvent`.
- `src/stream/tap.rs` — `StreamTap` (events sender + `CancellationToken`).
- `src/stream/ndjson.rs` — `LineBuffer`, `clip`.
- `src/stream/plain.rs` — UTF-8/ANSI-safe plain text decoder (ollama, fm, abi).
- `src/stream/claude.rs`, `src/stream/cursor.rs`, `src/stream/grok.rs` — NDJSON decoders.
- `src/agent/streaming.rs` — `run_once_streaming`.
- `src/tui/transcript.rs` — cells, event application, tool summary/diff/todo views.
- `src/tui/markdown.rs` — pulldown-cmark → wrapped ratatui lines.
- `src/tui/composer.rs` — multiline buffer, history persistence, reverse search.
- `src/tui/completion.rs` — `@file` and slash completion.
- `src/tui/worker.rs` — run thread (`spawn_run`) and child capture (`run_abbey_capture`).
- `src/tui/permission.rs` — permission-mode cycling per backend.
- `src/tui/keymap.rs` — key/paste/mouse state machine.
- `src/tui/render.rs` — layout drawing.
- `src/tui/overlays.rs` — palette, help, pickers, panels, confirm dialog.
- `src/tui/tests.rs` — TestBackend render snapshots + keymap tests.
- `tests/fixtures/streams/{claude,cursor,grok,ollama}-live.ndjson` (ollama: `.txt`) — dated live captures.

**Modify**
- `src/runtime/supervisor.rs`, `src/runtime/supervisor/unix.rs`, `src/runtime/supervisor/tests.rs` — stdout tap.
- `src/agent/mod.rs` — `stream`/`permission_mode` fields, `notice`, `run_once` dispatch, cancel-aware retry.
- `src/agent/argv.rs`, `src/agent/argv/tests.rs` — stream flags.
- `src/actions.rs` — `RunSpec.stream`, `RunSpec::streaming`.
- `src/session.rs` — `eprintln!` in `hybrid_run` path → `cfg.notice`.
- `src/highlight.rs` — `code_lines`.
- `src/tui/mod.rs`, `src/tui/app.rs` — rewritten; `src/tui/refresh.rs` kept (uses `state, cfg, doctor_lines, history, memory_lines, persona_lines, route_lines, skill_lines`).
- `src/lib.rs`, `Cargo.toml`, `src/claims/registry.rs`, `docs/claims.md`, `AGENTS.md`, `CLAUDE.md`, `docs/architecture.md`, `tests/cli_surface.rs`.

**Delete** (Task 15): `src/tui/tabs.rs`, `src/tui/ui.rs`, `src/tui/keys.rs`, `src/tui/keys_tests.rs`, `src/tui/overlay.rs`.

---

### Task 1: Supervisor stdout tap with tail retention

**Files:**
- Modify: `src/runtime/supervisor.rs` (the two `run_with_checkpoint` fns near line 314)
- Modify: `src/runtime/supervisor/unix.rs` (`run_with_checkpoint` ~189, `spawn_reader` ~315, `read_bounded` ~330)
- Test: `src/runtime/supervisor/tests.rs`

**Interfaces:**
- Produces: `crate::runtime::supervisor::run_tapped(spec: &ProcessSpec, limits: &SupervisorLimits, checkpoint: impl FnMut() -> bool, stdout_tap: std::sync::mpsc::Sender<Vec<u8>>) -> Result<SupervisorOutcome, SupervisorError>` (unix only). In tap mode stdout keeps the last `limits.stdout_bytes` bytes and never reports `StdoutLimit`; every chunk read is sent to `stdout_tap` in order; the sender is dropped when the stdout reader finishes.

- [ ] **Step 1: Write the failing tests** — append to `src/runtime/supervisor/tests.rs`:

```rust
#[cfg(unix)]
mod tap {
    use super::super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn sh(script: &str) -> ProcessSpec {
        ProcessSpec::inherited(
            std::path::PathBuf::from("/bin/sh"),
            vec!["-c".into(), script.into()],
        )
    }

    fn limits(stdout_bytes: usize) -> SupervisorLimits {
        SupervisorLimits {
            timeout: Duration::from_secs(10),
            terminate_grace: Duration::from_millis(200),
            stdout_bytes,
            stderr_bytes: 4096,
            poll_interval: Duration::from_millis(5),
        }
    }

    #[test]
    fn tapped_run_streams_every_chunk_in_order() {
        let (tx, rx) = mpsc::channel();
        let outcome = run_tapped(&sh("printf abc; sleep 0.05; printf def"), &limits(4096), || false, tx)
            .expect("tapped run");
        let streamed: Vec<u8> = rx.iter().flatten().collect();
        assert_eq!(streamed, b"abcdef");
        match outcome {
            SupervisorOutcome::Exited { status, stdout, .. } => {
                assert!(status.success());
                assert_eq!(stdout, b"abcdef");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn tapped_run_keeps_only_a_tail_and_never_overflows() {
        let (tx, rx) = mpsc::channel();
        let outcome = run_tapped(&sh("printf abcdefgh"), &limits(4), || false, tx).expect("run");
        assert_eq!(rx.iter().flatten().collect::<Vec<u8>>(), b"abcdefgh");
        match outcome {
            SupervisorOutcome::Exited { stdout, .. } => assert_eq!(stdout, b"efgh"),
            other => panic!("expected Exited with tail, got {other:?}"),
        }
    }

    #[test]
    fn tapped_run_cancels_promptly() {
        let (tx, _rx) = mpsc::channel();
        let started = Instant::now();
        let outcome = run_tapped(
            &sh("printf x; sleep 30"),
            &limits(4096),
            move || started.elapsed() > Duration::from_millis(100),
            tx,
        )
        .expect("run");
        assert!(matches!(outcome, SupervisorOutcome::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib runtime::supervisor::tests::tap`
Expected: FAIL — `cannot find function run_tapped`.

- [ ] **Step 3: Implement.** In `src/runtime/supervisor/unix.rs`:

  - Change the signature `pub(super) fn run_with_checkpoint(spec, limits, mut checkpoint)` to add a fourth parameter `stdout_tap: Option<Sender<Vec<u8>>>`.
  - Pass the new parameter into the stdout reader: `let stdout_reader = spawn_reader(stdout, StreamName::Stdout, limits.stdout_bytes, reader_tx.clone(), stdout_tap)?;`.
  - The stderr reader gets `None`.
  - Replace `spawn_reader` and `read_bounded` with:

```rust
fn spawn_reader<R: Read + Send + 'static>(
    reader: R,
    name: StreamName,
    cap: usize,
    sender: Sender<ReaderMessage>,
    tap: Option<Sender<Vec<u8>>>,
) -> Result<JoinHandle<()>, SupervisorError> {
    thread::Builder::new()
        .name(format!("abbey-supervisor-{name}"))
        .spawn(move || {
            let result = match tap {
                Some(tap) => read_tapped(reader, name, cap, &tap),
                None => read_bounded(reader, name, cap),
            };
            let _ = sender.send(ReaderMessage { name, result });
        })
        .map_err(SupervisorError::Spawn)
}

/// Stream every chunk to `tap` and retain only the last `cap` bytes. A tapped
/// stream is consumed live, so its retained copy is a diagnostic tail and can
/// never overflow.
fn read_tapped<R: Read>(
    mut reader: R,
    name: StreamName,
    cap: usize,
    tap: &Sender<Vec<u8>>,
) -> std::io::Result<CapturedStream> {
    let mut tail: Vec<u8> = Vec::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let _ = tap.send(buffer[..read].to_vec());
        tail.extend_from_slice(&buffer[..read]);
        if tail.len() > cap {
            let excess = tail.len() - cap;
            tail.drain(..excess);
        }
    }
    Ok(CapturedStream {
        name,
        bytes: tail,
        overflowed: false,
    })
}
```

  Keep `read_bounded` unchanged, and update the stderr `spawn_reader` call to pass `None`. In `src/runtime/supervisor.rs`:

  - Change the unix `run_with_checkpoint` body to `unix::run_with_checkpoint(spec, *limits, checkpoint, None)`.
  - Add:

```rust
#[cfg(unix)]
pub(crate) fn run_tapped(
    spec: &ProcessSpec,
    limits: &SupervisorLimits,
    checkpoint: impl FnMut() -> bool,
    stdout_tap: std::sync::mpsc::Sender<Vec<u8>>,
) -> Result<SupervisorOutcome, SupervisorError> {
    unix::run_with_checkpoint(spec, *limits, checkpoint, Some(stdout_tap))
}
```

  Run `grep -rn 'unix::run_with_checkpoint' src/` and update any other caller to pass `None`.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib runtime::supervisor`
Expected: all supervisor tests PASS, including the 3 new ones.

- [ ] **Step 5: Commit**

```bash
git add src/runtime/supervisor.rs src/runtime/supervisor/unix.rs src/runtime/supervisor/tests.rs
git commit -m "feat(supervisor): stdout chunk tap with bounded tail retention"
```

---

### Task 2: Stream events, decoder trait, plain decoder

**Files:**
- Create: `src/stream/mod.rs`, `src/stream/event.rs`, `src/stream/plain.rs`
- Modify: `src/lib.rs` (add `mod stream;` in alphabetical position after `mod state;`)

**Interfaces:**
- Produces:
  - `crate::stream::StreamEvent` (below).
  - `trait StreamDecoder: Send { fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent>; fn finish(&mut self) -> Vec<StreamEvent>; fn unknown_events(&self) -> u64 { 0 } }`.
  - `crate::stream::plain::PlainDecoder: Default + StreamDecoder`.

- [ ] **Step 1: Write the event and trait**

`src/stream/event.rs`:

```rust
//! Typed events decoded from an executor's live output stream.

/// One decoded unit of executor output, in arrival order.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolEnd {
        id: String,
        ok: bool,
        output: String,
    },
    /// Only emitted when the executor itself reported token counts.
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    SessionId(String),
    Notice(String),
    /// Exit code of the executor; 130 means the user interrupted the turn.
    Done {
        exit: i32,
    },
    Failed(String),
}
```

`src/stream/mod.rs`:

```rust
//! Executor stream decoding for the chat TUI. Each backend's native stream
//! (NDJSON for claude / cursor-agent / grok, plain text for ollama / fm / abi)
//! becomes ordered [`StreamEvent`]s. Decoders never panic on bad input.

mod event;
pub(crate) mod plain;

pub use event::StreamEvent;

/// Incremental decoder fed raw stdout chunks in arrival order.
pub trait StreamDecoder: Send {
    fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent>;
    fn finish(&mut self) -> Vec<StreamEvent>;
    /// Well-formed events of a type this decoder does not model.
    fn unknown_events(&self) -> u64 {
        0
    }
}
```

- [ ] **Step 2: Write the failing plain-decoder tests** — `src/stream/plain.rs` (tests first, struct stub `pub(crate) struct PlainDecoder;` so it compiles to failure on missing methods):

```rust
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
```

- [ ] **Step 3: Run to confirm failure**

Run: `cargo test --lib stream::plain`
Expected: FAIL (no `feed`/`finish` on `PlainDecoder`).

- [ ] **Step 4: Implement `PlainDecoder`** (above the tests):

```rust
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
```

Add `mod stream;` to `src/lib.rs` after `mod state;`.

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --lib stream::plain`
Expected: 3 PASS.

- [ ] **Step 6: Commit**

```bash
git add src/stream/mod.rs src/stream/event.rs src/stream/plain.rs src/lib.rs
git commit -m "feat(stream): StreamEvent, decoder trait, UTF-8/ANSI-safe plain decoder"
```

---

### Task 3: Live fixtures, NDJSON line buffer, claude decoder

**Files:**
- Create: `tests/fixtures/streams/claude-live.ndjson`, `cursor-live.ndjson`, `grok-live.ndjson`, `ollama-live.txt`, `tests/fixtures/streams/README.md`
- Create: `src/stream/ndjson.rs`, `src/stream/claude.rs`
- Modify: `src/stream/mod.rs`

**Interfaces:**
- Produces:
  - `crate::stream::ndjson::LineBuffer` with `fn push(&mut self, chunk: &[u8]) -> (Vec<String>, bool)`, where the bool means an over-long line was dropped, and `fn finish(&mut self) -> Option<String>`.
  - `crate::stream::ndjson::clip(s: &str, max_chars: usize) -> String`.
  - `crate::stream::claude::ClaudeDecoder: Default + StreamDecoder`.

- [ ] **Step 1: Record the live fixtures** (the executors are on this machine; each prompt is tiny and uses one harmless read tool). Run from a scratch directory so no repo file is touched:

```bash
mkdir -p /private/tmp/abbey-stream-fixtures && cd /private/tmp/abbey-stream-fixtures && printf 'hello\n' > note.txt
F=/Users/donaldfilimon/dev/active/abbey/tests/fixtures/streams; mkdir -p "$F"
claude -p --output-format stream-json --verbose --include-partial-messages --permission-mode acceptEdits \
  "Read note.txt with your Read tool, then reply with exactly: done" >| "$F/claude-live.ndjson"; echo EXIT:$?
cursor-agent -p --output-format stream-json --stream-partial-output --force \
  "Read note.txt, then reply with exactly: done" >| "$F/cursor-live.ndjson"; echo EXIT:$?
grok -p --output-format streaming-json "Read note.txt, then reply with exactly: done" >| "$F/grok-live.ndjson"; echo EXIT:$?
ollama run --nowordwrap gemma4:26b-mlx -- "Reply with exactly: done 🌍" >| "$F/ollama-live.txt"; echo EXIT:$?
```

Expected: `EXIT:0` for each installed executor.
- If `grok -p` is rejected, run `grok --help | grep -n -- '-p\|--print\|headless'` and use the headless flag it names.
- If an executor fails (auth, quota), keep going without that fixture, record the reason in `README.md`, and have that decoder's live test skip when the fixture is missing (`if !path.exists() { return; }`).

Scrub any absolute home paths and session tokens: `sed -i '' 's#/Users/[^"/]*#/Users/USER#g' "$F"/*`. Then write `README.md`:

```markdown
# Executor stream fixtures

Live captures recorded 2026-09-29 on macOS from the installed executors, via the
commands in docs/superpowers/plans/2026-09-29-tui-chat-parity.md Task 3 Step 1.
Home paths are scrubbed. These pin the observed wire shapes; vendor formats may change.
```

- [ ] **Step 2: Inspect the claude shape** — `head -c 3000 tests/fixtures/streams/claude-live.ndjson` and `grep -o '"type":"[a-z_]*"' tests/fixtures/streams/claude-live.ndjson | sort | uniq -c`. The decoder below expects:
  - top-level `type` ∈ `system` (with `subtype: init`, `session_id`), `stream_event` (with `event.type`: `content_block_delta` → `delta.type` `text_delta.text` / `thinking_delta.thinking`; `message_start`), `assistant` (`message.content[]` of `text` / `tool_use{id,name,input}`), `user` (`message.content[]` of `tool_result{tool_use_id,content,is_error}`), and `result` (`usage.input_tokens`, `usage.output_tokens`, `is_error`, `result`).
  - If the fixture differs, change the decoder to the fixture's field names. Never edit the fixture to fit the decoder.

- [ ] **Step 3: Write the failing tests** — `src/stream/ndjson.rs` tests and `src/stream/claude.rs` tests:

```rust
// src/stream/ndjson.rs — tests
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
```

```rust
// src/stream/claude.rs — tests
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
            r#"{"type":"system","subtype":"init","session_id":"s1"}"#, "\n",
            r#"{"type":"stream_event","event":{"type":"message_start"}}"#, "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"he"}}}"#, "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"llo"}}}"#, "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#, "\n",
            r#"{"type":"result","is_error":false,"usage":{"input_tokens":7,"output_tokens":2}}"#, "\n",
        );
        assert_eq!(
            decode(s),
            vec![
                StreamEvent::SessionId("s1".into()),
                StreamEvent::TextDelta("he".into()),
                StreamEvent::TextDelta("llo".into()),
                StreamEvent::Usage { input_tokens: 7, output_tokens: 2 },
            ]
        );
    }

    #[test]
    fn tool_use_and_result_pair_by_id() {
        let s = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"a.rs"}}]}}"#, "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"fn a(){}"}],"is_error":false}]}}"#, "\n",
        );
        assert_eq!(
            decode(s),
            vec![
                StreamEvent::ToolStart { id: "t1".into(), name: "Read".into(), input: serde_json::json!({"file_path":"a.rs"}) },
                StreamEvent::ToolEnd { id: "t1".into(), ok: true, output: "fn a(){}".into() },
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
        assert_eq!(decode(&format!("{s}\n")), vec![StreamEvent::Failed("quota exceeded".into())]);
    }

    #[test]
    fn live_fixture_decodes_text_and_a_read_tool() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/streams/claude-live.ndjson");
        let Ok(bytes) = std::fs::read(path) else { return };
        let mut d = ClaudeDecoder::default();
        let mut events = Vec::new();
        for chunk in bytes.chunks(7) {
            events.extend(d.feed(chunk));
        }
        events.extend(d.finish());
        let text: String = events.iter().filter_map(|e| match e { StreamEvent::TextDelta(t) => Some(t.as_str()), _ => None }).collect();
        assert!(text.to_lowercase().contains("done"), "text was {text:?}");
        assert!(events.iter().any(|e| matches!(e, StreamEvent::ToolStart { name, .. } if name == "Read")));
        assert!(events.iter().any(|e| matches!(e, StreamEvent::ToolEnd { .. })));
        assert!(!events.iter().any(|e| matches!(e, StreamEvent::Notice(_))));
    }
}
```

- [ ] **Step 4: Run to confirm failure**

Run: `cargo test --lib stream::`
Expected: FAIL (missing `LineBuffer`, `ClaudeDecoder`).

- [ ] **Step 5: Implement.** `src/stream/ndjson.rs`:

```rust
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
```

`src/stream/claude.rs` (above its tests):

```rust
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
            out.push(StreamEvent::Notice(format!("claude: unparsed line: {}", clip(line, 160))));
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
                            id: block["tool_use_id"].as_str().unwrap_or_default().to_string(),
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
                    out.push(StreamEvent::Usage { input_tokens: i, output_tokens: o });
                }
                if v["is_error"].as_bool() == Some(true) {
                    let msg = v["result"].as_str().or(v["subtype"].as_str()).unwrap_or("error");
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
            out.push(StreamEvent::Notice("claude: dropped an over-long stream line".into()));
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
```

In `src/stream/mod.rs` add `pub(crate) mod claude;` and `pub(crate) mod ndjson;`.

- [ ] **Step 6: Run the tests to confirm they pass**

Run: `cargo test --lib stream::`
Expected: all PASS. If `live_fixture_decodes_text_and_a_read_tool` fails, adjust the decoder field names to the fixture (Step 2 rule) and rerun.

- [ ] **Step 7: Commit**

```bash
git add tests/fixtures/streams src/stream/ndjson.rs src/stream/claude.rs src/stream/mod.rs
git commit -m "feat(stream): NDJSON framing, claude stream-json decoder, live fixtures"
```

---

### Task 4: cursor-agent decoder

**Files:**
- Create: `src/stream/cursor.rs`
- Modify: `src/stream/mod.rs` (`pub(crate) mod cursor;`)

**Interfaces:**
- Consumes: `LineBuffer`, `clip` (Task 3).
- Produces: `crate::stream::cursor::CursorDecoder: Default + StreamDecoder`.

- [ ] **Step 1: Inspect the fixture** — `grep -o '"type":"[a-z_]*"\|"subtype":"[a-z_]*"' tests/fixtures/streams/cursor-live.ndjson | sort | uniq -c` and `grep -m2 tool_call tests/fixtures/streams/cursor-live.ndjson | head -c 1500`. The decoder below expects:
  - `system` / `init` with `session_id`;
  - `assistant` with `message.content[].text`, where partial output makes each event a delta;
  - `thinking` with `text`;
  - `tool_call` with `subtype` `started` / `completed`, `call_id`, and `tool_call: { "<name>ToolCall": { "args": {…}, "result": { "success" | "error": … } } }`;
  - `result` with `is_error` and `result`.

  Adjust to the fixture if it differs.

- [ ] **Step 2: Write the failing tests** (`src/stream/cursor.rs`):

```rust
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
            r#"{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"readToolCall":{"args":{"path":"a.rs"}}}}"#, "\n",
            r#"{"type":"tool_call","subtype":"completed","call_id":"c1","tool_call":{"readToolCall":{"args":{"path":"a.rs"},"result":{"success":{"content":"x"}}}}}"#, "\n",
        );
        let ev = decode(s);
        assert_eq!(ev[0], StreamEvent::ToolStart { id: "c1".into(), name: "read".into(), input: serde_json::json!({"path":"a.rs"}) });
        assert!(matches!(&ev[1], StreamEvent::ToolEnd { id, ok: true, .. } if id == "c1"));
    }

    #[test]
    fn a_repeated_full_message_after_deltas_is_skipped() {
        let s = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"do"}]}}"#, "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ne"}]}}"#, "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"done"}]}}"#, "\n",
        );
        assert_eq!(decode(s), vec![StreamEvent::TextDelta("do".into()), StreamEvent::TextDelta("ne".into())]);
    }

    #[test]
    fn error_tool_result_is_not_ok() {
        let s = r#"{"type":"tool_call","subtype":"completed","call_id":"c2","tool_call":{"shellToolCall":{"result":{"error":{"message":"denied"}}}}}"#;
        assert!(matches!(decode(&format!("{s}\n")).as_slice(), [StreamEvent::ToolEnd { ok: false, .. }]));
    }

    #[test]
    fn live_fixture_text_matches_the_result_field() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/streams/cursor-live.ndjson");
        let Ok(raw) = std::fs::read_to_string(path) else { return };
        let mut d = CursorDecoder::default();
        let mut events = Vec::new();
        for chunk in raw.as_bytes().chunks(5) {
            events.extend(d.feed(chunk));
        }
        events.extend(d.finish());
        let text: String = events.iter().filter_map(|e| match e { StreamEvent::TextDelta(t) => Some(t.as_str()), _ => None }).collect();
        let result = raw.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["type"] == "result").and_then(|v| v["result"].as_str().map(str::to_string));
        if let Some(result) = result {
            assert_eq!(text.trim(), result.trim());
        }
        assert!(events.iter().any(|e| matches!(e, StreamEvent::ToolStart { .. })));
    }
}
```

- [ ] **Step 3: Run to confirm failure**

Run: `cargo test --lib stream::cursor`
Expected: FAIL (missing `CursorDecoder`).

- [ ] **Step 4: Implement** (above the tests):

```rust
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
            out.push(StreamEvent::Notice(format!("cursor: unparsed line: {}", clip(line, 160))));
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
                    let Some(t) = block["text"].as_str() else { continue };
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
                    Some("started") => out.push(StreamEvent::ToolStart { id, name, input: body["args"].clone() }),
                    Some("completed") => {
                        let result = &body["result"];
                        let ok = result.get("success").is_some();
                        let detail = if ok { &result["success"] } else { &result["error"] };
                        let output = detail.as_str().map(str::to_string).unwrap_or_else(|| clip(&detail.to_string(), 400));
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
            out.push(StreamEvent::Notice("cursor: dropped an over-long stream line".into()));
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
```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --lib stream::cursor`
Expected: PASS (adjust per fixture if the live test fails; never edit the fixture).

- [ ] **Step 6: Commit**

```bash
git add src/stream/cursor.rs src/stream/mod.rs
git commit -m "feat(stream): cursor-agent stream-json decoder"
```

---

### Task 5: grok decoder and `decoder_for`

**Files:**
- Create: `src/stream/grok.rs`
- Modify: `src/stream/mod.rs`

**Interfaces:**
- Produces:
  - `crate::stream::grok::GrokDecoder`.
  - `crate::stream::decoder_for(backend: crate::agent::AgentBackend) -> Box<dyn StreamDecoder>`.
  - `crate::stream::stream_output_format(backend: AgentBackend) -> Option<&'static str>`: `Claude`/`Cursor` → `"stream-json"`, `Grok` → `"streaming-json"`, others `None`.

- [ ] **Step 1: Inspect the fixture** — `grep -o '"sessionUpdate":"[a-z_]*"' tests/fixtures/streams/grok-live.ndjson | sort | uniq -c` and `head -c 1500 tests/fixtures/streams/grok-live.ndjson`. The decoder below finds the update object at the top level, under `update`, or under `params.update`. It maps:
  - `agent_message_chunk.content.text` → `TextDelta`;
  - `agent_thought_chunk.content.text` → `ThinkingDelta`;
  - `tool_call` with `toolCallId`, `title` (or `kind`) and `rawInput` → `ToolStart`;
  - `tool_call_update` whose `status` is `completed` or `failed` → `ToolEnd`.

  Adjust to the fixture if it differs.

- [ ] **Step 2: Write the failing tests** (`src/stream/grok.rs` and a mod.rs test):

```rust
// src/stream/grok.rs — tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{StreamDecoder, StreamEvent};

    #[test]
    fn acp_updates_map_to_events_at_any_nesting() {
        let s = concat!(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}"#, "\n",
            r#"{"update":{"sessionUpdate":"tool_call","toolCallId":"g1","title":"Read note.txt","rawInput":{"path":"note.txt"}}}"#, "\n",
            r#"{"params":{"update":{"sessionUpdate":"tool_call_update","toolCallId":"g1","status":"failed"}}}"#, "\n",
        );
        let mut d = GrokDecoder::default();
        let ev = d.feed(s.as_bytes());
        assert_eq!(ev[0], StreamEvent::TextDelta("hi".into()));
        assert!(matches!(&ev[1], StreamEvent::ToolStart { id, name, .. } if id == "g1" && name == "Read note.txt"));
        assert!(matches!(&ev[2], StreamEvent::ToolEnd { ok: false, .. }));
    }

    #[test]
    fn live_fixture_yields_text() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/streams/grok-live.ndjson");
        let Ok(bytes) = std::fs::read(path) else { return };
        let mut d = GrokDecoder::default();
        let mut ev = d.feed(&bytes);
        ev.extend(d.finish());
        assert!(ev.iter().any(|e| matches!(e, StreamEvent::TextDelta(_))));
    }
}
```

```rust
// src/stream/mod.rs — tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentBackend;

    #[test]
    fn every_backend_has_a_decoder_and_only_structured_ones_have_a_format() {
        for b in [AgentBackend::Cursor, AgentBackend::Grok, AgentBackend::Fm, AgentBackend::Abi, AgentBackend::Claude, AgentBackend::Ollama] {
            let mut d = decoder_for(b);
            let _ = d.feed(b"");
        }
        assert_eq!(stream_output_format(AgentBackend::Claude), Some("stream-json"));
        assert_eq!(stream_output_format(AgentBackend::Cursor), Some("stream-json"));
        assert_eq!(stream_output_format(AgentBackend::Grok), Some("streaming-json"));
        assert_eq!(stream_output_format(AgentBackend::Ollama), None);
        assert_eq!(stream_output_format(AgentBackend::Fm), None);
        assert_eq!(stream_output_format(AgentBackend::Abi), None);
    }
}
```

- [ ] **Step 3: Run to confirm failure**

Run: `cargo test --lib stream::`
Expected: FAIL (missing items).

- [ ] **Step 4: Implement** `src/stream/grok.rs` (above tests):

```rust
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
            out.push(StreamEvent::Notice(format!("grok: unparsed line: {}", clip(line, 160))));
            return;
        };
        let Some(u) = update(&v) else {
            self.unknown += 1;
            return;
        };
        let text = || u["content"]["text"].as_str().unwrap_or_default().to_string();
        match u["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => out.push(StreamEvent::TextDelta(text())),
            Some("agent_thought_chunk") => out.push(StreamEvent::ThinkingDelta(text())),
            Some("tool_call") => out.push(StreamEvent::ToolStart {
                id: u["toolCallId"].as_str().unwrap_or_default().to_string(),
                name: u["title"].as_str().or(u["kind"].as_str()).unwrap_or("tool").to_string(),
                input: u["rawInput"].clone(),
            }),
            Some("tool_call_update") => match u["status"].as_str() {
                Some(status @ ("completed" | "failed")) => out.push(StreamEvent::ToolEnd {
                    id: u["toolCallId"].as_str().unwrap_or_default().to_string(),
                    ok: status == "completed",
                    output: clip(&u["content"].to_string(), 400),
                }),
                _ => {}
            },
            _ => self.unknown += 1,
        }
    }
}

impl StreamDecoder for GrokDecoder {
    fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        let (lines, overflow) = self.lines.push(chunk);
        let mut out = Vec::new();
        if overflow {
            out.push(StreamEvent::Notice("grok: dropped an over-long stream line".into()));
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
```

In `src/stream/mod.rs` add `pub(crate) mod grok;`, then:

```rust
use crate::agent::AgentBackend;

pub(crate) fn decoder_for(backend: AgentBackend) -> Box<dyn StreamDecoder> {
    match backend {
        AgentBackend::Claude => Box::new(claude::ClaudeDecoder::default()),
        AgentBackend::Cursor => Box::new(cursor::CursorDecoder::default()),
        AgentBackend::Grok => Box::new(grok::GrokDecoder::default()),
        AgentBackend::Fm | AgentBackend::Ollama | AgentBackend::Abi => {
            Box::new(plain::PlainDecoder::default())
        }
    }
}

pub(crate) fn stream_output_format(backend: AgentBackend) -> Option<&'static str> {
    match backend {
        AgentBackend::Claude | AgentBackend::Cursor => Some("stream-json"),
        AgentBackend::Grok => Some("streaming-json"),
        AgentBackend::Fm | AgentBackend::Ollama | AgentBackend::Abi => None,
    }
}
```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --lib stream::`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/stream/grok.rs src/stream/mod.rs
git commit -m "feat(stream): grok ACP decoder and per-backend decoder selection"
```

---

### Task 6: StreamTap, AgentConfig fields, stream argv flags

**Files:**
- Create: `src/stream/tap.rs`
- Modify: `src/stream/mod.rs` (`mod tap; pub use tap::StreamTap;`)
- Modify: `src/agent/mod.rs` (struct `AgentConfig` ~line 41, its `Default` impl)
- Modify: `src/agent/argv.rs` (`build_args_fm`, `build_args_claude`, `build_args` default grammar)
- Test: `src/agent/argv/tests.rs`

**Interfaces:**
- Produces:
  - `crate::stream::StreamTap { pub events: Sender<StreamEvent>, pub cancel: crate::runtime::CancellationToken }` with `fn new(events: Sender<StreamEvent>) -> Self` and `fn notice(&self, msg: impl Into<String>)`; derives Clone, with a manual Debug.
  - `AgentConfig.stream: Option<StreamTap>`, default `None`.
  - `AgentConfig.permission_mode: Option<String>`, default `None`. It is claude's `--permission-mode` value, used when `force` is false and mode is not `plan`.

- [ ] **Step 1: Write the failing tests** — append to `src/agent/argv/tests.rs`:

```rust
fn streaming(backend: AgentBackend) -> AgentConfig {
    let (tx, _rx) = std::sync::mpsc::channel();
    AgentConfig {
        backend,
        print: true,
        output_format: crate::stream::stream_output_format(backend).map(str::to_string),
        stream: Some(crate::stream::StreamTap::new(tx)),
        auto_review: false,
        trust: false,
        ..AgentConfig::default()
    }
}

#[test]
fn stream_flags_stay_inside_their_backend() {
    let p = ["hi".to_string()];
    let claude = streaming(AgentBackend::Claude).build_args(None, &p);
    for f in ["--print", "stream-json", "--verbose", "--include-partial-messages"] {
        assert!(claude.contains(&f.to_string()), "claude missing {f}: {claude:?}");
    }
    assert!(!claude.contains(&"--stream-partial-output".to_string()));

    let cursor = streaming(AgentBackend::Cursor).build_args(None, &p);
    assert!(cursor.contains(&"--stream-partial-output".to_string()));
    assert!(!cursor.contains(&"--include-partial-messages".to_string()));

    let grok = streaming(AgentBackend::Grok).build_args(None, &p);
    assert!(grok.contains(&"streaming-json".to_string()));
    assert!(!grok.contains(&"--stream-partial-output".to_string()));

    let fm = streaming(AgentBackend::Fm).build_args(None, &p);
    assert!(!fm.contains(&"--no-stream".to_string()), "stream mode must let fm stream: {fm:?}");

    for b in [AgentBackend::Ollama, AgentBackend::Abi] {
        let a = streaming(b).build_args(None, &p);
        assert!(!a.iter().any(|x| x.contains("stream")), "{b:?} leaked a stream flag: {a:?}");
    }
}

#[test]
fn non_stream_argv_is_unchanged_by_the_new_fields() {
    let p = ["hi".to_string()];
    let plain = AgentConfig { backend: AgentBackend::Claude, print: true, ..AgentConfig::default() };
    let args = plain.build_args(None, &p);
    assert!(!args.contains(&"--verbose".to_string()));
    let fm = AgentConfig { backend: AgentBackend::Fm, print: true, ..AgentConfig::default() };
    assert!(fm.build_args(None, &p).contains(&"--no-stream".to_string()));
}

#[test]
fn claude_permission_mode_is_forwarded_unless_force_or_plan() {
    let p = ["hi".to_string()];
    let cfg = AgentConfig { backend: AgentBackend::Claude, permission_mode: Some("acceptEdits".into()), ..AgentConfig::default() };
    let args = cfg.build_args(None, &p);
    let i = args.iter().position(|a| a == "--permission-mode").expect("flag");
    assert_eq!(args[i + 1], "acceptEdits");
    let forced = AgentConfig { force: true, ..cfg };
    let args = forced.build_args(None, &p);
    let i = args.iter().position(|a| a == "--permission-mode").expect("flag");
    assert_eq!(args[i + 1], "bypassPermissions");
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib agent::argv::tests::stream_flags_stay_inside_their_backend`
Expected: FAIL (no field `stream`).

- [ ] **Step 3: Implement.** `src/stream/tap.rs`:

```rust
//! The TUI's handle into a streaming run: decoded events out, cancel in.

use super::StreamEvent;
use crate::runtime::CancellationToken;
use std::fmt;
use std::sync::mpsc::Sender;

#[derive(Clone)]
pub struct StreamTap {
    pub events: Sender<StreamEvent>,
    pub cancel: CancellationToken,
}

impl StreamTap {
    pub fn new(events: Sender<StreamEvent>) -> Self {
        Self { events, cancel: CancellationToken::new() }
    }

    /// Run-path diagnostics that would otherwise corrupt the alternate screen.
    pub fn notice(&self, msg: impl Into<String>) {
        let _ = self.events.send(StreamEvent::Notice(msg.into()));
    }
}

impl fmt::Debug for StreamTap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamTap")
            .field("cancelled", &self.cancel.is_cancelled())
            .finish_non_exhaustive()
    }
}
```

`src/stream/mod.rs`: add `mod tap;` and `pub use tap::StreamTap;`.

In `AgentConfig` add, after `cot_path`:

```rust
    /// Streaming sink for the chat TUI; `None` keeps every CLI path unchanged.
    pub stream: Option<crate::stream::StreamTap>,
    /// Claude `--permission-mode` chosen in the TUI (Shift-Tab); `force` and
    /// plan mode still win.
    pub permission_mode: Option<String>,
```

and `stream: None, permission_mode: None,` in `Default`.

In `src/agent/argv.rs`:

- `build_args_fm`: change `if self.print {` to `if self.print && self.stream.is_none() {`.
- `build_args_claude`: inside the `if self.print {` block, after the output-format push, add:

```rust
            if self.stream.is_some() {
                args.push("--verbose".into());
                args.push("--include-partial-messages".into());
            }
```

  Then change the permission block to:

```rust
        if self.force {
            args.push("--permission-mode".into());
            args.push("bypassPermissions".into());
        } else if self.mode.as_deref() == Some("plan") {
            args.push("--permission-mode".into());
            args.push("plan".into());
        } else if let Some(pm) = &self.permission_mode {
            args.push("--permission-mode".into());
            args.push(pm.clone());
        }
```

- Default grammar (`build_args`, inside `if self.print {` after the output-format push):

```rust
            if self.stream.is_some() && self.backend == AgentBackend::Cursor {
                args.push("--stream-partial-output".into());
            }
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib agent::`
Expected: all argv tests PASS (old + 3 new).

- [ ] **Step 5: Commit**

```bash
git add src/stream/tap.rs src/stream/mod.rs src/agent/mod.rs src/agent/argv.rs src/agent/argv/tests.rs
git commit -m "feat(agent): StreamTap on AgentConfig and per-backend stream argv"
```

---

### Task 7: Streaming run through the canonical path

**Files:**
- Create: `src/agent/streaming.rs`
- Modify: `src/agent/mod.rs` (`mod streaming;`, `notice`, `run_once` top, `run_resilient` eprintln sites and the cancel guard)
- Modify: `src/actions.rs` (`RunSpec.stream`, `RunSpec::streaming`, `run_agent`)
- Modify: `src/session.rs` (every `eprintln!` reached from `hybrid_run`: lines ~27, ~43, ~299 → `cfg.notice(format!(…))`)

**Interfaces:**
- Consumes: `run_tapped` (Task 1), `decoder_for`/`stream_output_format` (Task 5), `StreamTap` (Task 6).
- Produces:
  - `AgentConfig::notice(&self, msg: impl Into<String>)`.
  - `RunSpec { …, pub stream: Option<StreamTap> }` and `RunSpec::streaming(self, tap: StreamTap) -> Self`.
  - A streamed run sends zero or more events, then exactly one `StreamEvent::Done { exit }`. Exit 130 means cancelled.

- [ ] **Step 1: Write the failing tests** — bottom of `src/agent/streaming.rs`:

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::super::{AgentBackend, AgentConfig};
    use crate::state::AbbeyState;
    use crate::stream::{StreamEvent, StreamTap};
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::mpsc;

    fn scratch(tag: &str, script: &str) -> (std::path::PathBuf, AbbeyState, AgentConfig) {
        let dir = std::env::temp_dir().join(format!("abbey-stream-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("by-cwd")).unwrap();
        let agent = dir.join("agent");
        std::fs::write(&agent, script).unwrap();
        let mut perm = std::fs::metadata(&agent).unwrap().permissions();
        perm.set_mode(0o700);
        std::fs::set_permissions(&agent, perm).unwrap();
        let state = AbbeyState {
            state_dir: dir.clone(),
            chat_file: dir.join("chat-id"),
            model_file: dir.join("model"),
            history_file: dir.join("history.log"),
            cwd_dir: dir.join("by-cwd"),
            per_cwd: false,
            cwd: dir.clone(),
        };
        let cfg = AgentConfig {
            agent_path: agent,
            backend: AgentBackend::Ollama,
            transcript_dir: Some(dir.join("ollama")),
            ..AgentConfig::default()
        };
        (dir, state, cfg)
    }

    fn drain(rx: &mpsc::Receiver<StreamEvent>) -> Vec<StreamEvent> {
        rx.try_iter().collect()
    }

    #[test]
    fn streamed_run_emits_deltas_then_done_and_records_the_transcript() {
        let (dir, state, cfg) = scratch("ok", "#!/bin/sh\nprintf 'hel'; sleep 0.05; printf 'lo'\n");
        let (tx, rx) = mpsc::channel();
        let mut cfg = cfg;
        let code = crate::actions::run_agent(
            &mut cfg, &state, &["say hi".into()],
            crate::actions::RunSpec::resume().streaming(StreamTap::new(tx)),
        ).unwrap();
        assert_eq!(code, 0);
        let events = drain(&rx);
        let text: String = events.iter().filter_map(|e| match e { StreamEvent::TextDelta(t) => Some(t.as_str()), _ => None }).collect();
        assert_eq!(text, "hello");
        assert_eq!(events.last(), Some(&StreamEvent::Done { exit: 0 }));
        let chat = state.resolve_chat_for(AgentBackend::Ollama).unwrap().expect("chat saved");
        let transcript = std::fs::read_to_string(cfg.transcript_path(&chat).unwrap()).unwrap();
        assert!(transcript.contains("hello"));
        let routes = std::fs::read_to_string(dir.join("route.jsonl")).unwrap();
        assert_eq!(routes.lines().count(), 1, "exactly one route row per streamed turn");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn streaming_notices_go_to_the_tap_not_stderr() {
        let (dir, state, cfg) = scratch("notice", "#!/bin/sh\nprintf ok\n");
        let (tx, rx) = mpsc::channel();
        let mut cfg = cfg;
        crate::actions::run_agent(&mut cfg, &state, &["x".into()], crate::actions::RunSpec::fresh().streaming(StreamTap::new(tx))).unwrap();
        assert!(drain(&rx).iter().any(|e| matches!(e, StreamEvent::Notice(n) if n.contains("new chat"))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancelled_stream_run_does_not_retry_or_mint_a_chat() {
        let (dir, state, cfg) = scratch("cancel", "#!/bin/sh\nsleep 30\n");
        state.save_chat("keep-me").unwrap();
        let (tx, rx) = mpsc::channel();
        let tap = StreamTap::new(tx);
        tap.cancel.cancel();
        let mut cfg = AgentConfig { backend: AgentBackend::Claude, transcript_dir: Some(dir.join("claude")), ..cfg };
        let code = crate::actions::run_agent(&mut cfg, &state, &["x".into()], crate::actions::RunSpec::resume().streaming(tap)).unwrap();
        assert_eq!(code, 130);
        assert_eq!(drain(&rx).last(), Some(&StreamEvent::Done { exit: 130 }));
        assert_eq!(state.resolve_chat_for(AgentBackend::Claude).unwrap().as_deref(), Some("keep-me"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib agent::streaming`
Expected: FAIL (no module or `RunSpec::streaming`).

- [ ] **Step 3: Implement `src/agent/streaming.rs`** (above the tests):

```rust
//! Streaming mode of [`super::run_once`]: same argv, same continuity, but the
//! child's stdout is decoded live into [`StreamEvent`]s on the caller's tap.

use super::{AgentBackend, AgentConfig, MAX_CAPTURE_BYTES};
use crate::stream::{StreamEvent, StreamTap};
use anyhow::Result;
use std::sync::mpsc;

pub(super) const CANCELLED_EXIT: i32 = 130;

enum Ended {
    Exited { code: i32, success: bool, stderr: String },
    Cancelled,
}

pub(super) fn run_once_streaming(
    cfg: &AgentConfig,
    tap: &StreamTap,
    resume_id: Option<&str>,
    prompt_and_rest: &[String],
) -> Result<i32> {
    let _turn_lock = if cfg.backend.is_oneshot_local() {
        resume_id
            .filter(|id| !id.is_empty())
            .map(|id| cfg.lock_local_turn(id))
            .transpose()?
            .flatten()
    } else {
        None
    };
    let mut run_cfg = cfg.clone();
    run_cfg.print = true;
    run_cfg.output_format = crate::stream::stream_output_format(cfg.backend).map(str::to_string);

    let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>();
    let events = tap.events.clone();
    let mut decoder = crate::stream::decoder_for(cfg.backend);
    let forwarder = std::thread::Builder::new()
        .name("abbey-stream-decode".into())
        .spawn(move || {
            let mut text = String::new();
            let mut send = |ev: StreamEvent| {
                if let StreamEvent::TextDelta(t) = &ev {
                    text.push_str(t);
                }
                let _ = events.send(ev);
            };
            for chunk in chunk_rx {
                decoder.feed(&chunk).into_iter().for_each(&mut send);
            }
            decoder.finish().into_iter().for_each(&mut send);
            let unknown = decoder.unknown_events();
            (text, unknown)
        })?;

    let ended = capture(&run_cfg, resume_id, prompt_and_rest, chunk_tx, tap);
    let (text, unknown) = forwarder
        .join()
        .map_err(|_| anyhow::anyhow!("stream decoder thread panicked"))?;
    if unknown > 0 {
        tap.notice(format!("abbey: {unknown} unrecognised stream event(s) ignored"));
    }
    let ended = match ended {
        Ok(ended) => ended,
        Err(e) => {
            let _ = tap.events.send(StreamEvent::Failed(format!("{e:#}")));
            let _ = tap.events.send(StreamEvent::Done { exit: 1 });
            return Err(e);
        }
    };
    let code = match ended {
        Ended::Cancelled => {
            tap.notice("interrupted");
            CANCELLED_EXIT
        }
        Ended::Exited { code, success, stderr } => {
            let stderr = stderr.trim();
            if !stderr.is_empty() {
                tap.notice(stderr.to_string());
            }
            if success && let Some(id) = resume_id.filter(|i| !i.is_empty()) {
                if cfg.backend.is_oneshot_local() {
                    cfg.append_local_transcript(id, prompt_and_rest, &text);
                }
                if cfg.backend == AgentBackend::Claude {
                    cfg.touch_claude_session_marker(id);
                }
            }
            code
        }
    };
    let _ = tap.events.send(StreamEvent::Done { exit: code });
    Ok(code)
}

#[cfg(unix)]
fn capture(
    cfg: &AgentConfig,
    resume_id: Option<&str>,
    prompt_and_rest: &[String],
    chunks: mpsc::Sender<Vec<u8>>,
    tap: &StreamTap,
) -> Result<Ended> {
    use crate::runtime::supervisor::{ProcessSpec, SupervisorLimits, SupervisorOutcome, run_tapped};
    use std::time::Duration;
    let agent = cfg.exec_path()?;
    let args = cfg.build_args(resume_id, prompt_and_rest);
    let spec = ProcessSpec::inherited(agent.clone(), args.iter().map(std::ffi::OsString::from).collect());
    let limits = SupervisorLimits {
        timeout: Duration::from_secs(30 * 60),
        terminate_grace: Duration::from_secs(1),
        stdout_bytes: MAX_CAPTURE_BYTES,
        stderr_bytes: MAX_CAPTURE_BYTES,
        poll_interval: Duration::from_millis(20),
    };
    let cancel = tap.cancel.clone();
    match run_tapped(&spec, &limits, move || cancel.is_cancelled(), chunks) {
        Ok(SupervisorOutcome::Exited { status, stderr, .. }) => Ok(Ended::Exited {
            code: status.code().unwrap_or(1),
            success: status.success(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }),
        Ok(SupervisorOutcome::Cancelled) => Ok(Ended::Cancelled),
        Ok(SupervisorOutcome::TimedOut) => anyhow::bail!("agent run exceeded the 30-minute limit"),
        Ok(SupervisorOutcome::StderrLimit) => anyhow::bail!("agent stderr exceeded the {MAX_CAPTURE_BYTES}-byte limit"),
        Ok(SupervisorOutcome::StdoutLimit) => unreachable!("tapped stdout keeps a tail and never overflows"),
        Err(error) => anyhow::bail!("supervise {}: {error}", agent.display()),
    }
}

/// Non-Unix: no live tap — buffer the run, then deliver it as one chunk.
#[cfg(not(unix))]
fn capture(
    cfg: &AgentConfig,
    resume_id: Option<&str>,
    prompt_and_rest: &[String],
    chunks: mpsc::Sender<Vec<u8>>,
    _tap: &StreamTap,
) -> Result<Ended> {
    let (status, stdout, stderr) = cfg.run_capture(resume_id, prompt_and_rest)?;
    let _ = chunks.send(stdout.into_bytes());
    Ok(Ended::Exited { code: status.code().unwrap_or(1), success: status.success(), stderr })
}
```

In `src/agent/mod.rs`:

- Add `mod streaming;` after `mod argv;`.
- Add to `impl AgentConfig`:

```rust
    /// Diagnostic line: to the TUI tap when streaming, else stderr as before.
    pub(crate) fn notice(&self, msg: impl Into<String>) {
        match &self.stream {
            Some(tap) => tap.notice(msg),
            None => eprintln!("{}", msg.into()),
        }
    }
```

- At the top of `run_once`:

```rust
    if let Some(tap) = &cfg.stream {
        return streaming::run_once_streaming(cfg, tap, resume_id, prompt_and_rest);
    }
```

- In `run_resilient`, replace each `eprintln!(…)` with `cfg.notice(format!(…))`, keeping the same text. Then insert right after `let code = run_once(cfg, Some(&chat), prompt_and_rest, capture_print)?;`:

```rust
    if cfg.stream.as_ref().is_some_and(|t| t.cancel.is_cancelled()) {
        return Ok(code);
    }
```

In `src/session.rs`, every `eprintln!` inside `hybrid_run` and the helpers it calls (`apply_media_attach`, `maybe_inject_role_model`; `grep -n eprintln src/session.rs`) becomes `cfg.notice(format!(…))` with the same text. `hybrid_loop_run` is unchanged.

In `src/actions.rs`:

- Add `pub stream: Option<crate::stream::StreamTap>,` to `RunSpec`. `Default` and `Clone` still derive, because `Option<StreamTap>` is `Default` and `Clone`.
- Add:

```rust
    pub fn streaming(mut self, tap: crate::stream::StreamTap) -> Self {
        self.stream = Some(tap);
        self
    }
```

- In `run_agent`, add `if let Some(tap) = spec.stream.clone() { cfg.stream = Some(tap); }` as the first statement.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib agent:: && cargo test --lib session::`
Expected: PASS, including the 3 streaming tests.

- [ ] **Step 5: Confirm the CLI path is unchanged**

Run: `cargo test --test cli_surface`
Expected: PASS, including `print_bypasses_the_route_log_where_ask_appends`.

- [ ] **Step 6: Commit**

```bash
git add src/agent/streaming.rs src/agent/mod.rs src/actions.rs src/session.rs
git commit -m "feat(agent): streaming output sink through the canonical run path"
```

---

### Task 8: Markdown rendering with syntect code blocks

**Files:**
- Modify: `Cargo.toml` (`cargo add pulldown-cmark --no-default-features`)
- Modify: `src/highlight.rs` (add `code_lines`)
- Create: `src/tui/markdown.rs`

**Interfaces:**
- Produces:
  - `crate::highlight::code_lines(code: &str, lang: Option<&str>) -> Vec<Vec<((u8, u8, u8), String)>>`.
  - `crate::tui::markdown::render(md: &str, theme: &Theme, width: u16) -> Vec<Line<'static>>`, whose output is already wrapped to `width` columns.

- [ ] **Step 1: Add the dependency and the tests**

Run: `cargo add pulldown-cmark --no-default-features`
Expected: the version is added to `Cargo.toml` `[dependencies]` and resolves in `Cargo.lock`.

`src/tui/markdown.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Theme, ThemeId};

    fn plain(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn headings_lists_and_code_render_as_text() {
        let t = Theme::from_id(ThemeId::Ink);
        let out = plain(&render("# Title\n\n- one\n- two\n\n```rust\nfn a() {}\n```\n", &t, 80));
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
        for md in ["```rust\nfn a(", "**bold", "- [ ] item\n  - nest", "| a | b |\n|--"] {
            let _ = render(md, &t, 40);
        }
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib tui::markdown`
Expected: FAIL (module missing; declare `pub(crate) mod markdown;` in `src/tui/mod.rs`).

- [ ] **Step 3: Implement.** Add to `src/highlight.rs`:

```rust
/// Syntax-highlighted lines as `(rgb, text)` runs for non-ANSI renderers (TUI).
pub fn code_lines(code: &str, lang: Option<&str>) -> Vec<Vec<((u8, u8, u8), String)>> {
    let ps = syntaxes();
    let syntax = find_syntax(ps, lang, None);
    let mut h = HighlightLines::new(syntax, theme());
    let mut out = Vec::new();
    for line in LinesWithEndings::from(code) {
        let runs = match h.highlight_line(line, ps) {
            Ok(ranges) => ranges
                .into_iter()
                .map(|(style, text)| {
                    let c = style.foreground;
                    ((c.r, c.g, c.b), text.trim_end_matches('\n').to_string())
                })
                .collect(),
            Err(_) => vec![((200, 200, 200), line.trim_end_matches('\n').to_string())],
        };
        out.push(runs);
    }
    out
}
```

`src/tui/markdown.rs` (above tests):

```rust
//! Markdown → pre-wrapped ratatui lines. Pre-wrapping makes transcript
//! scrolling exact (line counts are known before drawing).

use super::theme::Theme;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

pub(crate) fn render(md: &str, theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let mut r = Renderer { theme, lines: Vec::new(), cur: Vec::new(), styles: vec![Style::default()], prefix: String::new(), list: Vec::new(), code: None };
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
    r.lines.into_iter().flat_map(|l| wrap(l, width.max(8))).collect()
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
            self.cur.push(Span::styled(self.prefix.clone(), Style::default().fg(self.theme.fg_dim)));
        }
        let style = self.style();
        self.cur.push(Span::styled(t.to_string(), style));
    }
    fn code_block(&mut self, lang: Option<&str>, body: &str) {
        self.flush();
        for runs in crate::highlight::code_lines(body, lang) {
            let mut spans = vec![Span::styled("│ ", Style::default().fg(self.theme.fg_dim))];
            spans.extend(runs.into_iter().map(|((r, g, b), t)| Span::styled(t, Style::default().fg(Color::Rgb(r, g, b)))));
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
                let s = Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD);
                self.push_style(if level == HeadingLevel::H1 { s.add_modifier(Modifier::UNDERLINED) } else { s });
            }
            Event::End(TagEnd::Heading(_)) => {
                self.styles.pop();
                self.flush();
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => self.blank(),
            Event::Start(Tag::Emphasis) => self.push_style(Style::default().add_modifier(Modifier::ITALIC)),
            Event::Start(Tag::Strong) => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Event::Start(Tag::Strikethrough) => self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT)),
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
                self.lines.push(Line::styled("────────", Style::default().fg(self.theme.fg_dim)));
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
```

`Theme` (src/tui/theme.rs) already has `accent`, `fg_dim`, `ok`, `warn`, `error`; use exactly those.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib tui::markdown && cargo test --lib highlight`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/highlight.rs src/tui/markdown.rs src/tui/mod.rs
git commit -m "feat(tui): pulldown-cmark markdown with syntect code blocks and exact wrapping"
```

---

### Task 9: Transcript model

**Files:**
- Create: `src/tui/transcript.rs`

**Interfaces:**
- Consumes: `StreamEvent` (Task 2), `markdown::render`/`wrap` (Task 8).
- Produces:
  - `Transcript { cells: Vec<Cell>, usage: Option<(u64, u64)>, session: Option<String> }`.
  - Methods: `push_user(&mut self, text: &str)`, `push_notice(&mut self, text: impl Into<String>)`, `push_error(…)`, `apply(&mut self, ev: StreamEvent)`, `toggle_last_expandable(&mut self)`, `lines(&self, theme: &Theme, width: u16) -> Vec<Line<'static>>`.
  - `Cell` enum; `ToolStatus`.
  - Helpers `tool_summary(name, input) -> String`, `diff_lines(name, input) -> Option<Vec<DiffLine>>`, `todo_items(name, input) -> Option<Vec<(String, String)>>`.

- [ ] **Step 1: Write the failing tests** (bottom of `src/tui/transcript.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::StreamEvent;
    use serde_json::json;

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
        t.apply(StreamEvent::ToolStart { id: "a".into(), name: "Bash".into(), input: json!({"command":"ls"}) });
        t.apply(StreamEvent::TextDelta("between".into()));
        t.apply(StreamEvent::ToolEnd { id: "a".into(), ok: false, output: "boom".into() });
        let Cell::Tool(tool) = &t.cells[0] else { panic!() };
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
        assert_eq!(tool_summary("Bash", &json!({"command":"cargo test"})), "cargo test");
        assert_eq!(tool_summary("Read", &json!({"file_path":"src/a.rs"})), "src/a.rs");
        let d = diff_lines("Edit", &json!({"old_string":"a\nb","new_string":"a\nc"})).unwrap();
        assert_eq!(d, vec![DiffLine::Removed("a".into()), DiffLine::Removed("b".into()), DiffLine::Added("a".into()), DiffLine::Added("c".into())]);
        let todos = todo_items("TodoWrite", &json!({"todos":[{"content":"x","status":"completed"}]})).unwrap();
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
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib tui::transcript` (add `mod transcript;` to `src/tui/mod.rs`)
Expected: FAIL.

- [ ] **Step 3: Implement** (above the tests):

```rust
//! The conversation as cells, built from user input and [`StreamEvent`]s.

use super::markdown;
use super::theme::Theme;
use crate::stream::StreamEvent;
use crate::stream::ndjson::clip;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

pub(crate) const MAX_CELLS: usize = 2000;
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
}

impl Transcript {
    fn push(&mut self, cell: Cell) {
        self.cells.push(cell);
        if self.cells.len() > MAX_CELLS {
            let excess = self.cells.len() - MAX_CELLS;
            self.cells.drain(..excess);
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

    pub(crate) fn apply(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::TextDelta(t) => match self.cells.last_mut() {
                Some(Cell::Assistant(s)) => s.push_str(&t),
                _ => self.push(Cell::Assistant(t)),
            },
            StreamEvent::ThinkingDelta(t) => match self.cells.last_mut() {
                Some(Cell::Thinking { text, .. }) => text.push_str(&t),
                _ => self.push(Cell::Thinking { text: t, expanded: false }),
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
                    t.status = if ok { ToolStatus::Ok } else { ToolStatus::Failed };
                    t.output = output;
                }
            }
            StreamEvent::Usage { input_tokens, output_tokens } => {
                self.usage = Some((input_tokens, output_tokens));
            }
            StreamEvent::SessionId(id) => self.session = Some(id),
            StreamEvent::Notice(n) => self.push_notice(n),
            StreamEvent::Done { exit: 0 } => {}
            StreamEvent::Done { exit: 130 } => self.push_notice("⏹ interrupted"),
            StreamEvent::Done { exit } => self.push_error(format!("executor exited {exit}")),
            StreamEvent::Failed(msg) => self.push_error(msg),
        }
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
        for cell in &self.cells {
            cell_lines(cell, theme, width, &mut out);
            out.push(Line::default());
        }
        out
    }
}

fn cell_lines(cell: &Cell, theme: &Theme, width: u16, out: &mut Vec<Line<'static>>) {
    let dim = Style::default().fg(theme.fg_dim);
    let wrap_into = |line: Line<'static>, out: &mut Vec<Line<'static>>| out.extend(markdown::wrap(line, width));
    match cell {
        Cell::User(t) => {
            for (i, l) in t.lines().enumerate() {
                let marker = if i == 0 { "› " } else { "  " };
                wrap_into(Line::from(vec![Span::styled(marker, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)), Span::raw(l.to_string())]), out);
            }
        }
        Cell::Assistant(md) => out.extend(markdown::render(md, theme, width)),
        Cell::Thinking { text, expanded } => {
            if *expanded {
                for l in text.lines() {
                    wrap_into(Line::styled(format!("┊ {l}"), dim.add_modifier(Modifier::ITALIC)), out);
                }
            } else {
                wrap_into(Line::styled(format!("✻ thinking… ({} chars, Ctrl-O)", text.chars().count()), dim), out);
            }
        }
        Cell::Tool(t) => {
            let (glyph, color) = match t.status {
                ToolStatus::Running => ("●", theme.warn),
                ToolStatus::Ok => ("✓", theme.ok),
                ToolStatus::Failed => ("✗", theme.error),
            };
            wrap_into(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(color)),
                Span::styled(t.name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {}", tool_summary(&t.name, &t.input)), dim),
            ]), out);
            if let Some(items) = todo_items(&t.name, &t.input) {
                for (status, text) in items {
                    let mark = match status.as_str() { "completed" => "☑", "in_progress" => "◐", _ => "☐" };
                    wrap_into(Line::raw(format!("  {mark} {text}")), out);
                }
            } else if let Some(diff) = diff_lines(&t.name, &t.input) {
                let shown = if t.expanded { diff.len() } else { diff.len().min(12) };
                for d in &diff[..shown] {
                    let (s, c) = match d { DiffLine::Removed(l) => (format!("  - {l}"), theme.error), DiffLine::Added(l) => (format!("  + {l}"), theme.ok) };
                    wrap_into(Line::styled(s, Style::default().fg(c)), out);
                }
                if shown < diff.len() {
                    wrap_into(Line::styled(format!("  … {} more (Ctrl-O)", diff.len() - shown), dim), out);
                }
            }
            if t.expanded && !t.output.is_empty() {
                for l in t.output.lines().take(200) {
                    wrap_into(Line::styled(format!("  │ {l}"), dim), out);
                }
            }
        }
        Cell::Notice(n) => wrap_into(Line::styled(format!("· {n}"), dim), out),
        Cell::Error(e) => wrap_into(Line::styled(format!("✗ {e}"), Style::default().fg(theme.error)), out),
    }
}

pub(crate) fn tool_summary(_name: &str, input: &Value) -> String {
    for key in ["command", "file_path", "path", "pattern", "url", "query", "description"] {
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
                add_pair(e["old_string"].as_str().unwrap_or(""), e["new_string"].as_str().unwrap_or(""));
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
            .map(|t| (t["status"].as_str().unwrap_or("pending").to_string(), t["content"].as_str().unwrap_or("").to_string()))
            .collect(),
    )
}
```

`ndjson` is already `pub(crate)` (Task 3). Theme colours used: `accent`, `fg_dim`, `ok`, `warn`, `error` (existing fields).

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib tui::transcript`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/tui/transcript.rs src/tui/mod.rs
git commit -m "feat(tui): transcript cells with tool, diff, todo and thinking views"
```

---

### Task 10: Composer (multiline buffer, history, reverse search)

**Files:**
- Create: `src/tui/composer.rs`

**Interfaces:**
- Produces `Composer` with pub fields `text: String` and `cursor: usize` (a byte offset, always on a char boundary), plus these methods:
  - editing: `insert_char(char)`, `insert_str(&str)`, `newline()`, `backspace()`, `delete()`, `left()`, `right()`, `word_left()`, `word_right()`, `line_start()`, `line_end()`, `kill_line_before()`, `kill_word_before()`;
  - navigation: `up() -> bool` and `down() -> bool` (false means already on the first or last line);
  - `take() -> String`, `is_empty()`, `cursor_row_col() -> (usize, usize)`.
- Produces `History`:
  - `load(state_dir: &Path) -> History`, `push(&mut self, entry: &str)`, `prev(&mut self, current: &str) -> Option<String>`, `next(&mut self) -> Option<String>`, `search(&self, query: &str) -> Option<&str>`.
  - Stored as JSON strings, one per line, in `<state_dir>/tui-history.jsonl`, keeping the last 500.

- [ ] **Step 1: Write the failing tests**:

```rust
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
        for _ in 0..30 { c.left(); }
        for _ in 0..3 { c.right(); }
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
        let dir = std::env::temp_dir().join(format!("abbey-hist-{}", std::process::id()));
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
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib tui::composer` (add `mod composer;` to `src/tui/mod.rs`)
Expected: FAIL.

- [ ] **Step 3: Implement**:

```rust
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
        self.text[..i].char_indices().next_back().map_or(0, |(p, _)| p)
    }
    fn next_boundary(&self, i: usize) -> usize {
        self.text[i..].chars().next().map_or(i, |c| i + c.len_utf8())
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
        self.cursor = trimmed.rfind(char::is_whitespace).map_or(0, |i| i + trimmed[i..].chars().next().map_or(1, char::len_utf8));
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
        let end = self.text[self.cursor..].find('\n').map_or(self.text.len(), |i| self.cursor + i);
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
        Self { path: Some(path), entries, idx: None, draft: String::new() }
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
            let body: String = self.entries.iter().filter_map(|e| serde_json::to_string(e).ok()).map(|l| l + "\n").collect();
            let tmp = path.with_extension("jsonl.tmp");
            if std::fs::File::create(&tmp).and_then(|mut f| f.write_all(body.as_bytes())).is_ok() {
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
        self.entries.iter().rev().find(|e| e.contains(query)).map(String::as_str)
    }

    pub(crate) fn entries(&self) -> &[String] {
        &self.entries
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib tui::composer`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/tui/composer.rs src/tui/mod.rs
git commit -m "feat(tui): multiline composer with persisted history and reverse search"
```

---

### Task 11: `@file` and slash completion

**Files:**
- Create: `src/tui/completion.rs`

**Interfaces:**
- Consumes: `predict::rank`, `predict::accept_text`, `Prediction` (existing `src/tui/predict.rs`).
- Produces:
  - `enum Completion { Slash(Vec<Prediction>), File { token_start: usize, items: Vec<String> } }`.
  - `fn complete(text: &str, cursor: usize, history: &[String], files: &FileIndex, llm_boost: Option<&'static str>) -> Option<Completion>`.
  - `fn accept(text: &str, cursor: usize, c: &Completion, idx: usize) -> (String, usize)`.
  - `FileIndex::load(root: &Path) -> FileIndex` (git ls-files, else a bounded walk) and `FileIndex::from_paths(Vec<String>)` for tests.
  - `fn fuzzy_score(candidate: &str, query: &str) -> Option<i32>`.

- [ ] **Step 1: Write the failing tests**:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> FileIndex {
        FileIndex::from_paths(vec!["src/tui/app.rs".into(), "src/stream/mod.rs".into(), "README.md".into()])
    }

    #[test]
    fn at_token_under_cursor_completes_paths() {
        let text = "look at @strmo please";
        let cursor = "look at @strmo".len();
        let Some(Completion::File { token_start, items }) = complete(text, cursor, &[], &idx(), None) else { panic!() };
        assert_eq!(token_start, "look at ".len());
        assert_eq!(items[0], "src/stream/mod.rs");
        let (new, cur) = accept(text, cursor, &Completion::File { token_start, items }, 0);
        assert_eq!(new, "look at @src/stream/mod.rs  please");
        assert_eq!(cur, "look at @src/stream/mod.rs ".len());
    }

    #[test]
    fn slash_prefix_uses_the_catalog_ranker() {
        let Some(Completion::Slash(p)) = complete("/hel", 4, &[], &idx(), None) else { panic!() };
        assert_eq!(p[0].name, "help");
    }

    #[test]
    fn plain_text_has_no_completion() {
        assert!(complete("hello world", 11, &[], &idx(), None).is_none());
    }

    #[test]
    fn fuzzy_prefers_contiguous_and_basename_matches() {
        assert!(fuzzy_score("src/tui/app.rs", "app").unwrap() > fuzzy_score("src/tui/app.rs", "srs").unwrap());
        assert!(fuzzy_score("README.md", "zz").is_none());
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib tui::completion` (add `mod completion;`)
Expected: FAIL.

- [ ] **Step 3: Implement**:

```rust
//! Composer completion: `/slash` via the existing ranker, `@path` fuzzy files.

use super::predict::{self, Prediction};
use std::path::Path;
use std::process::Command;

const MAX_FILES: usize = 20_000;
const MAX_ITEMS: usize = 8;
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".build", "zig-out", ".zig-cache"];

#[derive(Debug, Default, Clone)]
pub(crate) struct FileIndex {
    paths: Vec<String>,
}

impl FileIndex {
    pub(crate) fn from_paths(paths: Vec<String>) -> Self {
        Self { paths }
    }

    pub(crate) fn load(root: &Path) -> Self {
        let git = Command::new("git").arg("-C").arg(root).args(["ls-files", "-co", "--exclude-standard"]).output();
        if let Ok(out) = git
            && out.status.success()
        {
            let paths = String::from_utf8_lossy(&out.stdout).lines().take(MAX_FILES).map(str::to_string).collect();
            return Self { paths };
        }
        let mut paths = Vec::new();
        let mut stack = vec![(root.to_path_buf(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
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
    File { token_start: usize, items: Vec<String> },
}

pub(crate) fn fuzzy_score(candidate: &str, query: &str) -> Option<i32> {
    let cand: Vec<char> = candidate.to_lowercase().chars().collect();
    let base_start = candidate.rfind('/').map_or(0, |i| candidate[..=i].chars().count());
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
    let before = &text[..cursor];
    let start = before.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let token = &before[start..];
    token.strip_prefix('@').map(|q| (start, q))
}

pub(crate) fn complete(text: &str, cursor: usize, history: &[String], files: &FileIndex, llm_boost: Option<&'static str>) -> Option<Completion> {
    if text.starts_with('/') && !text.contains('\n') && !text[..cursor].contains(' ') {
        let preds = predict::rank(text, history, llm_boost);
        return (!preds.is_empty()).then_some(Completion::Slash(preds));
    }
    let (token_start, query) = at_token(text, cursor)?;
    let mut scored: Vec<(i32, &String)> = files.paths.iter().filter_map(|p| fuzzy_score(p, query).map(|s| (s, p))).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let items: Vec<String> = scored.into_iter().take(MAX_ITEMS).map(|(_, p)| p.clone()).collect();
    (!items.is_empty()).then_some(Completion::File { token_start, items })
}

pub(crate) fn accept(text: &str, cursor: usize, c: &Completion, idx: usize) -> (String, usize) {
    match c {
        Completion::Slash(preds) => {
            let Some(p) = preds.get(idx) else { return (text.to_string(), cursor) };
            let new = predict::accept_text(text, p.name);
            let len = new.len();
            (new, len)
        }
        Completion::File { token_start, items } => {
            let Some(path) = items.get(idx) else { return (text.to_string(), cursor) };
            let insert = format!("@{path} ");
            let new = format!("{}{insert}{}", &text[..*token_start], &text[cursor..]);
            (new, token_start + insert.len())
        }
    }
}
```

Note `accept_text` returns the completed slash text (existing behaviour, `predict.rs:277`).

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib tui::completion`
Expected: PASS. If `accept_text` leaves a trailing space differently from what the test expects, fix the test assertion for the slash case only; the file case is specified here.

- [ ] **Step 5: Commit**

```bash
git add src/tui/completion.rs src/tui/mod.rs
git commit -m "feat(tui): @file fuzzy completion and slash completion"
```

---

### Task 12: Run worker, child capture, permission modes

**Files:**
- Create: `src/tui/worker.rs`, `src/tui/permission.rs`

**Interfaces:**
- Consumes: `RunSpec::streaming`, `StreamTap` (Tasks 6–7), `slash::SLASH_CATALOG`, `slash_alias::resolve_name`, `slash::parse_slash`.
- Produces from `worker.rs`:
  - `enum RunKind { Prompt { text: String, fresh: bool }, PleaseFix(String), AgentSlash(String) }`.
  - `struct RunHandle { pub events: Receiver<StreamEvent>, pub done: Receiver<(i32, Option<String>)>, pub cancel: CancellationToken, pub started: Instant }`.
  - `fn spawn_run(cfg: AgentConfig, state: AbbeyState, kind: RunKind) -> std::io::Result<RunHandle>`.
  - `enum SlashRoute { Agent, Local, Interactive }` and `fn route_slash(input: &str) -> SlashRoute`.
  - `struct ChildOutput { pub code: i32, pub stdout: String, pub stderr: String }` and `fn run_abbey_capture(args: &[String]) -> anyhow::Result<ChildOutput>`.
- Produces from `permission.rs`:
  - `fn label(cfg: &AgentConfig) -> String`.
  - `fn cycle(cfg: &mut AgentConfig) -> String`, which returns the new label: claude `default → acceptEdits → plan → bypassPermissions → default` via `permission_mode` (with `bypassPermissions` setting `force`); cursor toggles `force`; other backends return `"n/a (executor-managed)"` unchanged.

- [ ] **Step 1: Write the failing tests** (in each file):

```rust
// src/tui/permission.rs tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentBackend, AgentConfig};

    #[test]
    fn claude_cycles_four_modes_and_force_tracks_bypass() {
        let mut cfg = AgentConfig { backend: AgentBackend::Claude, ..AgentConfig::default() };
        let seen: Vec<String> = (0..4).map(|_| cycle(&mut cfg)).collect();
        assert_eq!(seen, ["acceptEdits", "plan", "bypassPermissions", "default"]);
        assert!(!cfg.force);
    }

    #[test]
    fn cursor_toggles_force_and_others_are_executor_managed() {
        let mut cursor = AgentConfig { backend: AgentBackend::Cursor, ..AgentConfig::default() };
        assert_eq!(cycle(&mut cursor), "force");
        assert!(cursor.force);
        assert_eq!(cycle(&mut cursor), "ask");
        let mut ollama = AgentConfig { backend: AgentBackend::Ollama, ..AgentConfig::default() };
        assert_eq!(cycle(&mut ollama), "n/a (executor-managed)");
    }
}
```

```rust
// src/tui/worker.rs tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_routing_follows_the_catalog_kind_and_aliases() {
        assert!(matches!(route_slash("/review"), SlashRoute::Agent));
        assert!(matches!(route_slash("/plan write it"), SlashRoute::Agent));
        assert!(matches!(route_slash("/doctor"), SlashRoute::Local));
        assert!(matches!(route_slash("/reset"), SlashRoute::Local));
        assert!(matches!(route_slash("/voice"), SlashRoute::Interactive));
        assert!(matches!(route_slash("/listen"), SlashRoute::Interactive));
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib tui::permission tui::worker` (add `mod permission; mod worker;`)
Expected: FAIL.

- [ ] **Step 3: Implement `src/tui/permission.rs`**:

```rust
//! Shift-Tab permission mode: shown in the header, forwarded as executor flags.

use crate::agent::{AgentBackend, AgentConfig};

const CLAUDE_MODES: [&str; 4] = ["default", "acceptEdits", "plan", "bypassPermissions"];

pub(crate) fn label(cfg: &AgentConfig) -> String {
    match cfg.backend {
        AgentBackend::Claude if cfg.force => "bypassPermissions".into(),
        AgentBackend::Claude => cfg.permission_mode.clone().unwrap_or_else(|| "default".into()),
        AgentBackend::Cursor => if cfg.force { "force" } else { "ask" }.into(),
        _ => "n/a (executor-managed)".into(),
    }
}

pub(crate) fn cycle(cfg: &mut AgentConfig) -> String {
    match cfg.backend {
        AgentBackend::Claude => {
            let cur = label(cfg);
            let i = CLAUDE_MODES.iter().position(|m| *m == cur).unwrap_or(0);
            let next = CLAUDE_MODES[(i + 1) % CLAUDE_MODES.len()];
            cfg.force = next == "bypassPermissions";
            cfg.permission_mode = (next != "default" && next != "bypassPermissions").then(|| next.to_string());
        }
        AgentBackend::Cursor => cfg.force = !cfg.force,
        _ => {}
    }
    label(cfg)
}
```

- [ ] **Step 4: Implement `src/tui/worker.rs`**:

```rust
//! Background runs for the chat TUI: agent turns stream through the canonical
//! path on a thread; local slash commands run as a captured `abbey` child so
//! their stdout lands in the transcript instead of on the alternate screen.

use crate::actions::{RunSpec, run_agent};
use crate::agent::AgentConfig;
use crate::runtime::CancellationToken;
use crate::slash::{SLASH_CATALOG, SlashKind};
use crate::state::AbbeyState;
use crate::stream::{StreamEvent, StreamTap};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

const INTERACTIVE_SLASH: &[&str] = &["voice", "listen"];

pub(crate) enum RunKind {
    Prompt { text: String, fresh: bool },
    PleaseFix(String),
    AgentSlash(String),
}

pub(crate) struct RunHandle {
    pub events: Receiver<StreamEvent>,
    pub done: Receiver<(i32, Option<String>)>,
    pub cancel: CancellationToken,
    pub started: Instant,
}

pub(crate) fn spawn_run(mut cfg: AgentConfig, state: AbbeyState, kind: RunKind) -> std::io::Result<RunHandle> {
    let (ev_tx, events) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    let tap = StreamTap::new(ev_tx);
    let cancel = tap.cancel.clone();
    std::thread::Builder::new().name("abbey-tui-run".into()).spawn(move || {
        let result = match kind {
            RunKind::Prompt { text, fresh } => {
                let spec = if fresh { RunSpec::fresh() } else { RunSpec::resume() };
                run_agent(&mut cfg, &state, &[text], spec.streaming(tap))
            }
            RunKind::PleaseFix(input) => {
                let text = crate::please_fix::build_prompt_soft(&input);
                run_agent(&mut cfg, &state, &[text], RunSpec::max().streaming(tap))
            }
            RunKind::AgentSlash(cmd) => {
                cfg.stream = Some(tap);
                crate::slash_dispatch::dispatch_slash(&cmd, &state, &mut cfg)
            }
        };
        let _ = done_tx.send(match result {
            Ok(code) => (code, None),
            Err(e) => (1, Some(format!("{e:#}"))),
        });
    })?;
    Ok(RunHandle { events, done, cancel, started: Instant::now() })
}

pub(crate) enum SlashRoute {
    Agent,
    Local,
    Interactive,
}

pub(crate) fn route_slash(input: &str) -> SlashRoute {
    let Some((name, _)) = crate::slash::parse_slash(input) else { return SlashRoute::Local };
    let name = crate::slash_alias::resolve_name(name).unwrap_or(name);
    if INTERACTIVE_SLASH.contains(&name) {
        return SlashRoute::Interactive;
    }
    match SLASH_CATALOG.iter().find(|c| c.name == name).map(|c| c.kind) {
        Some(SlashKind::Agent) => SlashRoute::Agent,
        _ => SlashRoute::Local,
    }
}

pub(crate) struct ChildOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// `abbey <args…>` as a bounded, captured child (60 s, 1 MiB per stream).
pub(crate) fn run_abbey_capture(args: &[String]) -> anyhow::Result<ChildOutput> {
    let exe = std::env::current_exe()?;
    #[cfg(unix)]
    {
        use crate::runtime::supervisor::{ProcessSpec, SupervisorLimits, SupervisorOutcome, run_with_checkpoint};
        use std::time::Duration;
        let spec = ProcessSpec::inherited(exe, args.iter().map(std::ffi::OsString::from).collect());
        let limits = SupervisorLimits {
            timeout: Duration::from_secs(60),
            terminate_grace: Duration::from_millis(500),
            stdout_bytes: 1024 * 1024,
            stderr_bytes: 1024 * 1024,
            poll_interval: Duration::from_millis(10),
        };
        match run_with_checkpoint(&spec, &limits, || false) {
            Ok(SupervisorOutcome::Exited { status, stdout, stderr }) => Ok(ChildOutput {
                code: status.code().unwrap_or(1),
                stdout: String::from_utf8_lossy(&stdout).into_owned(),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
            }),
            Ok(other) => anyhow::bail!("abbey child ended as {other:?}"),
            Err(e) => anyhow::bail!("abbey child: {e}"),
        }
    }
    #[cfg(not(unix))]
    {
        let out = std::process::Command::new(exe).args(args).output()?;
        Ok(ChildOutput {
            code: out.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}
```

Check that `crate::slash::parse_slash` and `crate::slash_alias::resolve_name` are reachable from `tui` (both are used by `slash_dispatch.rs:37-40`). If either is private, make it `pub(crate)`. Also make `please_fix::build_prompt_soft` visible if needed (the old `app.rs` already calls it).

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --lib tui::permission tui::worker`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/tui/worker.rs src/tui/permission.rs src/tui/mod.rs src/slash.rs src/slash_alias.rs
git commit -m "feat(tui): streaming run worker, captured slash child, permission modes"
```

---

### Task 13: App state and keymap state machine

**Files:**
- Rewrite: `src/tui/app.rs` (App state only; the loop moves to Task 15)
- Create: `src/tui/keymap.rs`
- Test: `src/tui/tests.rs` (keymap section)

**Interfaces:**
- Consumes: Tasks 9–12. Keeps `refresh.rs` working by retaining the fields `state, cfg, doctor_lines, history, memory_lines, persona_lines, route_lines, skill_lines`, plus `aliases` and `live_models` for the model picker.
- Produces:
  - `App` with pub fields `transcript: Transcript`, `composer: Composer`, `history_log: History`, `files: FileIndex`, `completion: Option<Completion>`, `completion_idx: usize`, `run: Option<RunHandle>`, `queued: Vec<String>`, `overlay: Overlay`, `scroll_from_bottom: usize`, `status: String`, `should_quit: bool`, `ctrl_c_armed: bool`, `last_esc: Option<Instant>`, `last_submitted: Option<String>`, `pending_suspend: Option<Suspend>`, `theme_id`, `theme`, and `tick`.
  - `enum Overlay { None, Palette { query: String, idx: usize }, Help, Panel(Panel), ModelPicker { idx: usize }, ResumePicker { idx: usize }, Confirm { command: Vec<String> }, Search { query: String } }`.
  - `enum Panel { Memory, Routes, Skills, Doctor, Personas, Claims }`.
  - `enum Suspend { Editor, InteractiveSlash(String) }`.
  - `App::handle_key(&mut self, key: KeyEvent)`, `App::handle_paste(&mut self, text: &str)`, `App::handle_mouse(&mut self, kind: MouseEventKind)`, `App::pump(&mut self)` (drains run events and finishes runs), `App::submit(&mut self)`.

- [ ] **Step 1: Write the failing tests** — `src/tui/tests.rs` (declared as `#[cfg(test)] mod tests;` in `src/tui/mod.rs`):

```rust
use super::app::{App, Overlay};
use crate::agent::{AgentBackend, AgentConfig};
use crate::state::AbbeyState;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

pub(super) fn scratch_app(tag: &str, backend: AgentBackend) -> App {
    let dir = std::env::temp_dir().join(format!("abbey-tui-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("by-cwd")).unwrap();
    let state = AbbeyState {
        state_dir: dir.clone(),
        chat_file: dir.join("chat-id"),
        model_file: dir.join("model"),
        history_file: dir.join("history.log"),
        cwd_dir: dir.join("by-cwd"),
        per_cwd: false,
        cwd: dir,
    };
    App::new(state, AgentConfig { backend, ..AgentConfig::default() }).expect("headless App")
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}
fn press(app: &mut App, code: KeyCode) {
    app.handle_key(key(code, KeyModifiers::NONE));
}
fn ctrl(app: &mut App, c: char) {
    app.handle_key(key(KeyCode::Char(c), KeyModifiers::CONTROL));
}
fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        press(app, KeyCode::Char(c));
    }
}

#[test]
fn shift_enter_and_ctrl_j_insert_newlines_enter_submits() {
    let mut app = scratch_app("newline", AgentBackend::Cursor);
    type_str(&mut app, "a");
    app.handle_key(key(KeyCode::Enter, KeyModifiers::SHIFT));
    type_str(&mut app, "b");
    ctrl(&mut app, 'j');
    type_str(&mut app, "c");
    assert_eq!(app.composer.text, "a\nb\nc");
}

#[test]
fn bracketed_paste_inserts_without_submitting() {
    let mut app = scratch_app("paste", AgentBackend::Cursor);
    app.handle_paste("line one\nline two\n");
    assert_eq!(app.composer.text, "line one\nline two\n");
    assert!(app.run.is_none());
    assert!(app.transcript.cells.is_empty());
}

#[test]
fn ctrl_c_clears_then_quits() {
    let mut app = scratch_app("ctrlc", AgentBackend::Cursor);
    type_str(&mut app, "draft");
    ctrl(&mut app, 'c');
    assert!(app.composer.is_empty());
    assert!(!app.should_quit);
    ctrl(&mut app, 'c');
    assert!(app.should_quit);
}

#[test]
fn shift_tab_cycles_claude_permission_mode() {
    let mut app = scratch_app("perm", AgentBackend::Claude);
    app.handle_key(key(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.cfg.permission_mode.as_deref(), Some("acceptEdits"));
}

#[test]
fn bang_command_requires_confirmation_and_n_cancels() {
    let mut app = scratch_app("bang", AgentBackend::Cursor);
    type_str(&mut app, "!whoami");
    press(&mut app, KeyCode::Enter);
    assert!(matches!(&app.overlay, Overlay::Confirm { command } if command == &vec!["whoami".to_string()]));
    press(&mut app, KeyCode::Char('n'));
    assert!(matches!(app.overlay, Overlay::None));
    assert!(app.transcript.cells.iter().all(|c| !matches!(c, super::transcript::Cell::Assistant(_))));
}

#[test]
fn typing_while_running_queues_and_esc_cancels() {
    let mut app = scratch_app("queue", AgentBackend::Cursor);
    let (ev_tx, events) = std::sync::mpsc::channel();
    let (_done_tx, done) = std::sync::mpsc::channel();
    let cancel = crate::runtime::CancellationToken::new();
    app.run = Some(super::worker::RunHandle { events, done, cancel: cancel.clone(), started: std::time::Instant::now() });
    drop(ev_tx);
    type_str(&mut app, "next thing");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.queued, vec!["next thing".to_string()]);
    press(&mut app, KeyCode::Esc);
    assert!(cancel.is_cancelled());
}

#[test]
fn esc_esc_on_idle_recalls_last_prompt() {
    let mut app = scratch_app("escesc", AgentBackend::Cursor);
    app.last_submitted = Some("previous".into());
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.composer.text, "previous");
}

#[test]
fn ctrl_k_opens_palette_and_esc_closes_it() {
    let mut app = scratch_app("palette", AgentBackend::Cursor);
    ctrl(&mut app, 'k');
    assert!(matches!(app.overlay, Overlay::Palette { .. }));
    press(&mut app, KeyCode::Esc);
    assert!(matches!(app.overlay, Overlay::None));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib tui::tests`
Expected: FAIL (the new `App` API does not exist). The old `keys_tests.rs` and `keys.rs` also stop compiling here. Remove `mod keys;`, `mod ui;`, `mod tabs;` and `mod overlay;` from `src/tui/mod.rs`, and delete the old files in Task 15. Until Task 15, keep `pub use app::run_tui;` compiling with the temporary stub in Step 3.

- [ ] **Step 3: Implement `src/tui/app.rs`** (state and helpers):

```rust
//! Chat-first TUI state. Drawing lives in `render.rs`, keys in `keymap.rs`,
//! the terminal loop in `run_loop.rs`.

use super::completion::{Completion, FileIndex};
use super::composer::{Composer, History};
use super::theme::{Theme, ThemeId};
use super::transcript::Transcript;
use super::worker::{self, RunHandle, RunKind, SlashRoute};
use crate::agent::AgentConfig;
use crate::models;
use crate::state::AbbeyState;
use anyhow::Result;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Panel {
    Memory,
    Routes,
    Skills,
    Doctor,
    Personas,
    Claims,
}

impl Panel {
    pub(crate) const ALL: [Panel; 6] = [Panel::Memory, Panel::Routes, Panel::Skills, Panel::Doctor, Panel::Personas, Panel::Claims];
    pub(crate) fn title(self) -> &'static str {
        match self {
            Panel::Memory => "Memory",
            Panel::Routes => "Routes",
            Panel::Skills => "Skills",
            Panel::Doctor => "Doctor",
            Panel::Personas => "Personas",
            Panel::Claims => "Claims",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Overlay {
    None,
    Palette { query: String, idx: usize },
    Help,
    Panel(Panel),
    ModelPicker { idx: usize },
    ResumePicker { idx: usize },
    Confirm { command: Vec<String> },
    Search { query: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Suspend {
    Editor,
    InteractiveSlash(String),
}

pub struct App {
    pub state: AbbeyState,
    pub cfg: AgentConfig,
    pub theme_id: ThemeId,
    pub theme: Theme,
    pub transcript: Transcript,
    pub composer: Composer,
    pub history_log: History,
    pub files: FileIndex,
    pub completion: Option<Completion>,
    pub completion_idx: usize,
    pub run: Option<RunHandle>,
    pub queued: Vec<String>,
    pub overlay: Overlay,
    pub scroll_from_bottom: usize,
    pub status: String,
    pub should_quit: bool,
    pub ctrl_c_armed: bool,
    pub last_esc: Option<Instant>,
    pub last_submitted: Option<String>,
    pub pending_suspend: Option<Suspend>,
    pub tick: u64,
    pub claims_lines: Vec<String>,
    // Consumed by refresh.rs (kept from the previous TUI) and the model picker.
    pub route_lines: Vec<String>,
    pub doctor_lines: Vec<String>,
    pub history: Vec<crate::state::HistoryEntry>,
    pub aliases: Vec<(String, String)>,
    pub live_models: Vec<String>,
    pub persona_lines: Vec<String>,
    pub memory_lines: Vec<String>,
    pub skill_lines: Vec<String>,
}

impl App {
    pub fn new(state: AbbeyState, mut cfg: AgentConfig) -> Result<Self> {
        cfg.model = state.read_model();
        let theme_id = ThemeId::resolve(&state.state_dir);
        let history_log = History::load(&state.state_dir);
        let files = FileIndex::load(&state.cwd);
        let mut app = Self {
            history: state.history(40),
            aliases: models::alias_table().iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect(),
            state,
            cfg,
            theme_id,
            theme: Theme::from_id(theme_id),
            transcript: Transcript::default(),
            composer: Composer::default(),
            history_log,
            files,
            completion: None,
            completion_idx: 0,
            run: None,
            queued: Vec::new(),
            overlay: Overlay::None,
            scroll_from_bottom: 0,
            status: "Enter send · Shift-Enter newline · Esc interrupt · Ctrl-K palette · F1 help".into(),
            should_quit: false,
            ctrl_c_armed: false,
            last_esc: None,
            last_submitted: None,
            pending_suspend: None,
            tick: 0,
            claims_lines: Vec::new(),
            route_lines: Vec::new(),
            doctor_lines: Vec::new(),
            live_models: Vec::new(),
            persona_lines: Vec::new(),
            memory_lines: Vec::new(),
            skill_lines: Vec::new(),
        };
        app.refresh_doctor();
        app.refresh_personas();
        app.refresh_memory();
        app.refresh_skills();
        if !app.cfg.backend.supports_account_surface() {
            app.refresh_models_live();
        }
        Ok(app)
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    pub fn cycle_theme(&mut self) {
        self.theme_id = self.theme_id.cycle();
        self.theme = Theme::from_id(self.theme_id);
        let _ = ThemeId::save(&self.state.state_dir, self.theme_id);
        self.status = format!("theme → {}", self.theme_id.as_str());
    }

    pub fn refresh_models_live(&mut self) {
        if let Ok(text) = self.cfg.list_models_text() {
            self.live_models = text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        }
    }

    // `cycle_backend`: move verbatim from the previous app.rs (lines 146-168).

    pub fn refresh_all(&mut self) {
        self.refresh_doctor();
        self.refresh_personas();
        self.refresh_memory();
        self.refresh_skills();
        self.history = self.state.history(40);
        self.status = "refreshed".into();
    }

    /// Submit the composer: slash → routed; `!cmd` → confirm; else an agent turn.
    pub fn submit(&mut self) {
        let text = self.composer.text.trim_end().to_string();
        if text.trim().is_empty() {
            return;
        }
        if self.is_running() {
            self.queued.push(text);
            self.composer.take();
            self.status = format!("queued ({})", self.queued.len());
            return;
        }
        self.composer.take();
        self.completion = None;
        self.history_log.push(&text);
        self.last_submitted = Some(text.clone());
        self.scroll_from_bottom = 0;
        if let Some(cmd) = text.strip_prefix('!') {
            let command: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
            if !command.is_empty() {
                self.overlay = Overlay::Confirm { command };
            }
            return;
        }
        if text.starts_with('/') {
            self.run_slash(&text);
            return;
        }
        self.transcript.push_user(&text);
        self.start(RunKind::Prompt { text, fresh: false });
    }

    pub(crate) fn start(&mut self, kind: RunKind) {
        match worker::spawn_run(self.cfg.clone(), self.state.clone(), kind) {
            Ok(handle) => {
                self.run = Some(handle);
                self.status = "running · Esc to interrupt".into();
            }
            Err(e) => self.transcript.push_error(format!("could not start run: {e}")),
        }
    }

    fn run_slash(&mut self, text: &str) {
        self.transcript.push_user(text);
        match worker::route_slash(text) {
            SlashRoute::Agent => self.start(RunKind::AgentSlash(text.to_string())),
            SlashRoute::Interactive => self.pending_suspend = Some(super::app::Suspend::InteractiveSlash(text.to_string())),
            SlashRoute::Local => self.run_local(&[text.to_string()]),
        }
    }

    /// Captured `abbey …` child; output lands in the transcript.
    pub(crate) fn run_local(&mut self, args: &[String]) {
        match worker::run_abbey_capture(args) {
            Ok(out) => {
                let body = format!("{}{}", out.stdout, out.stderr);
                if !body.trim().is_empty() {
                    self.transcript.push_notice(format!("```\n{}\n```", body.trim_end()));
                }
                if out.code != 0 {
                    self.transcript.push_error(format!("exit {}", out.code));
                }
            }
            Err(e) => self.transcript.push_error(format!("{e:#}")),
        }
        self.cfg.model = self.state.read_model();
        self.refresh_all();
    }

    /// Execute a confirmed `!cmd` through the OS allowlist gate.
    pub(crate) fn run_confirmed(&mut self, command: Vec<String>) {
        let mut args = vec!["os".to_string(), "execute".into(), "--confirm".into()];
        args.extend(command);
        self.run_local(&args);
    }

    /// Drain run events; finish the run and start the next queued prompt.
    pub fn pump(&mut self) {
        let Some(run) = &self.run else { return };
        while let Ok(ev) = run.events.try_recv() {
            self.transcript.apply(ev);
        }
        if let Ok((code, err)) = run.done.try_recv() {
            while let Ok(ev) = run.events.try_recv() {
                self.transcript.apply(ev);
            }
            if let Some(e) = err {
                self.transcript.push_error(e);
            }
            let secs = run.started.elapsed().as_secs();
            self.run = None;
            self.status = format!("done · exit {code} · {secs}s");
            self.history = self.state.history(40);
            self.refresh_memory();
            if !self.queued.is_empty() {
                let next = self.queued.remove(0);
                self.composer.set(next);
                self.submit();
            }
        }
    }
}
```

The panels need claims text. Fill `claims_lines` in `refresh_all` and `new` by running `worker::run_abbey_capture(&["claims".into()])` lazily when the Claims panel opens (Task 14). Do not add a claims call to `new`.

`src/tui/keymap.rs`:

```rust
//! Key, paste and mouse handling for the chat TUI.

use super::app::{App, Overlay, Panel, Suspend};
use super::completion;
use super::permission;
use super::worker::RunKind;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use std::time::{Duration, Instant};

const ESC_ESC_WINDOW: Duration = Duration::from_millis(600);

impl App {
    pub fn handle_paste(&mut self, text: &str) {
        if matches!(self.overlay, Overlay::None) {
            self.composer.insert_str(text);
            self.update_completion();
        }
    }

    pub fn handle_mouse(&mut self, kind: MouseEventKind) {
        match kind {
            MouseEventKind::ScrollUp => self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(3),
            MouseEventKind::ScrollDown => self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(3),
            _ => {}
        }
    }

    fn update_completion(&mut self) {
        self.completion = completion::complete(&self.composer.text, self.composer.cursor, self.history_log.entries(), &self.files, None);
        self.completion_idx = 0;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if !(ctrl && key.code == KeyCode::Char('c')) {
            self.ctrl_c_armed = false;
        }
        if !matches!(self.overlay, Overlay::None) {
            self.handle_overlay_key(key);
            return;
        }
        match (key.code, ctrl) {
            (KeyCode::Char('c'), true) => {
                if self.composer.is_empty() && self.ctrl_c_armed {
                    if let Some(run) = &self.run {
                        run.cancel.cancel();
                    }
                    self.should_quit = true;
                } else {
                    self.composer.take();
                    self.completion = None;
                    self.ctrl_c_armed = true;
                    self.status = "Ctrl-C again to quit".into();
                }
            }
            (KeyCode::Char('k'), true) => self.overlay = Overlay::Palette { query: String::new(), idx: 0 },
            (KeyCode::Char('t'), true) => self.cycle_theme(),
            (KeyCode::Char('b'), true) if !self.is_running() => self.cycle_backend(),
            (KeyCode::Char('o'), true) => self.transcript.toggle_last_expandable(),
            (KeyCode::Char('r'), true) => self.overlay = Overlay::Search { query: String::new() },
            (KeyCode::Char('g'), true) => self.pending_suspend = Some(Suspend::Editor),
            (KeyCode::Char('n'), true) if !self.is_running() => {
                self.transcript.push_notice("new chat");
                let text = self.composer.take();
                self.start(RunKind::Prompt { text, fresh: true });
            }
            (KeyCode::Char('p'), true) if !self.is_running() => {
                let input = self.composer.take();
                self.start(RunKind::PleaseFix(input));
            }
            (KeyCode::Char('l'), true) => self.refresh_all(),
            (KeyCode::Char('j'), true) => self.composer.newline(),
            (KeyCode::Char('a'), true) => self.composer.line_start(),
            (KeyCode::Char('e'), true) => self.composer.line_end(),
            (KeyCode::Char('u'), true) => self.composer.kill_line_before(),
            (KeyCode::Char('w'), true) => self.composer.kill_word_before(),
            (KeyCode::F(1), _) => self.overlay = Overlay::Help,
            (KeyCode::Char('?'), false) if self.composer.is_empty() => self.overlay = Overlay::Help,
            (KeyCode::BackTab, _) => self.status = format!("permission → {}", permission::cycle(&mut self.cfg)),
            (KeyCode::Esc, _) => self.on_esc(),
            (KeyCode::Enter, _) if key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => self.composer.newline(),
            (KeyCode::Enter, _) | (KeyCode::Tab, _) if self.completion.is_some() => self.accept_completion(),
            (KeyCode::Enter, _) => self.submit(),
            (KeyCode::Up, _) if self.completion.is_some() => self.completion_idx = self.completion_idx.saturating_sub(1),
            (KeyCode::Down, _) if self.completion.is_some() => self.completion_idx = self.completion_idx.saturating_add(1),
            (KeyCode::Up, _) => {
                if !self.composer.up() && let Some(prev) = self.history_log.prev(&self.composer.text) {
                    self.composer.set(prev);
                }
            }
            (KeyCode::Down, _) => {
                if !self.composer.down() && let Some(next) = self.history_log.next() {
                    self.composer.set(next);
                }
            }
            (KeyCode::Left, _) if key.modifiers.contains(KeyModifiers::ALT) => self.composer.word_left(),
            (KeyCode::Right, _) if key.modifiers.contains(KeyModifiers::ALT) => self.composer.word_right(),
            (KeyCode::Left, _) => self.composer.left(),
            (KeyCode::Right, _) => self.composer.right(),
            (KeyCode::Home, _) => self.composer.line_start(),
            (KeyCode::End, _) => self.composer.line_end(),
            (KeyCode::PageUp, _) => self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(10),
            (KeyCode::PageDown, _) => self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(10),
            (KeyCode::Backspace, _) => {
                self.composer.backspace();
                self.update_completion();
            }
            (KeyCode::Delete, _) => self.composer.delete(),
            (KeyCode::Char(c), false) => {
                self.composer.insert_char(c);
                self.update_completion();
            }
            _ => {}
        }
    }

    fn on_esc(&mut self) {
        if self.completion.take().is_some() {
            return;
        }
        if let Some(run) = &self.run {
            run.cancel.cancel();
            self.status = "interrupting…".into();
            return;
        }
        let now = Instant::now();
        if self.last_esc.is_some_and(|t| now.duration_since(t) < ESC_ESC_WINDOW) {
            if let Some(prev) = self.last_submitted.clone() {
                self.composer.set(prev);
            }
            self.last_esc = None;
        } else {
            self.last_esc = Some(now);
        }
    }

    fn accept_completion(&mut self) {
        let Some(c) = self.completion.take() else { return };
        let (text, cursor) = completion::accept(&self.composer.text, self.composer.cursor, &c, self.completion_idx);
        self.composer.text = text;
        self.composer.cursor = cursor;
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) {
        let overlay = std::mem::replace(&mut self.overlay, Overlay::None);
        self.overlay = match (overlay, key.code) {
            (_, KeyCode::Esc) => Overlay::None,
            (Overlay::Confirm { command }, KeyCode::Char('y' | 'Y')) => {
                self.transcript.push_user(&format!("!{}", command.join(" ")));
                self.run_confirmed(command);
                Overlay::None
            }
            (Overlay::Confirm { .. }, _) => {
                self.status = "command not run".into();
                Overlay::None
            }
            (Overlay::Search { mut query }, KeyCode::Char(c)) => {
                query.push(c);
                Overlay::Search { query }
            }
            (Overlay::Search { mut query }, KeyCode::Backspace) => {
                query.pop();
                Overlay::Search { query }
            }
            (Overlay::Search { query }, KeyCode::Enter) => {
                if let Some(hit) = self.history_log.search(&query).map(str::to_string) {
                    self.composer.set(hit);
                }
                Overlay::None
            }
            (other, code) => self.overlay_nav(other, code),
        };
    }
}
```

`overlay_nav`, which handles the palette, pickers and panels, is implemented in Task 14 in `overlays.rs` as `impl App { pub(crate) fn overlay_nav(&mut self, o: Overlay, code: KeyCode) -> Overlay }`. For this task, add a temporary stub `pub(crate) fn overlay_nav(&mut self, o: Overlay, _code: KeyCode) -> Overlay { o }` at the bottom of `keymap.rs`; Task 14 removes it. Also add a temporary `pub fn run_tui(_: AbbeyState, _: AgentConfig) -> anyhow::Result<i32> { Ok(0) }` in `app.rs`, which Task 15 replaces.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib tui::`
Expected: keymap tests PASS; transcript/composer/completion/markdown tests still PASS.

- [ ] **Step 5: Commit**

```bash
git add src/tui/app.rs src/tui/keymap.rs src/tui/tests.rs src/tui/mod.rs
git commit -m "feat(tui): chat App state and keymap (queue, interrupt, confirm, permission)"
```

---

### Task 14: Rendering and overlays

**Files:**
- Create: `src/tui/render.rs`, `src/tui/overlays.rs`
- Modify: `src/tui/keymap.rs` (remove the `overlay_nav` stub)
- Test: `src/tui/tests.rs` (render section)

**Interfaces:**
- Consumes: `App` (Task 13), `transcript.lines`, `widgets::{rounded_block, dim_style, accent_style}`, `permission::label`.
- Produces:
  - `render::draw(f: &mut Frame, app: &App)`.
  - `overlays::draw(f: &mut Frame, area: Rect, app: &App)`.
  - `App::overlay_nav`.
  - `overlays::palette_items() -> Vec<PaletteItem>` and `overlays::fuzzy_filter`, both moved verbatim from the old `overlay.rs`. Add `PaletteAction::OpenPanel(Panel)`, `PaletteAction::ModelPicker` and `PaletteAction::ResumePicker`, and replace `GotoDoctor` with `OpenPanel(Panel::Doctor)`.

- [ ] **Step 1: Write the failing render tests** (append to `src/tui/tests.rs`):

```rust
use ratatui::{Terminal, backend::TestBackend};

fn screen(app: &App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| super::render::draw(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect()
}

#[test]
fn transcript_shows_user_markdown_and_tool_cells() {
    let mut app = scratch_app("render", AgentBackend::Claude);
    app.transcript.push_user("fix the bug");
    app.transcript.apply(crate::stream::StreamEvent::TextDelta("**Done** — see `a.rs`".into()));
    app.transcript.apply(crate::stream::StreamEvent::ToolStart { id: "t".into(), name: "Bash".into(), input: serde_json::json!({"command":"cargo test"}) });
    let s = screen(&app, 80, 24);
    assert!(s.contains("› fix the bug"));
    assert!(s.contains("Done — see a.rs"));
    assert!(s.contains("Bash  cargo test"));
    assert!(s.contains("claude"), "header names the backend");
    assert!(s.contains("default"), "header names the permission mode");
}

#[test]
fn narrow_terminal_renders_without_panic_and_keeps_the_composer() {
    let mut app = scratch_app("narrow", AgentBackend::Ollama);
    app.composer.set("hello\nworld".into());
    let s = screen(&app, 40, 12);
    assert!(s.contains("hello"));
    assert!(s.contains("world"));
}

#[test]
fn overlays_render_on_top() {
    let mut app = scratch_app("overlays", AgentBackend::Cursor);
    for o in [
        Overlay::Help,
        Overlay::Palette { query: String::new(), idx: 0 },
        Overlay::Panel(super::app::Panel::Doctor),
        Overlay::ModelPicker { idx: 0 },
        Overlay::ResumePicker { idx: 0 },
        Overlay::Confirm { command: vec!["whoami".into()] },
        Overlay::Search { query: "x".into() },
    ] {
        app.overlay = o;
        let s = screen(&app, 80, 24);
        assert!(!s.trim().is_empty());
    }
    app.overlay = Overlay::Confirm { command: vec!["whoami".into()] };
    assert!(screen(&app, 80, 24).contains("whoami"));
}

#[test]
fn scrolling_up_hides_the_newest_line() {
    let mut app = scratch_app("scroll", AgentBackend::Cursor);
    for i in 0..60 {
        app.transcript.push_notice(format!("line-{i}"));
    }
    assert!(screen(&app, 60, 20).contains("line-59"));
    app.scroll_from_bottom = 30;
    assert!(!screen(&app, 60, 20).contains("line-59"));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib tui::tests`
Expected: FAIL (no `render` module).

- [ ] **Step 3: Implement `src/tui/render.rs`**:

```rust
//! Layout: header · transcript · completion menu · composer · status line.

use super::app::{App, Overlay};
use super::completion::Completion;
use super::{overlays, permission, widgets};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub(crate) fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let composer_rows = u16::try_from(app.composer.text.split('\n').count()).unwrap_or(1).clamp(1, 8) + 2;
    let menu_rows = match &app.completion {
        Some(Completion::Slash(p)) => u16::try_from(p.len().min(6)).unwrap_or(0),
        Some(Completion::File { items, .. }) => u16::try_from(items.len().min(6)).unwrap_or(0),
        None => 0,
    };
    let [header, body, menu, composer, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(menu_rows),
        Constraint::Length(composer_rows),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(f, header, app);
    draw_transcript(f, body, app);
    draw_menu(f, menu, app);
    draw_composer(f, composer, app);
    draw_status(f, status, app);
    if !matches!(app.overlay, Overlay::None) {
        overlays::draw(f, area, app);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let accent = widgets::accent_style(&app.theme);
    let dim = widgets::dim_style(&app.theme);
    let line = Line::from(vec![
        Span::styled(" abbey ", accent.add_modifier(Modifier::BOLD)),
        Span::styled("· ", dim),
        Span::raw(app.cfg.backend.label().to_string()),
        Span::styled(" · ", dim),
        Span::raw(app.cfg.model.clone()),
        Span::styled(" · perm ", dim),
        Span::raw(permission::label(&app.cfg)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_transcript(f: &mut Frame, area: Rect, app: &App) {
    let lines = app.transcript.lines(&app.theme, area.width.saturating_sub(1));
    let height = usize::from(area.height);
    let max_scroll = lines.len().saturating_sub(height);
    let from_bottom = app.scroll_from_bottom.min(max_scroll);
    let start = lines.len().saturating_sub(height + from_bottom);
    let visible: Vec<Line<'static>> = lines.into_iter().skip(start).take(height).collect();
    f.render_widget(Paragraph::new(visible), area);
    if from_bottom > 0 {
        let hint = Rect { x: area.x, y: area.bottom().saturating_sub(1), width: area.width, height: 1 };
        f.render_widget(Paragraph::new(Line::styled(format!("↓ {from_bottom} lines below · PgDn"), widgets::dim_style(&app.theme))), hint);
    }
}

fn draw_menu(f: &mut Frame, area: Rect, app: &App) {
    if area.height == 0 {
        return;
    }
    let rows: Vec<(String, String)> = match &app.completion {
        Some(Completion::Slash(p)) => p.iter().map(|p| (format!("/{}", p.name), p.help.to_string())).collect(),
        Some(Completion::File { items, .. }) => items.iter().map(|i| (format!("@{i}"), String::new())).collect(),
        None => Vec::new(),
    };
    let sel = app.completion_idx.min(rows.len().saturating_sub(1));
    let lines: Vec<Line<'static>> = rows
        .into_iter()
        .enumerate()
        .take(usize::from(area.height))
        .map(|(i, (a, b))| {
            let style = if i == sel { widgets::list_highlight_style(&app.theme) } else { Style::default() };
            Line::from(vec![Span::styled(format!(" {a} "), style), Span::styled(b, widgets::dim_style(&app.theme))])
        })
        .collect();
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_composer(f: &mut Frame, area: Rect, app: &App) {
    let title = if app.is_running() { "queue a message" } else { "message" };
    let block = widgets::rounded_block(title, &app.theme, true);
    let inner = block.inner(area);
    let lines: Vec<Line<'static>> = app.composer.text.split('\n').map(|l| Line::raw(l.to_string())).collect();
    let (row, col) = app.composer.cursor_row_col();
    let visible = usize::from(inner.height.max(1));
    let skip = row.saturating_sub(visible - 1);
    f.render_widget(Paragraph::new(lines.into_iter().skip(skip).collect::<Vec<_>>()).block(block), area);
    let x = inner.x + u16::try_from(col).unwrap_or(0).min(inner.width.saturating_sub(1));
    let y = inner.y + u16::try_from(row - skip).unwrap_or(0);
    if matches!(app.overlay, Overlay::None) {
        f.set_cursor_position((x, y));
    }
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let dim = widgets::dim_style(&app.theme);
    let mut spans = Vec::new();
    if let Some(run) = &app.run {
        let frame = SPINNER[usize::try_from(app.tick / 2).unwrap_or(0) % SPINNER.len()];
        spans.push(Span::styled(format!(" {frame} {}s ", run.started.elapsed().as_secs()), widgets::accent_style(&app.theme)));
    }
    let tokens = app.transcript.usage.map_or_else(|| "n/a".to_string(), |(i, o)| format!("{i}↑ {o}↓"));
    spans.push(Span::styled(format!(" tokens {tokens} "), dim));
    if !app.queued.is_empty() {
        spans.push(Span::styled(format!(" queued {} ", app.queued.len()), dim));
    }
    spans.push(Span::raw(format!(" {}", app.status)));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
```

`AgentBackend::label()` already exists; `cycle_backend` uses it.

- [ ] **Step 4: Implement `src/tui/overlays.rs`.**

  First, move `PaletteItem`, `BUILTIN`, `palette_items`, `fuzzy_filter`, `help_lines` and `centered` verbatim from the old `src/tui/overlay.rs`. Then edit `PaletteAction`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    Slash(&'static str),
    NewChat,
    PleaseFix,
    CycleBackend,
    Refresh,
    CycleTheme,
    OpenPanel(super::app::Panel),
    ModelPicker,
    ResumePicker,
    Quit,
}
```

  In `BUILTIN`, replace the `doctor` item's action with `PaletteAction::OpenPanel(Panel::Doctor)` and append:

```rust
    PaletteItem { id: "model", label: "Switch model", detail: "Pick from the live model list", action: PaletteAction::ModelPicker },
    PaletteItem { id: "resume", label: "Resume chat", detail: "Pick a previous conversation", action: PaletteAction::ResumePicker },
    PaletteItem { id: "memory", label: "Memory", detail: "Memory layers panel", action: PaletteAction::OpenPanel(Panel::Memory) },
    PaletteItem { id: "routes", label: "Routes", detail: "Route audit tail", action: PaletteAction::OpenPanel(Panel::Routes) },
    PaletteItem { id: "skills", label: "Skills", detail: "Skills and plugins inventory", action: PaletteAction::OpenPanel(Panel::Skills) },
    PaletteItem { id: "personas", label: "Personas", detail: "Persona and role bindings", action: PaletteAction::OpenPanel(Panel::Personas) },
    PaletteItem { id: "claims", label: "Claims", detail: "Capability ledger", action: PaletteAction::OpenPanel(Panel::Claims) },
```

  Update `help_lines()` to list this keymap:
  - Enter send; Shift/Alt-Enter or Ctrl-J newline; Esc interrupt, or Esc Esc to recall the last prompt.
  - Ctrl-C clear, then quit; Ctrl-K palette; Ctrl-R history search; Ctrl-G `$EDITOR`; Ctrl-O expand tool/thinking.
  - Shift-Tab permission mode; Ctrl-B backend; Ctrl-T theme; Ctrl-N new chat; Ctrl-P please-fix; Ctrl-L refresh.
  - PgUp/PgDn/wheel scroll; `@` file completion; `/` commands; `!cmd` allowlisted OS command (asks first).

  Then add:

```rust
use super::app::{App, Overlay, Panel};
use super::widgets;
use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};

fn panel_lines(app: &App, p: Panel) -> Vec<String> {
    match p {
        Panel::Memory => app.memory_lines.clone(),
        Panel::Routes => app.route_lines.clone(),
        Panel::Skills => app.skill_lines.clone(),
        Panel::Doctor => app.doctor_lines.clone(),
        Panel::Personas => app.persona_lines.clone(),
        Panel::Claims => app.claims_lines.clone(),
    }
}

fn model_rows(app: &App) -> Vec<String> {
    if app.live_models.is_empty() {
        app.aliases.iter().map(|(a, b)| format!("{a}  {b}")).collect()
    } else {
        app.live_models.clone()
    }
}

fn list(f: &mut Frame, area: Rect, app: &App, title: &str, rows: &[String], sel: Option<usize>) {
    let r = centered(area, area.width.saturating_sub(8).min(100), area.height.saturating_sub(4).min(30));
    let lines: Vec<Line<'static>> = rows
        .iter()
        .enumerate()
        .map(|(i, s)| if Some(i) == sel { Line::styled(s.clone(), widgets::list_highlight_style(&app.theme)) } else { Line::raw(s.clone()) })
        .collect();
    let skip = sel.map_or(0, |s| s.saturating_sub(usize::from(r.height.saturating_sub(3))));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(lines.into_iter().skip(skip).collect::<Vec<_>>()).wrap(Wrap { trim: false }).block(widgets::rounded_block(title, &app.theme, true)), r);
}

pub(crate) fn draw(f: &mut Frame, area: Rect, app: &App) {
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => list(f, area, app, "Help · Esc", &help_lines().into_iter().map(str::to_string).collect::<Vec<_>>(), None),
        Overlay::Palette { query, idx } => {
            let items = fuzzy_filter(&palette_items(), query);
            let mut rows = vec![format!("> {query}")];
            rows.extend(items.iter().map(|i| format!("{}  {}", i.label, i.detail)));
            list(f, area, app, "Command palette", &rows, Some(idx + 1));
        }
        Overlay::Panel(p) => list(f, area, app, &format!("{} · Esc", p.title()), &panel_lines(app, *p), None),
        Overlay::ModelPicker { idx } => list(f, area, app, "Model · Enter select", &model_rows(app), Some(*idx)),
        Overlay::ResumePicker { idx } => {
            let rows: Vec<String> = app.history.iter().map(|h| format!("{}  {}  {}", h.timestamp, h.chat_id, h.cwd)).collect();
            list(f, area, app, "Resume · Enter select", &rows, Some(*idx));
        }
        Overlay::Confirm { command } => list(
            f,
            area,
            app,
            "Run OS command?",
            &[format!("abbey os execute --confirm {}", command.join(" ")), String::new(), "Allowlist only. y = run · any other key = cancel".into()],
            None,
        ),
        Overlay::Search { query } => {
            let hit = app.history_log.search(query).unwrap_or("(no match)").to_string();
            list(f, area, app, "History search · Enter accept", &[format!("> {query}"), hit], None);
        }
    }
}

impl App {
    pub(crate) fn overlay_nav(&mut self, o: Overlay, code: KeyCode) -> Overlay {
        match (o, code) {
            (Overlay::Palette { mut query, idx }, KeyCode::Char(c)) => {
                query.push(c);
                let _ = idx;
                Overlay::Palette { query, idx: 0 }
            }
            (Overlay::Palette { mut query, .. }, KeyCode::Backspace) => {
                query.pop();
                Overlay::Palette { query, idx: 0 }
            }
            (Overlay::Palette { query, idx }, KeyCode::Down) => Overlay::Palette { query, idx: idx + 1 },
            (Overlay::Palette { query, idx }, KeyCode::Up) => Overlay::Palette { query, idx: idx.saturating_sub(1) },
            (Overlay::Palette { query, idx }, KeyCode::Enter) => {
                let items = fuzzy_filter(&palette_items(), &query);
                match items.get(idx).map(|i| i.action) {
                    Some(a) => self.palette_action(a),
                    None => Overlay::None,
                }
            }
            (Overlay::ModelPicker { idx }, KeyCode::Down) => Overlay::ModelPicker { idx: idx + 1 },
            (Overlay::ModelPicker { idx }, KeyCode::Up) => Overlay::ModelPicker { idx: idx.saturating_sub(1) },
            (Overlay::ModelPicker { idx }, KeyCode::Enter) => {
                if let Some(row) = model_rows(self).get(idx) {
                    let name = row.split_whitespace().next().unwrap_or("").to_string();
                    self.run_local(&[format!("/model {name}")]);
                }
                Overlay::None
            }
            (Overlay::ResumePicker { idx }, KeyCode::Down) => Overlay::ResumePicker { idx: idx + 1 },
            (Overlay::ResumePicker { idx }, KeyCode::Up) => Overlay::ResumePicker { idx: idx.saturating_sub(1) },
            (Overlay::ResumePicker { idx }, KeyCode::Enter) => {
                if let Some(h) = self.history.get(idx) {
                    let id = h.chat_id.clone();
                    match self.state.save_chat(&id) {
                        Ok(()) => self.transcript.push_notice(format!("resumed chat {id}")),
                        Err(e) => self.transcript.push_error(format!("{e:#}")),
                    }
                }
                Overlay::None
            }
            (Overlay::Panel(p), KeyCode::Tab) => {
                let i = Panel::ALL.iter().position(|x| *x == p).unwrap_or(0);
                self.open_panel(Panel::ALL[(i + 1) % Panel::ALL.len()])
            }
            (Overlay::Help | Overlay::Panel(_), _) => Overlay::None,
            (other, _) => other,
        }
    }

    pub(crate) fn open_panel(&mut self, p: Panel) -> Overlay {
        if p == Panel::Claims && self.claims_lines.is_empty() {
            if let Ok(out) = super::worker::run_abbey_capture(&["claims".into()]) {
                self.claims_lines = out.stdout.lines().map(str::to_string).collect();
            }
        }
        Overlay::Panel(p)
    }

    fn palette_action(&mut self, a: PaletteAction) -> Overlay {
        use super::worker::RunKind;
        match a {
            PaletteAction::Slash(name) => {
                self.composer.set(format!("/{name} "));
                Overlay::None
            }
            PaletteAction::NewChat => {
                self.transcript.push_notice("new chat");
                self.start(RunKind::Prompt { text: String::new(), fresh: true });
                Overlay::None
            }
            PaletteAction::PleaseFix => {
                let input = self.composer.take();
                self.start(RunKind::PleaseFix(input));
                Overlay::None
            }
            PaletteAction::CycleBackend => {
                self.cycle_backend();
                Overlay::None
            }
            PaletteAction::Refresh => {
                self.refresh_all();
                Overlay::None
            }
            PaletteAction::CycleTheme => {
                self.cycle_theme();
                Overlay::None
            }
            PaletteAction::OpenPanel(p) => self.open_panel(p),
            PaletteAction::ModelPicker => Overlay::ModelPicker { idx: 0 },
            PaletteAction::ResumePicker => {
                self.history = self.state.history(40);
                Overlay::ResumePicker { idx: 0 }
            }
            PaletteAction::Quit => {
                self.should_quit = true;
                Overlay::None
            }
        }
    }
}
```

  Clamp every picker `idx` to the row count at draw and select time. `rows.get(idx)` already returns `None` safely. Finally, remove the `overlay_nav` stub from `keymap.rs` and add `mod overlays; mod render;` to `src/tui/mod.rs`.

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --lib tui::`
Expected: all TUI tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/tui/render.rs src/tui/overlays.rs src/tui/keymap.rs src/tui/tests.rs src/tui/mod.rs
git commit -m "feat(tui): chat layout, completion menu, palette, pickers and panels"
```

---

### Task 15: Terminal loop, suspend, cut-over

**Files:**
- Create: `src/tui/run_loop.rs`
- Modify: `src/tui/mod.rs`, `src/tui/app.rs` (remove the `run_tui` stub)
- Delete: `src/tui/tabs.rs`, `src/tui/ui.rs`, `src/tui/keys.rs`, `src/tui/keys_tests.rs`, `src/tui/overlay.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `pub fn run_tui(state: AbbeyState, cfg: AgentConfig) -> Result<i32>`, called from `src/entry.rs:67` with the signature unchanged.

- [ ] **Step 1: Write the failing test** (append to `src/tui/tests.rs`):

```rust
#[test]
fn pump_applies_events_finishes_and_dequeues() {
    let mut app = scratch_app("pump", AgentBackend::Cursor);
    let (ev_tx, events) = std::sync::mpsc::channel();
    let (done_tx, done) = std::sync::mpsc::channel();
    app.run = Some(super::worker::RunHandle { events, done, cancel: crate::runtime::CancellationToken::new(), started: std::time::Instant::now() });
    ev_tx.send(crate::stream::StreamEvent::TextDelta("answer".into())).unwrap();
    ev_tx.send(crate::stream::StreamEvent::Done { exit: 0 }).unwrap();
    done_tx.send((0, None)).unwrap();
    app.pump();
    assert!(app.run.is_none());
    assert!(matches!(app.transcript.cells.last(), Some(super::transcript::Cell::Assistant(s)) if s == "answer"));
    assert!(app.status.contains("exit 0"));
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test --lib tui::tests::pump_applies_events_finishes_and_dequeues`
Expected: PASS already (from Task 13's `pump`). If it fails, fix `pump`, not the test.

- [ ] **Step 3: Implement `src/tui/run_loop.rs`**:

```rust
//! Terminal lifecycle: alternate screen, bracketed paste, mouse, the event
//! loop, and suspend/resume for `$EDITOR` and interactive slash commands.

use super::app::{App, Suspend};
use crate::agent::AgentConfig;
use crate::state::AbbeyState;
use anyhow::Result;
use crossterm::event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{Stdout, stdout};
use std::time::Duration;

type Term = Terminal<CrosstermBackend<Stdout>>;

fn enter() -> Result<Term> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    Ok(Terminal::new(CrosstermBackend::new(out))?)
}

fn leave(term: &mut Term) -> Result<()> {
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen, DisableMouseCapture, DisableBracketedPaste)?;
    term.show_cursor()?;
    Ok(())
}

fn suspend(term: &mut Term, app: &mut App, what: Suspend) -> Result<()> {
    leave(term)?;
    match what {
        Suspend::Editor => {
            let path = app.state.state_dir.join("tui-compose.md");
            std::fs::write(&path, &app.composer.text)?;
            let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "vi".into());
            let mut parts = editor.split_whitespace();
            let bin = parts.next().unwrap_or("vi").to_string();
            let status = std::process::Command::new(bin).args(parts).arg(&path).status();
            match status {
                Ok(s) if s.success() => app.composer.set(std::fs::read_to_string(&path)?.trim_end().to_string()),
                Ok(s) => app.status = format!("editor exited {s}"),
                Err(e) => app.status = format!("editor: {e}"),
            }
        }
        Suspend::InteractiveSlash(cmd) => {
            let code = crate::slash_dispatch::dispatch_slash(&cmd, &app.state, &mut app.cfg).unwrap_or(1);
            app.transcript.push_notice(format!("{cmd} → exit {code}"));
        }
    }
    enable_raw_mode()?;
    execute!(term.backend_mut(), EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    term.clear()?;
    Ok(())
}

pub fn run_tui(state: AbbeyState, cfg: AgentConfig) -> Result<i32> {
    let mut app = App::new(state, cfg)?;
    let mut term = enter()?;
    let result = (|| -> Result<i32> {
        let mut was_running = false;
        loop {
            app.pump();
            if was_running && !app.is_running() {
                // Anything a child wrote to the real terminal is repainted away.
                term.clear()?;
            }
            was_running = app.is_running();
            term.draw(|f| super::render::draw(f, &app))?;
            if let Some(what) = app.pending_suspend.take() {
                suspend(&mut term, &mut app, what)?;
            }
            if app.should_quit {
                return Ok(0);
            }
            let wait = if app.is_running() { 33 } else { 100 };
            if event::poll(Duration::from_millis(wait))? {
                match event::read()? {
                    Event::Key(k) => app.handle_key(k),
                    Event::Paste(s) => app.handle_paste(&s),
                    Event::Mouse(m) => app.handle_mouse(m.kind),
                    _ => {}
                }
            }
            app.tick = app.tick.wrapping_add(1);
        }
    })();
    if let Some(run) = &app.run {
        run.cancel.cancel();
    }
    leave(&mut term)?;
    result
}
```

Set `src/tui/mod.rs` to:

```rust
//! Abbey chat-first TUI (ratatui + crossterm).

mod app;
mod completion;
mod composer;
mod keymap;
pub(crate) mod markdown;
mod overlays;
mod permission;
mod predict;
mod refresh;
mod render;
mod run_loop;
pub(crate) mod theme;
mod transcript;
mod widgets;
mod worker;

#[cfg(test)]
mod tests;

pub use run_loop::run_tui;
```

Then:
- Remove the `run_tui` stub from `app.rs`.
- Delete the old files: `git rm src/tui/tabs.rs src/tui/ui.rs src/tui/keys.rs src/tui/keys_tests.rs src/tui/overlay.rs`.
- If `refresh.rs` or `predict.rs` reference `super::tabs` or old `App` fields, repoint them to the new names. Their fields were kept; `Tab`/`Focus` usages in `refresh.rs` are removed.
- If the predict module's LLM rerank helpers (`spawn_llm_hint`, `LlmHint`) are now unused, keep them `pub(super)` and add `#[allow(dead_code)]` only if clippy flags them. Better: pass `None` for `llm_boost` as Task 13 does, and delete `spawn_llm_hint`/`LlmHint`/`InFlightGuard` together with their tests if nothing calls them. Record which you chose in the commit message.

- [ ] **Step 4: Build, lint, and run the tests**

Run: `cargo clippy --all-targets -- -D warnings && cargo test --lib tui::`
Expected: clean clippy; all TUI tests PASS.

- [ ] **Step 5: Check file sizes**

Run: `wc -l src/tui/*.rs src/stream/*.rs src/agent/streaming.rs | sort -n | tail -6`
Expected: every file < 600 (`tui`) / < 400 (`stream`). If one is over, split it along its existing sections before committing.

- [ ] **Step 6: Commit**

```bash
git add -A src/tui
git commit -m "feat(tui): chat-first terminal loop with paste, mouse, editor and suspend; remove tab dashboard"
```

---

### Task 16: Claims, docs, gate, live smoke

**Files:**
- Modify: `src/claims/registry.rs` (append two rows before the first `Proposed` entry, matching the existing entry syntax), `docs/claims.md` + ledger headers (generated)
- Modify: `CLAUDE.md` (module map: add a `stream/` row, update the `tui/` row), `AGENTS.md` (execution-path section), `docs/architecture.md` (execution-path diagram note)
- Test: `tests/cli_surface.rs`

**Interfaces:**
- Consumes: the finished feature.

- [ ] **Step 1: Add the regression test** for the unchanged CLI output (append to `tests/cli_surface.rs`, reusing that file's existing `ABBEY_STATE_DIR` scratch helper; grep `fn scratch` or `ABBEY_STATE_DIR` in the file for its name):

```rust
#[test]
fn slash_help_output_is_unchanged_by_streaming_support() {
    let out = abbey_cmd_in_scratch(&["/help"]).output().expect("run abbey /help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("/help"));
    assert!(!stdout.contains("\"type\":"), "no stream JSON may leak into CLI output");
}
```

`abbey_cmd_in_scratch` stands for the existing helper that builds `Command::new(env!("CARGO_BIN_EXE_abbey"))` with a throwaway `ABBEY_STATE_DIR`. Use that helper's real name.

- [ ] **Step 2: Add the claims rows** in `src/claims/registry.rs`, copying the field layout of the neighbouring `backend-claude-code` entry:
  - `tui-chat-streaming-transcript`, status Current, title "chat-first TUI with live streaming transcript". Evidence:

    > the ratatui TUI renders a scrolling transcript fed by typed stream events through the canonical run path (one route.jsonl row per turn, resume/retry unchanged); multiline composer with persisted history and Ctrl-R search, @file fuzzy completion (mention text only, no attachment), slash completion from the catalog, Esc interrupt via process-group teardown (exit 130, no resume retry), queued type-ahead, Shift-Tab executor permission mode (claude modes, cursor --force; others executor-managed), !cmd through the os allowlist only after a y confirmation, and today's panels as overlays. Evidence is ratatui TestBackend rendering and keymap tests plus a manual live smoke; token usage shows only when the executor reports it; non-Unix hosts buffer the run and deliver it at exit.

  - `executor-stream-adapters`, status Current, title "executor stream decoders (claude, cursor-agent, grok, plain)". Evidence:

    > NDJSON decoders for claude stream-json with partial messages, cursor-agent stream-json with partial output, and grok streaming-json ACP updates, plus a UTF-8- and ANSI-safe plain decoder for ollama, fm and abi. Tests cover split chunks, split code points, malformed and unknown lines, and dated live captures in tests/fixtures/streams (recorded 2026-09-29). Vendor wire formats may change and are not guaranteed; abi's live stream is untested because abi is not installed on the recording host.

  - `tui-executor-host-approvals`, status Proposed, title "in-TUI approve/deny for executor permission prompts". Evidence:

    > approved product direction; approvals remain with each executor's permission mode, which the TUI shows and switches. Claude's host-answered permission protocol is not integrated.

  Then run the file-size guard on the registry: `wc -l src/claims/registry.rs`. The check warns above 800 lines and fails above 1000. If it is over 1000, move the new rows into a new `src/claims/registry_tui.rs`, chained into the registry list the same way the existing registry is assembled. Read the top of `registry.rs` for the pattern.

- [ ] **Step 3: Regenerate and verify the ledgers**

Run: `python3 tools/check_claims_sync.py --write && python3 tools/check_claims_sync.py`
Expected: `OK` and the headers in `AGENTS.md`/`CLAUDE.md` show the new counts.

- [ ] **Step 4: Update the docs.**
  - **`CLAUDE.md`, command table:** add `cargo test --lib tui::` and `cargo test --lib stream::`.
  - **`CLAUDE.md`, module map:** set the `tui/` row to "chat-first ratatui app: streaming transcript, multiline composer, @file/slash completion, overlays (panels, pickers, palette), keymap; `predict.rs` is ranked slash prediction". Add a `stream/` row: "executor stream decoders (claude/cursor/grok NDJSON, plain text) → `StreamEvent`; `StreamTap` carries events and cancellation for streaming runs". Add `streaming.rs` to the `agent/` row text.
  - **`AGENTS.md`, execution path:** add a paragraph. With `RunSpec::streaming(tap)`, `run_once` runs the executor under the supervisor with a stdout tap and decodes it live; this is a sink on the canonical path, not a bypass, so persona wrap and the route-log row still apply. Every diagnostic on that path goes through `AgentConfig::notice`, never a bare `eprintln!`.
  - **`docs/architecture.md`:** add the same one-line note to the execution-path section.

  Run: `python3 tools/check_instructions.py`
  Expected: `instructions: OK`.

- [ ] **Step 5: Run the full gate**

Run: `./check.sh >| /private/tmp/abbey-gate.log 2>&1; echo EXIT:$?; tail -5 /private/tmp/abbey-gate.log`
Expected: `EXIT:0` and the log's own final verdict line reports success across all four modes. Fix the first failure and rerun until green. Do not pipe `./check.sh`.

- [ ] **Step 6: Live smoke in a real TTY** (the human partner, or the engineer in a real terminal):
  1. `cargo build && ABBEY_BACKEND=ollama ./target/debug/abbey`: type `say hi in five words`. Text streams in place, and the status shows `tokens n/a`.
  2. Type a long prompt, press Enter, then Esc mid-stream. The transcript shows `⏹ interrupted`, and `pgrep -fl 'ollama run'` right after shows no child from this run.
  3. `ABBEY_BACKEND=claude ./target/debug/abbey`: ask `read Cargo.toml and name the crate`. A `● Read  Cargo.toml` cell turns into `✓`, and the token counts appear.
  4. Press Shift-Tab. The header permission mode changes.
  5. Type `!whoami`, then y. The output appears in the transcript. Type `!rm -rf x`, then y. The os gate denies it: `denied: … not on the OS-control allowlist`.
  6. Quit and relaunch, then press Up. Last session's prompt appears (persisted history).

  Record what was and was not verified live in the commit message body.

- [ ] **Step 7: Commit**

```bash
git add src/claims tests/cli_surface.rs docs/claims.md AGENTS.md CLAUDE.md docs/architecture.md
git commit -m "docs(claims): chat TUI streaming and executor stream adapters"
```

---

## Out of this plan

- **Claude host-answered approvals** (`--permission-prompts host` with `--input-format stream-json`). The control-message shape has not been recorded, so this stays a Proposed claim. It gets its own plan after a live capture.
- **Word-boundary wrapping.** The transcript uses exact character wrapping in this plan.
