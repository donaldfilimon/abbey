# Guarded Windows ARM CLI CI source

A disabled, administrator-enabled Windows 11 ARM64 CLI qualification job is authored. Same-repository trusted heads, exact public dependency pins, read-only permissions, pinned actions and owner-runner labels match existing policy. Git Bash reuses the existing tokenless checkout helper with explicit Windows path conversion. PowerShell preflight and cleanup refuse reparse points and scope removal to exact checkout/scratch roots. Exclusive provisioned runners and administrator cleanup after hard cancellation remain prerequisites.

Qualification source asserts native ARM64 OS/PowerShell/Rust and Windows client build, runs locked CLI feature cohorts, inspects ARM64 PE headers, and executes version/edition probes. Scratch HOME/profile/config/state paths isolate both editions and inherited credential/provider overrides are removed. Review found the independent cohort checks omitted coexistence; the fix retains each executable, invokes both edition pairs, verifies distinct effective roots and hashes both editions' seeded state/config after each invocation.

Recorded source guards pass: 60 workflow, two Windows mutation suites, 16 existing executed-run checker and 13 installer source tests. Expected missing-job and missing-coexistence RED results were observed. Scoped reviews passed spec compliance and quality after the fix. These are static/source checks on macOS; neither PowerShell executable is installed. No PowerShell parser/runtime, Windows/NTFS, Git Bash Windows, native dependency, installer or actual CI execution is claimed. This is a CLI source slice, not a full root/desktop gate or a foundation completion. Named pipes, Job Objects, installer acceptance and native Windows 11 ARM remain open. Required root/desktop qualification of the integrated snapshot is pending.

Logs, scoped diffs and reviews are in the adjacent directory. Existing executed-run full-gate semantics were not broadened; skipped or disabled jobs remain missing proof.

The integrated platform-tooling snapshot subsequently passed root (546.36 s)
then desktop (25.88 s) serially, and all four additional WDBX/personal/accelerator
feature combinations (96.81/98.63/96.59/100.15 s), with no source hash/mode drift.
Exact snapshot and receipts are retained under platform-tooling in the completion
review-fixes receipt. This acceptance applies to that recorded source, not later
in-progress credential-reader/tooling changes, and supplies no native Linux or
Windows execution. Root retained upstream WDBX and claims timeout PIPE warnings;
the latter has a separately reviewed fix for the next source snapshot.
