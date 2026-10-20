# Latest-canonical Abbey qualification readiness — 2026-10-20

Status: reviewed targeted repair; latest three-repository root/desktop qualification pending producer freeze. This does not extend prior exact-pin receipts or qualify other sessions' changing source.

Independent review found one P2 in external config initialization: AlreadyExists swallowed directory/special-file destinations and falsely reported success. The pre-repair diff is preserved in the evidence directory. Narrow ownership accepted after ABI coordination; all existing external edits preserved.

Repair: src/config.rs checks symlink_metadata after AlreadyExists and rejects non-file/non-symlink occupants. src/commands.rs identifies an existing dangling symlink without following it so CLI reports left untouched. New unit regressions preserve directory contents and Unix sockets; tests/config_init.rs exercises directory failure and dangling-link reporting through real binaries. Exclusive creation and original dangling-link preservation remain unchanged.

RED: directory/socket unit tests failed, and both CLI regressions failed before repair. GREEN: 18 filtered unit tests passed; 2 CLI tests default and 2 personal passed. rustfmt and scoped whitespace checks passed. These checks used an isolated focused consumer candidate with previously qualified ABI/WDBX sources, not a latest-producer gate.

Independent review found no blocking repair finding. One reporting race remains: a concurrent creator between CLI precheck and helper can cause inaccurate wrote copy while no-overwrite remains enforced. Returning an explicit Created/Existing outcome is a separate follow-up.

ABI/WDBX owners and coordinator have been notified. WDBX decoder focused checks/review are separate from newer outside-lane episode-store/filesystem edits, which have blocked formatting and lack an owner-ready source release. No live or frozen latest full consumer gate is attributed until coherent producer handoff. No protected-file changes, commits, pushes, hosted CI, foreign resets or source rollback by this lane.

Evidence: /Users/donald/.codex/verification/abbey-latest-canonical-20261020/ — reviewed-source-checkpoint.json, config-pre-repair.diff, config-red.log, config-cli-red.log, config-green.log, config-cli-green.log, config-cli-personal-green.log, config-ready.json and config-review.md.
