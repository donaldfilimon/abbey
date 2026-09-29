# Abbey chat-first TUI redesign

Status: Design approved on 2026-09-29; spec awaiting review

## Purpose

Rebuild Abbey's ratatui TUI as a chat-first interface with the core
interaction loop of Codex, Claude Code, and OpenCode: a scrolling transcript
that streams the executor's output, shows tool activity inline, and can be
interrupted, driven from a multiline composer. Today's dashboard tabs remain
available as overlays.

Parity here means Abbey's own interface reaches that interaction loop over
the executors it already wires. It does not reimplement any vendor runtime
(`oos-reimplement-vendor-runtimes`), and `surface-parity-grok-codex-claude`
stays Partial.

## Current state (measured 2026-09-29)

- `src/tui/` is 3,283 lines: a 7-tab dashboard around a single-line prompt.
- On submit, the TUI leaves the alternate screen and calls `run_agent`. For
  most backends this reaches `run_interactive` (`src/agent/mod.rs`), which gives
  the child the terminal with inherited stdio. The TUI then returns showing only
  the exit code.
- Nothing streams. `run_capture` buffers until the child exits, and its
  supervisor checkpoint is `|| false`, so it cannot be cancelled. No executor
  stream format is parsed.
- The run events in `src/app_core/run.rs` report lifecycle state only.

## Decisions

1. **Chat first, tabs as overlays.** The transcript is the main view.
2. **Tool events come from the executor's own stream.** Abbey decodes each
   executor's native stream into typed events. Approvals stay with the
   executor's permission mode, which the TUI shows and can switch. Abbey adds
   no tool loop of its own, so `runtime-provider-neutral-owned` stays Proposed.
3. **One rewrite.** The new TUI replaces the old one in a single change set.

## Executor stream surfaces (verified from `--help` on 2026-09-29)

| Backend | Stream flags | Decoder |
|---|---|---|
| claude | `-p --output-format stream-json --verbose --include-partial-messages`; `--permission-mode` | `claude` NDJSON |
| cursor-agent | `-p --output-format stream-json --stream-partial-output`; `--force` | `cursor` NDJSON |
| grok | `--output-format streaming-json` (ACP session updates) | `grok` NDJSON |
| fm | `fm respond --stream` (default on) | `plain` |
| ollama | `ollama run` (plain text) | `plain` |
| abi | `abi complete` (not installed locally) | `plain`, tested with fixtures only |

## Invariants

- **One canonical execution path.** Streaming is an output sink threaded through
  `run_agent → hybrid_run → run_resilient → run_once`, never a second runner.
  Persona/role wrap, prefs injection, the `route.jsonl` row, chat resume and
  retry, and local transcript persistence stay where they are.
- The CLI's default sink keeps today's output exactly: `abbey ask`, `print`,
  `commit`, and `voice ask` do not change.
- Per-backend argv grammars stay isolated (`src/agent/argv.rs`). Stream flags
  are appended only inside the owning backend's builder.
- `os_control` never runs without explicit confirmation and the allowlist, in
  both editions. The TUI's `!cmd` goes through that path behind a dialog.
- Token usage is shown only when the executor reports it; otherwise `n/a`
  (`oos-fake-cost-accounting`).
- The live `cfg.backend` drives per-call behaviour, never
  `AgentBackend::from_env()` (`tasks/lessons.md`).
- Every `.rs` file stays under 800 lines (target: 600 for `tui/`, 400 for
  `stream/`); `main.rs` stays under 200.
- The one new dependency is `pulldown-cmark`. No async runtime and no HTTP
  client.

## Design

### 1. Stream layer: `src/stream/`

- `event.rs` defines `StreamEvent`:
  - `TextDelta(String)`, `ThinkingDelta(String)`
  - `ToolStart { id, name, summary, input: serde_json::Value }`
  - `ToolEnd { id, ok, summary }`
  - `Usage { input, output }`, only when reported
  - `SessionId(String)`, `Notice(String)`
  - `Done { exit: i32 }`, `Failed(String)`
- `adapters/{claude,cursor,grok,plain}.rs` implement a decoder:

  ```rust
  trait StreamDecoder {
      fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent>;
      fn finish(&mut self) -> Vec<StreamEvent>;
  }
  ```

  - NDJSON decoders buffer by line.
  - Unknown event types are ignored and counted; the count is visible in
    `/debug`.
  - A malformed line becomes a `Notice`; a decoder never panics.
  - `plain` splits UTF-8 safely across chunk boundaries.
- `sink.rs` defines `OutputSink { Terminal, Capture, Stream(Sender<StreamEvent>) }`.

### 2. Run-path changes

- `RunSpec` gains `sink: OutputSink`, defaulting to `Terminal`.
- With a `Stream` sink, `run_once` always takes the capture path. Each argv
  builder adds its backend's stream flags from the table above.
- `src/runtime/supervisor/unix.rs`: `read_bounded` gains an optional chunk tap
  `Sender<(StreamName, Vec<u8>)>`. In tap mode, stdout keeps only a bounded
  tail, so long agentic turns do not hit the 4 MiB `StdoutLimit`. Stderr is
  unchanged.
- `run_capture` takes a real `CancellationToken` in place of `|| false`.
  Cancelling uses the existing process-group SIGTERM → SIGKILL teardown.
- A claude `SessionId` event from `system/init` feeds the existing session
  marker and resume logic.
- On non-Unix hosts, a `Stream` sink falls back to buffered capture and one
  `TextDelta` at the end. The claims text states this.

### 3. TUI: `src/tui/`

**Reused:** `theme.rs`, `widgets.rs`, `predict.rs` (slash and intent ranking
plus the optional Ollama rerank), the overlay fuzzy filter, the `refresh.rs`
producers (now overlay panel content), and `cycle_backend`.

**Layout,** top to bottom:
- Header: brand, backend, model, persona/role, permission mode.
- Transcript: scrollable with mouse wheel, PgUp/PgDn, and jump-to-bottom.
- Composer.
- Status line: run state, elapsed time, tokens or `n/a`, chat id, queued-prompt
  count.

**Modules:**
- `app.rs`: state and the event loop.
  - Crossterm polls at 16–50 ms while a run streams.
  - A run thread sends `StreamEvent`s over `mpsc`.
  - Bracketed paste is enabled.
- `transcript.rs`: cells.
  - `User`, `Assistant` (markdown), and `Thinking` (collapsed, toggleable).
  - `Tool`: name, one-line summary, running/succeeded/failed status, and an
    expandable input and result.
  - `Diff`, built from claude `Edit`/`Write`/`MultiEdit` and cursor edit
    inputs.
  - `Todo`, from claude `TodoWrite`.
  - `Notice` and `Error`.
- `markdown.rs`: `pulldown-cmark` to ratatui `Line`s. Fenced code goes through
  the existing `highlight.rs` syntect path. Only the streaming cell is
  re-rendered.
- `composer.rs`:
  - Multiline: Shift-Enter, Alt-Enter or Ctrl-J add a newline; Enter submits.
  - Editing: word motions and Ctrl-A/E/U/W/K.
  - History: persisted and bounded under the state dir, with Ctrl-R reverse
    search.
  - `@path` fuzzy file completion: `git ls-files`, else a bounded walk of cwd.
  - Slash menu from `SLASH_CATALOG` and `SLASH_ALIASES` through `predict.rs`.
  - Ctrl-G opens `$EDITOR`.
  - Paste-burst detection.
- `keymap.rs`:
  - Esc interrupts a running turn. Esc-Esc when idle recalls the last prompt
    for editing.
  - Ctrl-C clears the input; a second press quits.
  - Shift-Tab cycles the executor permission mode. Claude:
    `default/acceptEdits/plan/bypassPermissions`. Cursor: `--force` on/off.
    Other backends show `n/a (executor-managed)`.
  - Typing while a run is active queues the prompt, sent when the run
    completes.
  - `!cmd` goes through `os_control` behind a confirmation dialog.
- `overlays.rs`:
  - Palette (Ctrl-K), help (F1 or `?`), and model, backend and resume pickers.
    The resume picker reads `AbbeyState::history`.
  - Panels replacing the tabs: Memory, Routes, Skills, Doctor, Personas,
    Claims.
- Slash commands run inside the TUI and print into the transcript. Only
  interactive ones (`voice listen`, `$EDITOR`) suspend the alternate screen.

### 4. Stretch: claude host approvals

This is claude only, and droppable without affecting the rest. It uses
`--permission-prompts host` with `--input-format stream-json`, rendered as an
approve / deny / always dialog. It is implemented only if a recorded live
transcript confirms the control-message shape. Otherwise the TUI displays the
permission mode only.

## Claims and documentation

- Add registry rows in `src/claims/registry.rs`, then regenerate with
  `python3 tools/check_claims_sync.py --write`:
  - `tui-chat-streaming-transcript`: Current.
  - `executor-stream-adapters`: Current. Evidence boundary: decoders are tested
    against fixtures, each installed executor's live shape is recorded once,
    and vendor format stability is not claimed.
  - `tui-executor-host-approvals`: Proposed unless the stretch lands with live
    evidence.
- Add a `stream/` row to the CLAUDE.md module map and update the `tui/` row.
  `tools/check_instructions.py` requires every `src/` entry to be mapped.
- Describe `OutputSink` in the AGENTS.md execution-path section.

## Sequencing

- Implementation waits until the in-flight `learn distill` work lands. That work
  has uncommitted edits in `src/agent/{mod,backend}.rs`, `src/capture.rs` and
  `src/lib.rs`, which this design also touches. Commit only this work's paths.
- Never run the Rust gate concurrently with `./zig/tools/check.sh`.

## Verification

- **Decoders:** unit tests over NDJSON fixtures per executor, covering split
  chunks, split multi-byte UTF-8, malformed lines and unknown event types. One
  dated live capture per installed executor goes under
  `tests/fixtures/streams/`.
- **Supervisor:** tap-mode tests for tail retention, and for cancelling
  mid-stream leaving no process group.
- **Canonical path:** in `tests/cli_surface.rs`, a streamed run appends exactly
  one `route.jsonl` row, and `abbey ask` output is unchanged.
- **TUI:** ratatui `TestBackend` render snapshots (transcript cells, markdown,
  diff, tool expand, overlays, 60-column width), plus keymap state-machine
  tests (interrupt, queue, Ctrl-C twice, Shift-Tab cycle, `!` needs
  confirmation).
- **Gate:** `./check.sh >| /private/tmp/abbey-gate.log 2>&1; echo EXIT:$?` green
  in all four feature modes.
- **Live smoke** in a real TTY with `ABBEY_BACKEND=ollama` and `=claude`:
  - streaming renders;
  - Esc cancels, and `pgrep` confirms the child process group is gone;
  - resume continues the chat.

  The claims text records what was and was not driven live.
