# AGENTS.md
<!-- abbey-claims-sha256: 6451afc47d15af34424f5885e18a540bb2d317fba24d1f6323d6fcac4831d485 -->

Guidance for AI coding agents in this repository.

## Project

- Name: `abbey`
- Purpose: Hybrid coding-agent CLI/TUI — personas, Max/Gemma roles, parallel lanes, OS control, skills/plugins inventory, self-learn — backed by **cursor-agent** (+ optional `abi`)
- Stack: Rust nightly, edition 2024, `ratatui`, `clap`, path-dep `abi-ai`
- Root: `/Users/donaldfilimon/abbey`
- Install: `./install.sh` → `~/.local/bin/abbey` + Unix `abbeyd`
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
src/main.rs           pre-Clap SIGPIPE + library entry (<200 lines)
src/lib.rs            private implementation graph + narrow public API
src/entry.rs          CLI/TUI routing behind the library boundary
src/app_core/         typed read-only Status/Claims application contracts
src/bin/abbeyd.rs     bounded authenticated Unix daemon entry
src/daemon/           bounded client + owner-only UDS protocol/server (no execution or memory ownership)
src/cli.rs            clap Cli/Subcommand definitions
src/commands.rs       clap dispatch
src/output.rs         stdout helpers (broken pipe = success)
src/build_info.rs     build-stamp constants from build.rs
src/improve/          `abbey improve` — goal pick + lanes + check.sh gate
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
src/claims.rs        Current/Partial/Proposed/Blocked/OOS gate + refuse
src/platform.rs      host OS/threads + GPU/NPU/TPU detect
src/host.rs          portable PATH/PATHEXT + argv clamp + install paths
src/surfaces.rs      vision/cot/runtime honesty surfaces
src/learn.rs os_control.rs parallel.rs inventory.rs
src/tui/              7-tab ratatui
src/init/ gitops.rs agent/ models.rs state.rs config.rs
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

CLI: `abbey claims` · `abbey claims partial|proposed|blocked|oos` · `abbey claims refuse lora|multinode`
(source: `src/claims.rs` — keep this table and that module aligned).

| Claim | Status | Evidence boundary |
| --- | --- | --- |
| cursor-agent backend (CLI/TUI) | Current | Default executor, but not required when another configured backend is available. |
| Grok/Codex/Claude surface parity | Partial | Selected aliases/surfaces ship; polish remains; vendor runtimes are not reimplemented. |
| Persona Abbey/Aviva/Abi; Max/Gemma bindings | Current | Bindings are not local model weights. |
| SQLite memory, self-learn, 3-D map, lexical + opt-in semantic search | Current | OpenAI live paid-call proof remains credential-dependent and unverified. |
| WDBX in-process and cross-process lock | Current | Feature is off by default; fs4 maps to Unix `flock` and Windows `LockFileEx`. |
| Hybrid loop, parallel lanes, multi-subagents, same-host PATH peers | Current | Does not prove a production multi-VM mesh. |
| Goal-driven improve (`abbey improve` + `check.sh`) | Current | Bounded local apply; does not auto-mark goals done. |
| Shared application core + authenticated read-only `abbeyd` | Current | `abbey daemon status\|claims` uses versioned Status/Claims over an owner-only Unix socket; not the Proposed owned agent/tool runtime. |
| Portable Linux/macOS/Windows source surfaces | Current | macOS is locally exercised; Linux/Windows runtime proof remains open. |
| GPU/NPU/TPU host detection | Current | Inventory only, not accelerator execution. |
| CoT transcript viewer and structured `reason` wrap | Current | Abbey-owned hidden CoT engine/UI remains Out of scope. |
| Tool responsibility matrix, MCP inventory, ACP peer launch | Current | Inventory/delegation only; no Abbey-owned host yet. |
| OS allowlist + `--confirm` | Current | No allowlist bypass in the shipped edition. |
| Media path attach, delegated image/video generation, macOS voice I/O | Current | These do not establish local neural media models. |
| On-device `fm` and sibling `abi` backends | Current | Either can run without cursor-agent; a real `abi` binary is required for the ABI route. |
| Tauri 2 + React/TypeScript desktop GUI | Proposed | Ratatui remains the shipped UI until implementation and runtime proof land. |
| Provider-neutral Abbey-owned agent/tool runtime and MCP/ACP host | Proposed | Current executors remain delegated. |
| Production-capable local model weights | Proposed | Max/Gemma remain bindings until actual weights and evaluation evidence exist. |
| Fine-tuning / LoRA pipeline | Proposed | `train_candidate` is currently curation only. |
| GPU/NPU/TPU compilation, training, and inference | Proposed | Hardware detection is the only Current accelerator surface. |
| Local neural speech/image/video models | Proposed | Platform voice I/O and delegated media tools remain the Current substitutes. |
| Personal-unrestricted separate edition | Proposed | Must be separately packaged, locally controlled, isolated, consented, and auditable; shipped Abbey keeps its safety invariant. |
| Authenticated local three-VM shared-compute proof | Proposed | One Mac, three VMs is the next proof; Unix loopback multi-process is Current. |
| Production separate-physical-host / geographic-HA / multi-GPU mesh | Proposed | Remains Proposed even after the local three-VM proof. |
| Self-hosted Linux CI execution proof | Blocked | ABI dependency resolved at `32e372d7f522f5a6c9c0ef92c5b9612b52cfea05`; macOS ARM64 runner registered; Linux ARM64 not provisioned and its job is repository-variable gated; Linux/Windows proof stays open. |
| Reimplement vendor runtimes; fake cost accounting; bundled cloud TTS/STT | Out of scope | Do not imply these through compatibility surfaces. |
| Unrestricted shell/allowlist bypass in shipped Abbey | Out of scope | Separate personal edition is Proposed; this edition remains fail-closed. |
| Abbey-owned hidden CoT engine / interactive hidden-CoT UI | Out of scope | Transcript viewing is Current. |

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
- `abi` on this machine is often a **shell alias**, not a binary — both the WDBX CLI
  bridge and `ABBEY_BACKEND=abi` need a real one: `cargo build -p abi-cli` in `../abi`,
  then set `ABBEY_ABI_BIN` (or `abi_bin` in config.toml)
- Semantic embeddings default to `none`. Select `apple` or `openai` explicitly in
  `[embeddings]`; API keys stay in `ABBEY_EMBEDDING_API_KEY`/`OPENAI_API_KEY`, never
  `config.toml`. Provider/model changes create a new isolated vector space and never
  silently fall back or overwrite an older space.
- `abbey daemon` and `abbeyd` must use the same socket and exactly one bearer source.
  Bearer files are owner-only; client failures never fall back to in-process claims, and
  non-Unix remains unsupported until a named-pipe transport is implemented and proven.
- `abi wdbx` paths are **base paths** (dir + base name); Abbey opens a directory. Abbey's
  `<state>/wdbx/` is `<state>/wdbx/wdbx` to `abi` — `wdbx_bridge` translates, don't re-break it
- `DurableStore` has no cross-process locking; `WdbxMemory` adds an `fs4` advisory
  lock (Unix `flock` / Windows `LockFileEx`). Removing it corrupts the WAL
  irrecoverably under concurrent `abbey` processes
- `AgentBackend::from_env()` resolves **once per process** (env > config `backend`
  key > cursor). The TUI's Ctrl-B switch changes `AgentConfig::backend` at runtime,
  so never branch per-call behaviour on `from_env()` — thread `cfg.backend` through
  (`state.read_chat_for(cfg.backend)`). Getting this wrong let a cursor-launched
  session keep adopting `CURSOR_AGENT_CHAT_ID` after switching to `abi`, silently
  killing continuity

## Out of scope

- Reimplementing Cursor / Grok / Codex runtimes
- Fake token accounting; shipped-edition allowlist bypass; bundled cloud TTS/STT
- Abbey-owned hidden chain-of-thought engine / interactive hidden-CoT UI
- Large clean-slate rewrites without confirmation

LoRA and the other approved expansion capabilities are **Proposed**, not Out of
scope; that status authorizes roadmap work, not a Current claim or a successful
command before implementation and evidence.
