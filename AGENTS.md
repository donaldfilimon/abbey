# AGENTS.md

Canonical agent guidance for abbey-zig. `CLAUDE.md` points here.

abbey-zig is a ground-up, stdlib-only Zig rewrite of `../abbey` (Rust). The
Rust checkout, `../abi`, and `../wdbx` are READ-ONLY oracles: read their
source and run their already-built binaries, never edit, stage, build in, or
clean them. Every executor (`abi`, `ollama`, `claude`, `fm`, `grok`,
`cursor-agent`, `git`) is a subprocess with its own argv grammar; nothing
from `../abi` is linked.

## Toolchain and gate

Zig master, pinned by `build.zig.zon` `.minimum_zig_version`
(`0.17.0-dev.2251+1175a3e99`). The std API moves: read the source under
`zig env` -> `std_dir` before writing any std call, and cite the file in a
comment the first time an API is used. Do not write std calls from memory.

```sh
zig version
./tools/check.sh > /private/tmp/abbey-zig-gate.log 2>&1; echo EXIT:$?
```

`tools/check.sh` is the single gate. A green `zig build test` alone is weak
evidence; quote the gate's exit code and its `check.sh: OK` line. Never pipe
the gate through `tail`/`head` (that reports the pipe's exit code).

- `zig build` builds the safe edition (`abbey-zig`); `-Dpersonal=true`
  builds `abbey-zig-personal`. Editions separate identity and namespaces
  only; neither adds runtime authority.
- `zig build test-bin` installs the test executables; the gate runs them
  directly so the runner's `All N tests passed` line is quotable.

## Rules

- Errors are named error sets per module; no `anyerror` at a public
  boundary; no `catch unreachable` outside tests except where a comment
  proves the branch impossible (fixed-size formatting).
- Every allocation has an owner and a `deinit`/`free` in the same scope or a
  documented transfer. Commands use an arena per invocation. Tests use
  `std.testing.allocator`, whose leak report fails the gate.
- Library code never reads the process environment, cwd, or stdio: they
  arrive in `ctx.Ctx`.
- State lives only under the edition root (`ABBEY_ZIG_STATE_DIR`, default
  `~/.local/state/abbey-zig`). Never the Rust `~/.local/state/abbey`, and
  never `~/.abi` in tests: a test that spawns `abi` sets `HOME` to a temp dir.
- No stubs. A capability that is not implemented is a Proposed row in
  `docs/claims.md`, never a function that pretends. A capability without a
  test is Proposed, whatever the code looks like.
- `src/main.zig` <= 200 lines (entry only); every other file <= 1000.
- No em dashes in source comments, docs, or commit messages.

## Git policy

Work on `main` in this checkout. Branches and worktrees only when a task
needs isolation. This repository has NO remote: every commit exists only on
this disk until Donald adds one. Commit locally per phase and tag the phase
(`p1`, ...); never push. When a backup is needed, `git bundle` to
`~/at-risk-bundles/` and add it to that directory's README. Stage by exact
path, never `git add -A`.
