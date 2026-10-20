# Metadata-overflow cancellation investigation — 2026-10-20

Outcome: the retained cancellation assertion failure was not reproduced. No production cancellation code, output ceiling, deadline or retry behavior was changed. The only code change adds the received error and Failed events to the existing cancellation assertion in src/owned_cancel_tests/diagnostic_tests.rs.

## Evidence

The original root-gate failure shows an unset cancellation token but does not retain the error returned by metadata_owned or its events. Source inspection shows StdoutLimit emits StreamEvent::Failed, which sets the cancellation token; supervisor/validation/spawn/reader/teardown errors can return before that branch, and TimedOut returns an error without emitting Failed. Thus an unset token alone does not establish a broken overflow path. Which path caused the original failure remains unknown.

Using Rust 1.99.1 and the preserved candidate, rebuilt with diagnostic-only instrumentation:

- Exact-pin ABI7b9519b/WDBX5182bc3 composition: 400 focused overflow tests, eight workers, all passed; 12 complete personal-edition library suites, two concurrent processes, all passed with 823 tests each.
- Originally failing captured sibling composition: 400 focused overflow tests, eight workers, all passed; four complete personal-edition library suites, two concurrent processes. The overflow test passed in all four; three suites passed all 823 tests, one reported 822 passes and a save_cot_writes_header failure.
- The unrelated CoT test uses the shared fixed temp directory abbey-cot-test-hdr, so simultaneous suite processes can interfere. Its failure is retained, not treated as overflow evidence or silently reported as a passing suite. No CoT files were changed.
- rustfmt --edition 2024 --check on the changed diagnostic test passed.

Total target-test executions: 816, all passed (800 focused plus 16 full-suite executions). Full-suite runs produced 13,167 passes and one unrelated failure. These are reproduction results, not a full check.sh qualification of newer canonical edits.

## Preservation

Builds and stress tests used the preserved candidate, excluding other sessions' current canonical source changes. The canonical diagnostic test was clean before this lane's diagnostic change. Original sibling copies and logs remain preserved. The prior qualified exact-pin candidate source was restored and checked against its existing source manifest with zero byte differences. Existing qualification receipts were not rewritten. No commits, pushes, staging, resets, reverts, dependency changes, or external-session source edits were performed.

Logs/results: /Users/donald/.codex/verification/abbey-metadata-overflow-20261020/ (focused-stress.json, personal-suite-stress.json, original-focused-stress.json, original-personal-suite-stress.json, per-suite logs, diagnostic_tests.instrumented.rs, restored-qualified-source.json).
