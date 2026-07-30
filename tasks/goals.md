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

## On-device backend, spatial memory, and capability reporting
status: in_progress

Captured 2026-07-30 from: "complete abbey as an independent model not requiring any
backend and gpu acceleration with multi node and multi gpu support, npu/tpu support for
compilation and learning to run and operate new services and the os and computers itself,
shared compute between nodes, beautiful CLI/TUI, optional fm-CLI platform checking on
macOS 27, wdbx as a 3-D brain-like memory map people can teach."

Buildable and in scope for this goal:
- `ABBEY_BACKEND=fm` — Apple Foundation Models CLI (`/usr/bin/fm`) as a real executor, so
  Abbey runs **on-device with no cursor-agent and no network**. Verified present on this
  machine (macOS 27.0, `fm respond` ~1.1s, `system` model available)
- Platform/capability probe: `fm available`, on-device vs Private Cloud Compute, and the
  compute backends `abi-wdbx` can actually report (CPU/GPU/ANE selection)
- WDBX 3-D spatial memory: `abi-wdbx` already ships `SpatialIndex3D`/`Point3D` and
  `hybrid_spatial_search`; wiring Abbey's memory records to 3-D points is real work
- CLI/TUI polish

**Not buildable as stated — recorded here so the goal is not silently laundered:**
- "Independent model" in the sense of Abbey *being* a model (own weights, own training)
  is a research-scale project, and `AGENTS.md` already lists local Qwen/Gemma weights and
  LoRA/fine-tuning as **Out of scope**. What is achievable is *independence from
  cursor-agent* via an on-device backend — that is what this goal builds
- Multi-node / multi-GPU / NPU-TPU *compilation and learning*: `abi-wdbx` has
  `cluster`/`remote_compute`/`compute` reference implementations, so **capability
  reporting and shared-compute plumbing** are reachable; a distributed training runtime is
  not, and must not be claimed
- "Operate the OS and computers itself" autonomously conflicts with the standing safety
  invariant that OS execution is allowlist-only and always requires `--confirm`. Not
  relaxing that; see `os_control.rs`
- "More powerful than frontier models" is not an acceptance criterion anything can meet;
  ignored as a claim, treated as "make the CLI/TUI genuinely better"

## Broader polish
status: done
