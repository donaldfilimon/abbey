# AGENTS.md

Guidance for AI coding agents in this repository.

## Project

- Name: `abbey`
- Purpose: Hybrid coding-agent CLI/TUI — personas, Max/Gemma roles, parallel lanes, OS control, skills/plugins inventory, self-learn — backed by **cursor-agent** (+ optional `abi`)
- Stack: Rust nightly, edition 2024, `ratatui`, `clap`, path-dep `abi-ai`
- Root: `/Users/donaldfilimon/abbey`
- Install: `./install.sh` → `~/.local/bin/abbey`
- Gate: `./check.sh` (fmt + clippy -D warnings + test + file-size)

## Spec

- [docs/identity.md](docs/identity.md)
- [docs/architecture.md](docs/architecture.md)
- [docs/production.md](docs/production.md)

## Commands

- Build: `cargo build --release` · `./install.sh`
- Test / lint: `./check.sh` or `cargo test` · `cargo clippy --all-targets -- -D warnings`
- WDBX backend: `cargo build --features wdbx` (off by default; `check.sh` gates both sets)

## Layout

```
src/main.rs           thin entry (<200 lines)
src/commands.rs       clap dispatch
src/slash_dispatch.rs slash handler
src/session.rs        hybrid_run + flags
src/doctor.rs         doctor / persona / memory helpers
src/prompts.rs        review/commit prompts
src/persona.rs roles.rs route_log.rs
src/memory/          trait + sqlite + wdbx (feature-gated) + map
src/hybrid_loop.rs   Gemma interpret → Max implement, correlated
src/wdbx_bridge.rs   `abbey wdbx` → `abi wdbx` passthrough
src/please_fix.rs   last-failure prompt + capture summarizer
src/highlight.rs     syntect fence/file ANSI (auto on -p)
src/subagents.rs     multi-lane + local peer agents
src/claims.rs        Current/Proposed/OOS gate + refuse
src/platform.rs      host OS/threads + GPU/NPU/TPU detect
src/host.rs          portable PATH/PATHEXT + argv clamp + install paths
src/surfaces.rs      vision/cot/runtime honesty surfaces
src/learn.rs os_control.rs parallel.rs inventory.rs
src/tui/              7-tab ratatui
src/init/ gitops.rs agent.rs models.rs state.rs config.rs
docs/                 identity · architecture · production
tasks/                goals + todo
```

## Conventions

- Prefer small, reviewable diffs; match existing style
- Do not grow any `.rs` file past **1000** lines; keep `main.rs` under **200**
- Do not commit secrets; only commit when asked; never force-push `main`
- Keep parity claims honest (cursor-agent backend; `/cost` N/A; Max/Gemma = bindings)
- Goals: `tasks/goals.md`

## Hybrid routing and memory

See [docs/architecture.md](docs/architecture.md). Personas via `abi-ai`; Max/Gemma roles; SQLite memory; self-learn injects LTM preferences into `hybrid_run`.

### Claims gate

CLI: `abbey claims` · `abbey claims proposed|oos` · `abbey claims refuse embeddings|lora|multinode`
(source: `src/claims.rs` — keep this table and that module aligned).

| Claim | Current | Proposed | Out of scope |
| --- | --- | --- | --- |
| cursor-agent backend (CLI/TUI) | ✓ | | |
| Grok/Codex/Claude surface parity | partial | polish | reimplement runtimes |
| Persona Abbey/Aviva/Abi | ✓ | | |
| Max/Gemma role bindings | ✓ | | local Qwen/Gemma weights |
| SQLite memory + self-learn | ✓ | | |
| Parallel lanes (Max/Gemma/Aviva) | ✓ | | |
| Multi-subagent fan-out (`subagents` / synthesize) | ✓ | | |
| Local distributed peer agents (PATH CLIs) | ✓ | | multi-node agent mesh |
| linux/macos/windows primary host targets (portable surfaces) | ✓ | | voice/fm macOS-only; see `platform paths` |
| Multi-threaded subagent fan-out (`--jobs`) | ✓ | | GPU kernels |
| WDBX cross-process lock (Unix + Windows via fs2) | ✓ | | |
| GPU/NPU/TPU host detection (`abbey platform`) | ✓ | | accelerator runtime in Abbey |
| CoT transcript viewer (`abbey cot`) | ✓ | | Abbey-owned CoT engine/UI |
| Tool responsibility matrix (`abbey runtime`) | ✓ | | Abbey as tool runtime |
| OOS honesty (`abbey oos` / lora|weights|accel|shell|host) | ✓ | | (surfaces refuse; do not implement) |
| OS allowlist control | ✓ | | unrestricted shell |
| Skills/plugins inventory | ✓ | | |
| Unique build stamp | ✓ | | |
| Modular src/ (<1k files) | ✓ | | |
| WDBX in-process (feature `wdbx`, **off by default**) | ✓ | | |
| WDBX CLI bridge (`abbey wdbx` → `abi wdbx`) | ✓ (needs a real `abi` binary) | | |
| Hybrid loop (Gemma interpret → Max implement) | ✓ | | |
| Route confidence / alternate / fallback (audit only) | ✓ | | |
| `learn review` / `stats` (train_candidate curation) | ✓ | | |
| On-device backend (`ABBEY_BACKEND=fm`, macOS 26+) | ✓ | | |
| 3-D memory map (topic × recency × consolidation) | ✓ | | |
| please-fix capture summarizer + argv clamp | ✓ | | |
| Media path attach (`--image`/`--video`/`/image`) | ✓ | | local vision/video weights |
| Image/video generation via agent tools (`imagine`/`generate`) | ✓ | | local vision/video weights |
| Thinking aliases + `reason` structured wrap | ✓ | | Abbey-owned CoT engine |
| macOS voice I/O (Premium/Enhanced TTS + on-device STT) | ✓ | | cloud TTS/STT SaaS |
| MCP config inventory + cursor-agent mcp mgmt | ✓ | | Abbey as MCP host |
| ACP peer inventory / launch (`acp run`) | ✓ | | Abbey as ACP host |
| MCP/tools during a run (`--approve-mcps`) | ✓ | | Abbey as tool runtime |
| Auto code highlighting (fences on `-p`/print · `abbey highlight`) | ✓ | | full markdown UI / LSP |
| Semantic/learned memory embedding space | | ✓ | |
| Memory filter by source / timestamp / project | | ✓ | |
| Multi-node · multi-GPU · shared compute mesh | | ✓ | |
| GPU/NPU/TPU compilation, training, inference in Abbey | | | ✓ |
| Autonomous OS/service operation (no allowlist) | | | ✓ |
| Abbey as her own trained model (own weights) | | | ✓ |
| Local Qwen/Gemma weights | | | ✓ |
| Fine-tuning / LoRA | | | ✓ |
| Fake cost accounting | | | ✓ |

## Gotchas

- Toolchain: `rust-toolchain.toml` nightly + edition 2024. **`../abi` requires
  `rust-version = 1.99`.** A Homebrew-installed `rustc`/`cargo` on PATH shadows rustup's
  nightly (rustup's shims live in `~/.cargo/bin` and may be missing entirely), so the gate
  can fail with "rustc 1.97.1 is not supported by the following packages" while rustup's
  nightly is perfectly new enough. `check.sh` now detects this and prints the remedy
  (`brew unlink rust` / `rustup default nightly`)
- `abi-ai` path-dep expects sibling `../abi`; `--features wdbx` also needs `../abi/crates/abi-wdbx`
- Default `cargo clippy`/`cargo test` never compile the `wdbx` module — use `./check.sh`,
  which runs both feature sets, or gated code rots unnoticed
- Git helpers need a real repo history
- OS execute always needs `--confirm`
- `abi` on this machine is a **shell alias**, not a binary — the WDBX CLI bridge needs a real
  one: `cargo build -p abi-cli` in `../abi`, then set `ABBEY_ABI_BIN`
- `abi wdbx` paths are **base paths** (dir + base name); Abbey opens a directory. Abbey's
  `<state>/wdbx/` is `<state>/wdbx/wdbx` to `abi` — `wdbx_bridge` translates, don't re-break it
- `DurableStore` has no cross-process locking; `WdbxMemory` adds an `fs2` advisory
  lock (Unix `flock` / Windows `LockFileEx`). Removing it corrupts the WAL
  irrecoverably under concurrent `abbey` processes

## Out of scope

- Reimplementing Cursor / Grok / Codex runtimes
- Fake token accounting; LoRA runners
- Large clean-slate rewrites without confirmation
