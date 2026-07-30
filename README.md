# Abbey CLI / TUI

**v2.6** — production-structured hybrid CLI/TUI (modular `src/`, `./check.sh` gate).

Personas · Max/Gemma · parallel · OS control · skills/plugins · self-learn · unique build stamp.

Docs: [identity](docs/identity.md) · [architecture](docs/architecture.md) · [production](docs/production.md).

| Target CLI | Abbey surface |
|------------|----------------|
| **Grok Build** | TUI default, `-c/--continue`, `-w/--worktree`, `--cwd`, model aliases, session resume |
| **Codex** | `exec`/`print`, `review`, `doctor`, `login`/`logout`, `mcp`/`plugin`, `completion`, sandbox/force |
| **Claude Code** | slash commands (`/plan`, `/diff`, `/pr`, `/init`, `/commit`, `/clear`…), `-p`, ask/plan modes |

## Install

```bash
cd ~/abbey
./install.sh
# or:
cargo build --release
install -m 755 target/release/abbey ~/.local/bin/abbey
```

Requires: **Rust nightly** (`rust-toolchain.toml`), **cursor-agent** on PATH.

## Quick use

```bash
abbey                     # TUI
abbey "fix the flaky test"
abbey -c                  # continue session
abbey -m fable plan "…"
abbey exec "summarize src/"
abbey review
abbey security-review
abbey commit
abbey pr
abbey init                # scan cwd → AGENTS.md
abbey init --force        # overwrite
abbey init --print        # preview only
abbey init --agent        # local scan, then refine with cursor-agent
abbey doctor
abbey hybrid-loop "add a dark mode toggle"   # Gemma interprets → Max implements
abbey routes --correlation <id>              # both stages of one hybrid-loop run
abbey wdbx query                             # → `abi wdbx query <abbey store> --json`
abbey wdbx stats                             # in-process (needs --features wdbx)
abbey completion zsh > ~/.zsh/completions/_abbey_clap
```

### Slash (CLI or TUI prompt)

```text
/help  /plan  /ask  /diff  /review  /security-review
/commit  /pr  /init [--force|--print|--agent]  /branch name
/clear  /compact  /model fable  /memory  /skills  /permissions  /doctor
```

## Memory backends

SQLite is the default. The in-process WDBX backend (`abi-wdbx` `DurableStore`) is real but
**off by default** — it pulls a heavier dependency tree:

```bash
cargo build --features wdbx
ABBEY_MEMORY_BACKEND=wdbx abbey memory search "…"   # or memory_backend = "wdbx" in config.toml
```

`./install.sh` builds **without** the feature, so an installed `abbey` asked for `wdbx` will
fall back to SQLite — and say so in `abbey doctor`. It never silently pretends.

Concurrent `abbey` processes are safe on both backends: SQLite via its own file locking, WDBX
via an `flock(2)` guard Abbey holds for the store's lifetime (`abi-wdbx` itself has no
cross-process locking — without the guard, simultaneous writers corrupt the WAL beyond
recovery). The guard is Unix-only.

## Honest limits

- Backend is **cursor-agent**, not a reimplementation of Grok/Codex/Claude runtimes.
- `abbey wdbx <query|db|…>` shells out to `abi`, which must be a **real binary** on PATH —
  a shell alias will not do (`cargo build -p abi-cli` in `../abi`, then set `ABBEY_ABI_BIN`).
  `stats`/`checkpoint` are answered in-process and need `--features wdbx`.
- The WDBX backend uses the KV space; vector/embedding search through WDBX is not wired yet.
- The WDBX cross-process lock is `#[cfg(unix)]`; on non-Unix targets concurrent processes are
  unprotected.
- Personas, worker roles, and provenance memory are **rolling out** — see `tasks/goals.md`; do not assume local Gemma/Qwen weights.
- `/cost` is **N/A** (use Cursor account dashboard).
- Full MCP/plugin UIs pass through to cursor-agent when available.
- Worktree isolation depends on cursor-agent `--worktree`.
- `/init` local scan covers Rust/Node/Zig/Go/Python/Swift/Make/CMake; `--agent` refines via cursor-agent.

## Layout

```
src/
  main.rs         CLI dispatch + slash
  cli.rs          clap surface
  slash.rs        slash catalog
  init.rs         /init project scan → AGENTS.md
  gitops.rs       local git helpers
  agent.rs        cursor-agent invoke
  memory/         trait + sqlite · wdbx (feature-gated)
  hybrid_loop.rs  Gemma interpret → Max implement
  wdbx_bridge.rs  abbey wdbx → abi wdbx
  please_fix.rs  last-failure prompt
  models.rs       model aliases
  state.rs        per-cwd chats
  tui/            ratatui app
tasks/            goals + todo board
tests/
```
