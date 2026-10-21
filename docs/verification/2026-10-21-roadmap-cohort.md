# Roadmap cohort — 2026-10-21

Four roadmap phases landed on the retained Rust application: the Ollama
no-auto-pull preflight on the streamed path, the desktop Doctor effective-model
IPC, Metal-device evidence for exact-model inference, and a bounded build
budget for the claims synchronizer. A fifth item, the toolchain/sandbox PATH
fallbacks, was **already committed** before this cohort began and is described
here only because this receipt's earlier draft wrongly listed it as part of the
uncommitted set.

The tree is **uncommitted**; this receipt covers the working tree as described
below and is not a frozen three-repository cohort receipt. The exact file
counts are stated in *Gate evidence* below and are the counts at the moment
this receipt was written — the tree grew while the cohort was being reviewed,
so a reader should re-derive them rather than trust a stale number.

## 1. Toolchain and sandbox PATH fallbacks (already committed)

`tools/toolchain-env.sh` prepends common package-manager locations when
`rustup` is not already resolvable, and `tools/check_claims_sync.py` resolves
`cargo` through its own fallbacks. `docs/runtime.md` carries the sandbox PATH
note: a constrained shell (`/usr/bin:/bin`) still finds the pinned toolchain.

**These changes are committed, not part of the uncommitted set:**
`tools/toolchain-env.sh` landed in `9fd1007` and the claims synchronizer's
cargo resolution in `c14084c`. Only the `docs/runtime.md` note is uncommitted.

**The two fallbacks are not identical, and this receipt does not claim they
are.** The shell helper resolves `rustup` first and then asks it for the
toolchain-pinned `cargo` (`rustup which cargo`), so its answer follows the
repository's own toolchain override. The Python helper's uncommitted change
resolves `rustup which cargo` *first* and falls back to `which cargo` and
then to the usual install directories — so the Python path is the stronger of
the two, not the same one.

Evidence: the root gate's toolchain probe and the claims synchronizer both
resolve the pinned Rust 1.99.1 toolchain under the constrained PATH.

## 2. Ollama never auto-pulls

`AgentConfig::preflight_check` asks `ollama list` before an `ollama run <tag>`
is spawned and refuses a missing tag. The streamed path had no such gate — it
spawned `ollama run` directly — so `src/agent/streaming.rs` now runs the same
preflight before the spawn. Evidence:

- `src/agent/streaming.rs::tests::streamed_turn_refuses_to_pull_a_missing_local_model`
  — the stub `ollama` records that `list` was called and that `run` never was.
- `tests/ollama_no_pull.rs` — three process-level tests: a plain `print`, a
  prompted `print`, and a positive control where the model *is* present locally
  and the turn runs.

The streamed path was the only unguarded `ollama run` spawn site: the
interactive and one-shot capture paths already called the preflight, and
`passthrough` is only reached for the `cursor`/`grok` account verbs, which
never produce an `ollama run`.

## 3. Desktop Doctor effective-model IPC

The desktop is a client of the daemon, so the model a turn would use is the one
the **runtime** publishes, not one the desktop process guesses from its own
environment. The shared resolution helper stayed the single owner of the model
grammar:

- `src/effective_model.rs` is a public module. `EffectiveModel` carries the
  canonical backend label (`ollama`, `cursor-agent`, `grok`, `fm`, `abi`,
  `claude`) instead of the executor-selection enum, so the contract crosses the
  crate boundary without exposing the executor internals. `EffectiveModel::resolve`
  (label-keyed) and the crate-private typed entry point share one rules
  implementation; an unknown label resolves to `None` rather than to a guess.
- `V4Route` gained a typed `model` field — the exact model id the route's
  provider receives — so no client recovers a model from `route_id` or from the
  display `label`. The route backend is published in Abbey's canonical backend
  vocabulary (`abi`, `fm`), matching the route-id prefix that already used it.
  **Protocol v4 schema moved 1 → 2**: a daemon and a client that disagree on
  the version now fail closed at negotiation instead of one side silently
  ignoring a field the other requires.
- `desktop/src-tauri/src/commands.rs::app_effective_model` resolves the
  runtime's default available route through the shared helper, or an explicitly
  requested backend/model pair; a half-requested pair and an unknown backend
  are refused.
- `desktop/src/views/DoctorView.tsx` renders the answer and says where it came
  from.

### 3a. The selected route, not the first available one

The first draft of this receipt claimed the Doctor reported "the model a turn
would use right now". That was **not** what the code did. The command resolved
the runtime's *first* available route, while the Chat surface
(`desktop/src/conversations/controller.ts`) honours a user-selected route id.
Once a user chose a route in Chat, the Doctor named a model the desktop would
not use. The claim was wrong, and the claim was the evidence.

The fix threads the selected route id through: `App.tsx` passes the
controller's `routeId` to `DoctorView`, and the command resolves *that* route.
A route id the runtime does not publish, or publishes as unavailable, is
refused rather than silently replaced with a different route.

The read is also no longer coupled to write authority. It previously borrowed
the conversation session, which negotiates `WriteConversations` and
`SubmitTurns` alongside the route grant — so a daemon serving conversation
*reads* but withholding write authority would have refused an honest read-only
question. `desktop/src-tauri/src/backend/conversations.rs` now opens a separate
read-only session that requests exactly `ReadRoutes`.

The shared contract now says which authority answered. `EffectiveModel` carries
a `source` discriminator — `local-config` (the CLI and the TUI, which resolve
from the live local configuration) or `runtime-route` (the desktop, which
resolves from the runtime's published route) — and the doctor line names it.
The two authorities resolve the *same* model for the same pair; the source
differs, the answer does not.

### 3b. The desktop README drifted, and no gate caught it

`desktop/README.md`'s hand-written prose had **6** commands with no row in the
"complete Tauri command surface" table: `app_effective_model` from this
cohort, and five that predate it (`app_conversation_content`,
`app_conversation_routes`, `app_conversation_submit_turn`,
`app_conversation_lookup_turn`, `app_conversation_update`). The enumerated
count in the prose said 36 while `generate_handler!` enumerated 37.
`tools/check_claims_sync.py::desktop_inventory` cross-checks `generate_handler!`
against the TypeScript `COMMANDS` array, but nothing compared either against
the hand-written table, so the prose rotted silently.

`desktop_inventory` now also checks the README: the enumerated count must
match, and every enumerated command must have a table row. A missing row or a
stale count fails the gate with the drift named.

Evidence: `src/effective_model.rs` unit tests (label and typed paths agree;
the source discriminator is set correctly on both paths; a route resolution
equals the local resolution of the same pair),
`src/app_core/v4_tests.rs::v4_routes_publish_their_exact_model_id`,
`src/daemon/runtime_handler/tests.rs` (the published route carries `abi`/`local`),
`desktop/src-tauri/src/commands.rs` tests, `desktop/tests/doctor-effective-model.test.tsx`
(SSR card rendering, the selected-route case, and the no-selection case), the
live daemon proof `live_daemon_desktop_effective_model_comes_from_the_runtime_route`
and its selected-route sibling, and
`tools/tests/test_check_claims_sync.py::DesktopReadmeDriftTests`.

## 4. Metal-device evidence for exact-model inference

`tests/daemon_model_lifecycle.rs` starts a real daemon whose startup document
selects the Metal device and then runs the exact fixture:

```
metal branch: native — a real Metal device was compiled in and initialized, so the daemon loaded and executed the exact fixture on Metal
metal branch: output="world again" requested_device=Metal executed_device=Metal native_operations=9 fallback_used=false mixed_execution=false
```

The transcript above is a **captured run**, not a fixed property. The
`native_operations` count depends on the compiled kernel, not on the claim.

In a build without a Metal device the same fixture fails closed instead of
substituting one:

```
metal branch: fail-closed — no Metal device was available in this build or on this host, so the daemon refused to load the model instead of substituting a device
```

The durable store keeps neither the prompt nor the output, and a load operation
id is globally single-use.

### 4a. The cold-device path is now deterministic

The first draft of this receipt described a flaky-by-design test: a cold Metal
device compiles its shader before the run's first budget checkpoint, so the
first attempt could outlast both the daemon's one-second run budget and the
client's five-second read deadline, and the test *failed* if three attempts
did not answer.

The root cause is upstream. On the pinned ABI revision
(`7b9519b6cfb25cc9bc9db2982e0a901318ba039b`), `RunContext::finish` reports
`self.stop.unwrap_or(StopReason::Completed)` — it does **not** consult the
elapsed duration. The duration ceiling is consulted only at *event*
checkpoints, and a cold compile that happens before the daemon's first event
never reaches one. So on the pinned revision the daemon's one-second budget is
not actually a ceiling for a cold attempt; the **client's** read deadline is
the bound that decides it. (An uncommitted change in the sibling working tree
adds a post-hoc elapsed check to `finish()`, but the gate must not depend on
it: CI pins `7b9519b6`.)

The fix is therefore on the client side and is scoped to the Metal branch of
this test: it uses an explicit, longer read deadline
(`METAL_READ_TIMEOUT`) so a first-use shader compile can finish. The daemon's
one-second run budget is unchanged, the fail-closed guarantee is unchanged
(a host with no Metal device still refuses the load *and* the inference, never
substituting a device), and the default cohort is unchanged. The bounded
retry remains for the case where the daemon itself refuses the run, and every
attempt that answers must still carry native Metal evidence with no fallback
and no mixed execution.

The `daemon-protocol-v3-exact-model-inference` claim row records the Metal
evidence and its boundary. The `accelerator-kernel-execution-metal` row points
at its own evidence (`src/accel/tests.rs::with_accel::kernels_match_the_cpu_oracle_when_they_execute_natively`
and the other three accelerator tests). The `accelerator-execution-runtime`
row is **Proposed** and carries no `impl` or `tests` references — it names the
two separately-claimed narrow slices in prose and says that production
weights, general compilation or training, device placement, and non-Apple
accelerators remain unimplemented. Neither of those two rows claims the exact-
model inference evidence; the exact-model row does.

## 5. The claims synchronizer's build budget

`tools/check_claims_sync.py` produces the canonical manifest by running a real
`cargo run --bin abbey -- claims manifest`, so its budget has to cover a
*build* of the crate, not just the manifest emission. `check.sh` builds several
feature cohorts before the synchronizer runs, so the default-feature binary is
usually stale by the time the manifest is requested, and a rebuild of a
~90k-line crate plus its link step regularly exceeds a two-minute budget. A
budget that is too small turns an ordinary rebuild into a spurious gate
failure.

The change raises the default to 900 seconds, adds an
`ABBEY_CLAIMS_SYNC_TIMEOUT` override for a caller that knows it needs longer,
surfaces cargo's own stderr when the build fails (rather than "returned
non-zero exit status"), and refuses a non-JSON manifest with a clear message.
It also resolves the *pinned* toolchain's cargo first — via `rustup which
cargo` — and fails closed through the repository's own
`tools/toolchain_policy.py` when the compiler could be substituted, because the
claims digest is the canonical evidence anchor.

Evidence: `tools/tests/test_check_claims_sync.py::ManifestInvocationTests` —
the default budget, the override, the invalid-budget refusal, the expired-
budget message, cargo's own diagnosis on a failed build, the non-JSON
refusal, the pinned-toolchain preference, and the fail-closed policy check.

## Gate evidence

The gate ran through the repository's own pinned-sibling helper,
`tools/ci/verify-pinned-siblings.sh`, which reproduces the gate CI runs: the
sibling path dependencies are overridden to clean archives of the revisions
`.github/workflows/rust.yml` pins, and nothing in either checkout or in this
repository is written. The helper also documents the formatting trap this
cohort hit: `cargo fmt --all` reaches the sibling path dependencies, so the
gate now formats only Abbey's own package with `cargo fmt --package abbey`.

That trap was measured, not assumed. `cargo fmt --help` documents `--all` as
formatting "all packages, and also their local path-based dependencies", and a
direct instrumented run of `cargo fmt --all -- --check` was observed handing
rustfmt **90** files from `../abi` and `../wdbx` alongside Abbey's own 36. The
sibling's sources are not ours to reformat, and a sibling's in-flight work
must not fail Abbey's gate.

| Gate | Result |
| --- | --- |
| Root gate — default cohort | green (check, rustfmt, clippy `-D warnings`, build, tests) |
| Root gate — `--features wdbx` cohort | green (clippy `-D warnings`, build, tests) |
| Root gate — `--features personal-edition` cohort | green (clippy `-D warnings`, build, tests) |
| Root gate — `--features accel` cohort | green (clippy `-D warnings`, build, tests) |
| Root gate — warning-denied private-item rustdoc, all four modes | green |
| Root gate — claims/docs synchronization, Program 3 boundary, installer smoke, file-size guard | green |
| `python3 tools/check_claims_sync.py` (write + check) | green; 59 claims, schema 2, digest `e990e27c781fb17d774aa2018c73e2915701b6e34058c17f9680bd6df29e4133` |
| `python3 tools/check_instructions.py` | green |
| `python3 -m unittest discover -s tools/tests -p 'test_*.py'` | green; 175 tests |
| `desktop/check.sh` | green — codegen drift, typecheck, bun suites, frontend build, bundle security, default and personal-edition Rust tests, the live scratch-daemon proof, the configured-daemon no-fallback test, and the real linked binary |

### The working tree at the time of writing

47 tracked files changed and 6 untracked files, at approximately +2317 / −304
against `HEAD`. The tree was being edited by other sessions while this cohort
was reviewed, so these counts are a snapshot and not a contract; the
authoritative numbers are the ones `git diff --stat HEAD` reports at the moment
of the landing commit.

### Observed load-induced flakes (not caused by this cohort)

The Metal inference test is the one place where the cohort's own evidence
depends on a device being present and warm. It was run repeatedly in the
`--features accel` cohort and answered natively on every run.

### Sibling note (external, not an Abbey defect)

Both sibling checkouts were being modified by other sessions while this cohort
ran, so the gate ran through the repository's own pinned-sibling helper rather
than against the dirty working trees. The pinned revisions are ABI
`7b9519b6cfb25cc9bc9db2982e0a901318ba039b` and WDBX
`5182bc3a60300e1dd152611025e774c4b906638b`, matching
`.github/workflows/rust.yml` and the `ci-self-hosted-linux-proof` claim row.
Both sibling checkouts were left untouched, and no Abbey source was changed to
accommodate uncommitted sibling work.

The desktop gate hit the same thing for the same reason. The `../wdbx`
working tree was *actively* mid-edit: its new (untracked)
`crates/abi-wdbx/src/v3/episode/store/quota/mod.rs` used `override` as an
identifier, which is a reserved keyword in edition 2024, and its
`v3/offline/outbox.rs` had a non-exhaustive match. The desktop gate therefore
fails in the *sibling* crate before it reaches any Abbey code. Running the
desktop gate against the pinned siblings — the same override mechanism
`tools/ci/verify-pinned-siblings.sh` uses for the root gate — is what
qualifies the desktop half. This is exactly the situation the pinned-sibling
helper exists for, and the reason the gate is run against pinned revisions
rather than against whatever the sibling checkout happens to hold.
