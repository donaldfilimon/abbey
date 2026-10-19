# Rewrite decision — 2026-10-19

Status: Canceled — further Zig rewrite, migration, completion and cutover work.

Authority: Donald explicitly canceled the Zig rewrite plans and requested resumption of retained Rust implementations and consideration of the newest Rust release. This supersedes prior rewrite approvals and open continuation instructions.

Current: retained Rust application in Cargo.toml and src/. Resume product work there; check.sh is its gate.

Preservation: existing Zig code, commits, untracked files, dirty work and past qualification records remain intact. Historical checkboxes and evidence describe work already done or formerly planned; unfinished Zig tasks are canceled. No deployment, data migration or service cutover is performed.

Proposed: qualify Rust 1.99.1 stable against the retained implementation. Projects using nightly features may need nightly; do not replace a nightly pin with stable without the full repository gate. Project pins and lockfiles remain unchanged in this documentation-only cancellation.
