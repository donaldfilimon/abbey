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
│  abi-ai (path) — Abbey/Aviva/Abi contracts + router     │
│  memory — SQLite interim (WDBX bridge Proposed)         │
│  agent — cursor-agent / grok executor                   │
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
| `persona` / `roles` / `route_log` | Hybrid routing spine |
| `memory` | Trait + SQLite store |
| `learn` | Self-learn capture/digest |
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
4. **Self-learn:** `train_candidate` requires provenance; no silent deletes in reflect.
5. **Tooling:** Rust nightly via `rust-toolchain.toml`; gate with `./check.sh`.

## Feature matrix (Current)

- Grok/Codex/Claude parity CLI + slash
- Personas via abi-ai; Max/Gemma roles; route JSONL
- SQLite memory + learn pipeline
- Parallel lanes; OS allowlist; skills/plugins inventory
- Unique `ABBEY_BUILD_STAMP`; 7-tab TUI
- `/init` project scan → AGENTS.md

## Proposed (not Current)

- In-process `abi-wdbx` DurableStore feature
- Local Qwen 3.5 / Gemma 4 weights
- LoRA / fine-tuning runners
- Dedicated distributed agents
