# Rewrite decision — 2026-10-19

Status: Canceled — further Zig rewrite, migration, completion and cutover work.

Authority: Donald explicitly canceled the Zig rewrite plans and requested resumption of retained Rust implementations and consideration of the newest Rust release. This supersedes prior rewrite approvals and open continuation instructions.

Current: retained Rust application in Cargo.toml and src/. Resume product work there; check.sh is its gate.

Preservation: existing Zig code, commits, untracked files, dirty work and past qualification records remain intact. Historical checkboxes and evidence describe work already done or formerly planned; unfinished Zig tasks are canceled. No deployment, data migration or service cutover is performed.

Proposed: qualify Rust 1.99.1 stable against the retained implementation. Do not replace a nightly pin with stable without the full repository gate. The cancellation itself does not change compiler pins; dependency lockfile repairs are qualified separately.

Blocked — stable compiler qualification: on 2026-10-19, Rust 1.99.1 rejected the pinned WDBX `abi-compute` crate with E0554 at `#![feature(portable_simd)]`. Replacing or feature-gating that nightly SIMD dependency is required before ABI and Abbey can move to stable. Keep their validated nightly pins during the CI repair. This is a Rust migration constraint and does not reactivate Zig work.
