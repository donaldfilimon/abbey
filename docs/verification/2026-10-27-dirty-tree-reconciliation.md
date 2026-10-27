# Abbey dirty-tree reconciliation — 2026-10-27

Status: phase 1 (reconciliation only) complete. No build, test, gate or Cargo
command is claimed. Nothing was pushed; no branch, worktree, stash, reset,
rebase or force was used, and no existing worktree was touched.

## Starting state

- Local `main` at `8fdb4ed5f3a869726354bb89a8925f5e96aeaa00`: one commit ahead of
  and two behind the cached `origin/main` at
  `6901d119c87926ef632de74549c87b561cb7033e` (merge of PR #108, `4699a0f`
  "Finish the ralph module split and deflake the PTY slash harness").
  Merge base: `d34e258`.
- A dry-run fetch showed the remote has since advanced to `dea5a48`. It was not
  fetched; only the cached `6901d11` was integrated.

## Backup

Every dirty and untracked file was copied verbatim (paths preserved) into the
ignored `.work/reconcile-20261027/` before any change, with `SHA256SUMS`
(19 files), `HEAD.txt` and `tracked.diff`. Two files changed externally during
the task (see below) and were backed up again with addenda
`SHA256SUMS.addendum-0235` (`Cargo.lock`) and `SHA256SUMS.addendum-0255`
(`CLAUDE.md`, `Cargo.lock`, live copy under `live-0255/`).

## Per-file classification

| File | Classification | Evidence | Action |
| --- | --- | --- | --- |
| `CLAUDE.md` (original dirty bytes, sha `becc9ab6…`) | Superseded | Byte-identical to `6901d11:CLAUDE.md` | Taken from origin via the merge |
| `CLAUDE.md` (external edit at 02:12, sha `265f2e60…`) | Unique, not task-owned | Equals origin plus three module-map/command lines for the already-tracked `src/train/` and `src/models/selection_tests.rs`; origin's hunk is fully contained | Restored to HEAD only for the merge, then the live bytes were written back; left uncommitted |
| `src/ralph.rs` | Superseded | Byte-identical to `6901d11` | Taken from origin via the merge |
| `src/ralph/{mcp,run,tests}.rs` (untracked) | Superseded | Each byte-identical to `6901d11` | Removed before the merge, recreated by it |
| `tools/tests/smoke_tui_slash_pty.py` | Superseded | Byte-identical to `6901d11` | Taken from origin via the merge |
| `src/tui/predict.rs` | Unique | `origin/main` does not touch it (HEAD == origin) | Kept dirty, uncommitted |
| `src/tui/prediction_owner.rs` | Unique | Same | Kept dirty, uncommitted |
| `src/tui/tests/prediction_producer.rs` | Unique | Same | Kept dirty, uncommitted |
| `docs/verification/2026-10-26-dependency-security-qualification.md` | Unique, finished | Exists only on local `8fdb4ed`; the dirty diff completes the packet (final source boundary, 48-receipt review) | Committed in `d79f298` |
| `.../checks-summary.json` | Unique, finished | Same packet; valid JSON, 48 checks | Committed in `d79f298` |
| `tasks/completion.json` | Unique, finished | `origin/main` does not touch it; the dirty change only appends the `local_gate_execution` evidence entry | Committed in `d79f298` |
| `docs/plans/2026-10-27-abbey-*.md` (7, untracked) | Unique | Not in `origin/main` | Left untracked |
| `Cargo.lock` (external edit at 02:07) | Unique, ownership unknown | 40 registry version bumps (brotli, cc, icu_*, tonic*, uuid, wry, zbus, …); HEAD and origin both hold `c51285b0…`, the lock the 10-26 packet qualified | Left as-is, uncommitted |

No file was conflicting in a way the task had to resolve: `git merge-tree
--write-tree 6901d11 HEAD` exited 0, and its result matched the backup for every
superseded file.

The three TUI files match the `diff_sha256` `cc91a120…` recorded by the 10-26
packet's `delta-tests-*`, `audit-final`, `vendor-audit-final` and
`provenance-complete` receipts. In other words, they are exactly the bytes those
checks ran against. They are still not committed: the packet states that the
owner's prediction change is not a passing whole-gate claim, and builds are on
hold.

## Git integration

1. `d79f298` `docs(verification): finalize 2026-10-26 dependency-security
   qualification packet`: the report, `checks-summary.json` and the
   `tasks/completion.json` append. The committed blobs equal the backup hashes.
   Before it was committed, `tools/check_completion.py` and
   `tools/check_instructions.py` exited 0, `git diff --check` was clean and both
   JSON files parsed.
2. To clear the merge's way, the superseded tracked files were restored to HEAD
   and the untracked `src/ralph/*.rs` were removed. Before that, each one was
   re-verified as byte-equal to both its backup and `6901d11`.
3. `fbc0ad0` merged `6901d11` by SHA with `ort` and no conflicts. After the
   merge, HEAD's `CLAUDE.md`, `src/ralph*` and the PTY harness equal
   `6901d11`. `main` is now 3 ahead of and 0 behind the cached `origin/main`.
4. After the merge, the live tree matches the original manifest for every file
   except `CLAUDE.md`, which is expected. It matches the 02:55 addendum for
   `CLAUDE.md` and `Cargo.lock`. `check_instructions.py` and
   `check_completion.py` exit 0.

## Concerns

- **Concurrent writer.** Someone else changed `Cargo.lock` (02:07) and
  `CLAUDE.md` (02:12) in this canonical checkout, despite the sole-writer
  release. Their ownership is unverified, so both are preserved but neither is
  committed. If the `Cargo.lock` bump is kept, the 10-26 packet's recorded lock
  hash will no longer describe the tree.
- `tools/check_claims_sync.py` runs `cargo run --bin abbey -- claims manifest`.
  It was started once during this task, compiled for about 2–3 minutes and was
  stopped before it finished. It did not modify any tracked file. The claims
  sync was therefore not verified; that has to wait for the build hold to lift.
- During the task, one integrity check ran `shasum -c` inside the backup
  directory. That compared the backups with themselves, not with the live tree,
  so it missed the external `CLAUDE.md` edit. The checks above were run from the
  repository root.
- `origin/main` is at `dea5a48` and has not been fetched or reviewed.
