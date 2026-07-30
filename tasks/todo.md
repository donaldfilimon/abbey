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
