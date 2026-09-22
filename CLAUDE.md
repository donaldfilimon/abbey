# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`AGENTS.md` is canonical for this repository and wins on any conflict. Read
it first: it holds the Zig pin, the gate and its stage order, the layout,
the rules, and the git policy. This file adds only what AGENTS.md does not
say. `docs/claims.md` is the honest capability ledger (Current rows name
their tests).

## Inner loop

The gate is the only evidence of green:

```sh
./tools/check.sh > /private/tmp/abbey-zig-gate.log 2>&1; echo EXIT:$?
```

While iterating, from the repository root (goldens and fixtures resolve
against the cwd):

```sh
zig build test                        # lib + entry tests, safe edition
zig build test -Dpersonal=true        # personal edition
zig build test-bin && ./zig-out/bin/abbey-zig-lib-tests   # prints "All N tests passed"
```

There is no single-test filter. `build.zig` exposes no `-Dtest-filter`, and
on this Zig pin the build runner has no `--test-filter` flag: filters are
compile-time `addTest(.{ .filters = ... })` (lib/std/Build.zig). A bare
`zig test src/<file>.zig` does not work either, since `root.zig` imports the
`build_options` module that only `build.zig` provides. Adding a filter
option is a `build.zig` change, not something to improvise.

## Architecture in one pass

`src/main.zig` turns the process (args, cwd, env, stdio) into a `ctx.Ctx`
and calls `cli/dispatch.zig` `run`; nothing below that reads the process
directly. Dispatch loads config and state, builds an `AgentConfig`
(`agentConfig`), and routes verbs. Every generation surface goes through
one canonical path: `actions.runAgent(RunSpec)` -> `session.hybridRun`
(persona x role wrap, preference context, route audit, activity memory) ->
`agent/run.zig` `runResilient`, which spawns the backend with the argv
grammar from `agent/argv.zig`. The only exceptions are the headless
bypasses in `capture.zig`. The daemon (`daemon/`, `abbeyd-zig`) and the TUI
(`tui/`) reuse the same library module (`root.zig`, imported as `abbey`),
so a behavior change in `session`/`actions` reaches CLI, TUI and daemon at
once.

Most files are ports of a named Rust file in `../abbey` (the header comment
says which, e.g. `session.rs`, `actions.rs`). When behavior is in question,
read the Rust source and run the Rust binary as the oracle; never build in
or edit `../abbey`, `../abi`, or `../wdbx`.
