# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Abbey — a hybrid persona/role CLI/TUI written in Rust, backed by `cursor-agent` (+ optional `abi`). It mimics the surface of Grok Build, Codex, and Claude Code (slash commands, `-c`/continue, `exec`/`review`/`commit`/`pr`, TUI) while routing actual generation through `cursor-agent` and, for personas/identity, the sibling `abi-ai` crate.

Full agent guidance lives in [AGENTS.md](AGENTS.md) — read it too; this file does not duplicate its claims-gate table or gotchas.

## Commands

```bash
cargo build --release              # build
cargo build --features wdbx        # + in-process WDBX memory backend (off by default)
./install.sh                       # build + install to ~/.local/bin/abbey
./check.sh                         # production gate — see below
cargo test                         # default-feature tests only
cargo test --features wdbx         # includes src/memory/wdbx.rs
cargo test <test_name>             # single test
cargo clippy --all-targets -- -D warnings
cargo fmt --all
abbey doctor                       # build stamp + persona/role/memory/os honesty check
```

`./check.sh` is the merge bar — always run it before considering work done. It is fmt-check + clippy `-D warnings` + tests **for both feature sets**, plus the file-size guard. A bare `cargo test` never compiles `src/memory/wdbx.rs`, so it can pass while the gated backend is broken; that is exactly why the gate runs twice.

## Toolchain

Rust **nightly**, edition **2024**, pinned via `rust-toolchain.toml` (`rustfmt` + `clippy` components). `abbey` path-depends on `abi-ai = { path = "../abi/crates/abi-ai" }` (and, under `--features wdbx`, `abi-wdbx`) — this repo must be checked out as a sibling of `../abi` or the build fails.

## Architecture

Abbey has one canonical execution path shared by the CLI, slash commands, and the TUI — do not add a second way to invoke the agent.

```
CLI (clap) · TUI (ratatui) · slash catalog
        ↓ all three funnel into:
actions::run_agent           — canonical RunSpec, the single entry point
session::hybrid_run          — persona + role + prefs + routing decision
parallel                     — Max/Gemma/Aviva lane fan-out
hybrid_loop                  — Gemma interpret → Max implement, one correlation id
os_control                   — allowlist dry-run / execute --confirm
learn                        — correction/preference/digest → memory
inventory                    — skills/plugins/peer agent tools
        ↓ backed by:
abi-ai (sibling path dep)    — Abbey/Aviva/Abi persona contracts + router
memory (src/memory/)         — open_backend → Box<dyn MemoryStore>:
                                 sqlite.rs (default) · wdbx.rs (--features wdbx)
wdbx_bridge                  — `abbey wdbx` → `abi wdbx` subprocess passthrough
agent                        — cursor-agent process executor
```

Key modules (`src/`):

| Module | Responsibility |
|---|---|
| `main.rs` | thin entry — routing only, must stay under 200 lines |
| `actions.rs` | `RunSpec` + `run_agent` — the one path every surface calls |
| `commands.rs` | clap subcommand match → actions |
| `slash.rs` / `slash_dispatch.rs` | slash catalog + shared handler → actions |
| `session.rs` | global flag application, `hybrid_run`, history compaction |
| `persona.rs` / `roles.rs` / `route_log.rs` | hybrid routing spine (`route_decision` → conf/alt/fb on JSONL) |
| `please_fix.rs` | last-failure prompt + capture summarizer (argv-safe) |
| `media.rs` | image/video path attach → `--add-dir` + prompt note (no local vision) |
| `memory/` (`mod.rs`, `sqlite.rs`, `wdbx.rs`) | `MemoryStore` trait, shared reflect/validation, backend dispatch; add new backends here and they work everywhere |
| `hybrid_loop.rs` | two-stage Gemma→Max run; stages linked by `correlation` in the route log |
| `wdbx_bridge.rs` | `abbey wdbx` — passthrough to `abi wdbx`, plus in-process `stats`/`checkpoint` |
| `learn.rs` | self-learn capture/digest/review/stats into `train_candidate` |
| `os_control.rs` | cross-platform OS allowlist policy |
| `parallel.rs` | multi-lane fan-out (Max/Gemma/Aviva) |
| `inventory.rs` | skills/plugins/peer-agent-tool discovery |
| `init/` (`mod.rs`, `detect.rs`, `probe.rs`) | `abbey init` — scans a project and writes `AGENTS.md` |
| `tui/` (`app.rs`, `ui.rs`, `tabs.rs`, `mod.rs`) | 7-tab ratatui app |
| `doctor.rs` | doctor/debug/persona/role/memory checks |
| `agent.rs` / `models.rs` / `gitops.rs` / `state.rs` / `config.rs` | executor invocation (cursor · grok · on-device `fm`), model aliases, local git helpers, per-cwd state, config loading |

Personas (Abbey/Aviva/Abi) and Max/Gemma worker roles are defined in the sibling `abi-ai` crate (`../abi/crates/abi-ai/src/identity.rs`) — Abbey's own code consumes those contracts rather than redefining identity. See [docs/identity.md](docs/identity.md) for the distilled spec and Current/Proposed status of each claim.

## Conventions specific to this repo

- **File size is enforced by `check.sh`, not just style**: `main.rs` must stay under 200 lines (hard fail); other `.rs` files warn past 800 and hard-fail past 1000 lines. Split modules before hitting the ceiling rather than after.
- Prefer small, reviewable diffs that match existing style.
- Only commit when asked; never force-push `main`; never commit secrets.
- **Keep claims honest** — this project explicitly tracks what is shipped ("Current") vs. designed-only ("Proposed") vs. explicitly deferred ("Out of scope") in [AGENTS.md](AGENTS.md)'s claims-gate table and in the docs. Backend is `cursor-agent`, not a reimplementation of Grok/Codex/Claude runtimes; Max/Gemma are model-alias bindings, not local weights; `/cost` is intentionally N/A. Don't let new code or docs imply otherwise. A capability behind an off-by-default feature is "Current behind `--features X`", not plain Current — and only if the gate compiles and tests it.
- **A feature-gated module is invisible to the default gate.** If you add another `[features]` entry, add matching `clippy`/`test` lines to `check.sh`, or the code can rot while CI stays green.
- **Each backend gets its own argv grammar.** `fm` shares none of cursor-agent's flags; `build_args_fm` is built from scratch rather than filtered, and a test asserts no cursor flag can leak into it. Under `fm`, don't let `hybrid_run` inject role→model ids (its vocabulary is `system|pcc`), and don't forward account verbs — it has no account.
- OS execution (`os_control.rs`) must never run without `--confirm`, and only against the allowlist — this is a safety invariant, not a default to relax.
- **`WdbxMemory` must hold its `flock(2)` for the handle's whole life.** `abi-wdbx`'s `DurableStore` has no cross-process locking; without the guard, two concurrent `abbey` processes interleave WAL appends and leave the store permanently unreadable (verified: 20 writers → CRC mismatch, every later open fails). SQLite survives the same load unaided, so the lock is what makes the two backends interchangeable. `wdbx_bridge` takes the same lock when a passthrough targets Abbey's own store — new code paths that reach the store must not route around it.
- Read-only callers should use `memory::backend_path` (pure) rather than opening, and interactive ones `open_backend_with_timeout` — `learn status` once created the very store it was meant to report on, and the TUI redraw would otherwise stall 10s on a lock.
- `abi wdbx` takes **base paths** (parent dir + base name) while Abbey opens a **directory** — Abbey's `<state>/wdbx/` is `<state>/wdbx/wdbx` to `abi`. `wdbx_bridge` translates; passing the bare directory silently reads one level up and reports an empty store.
- Self-learn's `train_candidate` path requires provenance; don't add silent deletes to the reflect/digest flow.
- State (`~/.local/state/abbey`, including `memory.sqlite`) is runtime data — never commit it, and don't assume it exists in a fresh checkout.

## Docs map

- [docs/identity.md](docs/identity.md) — persona/role spec, Current vs. Proposed
- [docs/architecture.md](docs/architecture.md) — layered module map, production rules, feature matrix
- [docs/production.md](docs/production.md) — release gate, runtime deps, config/env vars, versioning, release checklist
- [tasks/goals.md](tasks/goals.md) / [tasks/todo.md](tasks/todo.md) / [tasks/lessons.md](tasks/lessons.md) — active goals and backlog
