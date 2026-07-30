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
