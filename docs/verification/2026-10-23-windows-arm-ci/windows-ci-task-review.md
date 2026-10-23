# Guarded Windows ARM CLI CI source review

Spec compliance: NEEDS CHANGE for the required standalone both-edition coexistence proof below. Code quality: NEEDS CHANGE for that missing qualification behavior; no additional discrete introduced security or runtime defect established by static inspection.

## Actionable finding

[P2] tools/ci/qualify-windows.ps1:76–90 builds four cohorts and checks each binary independently, but never exercises safe and personal executables together or asserts state/config preservation across their use. The brief explicitly requires executing safe/personal coexistence fixture binaries. Deferring the optional install.ps1 runtime fixture does not cover this separate CLI coexistence requirement. Retain each produced binary and add a scratch-only coexistence check that invokes both editions, verifies their distinct identities/config/state roots, and proves one edition does not alter the other's seeded data. Keep installation/service acceptance explicitly deferred. Add a source guard that fails if the required coexistence invocation/assertions disappear.

## Reviewed boundaries

The workflow preserves the exact availability/same-repository trust expression, read-only permissions, pinned actions and immutable public sibling revisions. Git Bash uses explicit path conversion and the existing tokenless allowlisted helper; no new public-fetch authority is introduced. Pre-checkout and pre-helper scans refuse reparse entries/ancestors before recursive deletion; final cleanup is always authored, exact-root scoped, and relies on the documented exclusive trusted runner assumption. A failure before Abbey checkout or hard cancellation can still prevent scripted cleanup; administrator residual cleanup remains required.

Qualification asserts ARM64 OS and PowerShell, Windows client build >=22000, exact Rust release/ARM64 host, checked native exit statuses, locked four-cohort commands, actual produced ARM64 PE type and binary identity. HOME/profile/state/config roots are scratch scoped, safe/personal roots distinct, credentials/provider settings cleared without their values, and Rust installation location retained explicitly. CLI-only source is not a root/desktop gate or an executed Windows pass. No executed-run full-gate semantics were broadened.

Reviewed windows-ci-brief.md including its Git Bash ruling, windows-ci-report.md, windows-ci-source.patch (the actual patch basename), workflow, both PowerShell helpers, new qualification source guards and workflow guard changes. No agents, builds, tests, network, PowerShell execution or services performed; only this requested external report was written. Python source guards cannot establish PowerShell parsing, NTFS behavior, Windows conversion, native dependencies or actual runtime qualification. Native Windows/installer/systemd-equivalent/desktop acceptance remains unproved.
