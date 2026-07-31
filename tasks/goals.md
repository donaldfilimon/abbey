# Goals

## Production structure + spec
status: done
- Decomposed `main.rs` (1074→86) into session/commands/slash_dispatch/doctor/prompts
- Spec expanded: docs/architecture.md, docs/production.md, identity claims Current
- `./check.sh` production gate (fmt + clippy -D + test + file-size)
- v2.6.0 modular layout under 1k-line rule

## Hybrid routing and memory
status: done
- Personas, Max/Gemma, SQLite learn, TUI 7 tabs, OS control, parallel, inventory, build stamp
- Re-opened 2026-07-30: was marked `done` while `tasks/todo.md` Phase 3 (WDBX bridge) and
  Phase 4 (Gemma→Max hybrid loop) were still unchecked — named subsystems, not cosmetic leftovers
- Phase 3 closed: in-process `WdbxMemory` over `abi-wdbx` `DurableStore` behind the
  off-by-default `wdbx` feature; `memory::open_backend` dispatches every call site;
  `check.sh` now gates both feature sets. Verified live: put/search/get/promote/export
  across separate processes, WAL recovery on reopen, `wdbx stats`/`checkpoint`
- Phase 3 hardening: concurrent `abbey` processes corrupted the WDBX WAL beyond recovery
  (20 writers → store permanently unreadable). Fixed with `flock(2)` per store; 40/40
  concurrent writes now land. Found by testing the case rather than assuming it
- Phase 3 bridge verified live against a locally built `abi-cli` (`cargo build -p abi-cli`
  in `../abi`), including a cross-check that `abi wdbx query` reads the same records the
  in-process backend wrote. Also fixed a path-convention trap: `abi` takes base paths,
  Abbey opens a directory
- Phase 4 closed: `abbey hybrid-loop` (Gemma interpret → Max implement) links both stages
  by correlation id; `abbey routes --correlation <id>` reads them back. Verified live
- Strategy brief re-captured 2026-07-30 as **Hybrid model-routing architecture strategy**
  for Proposed residuals (richer routing, semantic memory, train curation) — not a reopen

## On-device backend, spatial memory, and capability reporting
status: done
- `ABBEY_BACKEND=fm` on-device executor (no cursor-agent/network); argv isolation + account
  verb refusals verified
- Interpretable 3-D memory map (topic × recency × consolidation) on both backends;
  TUI Memory tab shows the map preview; `nearest_to` shared (no dead spatial dual-write)
- Deferred by construction remain Out of scope / Proposed: multi-node training, NPU/TPU
  learning, autonomous OS, semantic embedder

## Broader polish
status: done

**Review 2026-07-30:** `./check.sh` was **not runnable in a default shell** — Homebrew's
rustc/cargo 1.97.1 shadows rustup's nightly 1.99.0 on PATH (no shims in `~/.cargo/bin`),
and the sibling `../abi` crates now require `rust-version = 1.99`. The code was fine:
the full gate passes under nightly 1.99 (101+7 and 107+7 tests). Added a toolchain
guard to `check.sh` that turns cargo's bare "not supported by the following packages"
into the actual remedy (`brew unlink rust` / `rustup default nightly`), verified in
both the broken and working configurations.

## Hybrid model-routing architecture strategy
status: done

Captured 2026-07-30 from the Hybrid Model Routing and Memory Architecture Strategy
brief. Coarse intention: formalize model specialization as a first-class principle
(Abi routes/moderates, Abbey integrates, Aviva as precise expert mode; Max =
changeable technical worker role; Gemma = visual/conversational), keep memory
backend-replaceable with provenance, delay LoRA/fine-tuning until curated data
exists, treat activity+corrections as future training substrate.

**Closed as Current (buildable slices):** richer `route_decision` confidence /
alternate / fallback on the route log (audit only — no second execution path);
`abbey learn review` + `stats` for train_candidate provenance curation; please-fix
capture summarizer + argv clamp. Already-shipped Max/Gemma/personas/memory/hybrid-loop/
fm/3-D map remain as before.

**Residuals stay Proposed / Out of scope (not laundered):** semantic embedding
space; multi-node / NPU-TPU learning; autonomous OS; local weights / LoRA;
CouchDB/Python second memory stack. Claims gate in AGENTS.md stays binding.

**Review 2026-07-30 (`/goal /review`) — gap found, recorded not built:** the brief's
memory interface names *filter by source*, *by timestamp*, and *by project/domain*
(plus recency weighting and confidence thresholds in the retrieval layer).
`MemoryStore::filter` only takes `(retention, tag, limit)`. Unlike semantic search
these need no embedder, so they were neither shipped nor listed as residual — an
honesty hole in this goal's close. Now **Proposed** in the claims gate. Goal stays
`done`: this is a named future slice, not a re-opening.

**Polish (2026-07-30):** CLI/slash route line parity via `format_route_line`;
hybrid-loop stages record paired alt/fb; learn routes keep routing fields;
docs/doctor/TUI honesty; please-fix keeps cargo/rustc signal.

## Media, thinking, and tools surface
status: done

Path-attach media (`--image`/`--video`/`/image`/`/video`), thinking aliases
(`--thinking`/`/think` → Cursor model ids), and MCP/tool passthrough
(`abbey mcp`, `--approve-mcps`). No local vision weights; no Abbey-owned CoT UI;
tools remain cursor-agent's during a run.

## Image/video generation and reasoning
status: done

Agent-orchestrated `imagine` / `generate video` (cursor-agent or MCP tools write
files; Abbey has no local gen model) and `reason` (thinking model + structured
wrap). Video is best-effort and fails honestly without a video tool. `fm`
refuses generation.

## High-quality voice input/output
status: done

macOS `say` TTS with automatic Premium/Enhanced preference, on-device Apple
Speech STT (`abbey-stt`), and `voice ask` (listen → agent → speak). Cloud TTS/STT
SaaS and in-process neural voice weights stay Out of scope — download Apple
Premium voices in System Settings for super-high quality.

## MCP and ACP server surfaces
status: done

Config inventory for MCP (`abbey mcp status` over standard mcp.json paths) plus
cursor-agent management passthrough; ACP peer discovery/launch for gemini and
opencode. Abbey remains a client-side CLI — not an MCP host or ACP host. Tool
execution during runs stays inside cursor-agent (`--approve-mcps`).

## Auto code highlighting
status: done

syntect ANSI colour for markdown fences on captured `-p`/print/commit/hybrid-loop
output when stdout is a TTY; `abbey highlight` / `/highlight` for files and stdin;
`abbey diff` highlighted. Off via `NO_COLOR` or `ABBEY_HIGHLIGHT=0`. Not a full
markdown renderer or LSP semantic highlight.

## Multi-subagent and local distributed peers
status: done

`abbey subagents` catalog + `run --lanes/--peers/--jobs/--synthesize`. Abbey lanes
(max/gemma/aviva/abbey/abi/reviewer/security/planner) via cursor-agent; peers
(gemini/opencode/claude/codex) as same-host PATH CLIs. `parallel` remains the
default-lane alias. Multi-node mesh stays Proposed.

## Proposed / OOS claims surface
status: done

`abbey claims` (+ `roadmap`/`scope`) prints the Current / Proposed / Out of scope
gate from `src/claims.rs`; `refuse embeddings|lora|multinode|…` and
`memory embed` / `learn lora` exit 2 with substitutes. No claim laundering —
embeddings and multi-node stay Proposed; LoRA/weights/accelerator-runtime stay OOS.

## Platform targets + compute inventory
status: done

`abbey platform` / `compute`: linux/macos/windows primary-target matrix for portable
surfaces; `available_parallelism` thread budget driving subagent `--jobs`; GPU/NPU/TPU
host detect (report-only). WDBX lock via `fs2` on Unix and Windows. Voice/fm remain
macOS-only; Abbey still does not run GPU/NPU/TPU kernels.

## Vision / CoT / tool-runtime honesty
status: done

`abbey vision` · `abbey cot` · `abbey runtime` surface Current substitutes (path attach,
agent gen, reason transcript viewer, responsibility matrix) while refusing local
vision/video weights, Abbey-owned CoT engine/UI, and Abbey-as-tool-runtime (exit 2).
`abbey reason` saves a CoT transcript for `cot show`.

## OOS honesty pack (LoRA · weights · NPU/TPU · unrestricted OS · MCP/ACP host)
status: done

`abbey oos` index plus `abbey lora|weights|accel|shell|host` (and slash peers) print
Current substitutes and refuse with exit 2. Does **not** implement LoRA runners, local
weights, accelerator kernels, unrestricted shell, or Abbey-as-MCP/ACP-host — those stay
Out of scope. Cross-links: `learn lora`, `os refuse`, `platform refuse`, `mcp|acp refuse`.

## learn review/stats + OS allowlist Current surfaces
status: done

Hardened Current substitutes the OOS pack points at: `learn` status embeds train_candidate
curation counts; `review`/`stats` show provenance + ready (prov+conf≥0.9); top-level
`learn-review`/`learn-stats` aliases. `abbey os` / `allowlist` print the policy panel by
default; execute still requires `--confirm`; off-list denied. Unrestricted shell stays OOS.

## Cross-platform host support
status: done

PATHEXT-aware `which_bin`, Windows agent/install candidate paths, platform-aware argv
clamp (CreateProcess-safe), Windows OS allowlist of real executables, `abbey platform
paths` + this-host matrix column, `install.ps1`, softer `check.sh` cross-target smoke when
targets are already installed. Voice/fm remain macOS-only; accelerator runtimes stay OOS.
