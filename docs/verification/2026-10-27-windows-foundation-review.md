# Windows-foundation commit review (2026-10-27)

Plan: `docs/plans/2026-10-27-abbey-platforms-security-closure-plan.md`, Task 1,
Steps 1 and 2. Step 3 (fix commits on `abbey/windows-foundation`) needs the
owner to release `.work/windows-foundation/abbey` first, which is a Donald step.
This file is a read-only review. It changes no source and fixes nothing. Each
finding below becomes a follow-up task.

## Revisions

| Item | Value |
|---|---|
| Review base (`main` HEAD) | `0ad2fef80f0c1ec38dd1367882541e6a2cdec661` |
| Reviewed commit (`abbey/windows-foundation` tip) | `5be3d252a5a790afc12710aef0211bbdf111e430` |
| Merge base `main` / `abbey/windows-foundation` | `d34e25842bc5ef6299f3b94ffdd75f85d2cd7fb4` |
| `git -C ../abi rev-parse HEAD` | `dff096415715462370a18a9ad71e5eaf098197da` |
| `git -C ../wdbx rev-parse HEAD` | `93dbeab8987a06007430bf5ab17573ec5a80dc3e` |

The reviewer is not the commit's author. The review ran in task worktree
`task/wf-platforms-security-closure-t1-20261027`. The owned worktree
`.work/windows-foundation/abbey` was not opened, built in or modified. Every
file was read through `git show 5be3d25:<path>`, and every line reference
below points into that blob.

## Step 1: commit identity

- `git rev-list --count main..abbey/windows-foundation` → `1`
- `git merge-base main abbey/windows-foundation` → `d34e258…`
- `git show --stat 5be3d25` → 28 files, `+1849/-485`
- `git diff --stat d34e258 main -- <the 28 files>` → only `tasks/completion.json`
  changed on `main` since the base (`+20/-1`).

## Step 2: checklist

| # | Item | Result |
|---|---|---|
| a | Every `unsafe` is covered by `#[allow(unsafe_code)]` **and** has a `// SAFETY:` comment | **FAIL** (all blocks are covered by the allows, but only 6 of 25 have a SAFETY comment) |
| b | All Windows code is `cfg(windows)`, and `connection.rs` is a pure extraction of `unix.rs` | **PASS** (with the behavioral notes in F-4 and F-5) |
| c | `supervisor/windows.rs` and `daemon/windows.rs` are each under 800 lines | **PASS** (599 and 370) |
| d | The `Cargo.lock` delta is only the `windows-sys` line, with no version churn | **PASS** |
| e | The installed Windows daemon requires `--read-only`, with no fallback | **PASS** (CI gap in F-6) |
| f | The pipe DACL grants only the owner SID and rejects remote clients | **PASS by code reading** (not natively proven; see F-7) |
| g | The Job Object is assigned before the child is resumed | **PASS** |

### (a) unsafe coverage and SAFETY comments: FAIL

The commit message and diff contain 28 occurrences of the string `unsafe`:
2 file-level `#![allow(unsafe_code)]` attributes, 25 `unsafe` blocks and
expressions, and 1 line in the verification document
(`docs/verification/2026-10-26-windows-foundation.md`, "unsafe-code denial
enabled").

How coverage works: `Cargo.toml:46-47` sets `[lints.rust] unsafe_code = "deny"`
(deny, not forbid), so an inner `#![allow]` can override it.
`src/daemon/windows.rs:3` and `src/runtime/supervisor/windows.rs:2` are
file-level inner attributes. Each covers its whole module, including its
`#[cfg(test)]` submodules. `src/daemon/server/windows.rs` contains no `unsafe`.
Every unsafe block is therefore covered. Coverage passes.

SAFETY comments are where this item fails. The workspace enables neither
`clippy::undocumented_unsafe_blocks` nor `clippy::multiple_unsafe_ops_per_block`,
so nothing enforces them. The "SAFETY?" column below means a comment that
starts with `// SAFETY:` immediately above the block.

`src/daemon/windows.rs` (19 blocks, 6 documented):

| Line | Call(s) | SAFETY? |
|---|---|---|
| 31 | `OwnedHandle::from_raw_handle` | yes (30) |
| 41 | `OpenProcessToken` | yes (40) |
| 47 | `GetTokenInformation` (size probe) | yes (46) |
| 63 | `GetTokenInformation`, `&*…cast::<TOKEN_USER>()`, `ConvertSidToStringSidW`, raw `text.add` walk, `from_raw_parts`, `LocalFree` | yes (61-62); one comment covers 6 operations |
| 108 | `GetCurrentProcess` | yes (107) |
| 112 | `ConvertStringSecurityDescriptorToSecurityDescriptorW` | yes (111) |
| 127 | `CreateNamedPipeW` | **no** (125-126 is a rationale comment, not a SAFETY comment) |
| 140 | `LocalFree(descriptor)` | **no** |
| 153 | `GetCurrentProcess` | **no** |
| 156 | `CreateFileW` | **no** (155 is a rationale comment) |
| 168 | `GetNamedPipeServerProcessId` | **no** |
| 174 | `OpenProcess` | **no** |
| 182 | `SetNamedPipeHandleState` | **no** |
| 193 | `ConnectNamedPipe` | **no** |
| 200 | `GetLastError` | **no** |
| 211 | `DisconnectNamedPipe` | **no** |
| 256 | `ReadFile` | **no** |
| 269 | `GetLastError` | **no** |
| 289 | `WriteFile` | **no** |

`src/runtime/supervisor/windows.rs` (6 blocks, 0 documented):

| Line | Call(s) | SAFETY? |
|---|---|---|
| 77-124 | One 48-line block: `CreateJobObjectW`, `from_raw_handle`, `mem::zeroed`, `SetInformationJobObject`, `AssignProcessToJobObject`, `CreateToolhelp32Snapshot`, `Thread32First/Next`, `OpenThread`, `ResumeThread` | **no** (57-59 and 97-98 are rationale comments) |
| 130 | `TerminateJobObject` | **no** |
| 138 | `mem::zeroed::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>` | **no** |
| 139 | `QueryInformationJobObject` | **no** |
| 177 | `TerminateJobObject` (Drop) | **no** |
| 583 | test: `OpenProcess`, `from_raw_handle`, `WaitForSingleObject`, `GetLastError` | **no** |

### (b) cfg gating and the `connection.rs` extraction: PASS

The cfg gating is correct:

- `src/daemon/mod.rs:36-37` declares `#[cfg(windows)] mod windows;`.
- `src/daemon/server.rs:395-397` declares `#[cfg(windows)] mod windows;`.
- `connection.rs` is `#[cfg(any(unix, windows))]` (`server.rs:391-393`).
- `src/runtime/supervisor.rs:407-409` declares
  `#[cfg(windows)] #[path = "supervisor/windows.rs"] mod platform;`.
- The client's Windows `connect` is `#[cfg(windows)]` (`src/daemon/client.rs:634`).
- The `windows_sys` imports appear only in those cfg(windows) modules.
- The `--read-only` gate in `src/bin/abbeyd.rs:35-41` is `#[cfg(windows)]`.
- `src/bin/abbeyd.rs:6-7` gates the Unix runtime import with
  `#[cfg(not(windows))]`.
- The dependency is `[target.'cfg(windows)'.dependencies]` (`Cargo.toml:107-108`).

The one non-Windows change is mechanical: every former `#[cfg(not(unix))]`
stub now reads `#[cfg(not(any(unix, windows)))]`. That applies across
`client.rs`, `client/v3.rs`, `client/v3/v3_claims.rs`, `client/v3/v3_memory.rs`,
`client/v4.rs`, `commands.rs`, `runtime/delegated.rs`, `runtime/supervisor.rs`
and `runtime/supervisor/tests.rs`.

For the extraction, `diff <(git show 5be3d25^:src/daemon/server/unix.rs) <(git show 5be3d25:src/daemon/server/connection.rs)`
shows these hunks, and nothing else:

1. The `std::fs`/`io`/`os::unix`/`thread` imports were removed (connection.rs
   needs none of them).
2. `serve`, `prepare_private_directory`, `remove_stale_socket` and `SocketGuard`
   were removed from connection.rs. They remain verbatim in the new `unix.rs`
   (`unix.rs:9-61`, `90-157`).
3. `handle_connection(mut stream: UnixStream, …)` became
   `pub(super) fn handle_connection(mut stream: impl Read + Write, …)`
   (`connection.rs:3-4`).
4. The `set_nonblocking(false)` / `set_read_timeout` / `set_write_timeout`
   preamble moved, byte-identical, into a new Unix wrapper `handle_connection`
   (`unix.rs:63-85`), which then calls `super::connection::handle_connection`.
5. `write_wire_response`'s gate changed from `#[cfg(test)]` to
   `#[cfg(all(test, unix))]` (`connection.rs:292`). It is re-exported only for
   Unix tests (`unix.rs:87-88`).
6. `write_wire_response` and `write_wire_response_framed` take
   `&mut impl Write` instead of `&mut UnixStream` (`connection.rs:294`, `306`).

None of these hunks changes the frame read, frame-length cap, bearer
authentication, rate limiting, `request_id` correlation, response-version
selection or error envelopes. Those parts of the old file (old `unix.rs:84-425`)
are byte-identical in `connection.rs`.

The client side is not an extraction. It adds a new Windows transport: the
Unix `mod unix` is reused with `use super::super::windows::Pipe as UnixStream`
(`client.rs:226-227`), and a separate `#[cfg(windows)] fn connect` is added
(`client.rs:634-671`). `read_response`, envelope validation and redaction are
shared and unchanged. F-4 and F-5 record the behavioral differences.

### (c) file sizes: PASS

| File | Lines @5be3d25 | Rule |
|---|---|---|
| `src/runtime/supervisor/windows.rs` | 599 | < 800 ✔ |
| `src/daemon/windows.rs` | 370 | < 800 ✔ |
| `src/daemon/server/connection.rs` | 350 | ✔ |
| `src/daemon/server/windows.rs` | 92 | ✔ |
| `src/daemon/server/unix.rs` | 157 | ✔ |
| `src/bin/abbeyd.rs` | 70 | ✔ |
| `src/daemon/client.rs` | 722 | ✔ |
| `src/daemon/server.rs` | **822** (base 814) | over the 800 soft warning, under the 1000 hard limit. See F-9. |

### (d) Cargo.lock delta: PASS

`git show 5be3d25 -- Cargo.lock` adds exactly one line to the `abbey` package's
dependency list: `"windows-sys 0.61.2",`. `git show d34e258:Cargo.lock` already
has `[[package]] name = "windows-sys" version = "0.61.2"` (line 8367-8368), so
no package, version or checksum changed. `Cargo.toml` adds only the
`[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.61.2", features = [...] }`
block (10 `Win32_*` features). Lockfiles don't record features, which is
consistent with the one-line delta.

### (e) installed daemon requires `--read-only`: PASS

- `src/bin/abbeyd.rs:35-41`: on Windows, `run()` refuses unless argv after the
  program name is exactly `["--read-only"]`. It returns `Err("Windows foundation
  requires --read-only; …")`, and there is no other handler path. The
  store-backed `RuntimeHandler` is `#[cfg(not(windows))]` (`abbeyd.rs:33-34`),
  and the read-only handler is `AppService::default()`.
- `src/bin/abbeyd.rs:10-18`: the only other Windows argv accepted is exactly
  `["--version"]`. It prints a version and exits without serving.
- `install.ps1` (5be3d25, hunk at old line 90) installs the daemon only when
  `ABBEY_WINDOWS_FOUNDATION_INSTALL -eq 'enabled'`. It copies the file through a
  random staged name, probes `--version`, then moves it into place. It registers
  no service, scheduled task or autostart, so nothing can launch the daemon
  without the flag. The binary enforces the flag itself.
- `src/daemon/runtime_handler.rs` (hunk at 38) and `runtime_config.rs`
  (hunk at 39) still refuse runtime construction on Windows.

### (f) pipe DACL and remote rejection: PASS by code reading

- `src/daemon/windows.rs:108-109`: the SID comes from the process token
  (`OpenProcessToken`/`TokenUser`, `:37-82`), not from the environment. The SDDL
  is `O:{sid}D:P(A;;GA;;;{sid})`: the owner is the user SID, and a protected DACL
  (`P`, no inheritance) holds a single allow ACE that grants `GA` to that SID
  only. No SYSTEM, Administrators, Everyone or ALL APPLICATION PACKAGES ACE.
- `:130`: `FILE_FLAG_FIRST_PIPE_INSTANCE` (no squatting or second owner), and
  `:132` `nMaxInstances = 1`.
- `:131`: `PIPE_REJECT_REMOTE_CLIENTS` is set.
- `:84-98`: the pipe name embeds the edition slug, the user SID and a SHA-256
  hash of the UTF-16 socket path.
- `:155-165`: the client opens with `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`,
  so the server can't impersonate it.
- `:167-180`: the client resolves the server PID
  (`GetNamedPipeServerProcessId`) and rejects the pipe when that process's token
  SID differs from its own.

This is not native proof. The author's own document lists another-account and
remote-machine rejection as "Native proof still required"
(`docs/verification/2026-10-26-windows-foundation.md`). See F-7.

### (g) Job Object assigned before resume: PASS

`src/runtime/supervisor/windows.rs`:

1. `:212` `command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)`.
2. `:217-219` checks for pre-spawn cancellation before anything is created.
3. `:221` `command.spawn()` creates the process, which stays suspended.
4. `:222` `ChildGuard::new(child)` calls `assign` (`:76`):
   - `:78` `CreateJobObjectW`
   - `:84` `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, with no `BREAKAWAY_OK` or
     `SILENT_BREAKAWAY_OK`
   - `:85-89` `SetInformationJobObject`
   - `:91` `AssignProcessToJobObject`
5. Only after that: `:99-119` takes a Toolhelp thread snapshot, finds the
   child's thread, and `:114` `ResumeThread` must return a previous suspend
   count of `1`.
6. On any failure, `:66-73` kills and reaps the still-suspended child, and the
   job handle's drop (kill-on-close) is a second guarantee.
   `Drop for ChildGuard` (`:173-183`) also calls `TerminateJobObject`.

## Findings (follow-up tasks; none fixed here)

Severity: **M** = should be fixed before the commit merges to `main`.
**L** = hardening or hygiene.

- **F-1 (M, item a).** 19 of 25 `unsafe` blocks have no `// SAFETY:` comment.
  - `src/daemon/windows.rs:127,140,153,156,168,174,182,193,200,211,256,269,289`
  - `src/runtime/supervisor/windows.rs:77,130,138,139,177,583`

  `supervisor/windows.rs:77-124` is a single 48-line block that mixes about
  ten FFI calls. Split it into per-call blocks with invariants (handle
  validity, `THREADENTRY32.dwSize`, zeroed POD structs). Consider enabling
  `clippy::undocumented_unsafe_blocks` for these two modules so this is
  enforced.
- **F-2 (L, item g).** `supervisor/windows.rs:91-95`: when
  `SetInformationJobObject` or `AssignProcessToJobObject` fails, the code
  returns `SupervisorError::Teardown("assign suspended child to job failed")`
  and drops `last_os_error()`. That hides the nested-job refusal code
  (`ERROR_ACCESS_DENIED`) that the plan's "restrictive parent job" proof needs
  to distinguish. `:121-123` (resume failure) is lossy in the same way.
- **F-3 (L, item g).** `supervisor/windows.rs:97-118` resumes "the first
  Toolhelp thread whose owner PID is the child". The design fails closed (a
  foreign injected thread would return a suspend count other than 1 →
  `break` → error → kill). It still depends on a single-thread assumption that
  a primary-thread handle would make unnecessary. Record this as a design note,
  and revisit it if stable Rust or `CreateProcessW` with `PROCESS_INFORMATION`
  becomes available.
- **F-4 (M, item b, behavioral).** `daemon/windows.rs:215-245`: Windows
  `set_read_timeout`/`set_write_timeout` set **absolute** per-connection
  deadlines, while Unix socket timeouts are per-operation idle timeouts.
  - The server sets both deadlines once at accept
    (`server/windows.rs:18-21`), so handler execution time counts against the
    write budget.
  - The client sets them at connect (`client.rs:641-646`), so the whole
    response must arrive within `read_timeout` (5 s default) of connect.

  This is fine for the read-only v1 foundation. It is a semantic difference
  that must be re-qualified before any v2/v3/v4 or long-running handler is
  served over pipes.

  `deadline()` (`:224-234`) also rejects zero, `None` and > 30 s.
  `DaemonConfig.read_timeout`/`write_timeout` are `pub` fields
  (`config.rs:92-93`), so a caller-set value over 30 s makes the Windows
  server's `serve` return an error after the first accept, where Unix would
  accept the value.
- **F-5 (L, item b, behavioral).** `server/windows.rs:17-21,27-28`: errors from
  `accept()` or the timeout setters terminate the whole serve loop. Unix
  `handle_connection` (`unix.rs:75-82`) drops only the failing connection.
  `client.rs:644-646` maps a `set_write_timeout` failure to `ClientError::Read`,
  and `:647-649` maps every write error to `ConnectionHandoff`. That is
  mislabelling only. In `client.rs:226-227`, the Windows pipe is aliased as
  `UnixStream` inside `mod unix`, which is a naming smell worth a rename
  (`mod transport`).
- **F-6 (L, item e).** No automated check asserts the refusal path. Specifically:
  - `tools/ci/qualify-windows.ps1` (5be3d25) launches the installed daemon only
    with `--read-only`.
  - `tools/tests/test_windows_ci_qualification.py` only mutation-checks that
    this string is present.
  - Nothing runs the installed `abbeyd.exe` with no args, or with
    `--read-only --extra`, and asserts a non-zero exit.
  - `src/bin/abbeyd.rs` has no unit test for the argv gate.

  Add one negative installed-daemon step.
- **F-7 (L, item f).** The DACL, remote rejection and server-SID check are
  correct by reading but are not exercised natively. The supporting details:
  - The server does not verify the client's SID
    (`GetNamedPipeClientProcessId`). It relies on the DACL plus the bearer,
    which matches the Unix `0600` plus bearer model.
  - The SDDL carries no explicit mandatory label, so the default medium/no-write-up
    applies.
  - `PIPE_NOWAIT` (`:131`, `:181`) is a documented LAN-Manager-compatibility
    mode. The 5 ms polling loops (`:235-245`, `server/windows.rs:32`) are
    bounded but CPU-polling.

  Native another-account and remote-host rejection fixtures remain required,
  as the author's document states.
- **F-8 (L).** `supervisor/windows.rs:173-182`: `Drop` calls
  `self.child.wait()` without a bound. That wait is unbounded only if both
  `TerminateJobObject` and `Child::kill` fail. Bound it, or document why it
  can't hang.
- **F-9 (L, item c, adjacent).** `src/daemon/server.rs` grows from 814 to 822
  lines, which is already over the `check.sh` 800-line soft warning. It isn't
  one of the two files the checklist names. Track it for a split before it
  reaches the 1000-line hard limit.

## Task 2 input: the expected cherry-pick conflict

`tasks/completion.json` is the only one of the 28 files that `main` changed
after `d34e258`. The two sides touch different regions:

- **5be3d25 hunk** (`@@ -419,6 +419,11 @@`, goal `windows-foundation` →
  `evidence.source.refs`). It inserts five refs before
  `".github/workflows/rust.yml"`:
  `"src/daemon/windows.rs"`, `"src/daemon/server/windows.rs"`,
  `"src/runtime/supervisor/windows.rs"`, `"install.ps1"` and
  `"docs/verification/2026-10-26-windows-foundation.md"`.
- **main hunks since d34e258** (`@@ -1366,6 +1366,25 @@` and
  `@@ -1378,7 +1397,7 @@`, the dependency-security goal). They add
  `dependency_audit`/`consumer_strict_dependency_audit`/`local_gate_execution`/`historical_dependency_audit`
  evidence and rename `candidate_dependency_audit` to
  `historical_candidate_dependency_audit`.

The hunks don't overlap, so a textual conflict is **not** expected. A read-only
cherry-pick simulation,
`git merge-tree --write-tree --merge-base=5be3d25^ main 5be3d25`, exited 0 with
tree `d5dd152c578e2fcaf4bb3a943942189b543fcc80` and reported no conflicts.
Task 2 should still re-run `python3 tools/check_completion.py` after the pick,
because the windows-foundation refs must exist on the integration branch.

## Optional compile evidence: SKIPPED (this is not a pass)

`./tools/cargo.sh check --locked -p abbey --target x86_64-pc-windows-msvc` was
**not run**, for two reasons:

- On this same macOS host, the author's record
  (`docs/verification/2026-10-26-windows-foundation.md`, "Checks and limits")
  shows the full `--target x86_64-pc-windows-msvc` check blocked in `ring`'s C
  build because Windows SDK headers (`assert.h`) are missing.
- The host was overloaded (load average about 79 at review time), and the task
  limits builds to targeted runs.

No Windows compilation, Clippy or native execution of these modules is claimed
by this review. `check.sh` on macOS never compiles `cfg(windows)` modules. The
only full-compile evidence would come from the native Windows 11 ARM64 job.

## Commands run

```sh
dev-worktree repos/local/the-donald-company/company/legacy/abbey wf-platforms-security-closure-t1-20261027 main
git rev-parse HEAD; git rev-parse 5be3d25
git rev-list --count main..abbey/windows-foundation
git merge-base main abbey/windows-foundation
git show --stat 5be3d25
git -C ../abi rev-parse HEAD; git -C ../wdbx rev-parse HEAD
git show 5be3d25:<path> > scratch/new_<path>; git show 5be3d25^:<path> > scratch/old_<path>   # for each reviewed file
git show 5be3d25 -- Cargo.lock Cargo.toml .github/workflows/rust.yml tasks/completion.json install.ps1 \
  src/daemon/client.rs src/daemon/client/v3.rs src/daemon/client/v3/v3_claims.rs src/daemon/client/v3/v3_memory.rs \
  src/daemon/client/v4.rs src/daemon/mod.rs src/daemon/server.rs src/runtime/supervisor.rs src/commands.rs \
  src/daemon/runtime_config.rs src/daemon/runtime_handler.rs src/runtime/delegated.rs src/runtime/supervisor/tests.rs \
  tools/ci/qualify-windows.ps1 tools/tests/test_windows_ci_qualification.py
git show 5be3d25 | grep -c unsafe                       # 28
git show 5be3d25 | grep -n unsafe
grep -n unsafe new_src_daemon_windows.rs new_src_runtime_supervisor_windows.rs new_src_daemon_server_windows.rs
git show 5be3d25:Cargo.toml | grep -n -A2 '^\[lints'    # unsafe_code = "deny"
diff old_src_daemon_server_unix.rs new_src_daemon_server_connection.rs
wc -l new_*.rs
git show d34e258:Cargo.lock | grep -n -A1 'name = "windows-sys"'
git diff --stat d34e258 main -- $(git show --name-only --format= 5be3d25)
git diff d34e258 main -- tasks/completion.json
git merge-tree --write-tree --merge-base=5be3d25^ main 5be3d25   # exit 0, no conflicts
```

The scratch copies live in the session scratchpad, outside the repository.
