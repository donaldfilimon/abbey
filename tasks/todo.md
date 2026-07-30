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

### Slice 2 — WDBX as a 3-D memory map (done)
- [x] `coordinates()` places each memory at topic × recency × consolidation; pure and
      covered by the default gate
- [x] `abbey memory map` and `abbey memory near <id>` on **both** backends
- [x] Under `--features wdbx` the point is mirrored to a WDBX `SpatialRecord`, so the map
      is visible to `abi wdbx` too (verified: `spatial_records: 5`)
- [x] `abbey memory put --tag <subject>` — without it every record collapsed into one
      column, which made the topic axis useless. Untagged records now sit in a visible
      `(untagged)` column rather than being hash-scattered into fake topics
- [ ] Axes are deterministic, **not** a learned embedding space — Abbey has no embedder.
      Semantic placement stays Proposed

### Not started (deferred by construction — see goals.md)
- [ ] Multi-node / multi-GPU / shared compute between nodes
- [ ] NPU/TPU compilation and learning
- [ ] Autonomous operation of services and the OS (conflicts with the allowlist +
      `--confirm` safety invariant)
- [ ] TUI surface for the 3-D map
