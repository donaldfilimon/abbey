# Abbey architecture (production)
<!-- abbey-claims-sha256: 6451afc47d15af34424f5885e18a540bb2d317fba24d1f6323d6fcac4831d485 -->

Status key: **Current** = shipped · **Partial** = some shipped surface with stated gaps ·
**Proposed** = approved direction, not claimed live · **Blocked** = waiting on an external
prerequisite or proof · **Out of scope** = explicitly excluded.

## Layers

```text
┌─────────────────────────────────────────────────────────┐
│  CLI (clap) · TUI (ratatui) · slash                    │
├─────────────────────────────────────────────────────────┤
│  app_core — versioned Status/Claims contracts + policy  │
│  abbey daemon client ↔ authenticated Unix socket ↔ abbeyd│
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
| `main` / `entry` | Pre-Clap SIGPIPE shim + library-owned CLI/TUI/slash routing |
| `lib` / `app_core` | Private implementation graph plus public versioned Status/Claims contracts and standard-edition policy |
| `daemon` / `bin/abbeyd` | Authenticated client/server for a 64 KiB length-prefixed read-only Unix socket; no model/tool/memory/job ownership; non-Unix fails closed |
| `actions` | `RunSpec` + `run_agent` / review / commit / pr (one path) |
| `commands` | Clap subcommand match → actions |
| `slash` / `slash_dispatch` | Catalog + shared slash handler → actions |
| `session` | Global flags, `hybrid_run`, history compact |
| `doctor` | Doctor/debug/persona/role/memory/init helpers |
| `prompts` | Review/commit prompt builders |
| `persona` / `roles` / `route_log` | Hybrid routing spine (`route_decision` → confidence/alt/fb on JSONL) |
| `memory` | Records/filters plus explicit learned providers and space-isolated semantic index; SQLite default, WDBX under `--features wdbx` |
| `mesh` | Typed, bounded Unix bridge to ABI's authenticated 3–9-process loopback proof; non-Unix fails before spawn; not production multi-VM |
| `hybrid_loop` | Two-stage Gemma→Max run, correlated in the route log |
| `wdbx_bridge` | `abbey wdbx` — `abi wdbx` passthrough + in-process `stats`/`checkpoint` |
| `please_fix` | Last-failure prompt; capture summarizer (argv-safe) |
| `media` | Image/video path attach → `--add-dir` + prompt note (no local vision) |
| `generate` | Imagine/video gen prompts + structured `reason` (via cursor-agent tools) |
| `voice` | macOS Premium/Enhanced TTS (`say`) + on-device STT (`scripts/abbey-stt.swift`) |
| `protocols` | Provider-aware, secret-redacted MCP config inventory + ACP peer discovery/launch (not a host runtime) |
| `highlight` | syntect fence/file ANSI colour for `-p`/print/commit/diff (TTY; not a markdown UI) |
| `claims` | Current / Partial / Proposed / Blocked / Out of scope gate + honest refuse paths |
| `platform` | Host OS/arch matrix, thread budget, GPU/NPU/TPU detect (report-only) |
| `surfaces` | Vision/CoT/runtime honesty — neural media/owned host Proposed; hidden CoT OOS |
| `deferred` | Unavailable-capability honesty pack — Proposed items plus shipped-edition shell bypass OOS |
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

- Selected Grok/Codex/Claude-compatible CLI + slash surfaces (**Partial** parity)
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
- `abbey mesh local-demo --nodes 3..9`: typed authenticated Unix-local ABI process proof
  with quorum/conflict/read-repair/process-group teardown evidence; non-Unix fails before
  spawn; explicitly not production multi-VM
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
- Claims gate CLI: `abbey claims` / `refuse` for Partial, Proposed, Blocked, and
  Out-of-scope surfaces. Approved roadmap verbs still return exit 2 until proven.
- Shared application library plus `abbeyd`: exact versioned Status/Claims events
  over an owner-only Unix socket with bounded frames/timeouts and bearer-file
  authentication. `abbey daemon status|claims` consumes those same typed events
  without resolving a model executor and never falls back after auth/connect failure.
  This is a read-only control-plane foundation, not an owned
  agent/tool runtime, durable job manager, MCP host, or Windows named-pipe proof.
- Platform inventory: `abbey platform` / `compute` — linux/macos/windows matrix,
  threads, host GPU/NPU/TPU detect (not Abbey accelerator kernels)
- MCP/ACP surfaces: `abbey mcp status|paths|view` reads provider-aware JSON/TOML config;
  management names `cursor|codex|claude` explicitly; `abbey acp list|run` discovers/
  launches Gemini and OpenCode ACP peers. An Abbey-owned provider-neutral host is Proposed.

## Proposed (not Current)

See also `abbey claims proposed` (machine-readable gate in `src/claims.rs`).

- Tauri 2 + React/TypeScript desktop GUI, layered over the existing application/session state.
- Provider-neutral Abbey-owned agent and tool runtime, including an MCP/ACP host boundary.
- Production-capable local model weights plus an explicit fine-tuning/LoRA pipeline.
- GPU/NPU/TPU compilation, training, and inference runtime owned by Abbey.
- Local neural speech, image, and video models; current platform/delegated media remains distinct.
- A separately packaged personal-unrestricted edition with isolation, auditable consent, and no
  weakening of the shipped edition's allowlist + `--confirm` invariant.
- An authenticated local three-VM shared-compute proof on one Mac. The Current ABI bridge
  proves independent loopback processes only, not VM networking or shared compute.
- Production separate-physical-host / geographic-HA / multi-GPU operation remains a later
  Proposed stage even after the local three-VM proof succeeds.

These are architecture directions, not implementation claims. Their refusal paths remain exit 2.

## Blocked proof

- GitHub-hosted runs remain `startup_failure` with zero jobs scheduled. The ABI dependency
  publication blocker is resolved by merged ABI
  `32e372d7f522f5a6c9c0ef92c5b9612b52cfea05`. A macOS ARM64 self-hosted runner is
  registered; Linux ARM64 is not provisioned, so its job remains gated by an explicit
  repository variable. Linux and Windows runtime proof remain open.

## Out of scope

See `abbey claims oos`. Includes: reimplementing Grok/Codex/Claude runtimes; fake cost
accounting; bundled cloud TTS/STT SaaS; Abbey-owned hidden chain-of-thought engine/UI;
and unrestricted shell/allowlist bypass in the shipped edition. The separately packaged
personal-unrestricted edition is Proposed, not a waiver of current safety controls.
