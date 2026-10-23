# Metadata cancel fixture ordering fix

Read the repository AGENTS.md and metadata-cancel-investigation.md before editing. Scope was limited to two owned Python fixture files; existing concurrent edits were preserved.

## Exact changes

- `/Users/donald/sources/repos/abbey/tools/tests/smoke_tui_metadata_pty.py`: cancel mode sends the exact existing bracketed-paste grammar for `/doctor ` plus Enter and `metadata-queued-never-contact` plus Enter in one PTY write, before wait_call, wait_leaf, or screen waits. Quit mode keeps its original submit and readiness sequence. No production deadlines or retries changed, and no polling change was necessary.
- `/Users/donald/sources/repos/abbey/tools/tests/test_metadata_fixture.py`: added CancellationOrdering behavioral fake-session regression. It requires the single exact two-submission write before readiness, exact-probe and leaf/parent readiness plus actual queued-marker observation before Esc, joined exit 130 and discard before closure, and no-generation inspection before cleanup.

All existing actual native assertions remain: selected ABI --version identity, live metadata owner/leaf and supervisor, actual queued marker, Esc, joined exit 130, discard notice, exact PID/group closure before release/cleanup, and no queued-generation contact.

## RED then GREEN

Command for each run:
`PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/tests -p test_metadata_fixture.py -v`

RED on the old fixture: five tests ran, one failed at wait_call because the only input written was `/doctor ` plus Enter; the required queued draft Enter had not been written. Full output: `metadata-cancel-red.log` in this directory.

GREEN after the narrow correction: all five tests passed, including the real subprocess site-hook isolation test using TemporaryDirectory scratch state. Full output: `metadata-cancel-green.log` in this directory.

## Limits

This is Python fixture evidence only. No Cargo, builds, native PTY runs, production state/profile writes, commits, pushes, external services, or subagents were used. The controller owns native acceptance and frozen-source capture. One PTY write preserves input order but is not an atomic TUI transaction: arbitrary scheduler suspension beyond the real 500 ms production deadline can still fail the retained fail-closed readiness/queue assertions. Existing ordinary-control setup retries and -S descendant changes predated this task and were preserved, not introduced here.
