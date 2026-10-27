# Models/media/federation plan Task 17 — Program 3 live capture contract draft

Date: 2026-10-27. Scope: Task 17 Steps 1-3 of
`docs/plans/2026-10-27-abbey-models-media-federation-plan.md`. Step 4 (OB-18:
independent review, manager authorisation, and participant consent) is
Donald-gated and has not been done. This change adds no adapter, claim,
registry, `tasks/*.json`, or completion change.

## Revisions

- Abbey base: `0ad2fef80f0c1ec38dd1367882541e6a2cdec661` (task branch
  `task/wf-models-media-federation-t17-20261027`)
- `../abi`: `dff096415715462370a18a9ad71e5eaf098197da`
- `../wdbx`: `93dbeab8987a06007430bf5ab17573ec5a80dc3e`

## Changes

- `docs/program-3-live-capture-contract.md`: draft contract, marked as
  granting no authority.
- `tools/check_p3_readonly.py`: adds `LIVE_CAPTURE_PATTERNS`,
  `live_capture_surfaces(root)` and `all_surfaces(root)`. `main()` now scans
  every surface. Live-capture files get the full rule set, including the
  network rule, so the checker fails closed.
- `tools/tests/test_check_p3_readonly.py`: adds
  `test_live_capture_modules_cannot_import_effect_paths` and
  `test_checker_scans_live_capture_surfaces_alongside_program_three`.
- `docs/agent-guide.md`: the checker bullet now mentions the
  `guild_live_capture` paths.

## Red before green

`python3 -m unittest discover -s tools/tests -p 'test_check_p3_readonly.py' -v`
before the implementation: `Ran 6 tests ... FAILED (errors=2)`. The cause was
`AttributeError: module 'check_p3_readonly' has no attribute
'live_capture_surfaces'` (and `'all_surfaces'`). After the implementation, the
same command gave `Ran 6 tests ... OK`.

## Acceptance commands

| Command | Result |
|---|---|
| `python3 -m unittest discover -s tools/tests -p 'test_*.py'` | `Ran 274 tests` — OK |
| `python3 tools/check_p3_readonly.py` | `Program 3 read-only boundary: OK` |
| `python3 tools/check_instructions.py` | `instructions: OK` |
| `python3 tools/check_completion.py` | coverage OK (the contract file has no `plan` in its name and no checkboxes) |

## Not run

- `./check.sh` was not run because the session forbids the full gate under
  current machine load. Only Python tools and documentation changed. No Rust
  source changed, so cargo build, clippy, fmt, and test results are unaffected
  and were not rerun.
- The `program-3-guild-intelligence-synthetic` claim stays Current and was not
  widened. No live Discord access, recording, or authorisation took place.
