# TUI, Foundation Models, and learning — 2026-10-20

This is focused evidence for the retained Rust application, not a release or
packaged-product qualification. Other work is active in the adjacent ABI and
WDBX checkouts; an earlier gate attempt against those changing sources is not
substituted for a successful final production gate.

## Changes

- Chat-first responsive layout, workspace/provider context, useful empty state,
  Unicode display-cell-aware soft wrapping, and visible run/queue/cancel state.
- Ctrl-S provider picker; Ctrl-B still cycles installed executors. Apple model
  selection distinguishes on-device `system` from explicitly off-device `pcc`.
  Leaving Apple models resets the selected model to the next backend's `auto`.
  Unrelated local-command completion cannot replace the live selection with
  ambient `ABBEY_MODEL`; only successful explicit model mutations reload it.
- Bounded startup Git indexing using NUL-separated paths, a 500 ms subprocess
  budget, and 2 MiB output ceiling. The fallback filesystem walk limits time,
  entry count, retained bytes, and depth, without following directory symlinks.
  Invalid UTF-8 and control-containing names are omitted from completion.
- Read-only FM availability falls back from `models` to `available` only for a
  completed, unsuccessful unsupported-command response. Cancellation, overflow,
  genuine unavailability, and timeouts never trigger the compatibility call.
- CLI config comments respect quotes; basic/literal strings and Unicode escapes
  are decoded rather than silently corrupting paths.
- Route learning uses domain-separated content identities instead of timestamp
  alone, deduplicates a repeated tail, and preserves legacy evidence. Failed
  memory promotions are no longer reported as successful promotions.

## Focused checks

- `./tools/cargo.sh test --lib tui::`: 122 passed.
- `./tools/cargo.sh test --lib config::tests`: 23 passed.
- Focused learning tests: 13 passed.
- Focused FM compatibility tests: 6 passed; the existing two-grammar parser test
  also passed. Fixtures cover cancellation and refusal without fallback.
- The real provider PTY regression passed: start with ambient `ABBEY_MODEL=pcc`,
  choose FM in Ctrl-S, execute `/help`, then submit a prompt. Exactly one private
  fixture `fm respond --model system` call is required, with its response retained
  across redraw and terminal state restored at quit. No model service is called.
- All three metadata PTY cases passed after isolating their standard-library
  Python executable from unrelated site initialization (`-S`). They remain
  serialized; the production 500 ms deadline and execution assertions are intact.
- `./tools/cargo.sh build --bins`: passed on Rust 1.99.1, macOS ARM64.

## Live local FM check

The installed `/usr/bin/fm` rejected `models` but answered `available` with
`System model available`. Its `respond --help` listed the `system` model and
the transcript/instruction flags already used by Abbey.

Using a temporary config, workspace, and state root, with `ABBEY_BACKEND=fm`,
`ABBEY_AUTO_REVIEW=0`, and `ABBEY_TRUST=0`:

- `abbey doctor` exited 0 and reported on-device system available and Private
  Cloud Compute unavailable.
- `abbey -m system -p 'Reply with exactly READY.'` exited 0 with stdout `READY`.
  The transcript was written only under the scratch state root, then removed
  with that owned temporary directory.

This is one on-device compatibility smoke, not model-quality, coding-agent,
performance, or general Apple hardware acceptance. No PCC prompt was sent, no
model weights were trained, and no user's memory store was opened. The TUI
continues through the shared executor path; this adds no Abbey-owned tool loop.
Bot work is deferred at the user's direction until a bot checkout is available.

## Full gate

An attempt passed the default and WDBX stages, then failed a personal-edition
legacy-migration fixture's daemon-readiness check. Its private daemon log
reported the mutually exclusive bearer-source requirement. An unchanged focused
rerun passed. The fixture now removes the competing inherited bearer-file
variable, reports early daemon exit, and reaps its own child on timeout. A focused
personal-edition run with a deliberately poisoned bearer-file variable passed.
Overlapping Cargo feature builds can also replace shared target-directory
binaries, so the subsequent full attempt uses a dedicated build directory.

The isolated `./check.sh` run completed successfully with `check.sh: OK`.
All four clippy, test, and warning-denied private-item rustdoc modes passed,
along with 98 Python tests, claims/instruction checks, desktop IPC codegen drift,
the Program 3 boundary, and isolated installer verification/rollback checks.
Windows and Linux GNU cross-checks were skipped because their targets were not
installed. Existing sibling warnings and soft file-size warnings remain visible.
The log for that isolated attempt was not retained, and temporary
`/private/var/folders/c_/thtfn_7d1p9378rxsp4pwz6c0000gn/T/opencode/abbey-tui-fm-learning-isolated-gate.log` no longer
resolves, so that run's individual artifact is not recoverable — its result is
kept only as historical prose. A full `./check.sh` over this complete combined
tree passed on 2026-10-20 with `check.sh: OK` (all four clippy, test, and
warning-denied private-item rustdoc modes, 98 Python tests, claims
synchronization with the unchanged 59-claim registry digest, desktop IPC codegen
drift, the Program 3 boundary, and isolated installer verification/rollback
checks). Windows and Linux GNU cross-checks were skipped because their targets
were not installed. Existing sibling warnings and soft file-size warnings remain
visible.
This is a local gate result, not a frozen three-repository source-cohort,
Windows/Linux runtime, packaged-release, or full desktop-product qualification.
