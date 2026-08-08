# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Abbey — a hybrid persona/role CLI/TUI written in Rust, backed by `cursor-agent` (+ optional `abi`). It mimics the surface of Grok Build, Codex, and Claude Code (slash commands, `-c`/continue, `exec`/`review`/`commit`/`pr`, TUI) while routing actual generation through `cursor-agent` and, for personas/identity, the sibling `abi-ai` crate.

Full agent guidance lives in [AGENTS.md](AGENTS.md) — read it too; this file does not duplicate its claims-gate table or gotchas.

## Commands

```bash
cargo build --release              # build
cargo build --features wdbx        # + in-process WDBX memory backend (off by default)
./install.sh                       # build + install to ~/.local/bin/abbey (Windows: install.ps1)
./check.sh                         # production gate — see below
cargo test                         # default-feature tests only
cargo test --features wdbx         # includes src/memory/wdbx.rs
cargo test <test_name>             # single test
cargo test --test cli_surface      # binary-level integration tests only
cargo clippy --all-targets -- -D warnings
cargo fmt --all
abbey doctor                       # build stamp + persona/role/memory/os honesty check
```

`./check.sh` is the merge bar — always run it before considering work done. In order: a toolchain probe (a cheap `cargo check` that trips the `rust-version` gate during unit-graph construction, so a shadowed Homebrew cargo fails fast with the remedy printed), fmt-check, clippy `-D warnings`, tests — the last two **for both feature sets** — the file-size guard, then *soft* cross-compile checks for `x86_64-pc-windows-gnu` / `x86_64-unknown-linux-gnu` (skipped when the target isn't installed, and never a hard failure). A bare `cargo test` never compiles `src/memory/wdbx.rs`, so it can pass while the gated backend is broken; that is exactly why the gate runs twice.

`tests/` holds two integration files that drive the **built binary** (`CARGO_BIN_EXE_abbey`), because some guarantees only exist once the process runs — real exit codes, real stdout/stderr, the SIGPIPE reset that happens before `main`. `cli_surface.rs` covers those, and runs every test under a throwaway `ABBEY_STATE_DIR` so nothing touches the developer's real chat id, memory store, or route log; keep that property when adding to it. `slash_parse.rs` covers the slash/CLI surface and does **not** override the state dir — it is only safe because its cases are read-only or `current_dir`-scoped to a temp project, so a state-mutating test does not belong there without adding the override first.

## Toolchain

Rust **nightly**, edition **2024**, pinned via `rust-toolchain.toml` (`rustfmt` + `clippy` components). `abbey` path-depends on `abi-ai = { path = "../abi/crates/abi-ai" }` (and, under `--features wdbx`, `abi-wdbx`) — this repo must be checked out as a sibling of `../abi` or the build fails.

## Architecture

Abbey has one canonical execution path shared by the CLI, slash commands, and the TUI — do not add a second way to invoke the agent.

Two headless capture commands are the standing exceptions: `print`
(`commands.rs`) and `commit` (`actions::run_commit`) call
`AgentConfig::run_capture` directly and never reach `run_agent`. They therefore
skip `hybrid_run` entirely — no persona/role wrap, no prefs injection, and **no
`route.jsonl` entry** (verified: `abbey print …` leaves the route log unchanged
where `abbey ask …` appends a row). That is deliberate for single-shot piping,
but it means the routing audit does not see them. Keep the list at two.

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
| `cli.rs` | clap `Cli`/`Subcommand` definitions (Grok Build/Codex/Claude Code parity surface) |
| `actions.rs` | `RunSpec` + `run_agent` — the one path every surface calls |
| `commands.rs` | clap subcommand match → actions |
| `prompts.rs` | review/commit prompt builders over `gitops` diffs |
| `output.rs` | stdout helpers that treat a broken pipe as success (`abbey doctor \| head`) |
| `build_info.rs` | build-stamp constants from `build.rs` (version/git/target/profile/host) |
| `improve/` (`mod.rs`, `gate.rs`, `ledger.rs`, `report.rs`) | `abbey improve` — goal-ledger pick + local subagent lanes + `check.sh` gate; stops on green + no open slice, never auto-closes a goal to done |
| `slash.rs` / `slash_dispatch.rs` | slash catalog + shared handler → actions |
| `session.rs` | global flag application, `hybrid_run`, history compaction |
| `persona.rs` / `roles.rs` / `route_log.rs` | hybrid routing spine (`route_decision` → conf/alt/fb on JSONL) |
| `please_fix.rs` | last-failure prompt + capture summarizer (argv-safe) |
| `media.rs` | image/video path attach → `--add-dir` + prompt note (no local vision) |
| `generate.rs` | `imagine` / `generate video` / `reason` via cursor-agent tools (no local models) |
| `voice.rs` | macOS Premium/Enhanced TTS + on-device STT (`scripts/abbey-stt.swift`) |
| `protocols.rs` | MCP config inventory + ACP peer discovery/launch (not a host runtime) |
| `highlight.rs` | syntect ANSI for fenced code on `-p`/print + `abbey highlight` |
| `subagents.rs` | multi-subagent lanes + local PATH peer fan-out + synthesize |
| `claims.rs` | Current/Proposed/OOS gate + refuse paths (embeddings/LoRA/multi-node) |
| `platform.rs` | host OS matrix + threads + GPU/NPU/TPU detect (not accelerator runtime) |
| `surfaces.rs` | vision/cot/runtime honesty (viewer + matrix; weights/engine/host OOS) |
| `deferred.rs` | OOS honesty pack — lora/weights/accel/shell/host (+ `abbey oos` index) |
| `host.rs` | portable PATH/PATHEXT lookup, argv clamp, install/state path helpers |
| `memory/` (`mod.rs`, `sqlite.rs`, `wdbx.rs`, `map.rs`, `similarity.rs`) | `MemoryStore` trait, shared reflect/validation, backend dispatch (add new backends here and they work everywhere); `map.rs` = deterministic 3-D map (topic × recency × consolidation), `similarity.rs` = `memory similar` over `abi_ai::text_embedding` feature-hash vectors — **lexical, not learned**; both axes and distances answer surface-form questions, never semantic ones |
| `hybrid_loop.rs` | two-stage Gemma→Max run; stages linked by `correlation` in the route log |
| `wdbx_bridge.rs` | `abbey wdbx` — passthrough to `abi wdbx`, plus in-process `stats`/`checkpoint` |
| `learn.rs` | self-learn capture/digest/review/stats into `train_candidate` |
| `os_control.rs` | cross-platform OS allowlist policy |
| `parallel.rs` | multi-lane fan-out (Max/Gemma/Aviva) |
| `inventory.rs` | skills/plugins/peer-agent-tool discovery |
| `init/` (`mod.rs`, `detect.rs`, `probe.rs`) | `abbey init` — scans a project and writes `AGENTS.md` |
| `tui/` (`app.rs`, `keys.rs`, `ui.rs`, `tabs.rs`, `overlay.rs`, `refresh.rs`, `theme.rs`, `widgets.rs`, `mod.rs`) | 7-tab ratatui app; `keys.rs` holds key/palette/editor/mouse input handling split out of `app.rs` |
| `doctor.rs` | doctor/debug/persona/role/memory checks |
| `agent/` (`mod.rs`, `argv.rs`) / `models.rs` / `gitops.rs` / `state.rs` / `config.rs` | executor invocation (cursor · grok · on-device `fm` · `abi complete`; `argv.rs` holds the per-backend argv grammars + prompt/argv clamping), model aliases, local git helpers, per-cwd state, config loading |

Personas (Abbey/Aviva/Abi) and Max/Gemma worker roles are defined in the sibling `abi-ai` crate (`../abi/crates/abi-ai/src/identity.rs`) — Abbey's own code consumes those contracts rather than redefining identity. See [docs/identity.md](docs/identity.md) for the distilled spec and Current/Proposed status of each claim.

## Conventions specific to this repo

- **File size is enforced by `check.sh`, not just style**: `main.rs` must stay under 200 lines (hard fail); other `.rs` files warn past 800 and hard-fail past 1000 lines. Split modules before hitting the ceiling rather than after.
- Prefer small, reviewable diffs that match existing style.
- Only commit when asked; never force-push `main`; never commit secrets.
- **Keep claims honest** — this project explicitly tracks what is shipped ("Current") vs. designed-only ("Proposed") vs. explicitly deferred ("Out of scope") in [AGENTS.md](AGENTS.md)'s claims-gate table and in the docs. Backend is `cursor-agent`, not a reimplementation of Grok/Codex/Claude runtimes; Max/Gemma are model-alias bindings, not local weights; `/cost` is intentionally N/A. Don't let new code or docs imply otherwise. A capability behind an off-by-default feature is "Current behind `--features X`", not plain Current — and only if the gate compiles and tests it.
- **A feature-gated module is invisible to the default gate.** If you add another `[features]` entry, add matching `clippy`/`test` lines to `check.sh`, or the code can rot while CI stays green.
- **Each backend gets its own argv grammar.** `fm` and `abi` share none of cursor-agent's flags; `build_args_fm` / `build_args_abi` are built from scratch rather than filtered, and tests assert no cursor flag can leak into them. Under `fm`/`abi`, don't let `hybrid_run` inject role→model ids (`fm` vocabulary is `system|pcc`; under `abi` a cursor `claude-*` binding would look like an explicit live-transport request), and don't forward account verbs — they have no account.
- **`ABBEY_BACKEND=abi` runs Abbey with no cursor-agent.** Backend precedence is env > config `backend` key > cursor. Continuity under `abi` is Abbey's own — `<state>/abi/<chat>.transcript` plus a bounded context prefix — because `abi complete` is a stateless one-shot; don't let docs imply abi gained sessions.
- **Never branch per-call behaviour on `AgentBackend::from_env()`** — it is resolved once per process, while the TUI's Ctrl-B switch changes `AgentConfig::backend` at runtime. Thread the live `cfg.backend` through instead (`state.read_chat_for(cfg.backend)`, `AgentBackend::transcript_subdir()`). Reading the cached value is what let a cursor-launched session keep adopting `CURSOR_AGENT_CHAT_ID` after switching to `abi`, silently killing continuity — a guard that consults the wrong backend is not a guard.
- OS execution (`os_control.rs`) must never run without `--confirm`, and only against the allowlist — this is a safety invariant, not a default to relax.
- **`WdbxMemory` must hold its `fs4` advisory lock for the handle's whole life** (Unix `flock` / Windows `LockFileEx`). `abi-wdbx`'s `DurableStore` has no cross-process locking; without the guard, two concurrent `abbey` processes interleave WAL appends and leave the store permanently unreadable (verified: 20 writers → CRC mismatch, every later open fails). SQLite survives the same load unaided, so the lock is what makes the two backends interchangeable. `wdbx_bridge` takes the same lock when a passthrough targets Abbey's own store — new code paths that reach the store must not route around it.
- Read-only callers should use `memory::backend_path` (pure) rather than opening, and interactive ones `open_backend_with_timeout` — `learn status` once created the very store it was meant to report on, and the TUI redraw would otherwise stall 10s on a lock.
- `abi wdbx` takes **base paths** (parent dir + base name) while Abbey opens a **directory** — Abbey's `<state>/wdbx/` is `<state>/wdbx/wdbx` to `abi`. `wdbx_bridge` translates; passing the bare directory silently reads one level up and reports an empty store.
- Self-learn's `train_candidate` path requires provenance; don't add silent deletes to the reflect/digest flow.
- State (`~/.local/state/abbey`, including `memory.sqlite`) is runtime data — never commit it, and don't assume it exists in a fresh checkout.

## Docs map

- [docs/identity.md](docs/identity.md) — persona/role spec, Current vs. Proposed
- [docs/architecture.md](docs/architecture.md) — layered module map, production rules, feature matrix
- [docs/production.md](docs/production.md) — release gate, runtime deps, config/env vars, versioning, release checklist
- [tasks/goals.md](tasks/goals.md) / [tasks/todo.md](tasks/todo.md) / [tasks/lessons.md](tasks/lessons.md) — active goals and backlog
