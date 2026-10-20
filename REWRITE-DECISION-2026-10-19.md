# Rewrite decision — 2026-10-19

Status: Canceled — further Zig rewrite, migration, completion and cutover work.

Authority: Donald explicitly canceled the Zig rewrite plans and requested resumption of retained Rust implementations and consideration of the newest Rust release. This supersedes prior rewrite approvals and open continuation instructions.

Current: retained Rust application in Cargo.toml and src/. Resume product work there; check.sh is its gate.

Preservation: existing Zig code, commits, untracked files, dirty work and past qualification records remain intact. Historical checkboxes and evidence describe work already done or formerly planned; unfinished Zig tasks are canceled. No deployment, data migration or service cutover is performed.

Qualified follow-up (2026-10-20): Rust 1.99.1 stable passed the captured clean and combined root and desktop gates; see [the receipt](docs/verification/2026-10-20-rust-1991.md). The cancellation itself did not change compiler pins; the bounded candidate qualification concerns the compiler, selected sibling dependencies, and loader repair. The bounded migration is now on main; separate active dependency-upgrade and prototype work remains outside this qualification.

Historical blocker (2026-10-19; resolved only by the later qualified dependency composition): on 2026-10-19, Rust 1.99.1 rejected the pinned WDBX `abi-compute` crate with E0554 at `#![feature(portable_simd)]`. That observation applied to the dependency revision pinned that day; the later qualified ABI/WDBX composition removes this stable-compiler blocker. The earlier nightly evidence remains historical. This is a Rust migration constraint and does not reactivate Zig work.

## 2026-10-20 Zig inventory and stable check

The following retained inventory and check preceded the qualified compiler cutover recorded above.


`zig/` stays retained history. Its command set, from `zig/src/cli/args.zig`, is ask, print, commit, doctor, learn, routes, config, claims, memory, wdbx, daemon, tui, edition, version, and help. The help text calls that surface P1 CLI, P2 daemon protocol v1, and P3 TUI. Rust `src/cli.rs` already has each of those commands and additional product commands. The Zig daemon is the older read-only protocol v1 path. No Zig-only product behavior was found, so nothing was ported from `zig/` into `src/` in this pass. `src/init/detect.rs` still records `zig build` when it probes a user's project. That is user-repository detection, not Abbey's Zig application.

Stable check, without a pin change: `rustup run 1.99.1 cargo check --locked --manifest-path /Users/donald/sources/repos/abbey/Cargo.toml --workspace` exited 0 in 9.64s. `rustc 1.99.1 (58cf8f9c3 2026-10-13)`. Homebrew `cargo` does not accept a `+1.99.1` override, so `rustup run` selected the same toolchain. The 2026-10-19 `portable_simd` error did not reproduce on this check. `rust-toolchain.toml` remains `nightly-2026-09-01`. This was `cargo check`, not `./check.sh`, and it does not promote the pin.
