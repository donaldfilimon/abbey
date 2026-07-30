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
abbey completion zsh > ~/.zsh/completions/_abbey_clap
```

### Slash (CLI or TUI prompt)

```text
/help  /plan  /ask  /diff  /review  /security-review
/commit  /pr  /init [--force|--print|--agent]  /branch name
/clear  /compact  /model fable  /memory  /skills  /permissions  /doctor
```

## Honest limits

- Backend is **cursor-agent**, not a reimplementation of Grok/Codex/Claude runtimes.
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
  please_fix.rs  last-failure prompt
  models.rs       model aliases
  state.rs        per-cwd chats
  tui/            ratatui app
tasks/            goals + todo board
tests/
```
