# Todo

## Hybrid routing and memory

### Phase 1 — Spec spine
- [x] Path-dep `abi-ai` in Cargo.toml; `persona.rs`, `roles.rs`, `route_log.rs`
- [x] Max/Gemma role heuristics + config rebinds to cursor-agent model ids
- [x] Prompt assembly: persona prefix/suffix + role note via `actions` → `hybrid_run`
- [x] CLI/slash: `/persona`, `/role`; unit tests (no live agent)
- [x] `docs/identity.md`; AGENTS.md Hybrid section + claims table

### Phase 2 — Memory abstraction
- [x] `memory/` trait: store, get, search, provenance, promote, supersede
- [x] `SqliteMemory` under XDG (`ABBEY_STATE_DIR/memory.sqlite`)
- [x] Layers: stm, ltm, activity, train_candidate (gated writes)
- [x] Wire `/memory`, `abbey memory {put,get,search,promote}`; session STM capture
- [x] `abbey memory reflect` / learn digest (duplicates / low-confidence listing)

### Phase 3 — WDBX bridge
- [x] Feature `wdbx`: optional path-dep `abi-wdbx`; `WdbxMemory` implements the full
      `MemoryStore` trait over `DurableStore` KV (records as JSON under `mem/<id>`)
- [x] Backend dispatch: `memory::open_backend` → `Box<dyn MemoryStore>`; all call sites
      (session/learn/doctor/tui) route through config `memory_backend`
- [x] Fallback: `abbey wdbx <query|db|…>` subprocess to `abi wdbx …` (`query` gains `--json`
      and defaults to Abbey's own store); `stats`/`checkpoint` answered in-process.
      Verified live against a locally built `abi-cli`: `query`, `db verify`, `compute info`,
      exit-code propagation, and a cross-check that `abi` reads the same 40 records the
      in-process backend wrote
- [x] Cross-process safety extends to the bridge: an `abbey wdbx` passthrough aimed at Abbey's
      own store takes the same lock, so Abbey's own CLI can't route around its guard. TUI
      redraw opens with a 250 ms timeout and reports `unavailable:` instead of "empty"
- [x] Cross-process safety: `flock(2)` on `<store>/abbey.lock` held for the handle's life.
      Without it 20 concurrent `abbey` processes interleaved WAL appends and left the store
      **permanently unreadable** (CRC mismatch, every later open failed); with it 40/40
      concurrent writes land. SQLite passed the same test unaided — the lock is what makes
      the backends equivalent
- [x] Path convention: `abi` takes BASE paths (dir + base name), Abbey opens a directory —
      `<state>/wdbx/` is `<state>/wdbx/wdbx` to `abi`. The bridge translates so a bare
      directory can't silently read one level up and report an empty store
- [x] Doctor lines for backend path/status + whether the feature is linked; no SEA/learn
      duplication in Abbey
- [x] `check.sh` extended with `--features wdbx` clippy + test (the default run never
      compiles the gated code, so it could otherwise rot silently)

### Phase 4 — TUI + operational loop
- [x] TUI tabs: Personas, Memory, Skills (+ Home/Chats/Models/Doctor)
- [x] Doctor: persona prior, role bindings (Max→id, Gemma→id), memory backend
- [x] Hybrid visual+code loop: `abbey hybrid-loop` runs Gemma interpret → Max implement;
      both stages carry a shared `correlation` id in the route log, queryable via
      `abbey routes --correlation <id>` (unit-tested linkage; verified live end-to-end)

### Phase 5 — Config and migration
- [x] `~/.config/abbey/config.toml`: role→model, default persona, memory backend
- [x] Env overrides: `ABBEY_ROLE`, `ABBEY_PERSONA`, `ABBEY_MEMORY_BACKEND`
- [x] `abbey learn export` / train_candidate JSONL (provenance required)

## Broader polish
- [x] rustfmt all sources
- [x] init.rs needless raw-string hashes
- [x] agent: env_flag arms, if-let chat, Worktree enum
- [x] shared which_bin; use output::print
- [x] gitops unit tests (truncate_diff)
- [x] MIT LICENSE
- [x] mark goal done after verify
- [x] code-review --fix: `actions` RunSpec + `init/` split + TUI tabs extract

## On-device backend, spatial memory, and capability reporting

### Slice 1 — on-device `fm` backend (done)
- [x] `AgentBackend::Fm` → `/usr/bin/fm`; `ABBEY_BACKEND=fm` runs generation on-device
      with no cursor-agent and no network
- [x] Separate `fm` argv grammar; test proves no cursor flag (`--force`, `--sandbox`, …)
      can leak into it
- [x] chat id → `<state>/fm/<id>.transcript`; `create_chat` mints a local uuid since `fm`
      has no server. Verified: a second process recalled a fact from the transcript, and a
      control run with a fresh state dir did not
- [x] `hybrid_run` no longer injects role→model ids under `fm` (its vocabulary is
      `system|pcc`). **Consequence: under `fm` the Max/Gemma split is prompt-only**
- [x] ask/plan → `--instructions`; `fm_availability` parses de-coloured stdout because
      `fm available` exits non-zero even when the on-device model works
- [x] Interactive (non-`-p`) path verified: `abbey --no-tui "…"` exits 0 and leaves exactly
      one transcript — no chat burned per invocation
- [x] Account/session verbs (`status`/`ls`/`login`/`logout`/`mcp`) refuse honestly with
      exit 2 instead of forwarding to `fm` and printing an opaque usage error *with exit 0*;
      `models` lists `system`/`pcc` locally
- [x] `run_resilient` no longer retries-with-a-new-chat under `fm`: that retry exists for
      stale *server* sessions, and `fm` has no server — a non-zero exit is a real failure

### Slice 2 — WDBX as a 3-D memory map (done)
- [x] `coordinates()` places each memory at topic × recency × consolidation; pure and
      covered by the default gate (`src/memory/map.rs`)
- [x] `abbey memory map` and `abbey memory near <id>` on **both** backends via
      `memory::nearest_to` (shared math; no dead spatial dual-write)
- [x] `abbey memory put --tag <subject>` — without it every record collapsed into one
      column, which made the topic axis useless. Untagged records now sit in a visible
      `(untagged)` column rather than being hash-scattered into fake topics
- [x] Recency axis tested with controlled timestamps (it was degenerate at 0.00 in every
      hand test because the records were seconds old), plus a drift case that must not panic
- [x] Axes are deterministic, **not** a learned embedding space — Abbey has no embedder.
      Semantic placement stays Proposed (see deferred section)

### Slice 3 — TUI + argv safety (done)
- [x] Memory tab shows a 3-D map preview (topic × recency × depth)
- [x] Clamp prompt argv / please-fix captures to avoid `Argument list too long (os error 7)`
      when handing off to cursor-agent (96KiB ceiling + clearer E2BIG error)
- [x] please-fix capture summarizer: strip agent/TUI chrome, keep error signal, 24KiB body

### Deferred by construction (Proposed / Out of scope — not Abbey Current)

Closed work that *came out of* this section:
- [x] Surface via `abbey claims` / `refuse` (2026-07-30) — still not implemented as features
- [x] *Lexical* similarity search behind `MemoryStore` — **Current** (`abbey memory similar`,
      `src/memory/similarity.rs`). `abi-ai` n-gram feature hash + cosine, computed per query
      on both backends. Verified live: `memory search "chekpoint"` (typo) returns nothing
      while `memory similar "chekpoint"` ranks the checkpoint record first at 0.44
- [x] WDBX cross-process lock on Windows + Unix (`fs2`) — **Current**
- [x] Host GPU/NPU/TPU detection + thread matrix (`abbey platform`) — **Current**

**Boundaries — deliberately not built. Not checkboxes: these are decisions, not
pending work.** Re-opening one means changing the claims gate in `AGENTS.md` first,
not ticking a box here. `abbey improve` skips this whole section when picking work
(`ledger::is_deferred_section`) — an unchecked box here used to be nominated as the
next slice, which would have sent Max to build a capability `claims refuse` exits 2 for.

| Capability | Status | Why it stays closed |
|---|---|---|
| Semantic memory search (**learned** embedding space) | **Proposed** | The lexical slice does *not* close this — a feature hash has no trained semantics, so the same idea in different words still misses. `claims refuse embeddings` exits 2; abi-wdbx `put_vector` stays unwired (nothing persisted) |
| Multi-node · multi-GPU · shared compute mesh | **Proposed** | Cannot be honestly verified from one host. `abi-wdbx` ships `cluster`/`remote_compute` references Abbey does not orchestrate; `subagents --peers` (same-host PATH) is the Current substitute |
| GPU/NPU/TPU compilation · training · inference *in Abbey* | **Out of scope** | Abbey detects accelerators (`abbey platform`) but runs no kernels. Contradicts "backend is `cursor-agent`, not a reimplementation" (CLAUDE.md) |
| Autonomous operation of services and the OS | **Out of scope** | Contradicts the safety invariant in CLAUDE.md: OS execution "must never run without `--confirm`, and only against the allowlist — this is a safety invariant, not a default to relax" |
| Local Qwen/Gemma weights · LoRA · CouchDB-Python second stack | **Out of scope** | A decision, not a capability gap: `abi-nn` next door already ships `train_model`/`train_on_jsonl`, and Abbey deliberately does **not** depend on it. `train_candidate` stays curation substrate only |

## Hybrid model-routing architecture strategy

Buildable residuals closed 2026-07-30; Proposed/OOS stay deferred above.

- [x] Richer Abi routing policy: `roles::route_decision` confidence + alternate + fallback
      recorded on `RouteRecord`; shown by `abbey routes` / `/routes` (no auto second agent)
- [x] `abbey learn review` / `abbey learn stats` for train_candidate provenance curation
      (export remains; LoRA stays Out of scope)
- [x] Keep memory backend replaceable; **do not** add CouchDB/Python as a second stack

## Current-scope polish (post-d74e941)

- [x] Shared `format_route_line` for CLI + slash `/routes` (conf · stage · alt · fb)
- [x] `hybrid_loop::log_stage` records paired alternate + fallback on correlated stages
- [x] `learn_from_routes` preserves confidence/alternate/fallback in activity payload
- [x] Docs/clap/TUI/doctor honesty (map preview Current; learn review/stats; routing audit)
- [x] please-fix: keep cargo `Running` / rustc `error[` / `warning:`; drop only agent chrome
- [x] JSONL roundtrip + learn route-payload unit tests

## Media / thinking / tools (Current via cursor-agent)

- [x] `media` module: path resolve, `--add-dir`, prompt note (no pixel encode / local vision)
- [x] CLI `--image`/`--video`/`--media`/`--thinking`/`--approve-mcps`
- [x] Slash `/image` `/video` `/think`; prompt-token media discovery; video keywords → Gemma
- [x] Doctor + docs honesty: tools = passthrough; thinking = model alias; media = workspace paths

## Generation + reasoning (agent-orchestrated)

- [x] `abbey imagine` / `generate image|video` / `/imagine` `/gen-video` — prompt + out path via agent tools
- [x] `abbey reason` / `/reason` — Cursor thinking model + structured reasoning wrap
- [x] Refuse generation under `ABBEY_BACKEND=fm` or `abi`; docs/claims stay honest (no local gen weights)

## Abi backend (`ABBEY_BACKEND=abi`)

- [x] `AgentBackend::Abi` → `abi complete`; own argv grammar with real `--` separator
- [x] Live opt-in only (`live`/`anthropic`/bare `claude-*`); cursor thinking/role leftovers stay local
- [x] Binary resolution: `ABBEY_ABI_BIN`/`abi_bin` first; abi candidates never include cursor-agent
- [x] TUI backend honesty: Home `backend` chip · status-bar backend (warn when off-default) ·
      Doctor backend/roles lines (prompt-only under fm/abi) · Models tab static prefetch so
      the cursor alias table never shows under abi/fm
- [x] In-TUI backend switcher: Ctrl-B + palette "Cycle backend" via `resolve_agent_for`;
      unresolvable backends skipped, honest status when nothing else resolves
- [ ] "Modern TUI" beyond honesty (richer panes/layout) — needs a design pick (see goals.md
      residuals); windowed GUI would be a claims-gate amendment first
- [x] Account/MCP/gen refuse; no stale-chat retry; doctor + claims + docs honesty
- [x] Unit tests: no flag leak, `--` prompt position, normalize thinking→local, abi candidate paths

## High-quality voice I/O (macOS)

- [x] `voice` module: Premium→Enhanced→standard `say` TTS ranking; novelty voices demoted
- [x] On-device STT helper `scripts/abbey-stt.swift` (lazy `swiftc` into state `bin/`)
- [x] CLI `abbey voice|speak` + slash `/speak` `/listen` `/voice`; `voice ask` loop
- [x] Doctor/docs: how to download Premium voices; no cloud TTS SaaS claim

## MCP / ACP protocol surfaces

- [x] `protocols` module: parse `mcpServers` from standard mcp.json paths
- [x] `abbey mcp status|paths` local inventory; other verbs → cursor-agent
- [x] `abbey acp list|run` for gemini/opencode ACP stdio peers
- [x] Doctor + claims: Abbey is not an MCP/ACP host runtime

## Smart improve (ledger + gate)

- [x] `src/improve/{mod,ledger,gate}.rs` — status/plan/run, ledger parse, check.sh runner
- [x] CLI/slash/doctor/claims wiring; bounded `--confirm` + max-rounds/minutes
- [x] Unit tests for ledger/args/gate classify; no live agent in tests
- [x] Close goal after `./check.sh` green evidence (human ledger close) — verified live via
      the built binary: `abbey improve status` (ledger parse: 18 goals/86 todos, correct
      focus pick) and `abbey improve plan` (diagnose+implement prompt preview, no apply)
      against a green `./check.sh`. `improve run --confirm` (the Max force-apply path) was
      **not** exercised — that spends real cursor-agent credits and edits the tree
      autonomously, so it stays for a human-initiated run, not this close

### Known gate bug — file-size guard hard-fail is unreachable (closed)
- [x] `src/tui/app.rs` decomposed 1025→896 lines: the four background-refresh methods
      (`refresh_personas`/`refresh_memory`/`refresh_skills`/`refresh_doctor`) moved to a new
      `src/tui/refresh.rs` (`impl App` in a sibling module — same pattern `ui.rs` already
      uses for `super::app::App`). Mechanical move only, no behavior change; dropped five
      now-unused imports (`config`/`inventory`/`memory`/`persona`/`roles`) from `app.rs`
- [x] `check.sh`'s file-size guard reordered — `elif n > 1000: FAIL` now checked before
      `elif n > 800: WARN`, so the hard-fail branch is reachable again. Re-ran `./check.sh`
      with the reorder live: both flagged files (`app.rs` 896, `src/improve/mod.rs` 842)
      correctly only WARN; `gate::classify_failures`'s `"hard max 1000"` match still lines
      up with the FAIL message text, so the `FailKind::FileSize` classifier isn't dead
