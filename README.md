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
abbey claims                                 # Current / Proposed / Out of scope
abbey claims proposed                        # embeddings, multi-node, …
abbey claims refuse lora                     # honest exit 2
abbey platform                               # linux/macos/windows matrix + threads
abbey compute                                # GPU/NPU/TPU host detect (not Abbey kernels)
abbey vision                                 # path attach + agent gen (local weights OOS)
abbey cot show                               # last reason transcript (not Abbey CoT engine)
abbey runtime                                # who runs tools (Abbey is not the host)
abbey oos                                    # LoRA · weights · NPU/TPU · shell · MCP host
abbey lora|weights|accel|shell|host          # per-topic status / refuse (exit 2)
abbey hybrid-loop "add a dark mode toggle"   # Gemma interprets → Max implements
abbey subagents                              # catalog (abbey lanes + PATH peers)
abbey subagents run --lanes max,reviewer --synthesize "harden auth"
abbey parallel --peers gemini,claude "second opinions"   # local distributed peers
abbey routes --correlation <id>              # both stages (conf · alt · fb)
abbey learn review                           # train_candidate provenance curation
abbey learn stats
abbey wdbx query                             # → `abi wdbx query <abbey store> --json`
abbey wdbx stats                             # in-process (needs --features wdbx)
abbey completion zsh > ~/.zsh/completions/_abbey_clap
```

### Slash (CLI or TUI prompt)

```text
/help  /plan  /ask  /diff  /review  /security-review
/commit  /pr  /init [--force|--print|--agent]  /branch name
/clear  /compact  /model fable  /memory  /routes  /skills  /permissions  /doctor
/learn correction|preference|routes|digest|review|stats
```

## Backends

| Backend | `ABBEY_BACKEND` | Notes |
|---|---|---|
| cursor-agent | `cursor` (default) | hosted models, full tool surface |
| Grok Build | `grok` | passthrough |
| **On-device** | `fm` | Apple Foundation Models CLI, **macOS 26+** — no cursor-agent, no network |

```bash
ABBEY_BACKEND=fm abbey -p "summarise this error"     # runs on-device
ABBEY_BACKEND=fm abbey doctor                        # reports model availability
```

Under `fm`, conversations are kept as transcript files in `<state>/fm/` (it has no
server-side chat ids), and `ask`/`plan` become `--instructions`. Account/session verbs
(`status`, `ls`, `login`, `logout`, `mcp`) have no on-device equivalent and refuse with
exit 2 rather than forwarding. **Max and Gemma both resolve to the `system` model there,
so the role distinction is carried by the prompt, not the model.**

## The 3-D memory map

Every memory has a position on three interpretable axes — **topic** (subject tag),
**recency** (log-compressed age), **consolidation** (activity → stm → ltm →
train_candidate, lifted by confidence):

```bash
abbey memory put "plays fingerstyle guitar" --tag guitar --retention ltm
abbey memory map                 # the whole map
abbey memory near <id>           # what else do I know about this?
```

Tag your memories — untagged ones share one visible `(untagged)` column. Under
`--features wdbx` each point is mirrored into WDBX's spatial space, so `abi wdbx` sees
the same map.

This is a **deterministic layout, not a learned embedding space** — Abbey has no
embedder, and calling the distances semantic would overstate them. The TUI Memory
tab shows a short map preview; use the CLI for the full map.

## Routing audit

`abbey routes` / `/routes` print persona, role, model, **confidence**, stage,
**alternate**, and **fallback**. These fields are audit-only — Abbey does not
auto-run a second agent when confidence is low. Prefer `hybrid-loop` or `/gemma`
when the alternate is Gemma.

## Media, thinking, generation, and tools

Abbey does **not** embed pixels or ship local vision/generation weights. Attach and
generate both go through **cursor-agent** (and any MCP image/video tools it has):

```bash
# Attach (read)
abbey --image ./shot.png "what is wrong with this UI?"
abbey /image ./shot.png describe the layout

# Generate (write via agent tools)
abbey imagine "a fox reading in a candlelit library" --aspect 16:9 --out ./fox.png
abbey imagine --edit ./fox.png "make it dawn" --out ./fox-dawn.png
abbey generate video "short timelapse of a city skyline" --out ./sky.mp4
abbey /imagine --out=./mark.png abbey logo mark, simple

# Reason (Cursor thinking model + structured wrap)
abbey reason "should we split session.rs further?"
abbey --thinking xhigh "…"
abbey /reason compare sqlite vs wdbx for this workload

# MCP / ACP
abbey mcp                  # inventory ~/.cursor/mcp.json + project configs
abbey mcp paths
abbey mcp list             # cursor-agent's view / approval list
abbey --approve-mcps "…"   # tools run inside cursor-agent during a turn
abbey acp                  # ACP peers (gemini --acp, opencode acp, …)
abbey acp run gemini       # start ACP stdio server for an ACP host to attach

# Voice (macOS — Premium/Enhanced when downloaded)
abbey voice voices
abbey speak "build finished"                 # best installed voice
abbey voice speak -v "Zoe (Premium)" -o ./note.m4a "ship it"
abbey voice listen --seconds 5               # on-device Speech STT
abbey voice ask --seconds 6                  # listen → agent → speak

# Syntax highlight (auto on -p/print/commit fences when TTY)
abbey highlight src/main.rs
abbey highlight --lang rust -
abbey -p "show me a rust hello world"   # fenced blocks colourised
ABBEY_HIGHLIGHT=0 abbey -p "…"          # disable
```

- Generation fails honestly if the agent has no image/video tool — Abbey does not fake files.
- `/think` sets a Cursor `*-thinking-*` model id; `/reason` also applies a structured reasoning wrap.
- Voice prefers **Premium → Enhanced → standard** `say` voices; download them in
  System Settings → Accessibility → Spoken Content. STT builds `scripts/abbey-stt.swift`
  on first listen (Xcode CLT + Mic + Speech Recognition permission).
- Under `ABBEY_BACKEND=fm`, MCP/account/generation verbs refuse (exit 2).

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
recovery). `abbey wdbx` passthroughs aimed at Abbey's own store take the same lock, so the
bridge can't route around it. Caveats: the guard is **Unix-only**, and it does not extend to
an `abi` invoked directly by you against the same store.

## Honest limits

- Backend is **cursor-agent**, not a reimplementation of Grok/Codex/Claude runtimes.
- `abbey wdbx <query|db|…>` shells out to `abi`, which must be a **real binary** on PATH —
  a shell alias will not do (`cargo build -p abi-cli` in `../abi`, then set `ABBEY_ABI_BIN`).
  `stats`/`checkpoint` are answered in-process and need `--features wdbx`.
- The WDBX backend uses the KV space; vector/embedding search through WDBX is not wired yet.
- The WDBX cross-process lock uses `fs2` on Unix and Windows. GPU/NPU/TPU *host*
  detection is Current (`abbey platform`); accelerator runtimes inside Abbey are Out of scope.
- Under WDBX lock contention a run's background STM/activity write is dropped silently (it is
  best-effort by design). Explicit `abbey memory`/`abbey learn` writes surface the error.
- The TUI memory panel opens the store with a 250 ms timeout so a redraw never stalls; a
  locked store shows `unavailable: …` rather than reporting an empty store.
- Personas, Max/Gemma bindings, memory, hybrid-loop, fm, and the 3-D map are **Current** — do not assume local Gemma/Qwen weights or LoRA.
- `abbey learn review`/`stats` curate `train_candidate` provenance; they are not a trainer.
- `/cost` is **N/A** (use Cursor account dashboard).
- Full MCP/plugin UIs pass through to cursor-agent when available.
- Worktree isolation depends on cursor-agent `--worktree`.
- `/init` local scan covers Rust/Node/Zig/Go/Python/Swift/Make/CMake; `--agent` refines via cursor-agent.

## Layout

```
src/
  main.rs         thin entry (<200 lines)
  actions.rs      canonical RunSpec → run_agent
  cli.rs          clap surface
  slash.rs        slash catalog
  session.rs      hybrid_run + hybrid_loop_run
  roles.rs        route_decision (conf/alt/fb)
  route_log.rs    route.jsonl audit
  init/           /init project scan → AGENTS.md
  gitops.rs       local git helpers
  agent.rs        cursor · grok · fm executors
  memory/         trait + sqlite · wdbx (feature-gated) + map
  hybrid_loop.rs  Gemma interpret → Max implement
  wdbx_bridge.rs  abbey wdbx → abi wdbx
  please_fix.rs  last-failure prompt + capture summarizer
  learn.rs        self-learn + review/stats
  models.rs       model aliases
  state.rs        per-cwd chats
  tui/            7-tab ratatui app
tasks/            goals + todo board
docs/             identity · architecture · production
tests/
```
