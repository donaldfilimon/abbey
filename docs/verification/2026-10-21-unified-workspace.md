# Unified cargo workspace — 2026-10-21

The nested Tauri client is now a member of the repository's cargo workspace, so
the default `cargo` path builds and tests the whole application.

## What changed

- The root manifest gained a `[workspace]` table with `desktop/codegen` and
  `desktop/src-tauri` as members, and with all three packages as
  **default members**.
- `desktop/Cargo.toml` and `desktop/Cargo.lock` are gone: there is one
  workspace and one lockfile.
- `desktop/check.sh` now resolves cargo against the repository root
  (`--manifest-path $ABBEY_TOOLCHAIN_ROOT/Cargo.toml`); the bun, bundle, and
  packaging steps stay in `desktop/`.
- The root gate's IPC codegen step dropped `--manifest-path desktop/Cargo.toml`
  and its "stale `desktop/Cargo.lock` → UNMEASURED" escape hatch: with one
  lockfile, `--locked` failing is a hard failure everywhere.
- The Tauri bundle moved from `desktop/target/release/bundle/` to the
  repository root's `target/release/bundle/`, because cargo's target directory
  follows the *workspace* root. Every reference was moved with it: the release
  script, the packaged/menu provers, and `.github/workflows/desktop-release.yml`.

## Why `default-members` matters

A bare cargo command in a workspace whose root is also a package selects only
that root package. Without `default-members`, `cargo build` in this repository
would have built `abbey` and `abbeyd` and nothing else — the client would
still have been invisible to the default path, which is the exact failure the
unification exists to remove.

## The feature cohorts are scoped

`check.sh`'s three off-by-default feature cohorts select the packages that
*own* the feature: `-p abbey` for `wdbx` and `accel`, and `-p abbey
-p abbey-desktop` for `personal-edition` (the client has that feature too — it
selects the edition's bundle identity).

The default cohort still covers the whole workspace, client included. The
scoping matters because the client has no `wdbx` and no `accel` feature, and
rebuilding the Tauri dependency tree for a cohort that cannot use it costs
minutes of wall clock and gigabytes of disk on every run.

## Evidence

Verified against the sibling revisions `.github/workflows/rust.yml` pins
(`tools/ci/verify-pinned-siblings.sh`), on a tree that carries this change
set and nothing else in flight:

| Gate step | Result |
| --- | --- |
| toolchain probe, `cargo fmt --package abbey -- --check` | green |
| clippy `-D warnings`, default (every member) | green |
| tests, default (Abbey **and** the client) | green |
| clippy `-D warnings`, `--features wdbx` | green |
| tests, `--features wdbx` | one failure — see *The one red* below |
| clippy `-D warnings`, `--features personal-edition` (Abbey + client) | green |
| tests, `--features personal-edition` | green |
| clippy `-D warnings`, `--features accel` | green |
| tests, `--features accel` | green |
| warning-denied private-item rustdoc, all four modes | green |
| Python tool tests (182) | green |
| claims/docs synchronization, instruction drift, Program 3 boundary | green |
| desktop IPC codegen drift | green — "5 generated file(s) up to date" |
| installer syntax and accelerator layout | green |
| source file-size guard | green |
| `desktop/check.sh` (the desktop gate) | green — "desktop/check.sh: OK" |

Also verified directly:

- `cargo build --locked --bins` at the repository root produces all four
  binaries: `abbey`, `abbeyd`, `abbey-desktop` (the Tauri client), and
  `abbey-desktop-codegen`.
- `cargo clippy --locked --all-targets -- -D warnings` at the root is clean
  across all three members, so the client is now held to the same
  warning-denied standard as the rest of the repository.
- `cargo test --locked` at the root runs the client's tests alongside Abbey's.
- `bun run codegen:check` in `desktop/` still resolves the generator through
  the unified workspace and reports "5 generated file(s) up to date".
- The lockfile change is purely additive: 208 packages added (the Tauri tree),
  **zero removed, and zero existing versions changed**. The root's pinned
  `dirs 6.0.0`, `phf 0.11.3`/`0.12.1`, `syn`, `prost`, `proc-macro2`, and
  `strum` selections are all untouched; the Tauri tree's newer `dirs 7.0.0` and
  `phf 0.13.1` sit beside them.
- `tools/tests/test_workspace_topology.py` pins the topology: the members, the
  default members, the absence of a second manifest and lockfile, and the
  direction of the dependency (the client depends on the library, never the
  reverse). Removing `default-members` fails it.

## The one red

`tests/governed_sea/ledger_hardening.rs::failed_restore_refuses_success_without_changing_signed_evidence`
expects a refused restore; the pinned WDBX revision commits it instead. The
test passes against the sibling's working tree, because that behaviour — like
the sibling's `GuildEpisodePolicy` retention fields — exists only in the
sibling's *uncommitted* tree. No WDBX pin can satisfy it until that change is
committed, so the `--features wdbx` cohort is red against CI's pins until then.
That is a sibling-contract question, not a consequence of this change: the
fixture compiles against either revision, and the *behaviour* it asserts
matches only the newer one.

## Boundaries

The client is still a *client*. It depends on the `abbey` library and brings no
authority into the CLI; the root package does not depend on it. The client's
own `unsafe_code = "deny"` lint and its enumerated read-only IPC surface are
unchanged.

The root gate still does not run the frontend, the bundle, or the macOS
packaging acceptance: those need bun and a WebView toolchain that a gate a
headless machine can run must not require. `desktop/check.sh` remains the gate
for that, and the root gate additionally runs the locked IPC codegen drift
check.

The root gate's file-size guard still scans `src/` only, so the client's
sources are not measured by Abbey's module-size rule.
