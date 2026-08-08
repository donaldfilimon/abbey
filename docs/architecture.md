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
│  mesh — typed ABI authenticated local-process proof     │
├─────────────────────────────────────────────────────────┤
│  abi-ai (path) — Abbey/Aviva/Abi contracts + router     │
│  memory — SQLite (default) · WDBX (feature `wdbx`)      │
│  agent — cursor-agent · grok · fm (on-device) · abi (`abi complete`) executors │
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
| `memory` | Records/filters plus explicit learned providers and space-isolated semantic index; SQLite default, WDBX under `--features wdbx` |
| `mesh` | Typed, bounded bridge to ABI's authenticated 3–9-process loopback proof; not production multi-host |
| `hybrid_loop` | Two-stage Gemma→Max run, correlated in the route log |
| `wdbx_bridge` | `abbey wdbx` — `abi wdbx` passthrough + in-process `stats`/`checkpoint` |
| `please_fix` | Last-failure prompt; capture summarizer (argv-safe) |
| `media` | Image/video path attach → `--add-dir` + prompt note (no local vision) |
| `generate` | Imagine/video gen prompts + structured `reason` (via cursor-agent tools) |
| `voice` | macOS Premium/Enhanced TTS (`say`) + on-device STT (`scripts/abbey-stt.swift`) |
| `protocols` | Provider-aware, secret-redacted MCP config inventory + ACP peer discovery/launch (not a host runtime) |
| `highlight` | syntect fence/file ANSI colour for `-p`/print/commit/diff (TTY; not a markdown UI) |
| `claims` | Current / Proposed / Out of scope gate + honest refuse paths |
| `platform` | Host OS/arch matrix, thread budget, GPU/NPU/TPU detect (report-only) |
| `surfaces` | Vision/CoT/runtime honesty — viewer + matrix; weights/engine/host stay OOS |
| `deferred` | OOS honesty pack — `oos`/`lora`/`weights`/`accel`/`shell`/`host` (refuse, no impl) |
| `host` | portable `which_bin` (PATHEXT), argv budgets, install/state path report |
| `learn` | Self-learn capture/digest/review/stats |
| `os_control` | Cross-platform OS policy |
| `parallel` | Thin alias → `subagents` (default Max/Gemma/Aviva) |
| `subagents` | Named multi-subagent fan-out + local PATH peer CLIs + synthesize |
| `improve` | Goal ledger + bounded diagnose/implement loop; `./check.sh` hard bar |
| `inventory` | Provenance-aware skills and provider-explicit plugins/peer tools |
| `config` / `build_info` | Config + unique build stamp |
| `tui/` | tabs + app + ui (7-tab ratatui) |
| `init/` | probe + detect + `run_init` → AGENTS.md |
| `gitops` / `agent` / `models` / `state` | Existing surfaces |

## Production rules

1. **File size:** keep modules under ~800 lines; `main` under 200. (code-review 1k rule)
2. **Honesty:** Max/Gemma are role→model bindings, not local weights. `/cost` is N/A.
3. **OS control:** never execute without `--confirm`; whitelist only.
3b. **WDBX cross-process:** `WdbxMemory` must hold its `fs4` advisory lock for its
   whole life — `DurableStore` has no cross-process locking, and concurrent WAL
   appends corrupt the store irrecoverably.
4. **Self-learn:** `train_candidate` requires provenance; no silent deletes in reflect.
5. **Tooling:** Rust nightly via `rust-toolchain.toml`; gate with `./check.sh`.

## Feature matrix (Current)

- Grok/Codex/Claude parity CLI + slash
- Personas via abi-ai; Max/Gemma roles; route JSONL with confidence/alternate/fallback
- SQLite memory + learn pipeline (`review`/`stats` for train_candidate provenance)
- Parallel / multi-subagent lanes; local peer CLIs; OS allowlist; skills/plugins inventory
- Unique `ABBEY_BUILD_STAMP`; 7-tab TUI (Memory tab previews the 3-D map)
- `/init` project scan → AGENTS.md
- In-process `abi-wdbx` `DurableStore` memory backend — **behind `--features wdbx`, off by
  default**; `check.sh` gates both feature sets
- `abbey hybrid-loop` two-stage Gemma→Max run with correlated route records
- On-device execution via `ABBEY_BACKEND=fm` (Apple Foundation Models CLI, macOS 26+):
  own argv grammar, chat id → transcript-file mapping, honest refusal of account verbs;
  local MCP inventory remains backend-independent
- Abi-backend execution via `ABBEY_BACKEND=abi` (`abi complete`): own argv grammar with
  a real `--` separator; deterministic persona-template locally by default; bare
  `claude-*` / `live`/`anthropic` opt into abi's Anthropic transport; cursor
  role/thinking bindings stay local (no silent `--live`); account/gen refuse while local
  MCP inventory remains available;
  needs a real `abi` binary (`ABBEY_ABI_BIN` / `abi_bin`). Abbey-side continuity:
  turns append to `<state>/abi/<chat>.transcript` and a bounded (8 KiB) tail rides
  into the next turn as a context prefix — abi itself stays a stateless one-shot.
  Persistent default via config `backend = "cursor"|"grok"|"fm"|"abi"`
  (`ABBEY_BACKEND` env wins); in-TUI Ctrl-B cycles resolvable backends
- 3-D memory map (`abbey memory map` / `near`) on both memory backends; coordinates are
  computed from KV-backed records and are not spatially dual-written
- Exact project/source/reference/tag and inclusive RFC 3339 filtering across search,
  similar, semantic, map, near, and export surfaces
- Opt-in learned semantic memory (`none|apple|openai`): summary+subject-tags privacy
  boundary, stable provider/model/revision/dimension/normalization spaces, SQLite vector
  table, and WDBX v2 sub-store per space; no cross-provider fallback
- `abbey mesh local-demo --nodes 3..9`: typed authenticated local ABI process proof with
  quorum/conflict/read-repair/child-teardown evidence; explicitly not production multi-host
- Prompt argv clamp + please-fix capture summarizer (avoids E2BIG on cursor-agent)
- Media path attach (`--image`/`--video`/`/image`/`/video`) + thinking aliases
  (`--thinking`/`/think`) + `--approve-mcps` tool passthrough
- Agent-orchestrated generation: `abbey imagine` / `generate video` / `reason`
  (no local image/video/LoRA weights)
- High-quality macOS voice I/O: `abbey voice speak|listen|ask` (Premium/Enhanced
  `say` voices + on-device Speech STT; not a cloud TTS subscription)
- Auto code highlighting: syntect ANSI on markdown fences for `-p`/print/commit/
  hybrid-loop/parallel when stdout is a TTY; `abbey highlight` for files/stdin
- Multi-subagent / local peers: `abbey subagents run --lanes max,reviewer
  --peers gemini --synthesize` (same-host PATH CLIs; not multi-node)
- Goal-driven improve: `abbey improve status|plan|run --confirm` — ledger pick from
  `tasks/goals.md` + `todo.md`, local diagnose/implement lanes, hard bar `./check.sh`;
  does not auto-mark goals done; not a multi-node mesh
- Claims gate CLI: `abbey claims` / `refuse` for Proposed (production multi-host)
  and Out of scope (LoRA, local weights, GPU/NPU/TPU runtime, …)
- Platform inventory: `abbey platform` / `compute` — linux/macos/windows matrix,
  threads, host GPU/NPU/TPU detect (not Abbey accelerator kernels)
- MCP/ACP surfaces: `abbey mcp status|paths|view` reads provider-aware JSON/TOML config;
  management names `cursor|codex|claude` explicitly; `abbey acp list|run` discovers/
  launches Gemini and OpenCode ACP peers. Abbey is not an MCP or ACP host.

## Proposed (not Current)

See also `abbey claims proposed` (machine-readable gate in `src/claims.rs`).

- Production multi-host / multi-GPU / shared-compute agent mesh. The Current ABI bridge
  proves authenticated independent processes on loopback only; it supplies no separate-host
  deployment, shared accelerator scheduling, or production operations evidence.

## Out of scope

See `abbey claims oos`. Includes: LoRA / fine-tuning runners; local Qwen/Gemma weights;
Abbey-own trained weights; GPU/NPU/TPU compilation·training·inference *inside* Abbey;
autonomous unrestricted OS; Abbey as MCP/ACP host; fake cost accounting; cloud TTS/STT
SaaS; local vision/gen weights; reimplementing Grok/Codex/Claude runtimes.
