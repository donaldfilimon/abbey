# Evidence and appearance — 2026-10-21

Two cohorts that do not belong to the workspace unification: the TUI's own
appearance, and the places where a write was allowed to fail quietly.

## What changed

### The terminal is asked what it looks like

- The TUI asks the terminal for its default background with the `OSC 11`
  query, parses the reply, and classifies the terminal as light or dark so
  the palette matches. The query is read-only, bounded, and parsed — never
  executed.
- The reply's grammar is platform-independent; the *read* is not. A Windows
  console hands the answer to the application as console input events rather
  than as a byte stream, so the question is not asked there at all and the
  dark palette is kept. A palette must never be chosen from a value the app
  could not read.
- The read is a byte at a time and stops at the reply's terminator. A
  chunked read would take the bytes that follow the reply in the same chunk,
  and those bytes may be the user's keystrokes.
- The descriptor is put into non-blocking mode for the duration of the read
  and its flags are restored afterwards, so the query asks and changes
  nothing. A restore that fails is reported rather than swallowed: the
  palette stays dark and the user is told why.
- A terminal that has not answered when the window ends is given a short
  grace, so a slow answer is taken rather than read as user input. The grace
  is given whenever the reply is *incomplete* — a reply that started and was
  cut short is exactly the case the grace exists for.

### The costs are stated, not hidden

- The query is asked at startup, before the first frame and before the event
  loop starts, so the app has not yet acted on any input. A keystroke that
  lands inside the window is still lost: the terminal's input queue cannot
  be put back.
- A terminal that answers later than the grace is read by the event loop as
  ordinary key events. Once the bytes are in the event queue, the app
  cannot tell an `OSC` reply from typed text. This is the residual of the
  feature, and the module says so rather than claiming a guarantee it does
  not have.

### The palette is measured, not asserted

- Brightness is classified by WCAG relative luminance, not by the average of
  the channels: a channel average calls a saturated green dark (85), which a
  person reads as bright.
- Every palette is checked against the WCAG contrast ratio: at least 4.5:1
  for body text and at least 3:1 for UI borders. A colour that stops being
  legible fails the build rather than shipping.

### A lost audit record is a hole in the evidence

- The routing audit record is written with an error that fails the turn. A
  record that cannot be written must not disappear silently: the audit would
  then under-report what actually ran, and the turn would report success for
  a session whose evidence has a hole in it.
- The audit's write is now synced. A successful `write` only reaches the page
  cache, and a sync failure means the record can be lost with no error at
  all — which is the hole the write is supposed to close.
- The activity row next to it stays best-effort on purpose: it is a learning
  input, the memory store can legitimately be busy, and losing one row must
  not fail a turn that can answer.
- The same rule reaches the subagent lanes: their audit record carries the
  lanes' outcomes, so a write that fails there fails the command.

### The checker stopped reading a shell comment as a heading

- The instruction-shape checker counts ATX headings, and it now skips fenced
  code blocks. The command table is a ` ```bash ` block, and a `#` comment
  line inside it is a comment, not a section — reading one as a heading made
  a perfectly good command table fail the shape check.

### The fixtures stop mistaking "not observed" for "observed to be wrong"

- The provider PTY smoke test retries a scenario while its observation is
  still *indeterminate* (a marker that has not appeared when the bound
  expires) and never retries a *wrong* value — a model that was replaced, a
  prompt that was dispatched twice. Each attempt gets its own directory, so
  an interrupted attempt cannot leave state the next one inherits.
- The CLI surface fixture is hermetic: it removes the ambient live-context
  recipe variable, and it reports the child's stderr and the state
  directory's listing when an assertion fails, so a failure names its cause
  instead of only its symptom.

### Two claims were corrected to match the code

- `backend-ollama` now says what the code does: Abbey never auto-pulls. Every
  spawn path (interactive, one-shot capture, and streamed) preflights
  `ollama list`, and a tag that is not present locally fails the turn
  instead of being downloaded.
- `daemon-protocol-v3-exact-model-inference` now says what the duration
  ceiling actually does: it is consulted at the run's event checkpoints, so a
  cold device that compiles before its first event can outlast it. A run that
  reaches another checkpoint is refused rather than reported, while a run
  that completes without one is reported — and the client's read deadline is
  the effective bound.

## Evidence

| What | How it was checked | Result |
| --- | --- | --- |
| The reply's grammar | unit tests over `rgb:`, plain triples, `BEL` and `ST` terminators, a reply for another query, an incomplete reply, text typed before the reply | 25 tests green |
| The bounded read | a pipe that stays open and stays silent, read through the real non-blocking path | returns `None` inside the bound, never hangs |
| The flags are restored | the same pipe, read before and after | the flags are the ones it was found with |
| The read stops at the terminator | a pipe carrying the reply followed by `hi` | the reply is read, `hi` is still in the queue |
| The grace takes a late answer | a pipe written 30 ms after the window opened | the late answer is taken and parsed |
| An unterminated reply is not a colour | `rgb:ffff/ffff/ffff` with no terminator | refused, rather than read as white |
| The brightness rule | a saturated green | classified light, as the luminance rule says |
| The palettes are legible | every palette against the WCAG contrast ratio | at least 4.5:1 for body text, 3:1 for borders |
| The audit fails loudly | `tests/cli_surface.rs`: an unwritable route log | the turn fails instead of losing the record |
| The instruction checker | `tools/tests/test_check_instructions.py` | a comment in the command block is not a heading; a heading after a fenced block still is |
| The provider fixture | `tools/tests/smoke_tui_provider_pty.py` | retries only indeterminate observations |

## Boundaries

- The AX control driver (`desktop/scripts/drive-controls-macos.py`) drives the
  real window's real controls — appearance Light/Dark/System, collapse and
  expand sidebar, palette — and asserts the accessibility state that
  results. **It was not executed by the gate**: it needs macOS
  Accessibility permission for the driving process and a packaged bundle.
  The script and its Swift driver compile; the interaction is unproven
  until someone runs it with that permission.
- The desktop client's window is one piece of native glass: an
  `NSGlassEffectView` material above an opaque safety panel, with an
  autoresizing mask that tracks the window's height as well as its width.
  The unit tests assert the panel is opaque and the material's window
  ordering; the *visual* result is not something a unit test can prove.
- The terminal query's residual costs are real and are stated above: a
  keystroke inside the query window is lost, and a terminal that answers
  after the grace has its reply read as key events. Closing the second gap
  needs an input path the app can push back into, which this does not have.
- The Windows path keeps the dark palette. The query is not asked there, and
  no Windows terminal is exercised by any of this evidence.
