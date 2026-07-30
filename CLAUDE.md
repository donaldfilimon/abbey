# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Abbey — a hybrid persona/role CLI/TUI written in Rust, backed by `cursor-agent` (+ optional `abi`). It mimics the surface of Grok Build, Codex, and Claude Code (slash commands, `-c`/continue, `exec`/`review`/`commit`/`pr`, TUI) while routing actual generation through `cursor-agent` and, for personas/identity, the sibling `abi-ai` crate.

Full agent guidance lives in [AGENTS.md](AGENTS.md) — read it too; this file does not duplicate its claims-gate table or gotchas.

## Commands

```bash
cargo build --release              # build
./install.sh                       # build + install to ~/.local/bin/abbey
./check.sh                         # production gate: fmt --check + clippy -D warnings + test + file-size guard
cargo test                         # tests only
cargo test <test_name>             # single test
cargo clippy --all-targets -- -D warnings
cargo fmt --all
abbey doctor                       # build stamp + persona/role/memory/os honesty check
```

`./check.sh` is the merge bar — always run it (not just `cargo test`) before considering work done; it also enforces the file-size rule below and will fail the build if violated.

## Toolchain

Rust **nightly**, edition **2024**, pinned via `rust-toolchain.toml` (`rustfmt` + `clippy` components). `abbey` depends on a path dependency `abi-ai = { path = "../abi/crates/abi-ai" }` — this repo must be checked out as a sibling of `../abi` or the build fails.

## Architecture

Abbey has one canonical execution path shared by the CLI, slash commands, and the TUI — do not add a second way to invoke the agent.

```
CLI (clap) · TUI (ratatui) · slash catalog
        ↓ all three funnel into:
actions::run_agent           — canonical RunSpec, the single entry point
session::hybrid_run          — persona + role + prefs + routing decision
parallel                     — Max/Gemma/Aviva lane fan-out
os_control                   — allowlist dry-run / execute --confirm
learn                        — correction/preference/digest → SQLite
inventory                    — skills/plugins/peer agent tools
        ↓ backed by:
abi-ai (sibling path dep)    — Abbey/Aviva/Abi persona contracts + router
memory (src/memory/)         — SQLite store (WDBX bridge is Proposed, not shipped)
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
| `persona.rs` / `roles.rs` / `route_log.rs` | the hybrid routing spine (persona × role × logged route) |
| `memory/` (`mod.rs`, `sqlite.rs`) | memory trait + SQLite-backed store |
| `learn.rs` | self-learn capture/digest into `train_candidate` |
| `os_control.rs` | cross-platform OS allowlist policy |
| `parallel.rs` | multi-lane fan-out (Max/Gemma/Aviva) |
| `inventory.rs` | skills/plugins/peer-agent-tool discovery |
| `init/` (`mod.rs`, `detect.rs`, `probe.rs`) | `abbey init` — scans a project and writes `AGENTS.md` |
| `tui/` (`app.rs`, `ui.rs`, `tabs.rs`, `mod.rs`) | 7-tab ratatui app |
| `doctor.rs` | doctor/debug/persona/role/memory checks |
| `agent.rs` / `models.rs` / `gitops.rs` / `state.rs` / `config.rs` | executor invocation, model aliases, local git helpers, per-cwd state, config loading |

Personas (Abbey/Aviva/Abi) and Max/Gemma worker roles are defined in the sibling `abi-ai` crate (`../abi/crates/abi-ai/src/identity.rs`) — Abbey's own code consumes those contracts rather than redefining identity. See [docs/identity.md](docs/identity.md) for the distilled spec and Current/Proposed status of each claim.

## Conventions specific to this repo

- **File size is enforced by `check.sh`, not just style**: `main.rs` must stay under 200 lines (hard fail); other `.rs` files warn past 800 and hard-fail past 1000 lines. Split modules before hitting the ceiling rather than after.
- Prefer small, reviewable diffs that match existing style.
- Only commit when asked; never force-push `main`; never commit secrets.
- **Keep claims honest** — this project explicitly tracks what is shipped ("Current") vs. designed-only ("Proposed") vs. explicitly deferred ("Out of scope") in [AGENTS.md](AGENTS.md)'s claims-gate table and in the docs. Backend is `cursor-agent`, not a reimplementation of Grok/Codex/Claude runtimes; Max/Gemma are model-alias bindings, not local weights; `/cost` is intentionally N/A. Don't let new code or docs imply otherwise.
- OS execution (`os_control.rs`) must never run without `--confirm`, and only against the allowlist — this is a safety invariant, not a default to relax.
- Self-learn's `train_candidate` path requires provenance; don't add silent deletes to the reflect/digest flow.
- State (`~/.local/state/abbey`, including `memory.sqlite`) is runtime data — never commit it, and don't assume it exists in a fresh checkout.

## Docs map

- [docs/identity.md](docs/identity.md) — persona/role spec, Current vs. Proposed
- [docs/architecture.md](docs/architecture.md) — layered module map, production rules, feature matrix
- [docs/production.md](docs/production.md) — release gate, runtime deps, config/env vars, versioning, release checklist
- [tasks/goals.md](tasks/goals.md) / [tasks/todo.md](tasks/todo.md) / [tasks/lessons.md](tasks/lessons.md) — active goals and backlog
