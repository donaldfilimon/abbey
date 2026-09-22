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

Gate stages, in order: fmt, build both editions, leak-checked tests for both
editions, help goldens (`tests/golden/help/<cmd>.txt` vs `<cmd> --help`),
contracts qualification (`tools/abbey_contracts.py verify`, copied verbatim
from `../abi`, plus the lock digest), claims sync (`tools/check_claims.py`),
the Rust route-reader oracle (`tools/stages/rust_oracle.sh`, uses
`ABBEY_ZIG_RUST_ORACLE` or `~/.local/bin/abbey`), the Rust daemon client
against the Zig abbeyd (`tools/stages/rust_daemon_client.sh`, same binary;
a recording proxy proves its v2 -> v1 downgrade), the TUI live-backend grep
guard, the TUI in a real pty (`tools/stages/tui_pty.sh`: `stty -g` identical
before and after a quit and a SIGTERM), the real-abi end to end
run (`tools/stages/e2e_abi.sh`, needs `ABBEY_ZIG_E2E_ABI`), size guard.
Oracle and e2e stages print `SKIP:` when their binary is absent; a SKIP is
unmeasured, never a pass.

Change help text: edit `src/cli/help.zig`, then regenerate the golden with
`./zig-out/bin/abbey-zig <cmd> --help > tests/golden/help/<cmd>.txt` and
review the diff. Change a claim: edit `src/claims.zig`, rebuild, run
`python3 tools/check_claims.py --write`.

## Layout

`src/main.zig` entry; `src/root.zig` library root; `cli/` parser, help,
dispatch; `agent/` backend selection, argv grammars, execution; `memory/`
JSONL store, record shape, lexical similarity; `persona/` router and
contracts; `session.zig` + `actions.zig` the canonical path; `capture.zig`
the headless bypasses; `learn*.zig`; `route_log.zig`; `state/`; `config/`;
`util/` JSON, time, uuid, file helpers. `daemon/` is P2: `protocol.zig`
(transport-free v1 decision, unit tested), `route_audit.zig` + `text.zig`
(sanitizer), `server.zig` + `sys.zig` (socket, poll deadlines), `config.zig`,
`client.zig`, `cli.zig` (`abbey-zig daemon ...`); `src/abbeyd.zig` is the
`abbeyd-zig` entry. `tui/` is P3: `app.zig` (pure key/tab state machine over
decoded keys and an injected `Host`), `ui.zig` + `frame.zig` (cell grid,
plain and truecolor ANSI encoders), `input.zig` (byte -> key decoder),
`term.zig` (injectable Terminal, raw mode, restore, signals), `host.zig`
(live panel data), `run.zig` (event loop and the `tui` verb). The App holds
the caller's `AgentConfig` by pointer: Ctrl-B mutates it and the loop hands
the same pointer to `actions.runAgent`. Never read ABBEY_BACKEND or call
`backend.select` under `src/tui/` (the gate greps for it). Change a frame:
edit `ui.zig`, run the lib tests, review `.zig-cache/tmp/tui-golden/<name>.txt`,
copy it over `tests/golden/tui/<name>.txt`. Daemon sockets in tests and stages live under short
paths (`/private/tmp/abz-*` or the repo's `.zig-cache/tmp`): Darwin's
`sun_path` holds 104 bytes. `contracts/abbey/` is a
byte-identical copy of `../abbey/contracts/abbey`; never edit it here.

## Rules

- Errors are named error sets per module; no `anyerror` at a public
  boundary; no `catch unreachable` outside tests.
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
