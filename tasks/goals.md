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
- Phase 3 residual: the `abbey wdbx … → abi wdbx …` subprocess bridge is argv-tested only;
  `abi` is not a binary on this PATH, so the live passthrough is unverified (it errors
  honestly rather than claiming success)
- Phase 4 closed: `abbey hybrid-loop` (Gemma interpret → Max implement) links both stages
  by correlation id; `abbey routes --correlation <id>` reads them back. Verified live

## Broader polish
status: done
