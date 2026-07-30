# Abbey architecture (production)

Status key: **Current** = shipped · **Proposed** = designed, not claimed live · **Out of scope** = explicitly deferred.

## Layers

```text
┌─────────────────────────────────────────────────────────┐
│  CLI (clap)  ·  TUI (ratatui)  ·  slash catalog         │
├─────────────────────────────────────────────────────────┤
│  actions::run_agent — canonical RunSpec for CLI/slash/TUI │
│  session::hybrid_run — persona + role + prefs + route   │
│  parallel — Max/Gemma/Aviva lanes                       │
│  os_control — allowlist dry-run / execute --confirm     │
│  learn — correction / preference / digest → SQLite      │
│  inventory — skills / plugins / peer agent tools        │
├─────────────────────────────────────────────────────────┤
│  hybrid_loop — Gemma interpret → Max implement (linked) │
│  wdbx_bridge — `abbey wdbx` → `abi wdbx` passthrough    │
├─────────────────────────────────────────────────────────┤
│  abi-ai (path) — Abbey/Aviva/Abi contracts + router     │
│  memory — SQLite (default) · WDBX (feature `wdbx`)      │
│  agent — cursor-agent · grok · fm (on-device) executors │
└─────────────────────────────────────────────────────────┘
```

## Module map (`src/`)

| Module | Responsibility |
|--------|----------------|
| `main` | Entry + early routing (TUI / slash / continue) |
| `actions` | `RunSpec` + `run_agent` / review / commit / pr (one path) |
| `commands` | Clap subcommand match → actions |
| `slash` / `slash_dispatch` | Catalog + shared slash handler → actions |
| `session` | Global flags, `hybrid_run`, history compact |
| `doctor` | Doctor/debug/persona/role/memory/init helpers |
| `prompts` | Review/commit prompt builders |
| `persona` / `roles` / `route_log` | Hybrid routing spine (`route_decision` → confidence/alt/fb on JSONL) |
| `memory` | Trait + backend dispatch (`open_backend` → `Box<dyn MemoryStore>`); SQLite default, WDBX under `--features wdbx` |
| `hybrid_loop` | Two-stage Gemma→Max run, correlated in the route log |
| `wdbx_bridge` | `abbey wdbx` — `abi wdbx` passthrough + in-process `stats`/`checkpoint` |
| `please_fix` | Last-failure prompt; capture summarizer (argv-safe) |
| `media` | Image/video path attach → `--add-dir` + prompt note (no local vision) |
| `generate` | Imagine/video gen prompts + structured `reason` (via cursor-agent tools) |
| `learn` | Self-learn capture/digest/review/stats |
| `os_control` | Cross-platform OS policy |
| `parallel` | Multi-lane fan-out |
| `inventory` | Skills/plugins/peer tools |
| `config` / `build_info` | Config + unique build stamp |
| `tui/` | tabs + app + ui (7-tab ratatui) |
| `init/` | probe + detect + `run_init` → AGENTS.md |
| `gitops` / `agent` / `models` / `state` | Existing surfaces |

## Production rules

1. **File size:** keep modules under ~800 lines; `main` under 200. (code-review 1k rule)
2. **Honesty:** Max/Gemma are role→model bindings, not local weights. `/cost` is N/A.
3. **OS control:** never execute without `--confirm`; whitelist only.
3b. **WDBX cross-process:** `WdbxMemory` must hold `flock(2)` for its whole life —
   `DurableStore` has no cross-process locking, and concurrent WAL appends corrupt the
   store irrecoverably.
4. **Self-learn:** `train_candidate` requires provenance; no silent deletes in reflect.
5. **Tooling:** Rust nightly via `rust-toolchain.toml`; gate with `./check.sh`.

## Feature matrix (Current)

- Grok/Codex/Claude parity CLI + slash
- Personas via abi-ai; Max/Gemma roles; route JSONL with confidence/alternate/fallback
- SQLite memory + learn pipeline (`review`/`stats` for train_candidate provenance)
- Parallel lanes; OS allowlist; skills/plugins inventory
- Unique `ABBEY_BUILD_STAMP`; 7-tab TUI (Memory tab previews the 3-D map)
- `/init` project scan → AGENTS.md
- In-process `abi-wdbx` `DurableStore` memory backend — **behind `--features wdbx`, off by
  default**; `check.sh` gates both feature sets
- `abbey hybrid-loop` two-stage Gemma→Max run with correlated route records
- On-device execution via `ABBEY_BACKEND=fm` (Apple Foundation Models CLI, macOS 26+):
  own argv grammar, chat id → transcript-file mapping, honest refusal of account verbs
- 3-D memory map (`abbey memory map` / `near`) on both memory backends; mirrored into
  WDBX `SpatialRecord`s under `--features wdbx`
- Prompt argv clamp + please-fix capture summarizer (avoids E2BIG on cursor-agent)
- Media path attach (`--image`/`--video`/`/image`/`/video`) + thinking aliases
  (`--thinking`/`/think`) + `--approve-mcps` tool passthrough
- Agent-orchestrated generation: `abbey imagine` / `generate video` / `reason`
  (no local image/video/LoRA weights)

## Proposed (not Current)

- Vector/embedding search through WDBX (`put_vector`/`search`) — the backend currently
  uses the KV space only
- A *learned* memory embedding space. The 3-D map's axes are deterministic
  (topic × recency × consolidation); Abbey has no embedder
- Multi-node / multi-GPU / shared compute between nodes. `abi-wdbx` ships
  `cluster`/`remote_compute`/`compute` reference implementations Abbey does not yet use
- Windows cross-process safety for the WDBX backend: the `flock(2)` guard is `#[cfg(unix)]`,
  so concurrent processes are unprotected on non-Unix targets
- Local Qwen 3.5 / Gemma 4 weights
- LoRA / fine-tuning runners
- Dedicated distributed agents
