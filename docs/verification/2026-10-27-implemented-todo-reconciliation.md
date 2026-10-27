# Implemented correctness rows — re-verification on main — 2026-10-27

Status: evidence only (correctness plan Task 13,
`docs/plans/2026-10-27-abbey-correctness-plan.md`). No source, ledger, claim or
todo was changed. No row status was changed and no todo was ticked; Task 14
does that, and any gap below needs Donald's decision before it becomes a new
acceptance bullet.

This is **focused-test evidence on HEAD, not a gate receipt**. The main-sync
plan's Task 6 root gate has not been recorded yet. `./check.sh`,
`desktop/check.sh` and `tools/ci/verify-pinned-siblings.sh` were not run (the
machine was overloaded, and the CI sibling pins are known-broken on main and
are being fixed separately).

## Provenance

| Item | Value |
| --- | --- |
| Abbey HEAD (`git rev-parse HEAD`) | `0ad2fef80f0c1ec38dd1367882541e6a2cdec661` |
| `../abi` HEAD | `dff096415715462370a18a9ad71e5eaf098197da` |
| `../wdbx` HEAD | `93dbeab8987a06007430bf5ab17573ec5a80dc3e` |
| Checkout | Fresh task worktree at HEAD (`dev-worktree … wf-correctness-t13-20261027 main`), clean before this file. It holds committed HEAD only, so the uncommitted edits in the canonical checkout (`CLAUDE.md`, `Cargo.lock`, `src/tui/predict*.rs`, `src/tui/tests/prediction_producer.rs`) are **not** part of what was tested |
| Toolchain | `./tools/cargo.sh` → cargo 1.99.1 (pinned) |
| Build directory | A shared per-session Cargo target directory outside the checkout |
| Host / load | macOS (Darwin) arm64; 1-minute load average about 70 at the start, about 14 at the end |
| Date | 2026-10-27 |

Each Rust command was run as written below. `-p abbey --lib` or `--test <file>`
narrows the run to the binary that holds the test; the filter is otherwise the
plain test name. Every filter's `N passed` equals the number of tests the
row names for it, so no filter matched 0 tests and none matched an unintended
extra test. Every command exited 0 with `0 failed`.

## Results table

| Row | Tests (how the id was found) | Result on HEAD | Acceptance verdict |
| --- | --- | --- | --- |
| `wire-vocabularies` | 4 named ids in `completion.json` | 4/4 pass | **met** |
| `evidence-channels` | `src/claims/tests/evidence_channels.rs` (all) + `tools/tests/test_check_claims_sync.py` (named in `completion.json`) | 4 + 38 pass | **met** |
| `provider-model-switch` | Refs are prose/logs only; ids found by grep from the wave-1 prose ("every ordered provider pair", "failed-persistence identity preservation") → `src/tui/tests/providers.rs:259/281/317`. These names do not appear in `completion.json` | 3/3 pass | **met** |
| `prompt-budget` | Refs are prose/logs plus `src/argv_budget.rs`; the `argv_budget::tests` ids match `docs/verification/2026-10-22-completion-wave-1/final-budget.log`; process and builder ids found by grep | 5 + 4 pass | **gap**: the Unicode fixture does not cross real process validation |
| `deferred-readiness` | `tests/ollama_no_pull.rs` (4 ids, lifted from `ollama-green.log`, also listed at `src/claims/registry.rs:58`) + registry-listed `backend.rs`/`streaming.rs`/`cli_surface.rs` ids | 4 + 4 pass | **gap**: no executed test pins "local-only commands never spawn a readiness probe" |
| `voice-claim-boundary` | 1 named id + supplementary dispatch test `src/voice.rs:676` + `check_claims_sync.py` | 1 + 1 pass; sync OK | **met** on this macOS host. The non-macOS branch was inspected in source only |
| `corpus-golden-vectors` | `src/abbey_contracts/golden_tests.rs` (all) | 4/4 pass | **met** |
| `synthetic-guild-correctness` | Refs are logs only; ids lifted from the `test …` lines in `guild-green.log` (3 integration files) | 5 + 11 + 10 pass | **met** |
| `accelerator-install-isolation` | `tools/tests/test_install_accel_editions.py` | 1/1 pass | **met for edition isolation and staged-failure byte preservation**; the published-stage rollback was **not re-verified on HEAD** |

## Commands, exit codes and counts

| Row | Command | Exit | Count |
| --- | --- | --- | --- |
| wire-vocabularies | `./tools/cargo.sh test --locked -p abbey --lib completion_legacy_v1_claim_schema_is_rejected` | 0 | 1 passed |
| wire-vocabularies | `./tools/cargo.sh test --locked -p abbey --lib completion_legacy_v3_claim_schema_is_rejected_without_retry` | 0 | 1 passed |
| wire-vocabularies | `./tools/cargo.sh test --locked -p abbey --lib completion_legacy_v3_schema_never_reaches_authority_dispatch` | 0 | 1 passed |
| wire-vocabularies | `./tools/cargo.sh test --locked -p abbey --lib completion_legacy_v1_envelope_never_reaches_claim_dispatch` | 0 | 1 passed |
| wire-vocabularies (supplementary) | `./tools/cargo.sh test --locked -p abbey --lib completion_unaffected_v2_schema_three_still_round_trips` | 0 | 1 passed |
| wire-vocabularies (supplementary) | `./tools/cargo.sh test --locked -p abbey --lib completion_current_v1_schema_and_unchanged_v2_dispatch` | 0 | 1 passed |
| wire-vocabularies / voice (supplementary) | `./tools/cargo.sh run --locked -p abbey-desktop-codegen -- --check` | 0 | "5 generated file(s) up to date" |
| evidence-channels | `./tools/cargo.sh test --locked -p abbey --lib claims::tests::evidence_channels` | 0 | 4 passed |
| evidence-channels | `python3 -m unittest tools.tests.test_check_claims_sync -v` | 0 | Ran 38 tests, OK |
| evidence-channels / voice / wire (supplementary) | `python3 tools/check_claims_sync.py` | 0 | `claims/docs: OK (60 claims, schema 3, sha256 688b6a53…a7ea3d2bf91a1b9788fb5a2cbe6b2b33e1a838d2ba6f7a7b)` |
| voice-claim-boundary | `./tools/cargo.sh test --locked -p abbey --lib the_portable_host_claim_states_the_voice_dispatch_boundary` | 0 | 1 passed |
| voice-claim-boundary (supplementary) | `./tools/cargo.sh test --locked -p abbey --lib the_macos_voice_path_never_falls_back_to_a_portable_adapter` | 0 | 1 passed |
| corpus-golden-vectors | `./tools/cargo.sh test --locked -p abbey --lib abbey_contracts::golden_tests` | 0 | 4 passed |
| accelerator-install-isolation | `python3 -m unittest tools.tests.test_install_accel_editions -v` | 0 | Ran 1 test, OK |
| provider-model-switch | `./tools/cargo.sh test --locked -p abbey --lib same_provider_selection_preserves_the_explicit_model` | 0 | 1 passed |
| provider-model-switch | `./tools/cargo.sh test --locked -p abbey --lib every_cross_provider_transition_resets_the_model_and_explicit_selection` | 0 | 1 passed |
| provider-model-switch | `./tools/cargo.sh test --locked -p abbey --lib failed_transition_model_save_keeps_provider_model_and_identity_unchanged` | 0 | 1 passed |
| prompt-budget | `./tools/cargo.sh test --locked -p abbey --lib argv_budget::tests` | 0 | 5 passed |
| prompt-budget | `./tools/cargo.sh test --locked -p abbey --lib every_backend_counts_fixed_arguments_in_the_aggregate_prompt_budget` | 0 | 1 passed |
| prompt-budget | `./tools/cargo.sh test --locked -p abbey --lib clamped_multi_argument_prompt_passes_the_actual_capture_supervisor` | 0 | 1 passed |
| prompt-budget | `./tools/cargo.sh test --locked -p abbey --lib advertised_prompt_ceiling_fits_the_supervised_argument_limit` | 0 | 1 passed |
| prompt-budget | `./tools/cargo.sh test --locked -p abbey --lib argv_environment_path_and_limit_bounds_are_validated` | 0 | 1 passed |
| deferred-readiness | `./tools/cargo.sh test --locked -p abbey --test ollama_no_pull` | 0 | 4 passed |
| deferred-readiness | `./tools/cargo.sh test --locked -p abbey --lib ollama_probe_checks_the_exact_requested_tag` | 0 | 1 passed |
| deferred-readiness | `./tools/cargo.sh test --locked -p abbey --lib default_backend_prefers_ollama_but_never_requires_cursor` | 0 | 1 passed |
| deferred-readiness | `./tools/cargo.sh test --locked -p abbey --lib streamed_turn_refuses_to_pull_a_missing_local_model` | 0 | 1 passed |
| deferred-readiness | `./tools/cargo.sh test --locked -p abbey --test cli_surface local_verbs_need_no_executor_backend` | 0 | 1 passed |
| synthetic-guild-correctness | `./tools/cargo.sh test --locked -p abbey --test guild_intelligence_regressions` | 0 | 5 passed |
| synthetic-guild-correctness | `./tools/cargo.sh test --locked -p abbey --test guild_intelligence_permissions` | 0 | 11 passed |
| synthetic-guild-correctness | `./tools/cargo.sh test --locked -p abbey --test guild_intelligence_replay` | 0 | 10 passed |

The test counts match the 2026-10-22 focused logs: `ollama-green.log` 4,
`guild-green.log` 5/11/10, and `final-budget.log` 5.

## Acceptance comparison per row

### `wire-vocabularies` — met

> "Bump affected v1/v3 schemas, reject old clients before changed claim/model vocabularies, update canonical claim boundaries and generated wire samples"

- Bump: `src/app_core/contracts.rs:13` `APP_SCHEMA_V1 = 2`, `:17`
  `APP_SCHEMA_VERSION = 3` (v2 unchanged), `src/app_core/v3.rs:18`
  `APP_SCHEMA_V3 = 4`, enforced at `v3.rs:164`.
- Rejection before dispatch, server side:
  `src/daemon/server/schema_tests.rs:34` sends v3 schemas 0/1/2/3/`u16::MAX`
  and asserts `UnsupportedVersion` plus a handler call count of 0. `:65` sends v1
  `Status` and `Claims` with schema missing/1/3/`u16::MAX` and asserts
  `unsupported_version` plus a call count of 0.
- Rejection on the client side: `src/daemon/client/tests.rs:658` (a v1 status
  reporting schema 1 → `InvalidRuntimeStatus`) and
  `src/daemon/client/v3_tests.rs:431` (v3 negotiation at schema 3 → error).
  The fake server accepts exactly one connection (`client/tests.rs:579`), so
  the test proves the refusal but cannot tell a retry apart from a refusal.
  The acceptance text does not require no-retry.
- Positive paths are kept: `client/tests.rs:680` (v2/schema 3 round-trips)
  and `schema_tests.rs:87` (v1/schema 2 and literal v2 dispatch).
- Canonical claim boundaries and generated samples: `check_claims_sync.py` OK
  and `abbey-desktop-codegen --check` reports its 5 generated files up to date.
  This shows the generated TypeScript wire samples match canonical Rust on HEAD.

### `evidence-channels` — met

> "Channel-specific failed/expired evidence preserves independently verified local channels; registry schema and sync tests pin the representation"

- `src/claims/tests/evidence_channels.rs:15` and `:30`: a failed or expired
  external channel validates, keeps `local_live` as `verified`, and serializes
  as `{"state":"failed","refs":[…]}`/`expired`. `:45`: `Current` cannot hide
  failed or expired evidence, while `Partial` can carry it. `:59`: references
  must be non-empty and unique.
- Sync side: `tools/tests/test_check_claims_sync.py:207` (an old manifest
  schema is rejected before the new channel states), `:213`, `:224` and `:234`
  (exact state shapes).

### `provider-model-switch` — met

> "Every cross-provider transition selects/persists a valid default, same-provider selection preserves explicit model, and failed persistence leaves identity unchanged"

- `src/tui/tests/providers.rs:281` iterates every ordered pair in `PROVIDERS`.
  It asserts the backend, `system` for FM or `auto` otherwise, a cleared
  `model_selected`, the persisted model file contents, and the transcript
  directory.
- `:259`: same-provider selection (FM `pcc`, Ollama `custom:tag`) keeps both
  the in-memory model and the persisted model.
- `:317`: a failed model-file replacement leaves the backend, model, agent
  path and transcript directory unchanged and reports "provider unchanged".
  It covers 4 transitions, not every pair. The acceptance does not require
  every pair for the failure clause.

### `prompt-budget` — gap

> "Prompt/Doctor ceiling matches 32 KiB argument and 64 KiB aggregate bounds including fixed arguments; Unicode and multi-argument fixtures cross real process validation"

- Met: `src/argv_budget.rs:7-8` sets the 32 KiB and 64 KiB ceilings, and the
  supervisor imports them (`src/runtime/supervisor.rs:5-7`). The Doctor ceiling
  `src/host.rs:14-15` is the same function, pinned at `host.rs:279`. Fixed
  arguments consume the aggregate (`argv_budget.rs:71`; all six backends at
  `src/agent/argv/tests.rs:591`), and the supervisor rejects oversized argv
  (`src/runtime/supervisor/tests.rs:336`).
- **Gap:** the only fixture that crosses real process validation is
  `src/agent/argv/tests.rs:625`
  (`clamped_multi_argument_prompt_passes_the_actual_capture_supervisor`), and
  it uses ASCII (`"x".repeat(96 * 1024)` × 3). The Unicode fixtures
  (`"界".repeat(40_000)` at `argv/tests.rs:603` and `argv_budget.rs:75`) stop
  at `build_args`/`append_prompt_args` and never reach `capture_command` or
  the supervisor. Multi-argument crosses the supervisor; Unicode does not.

### `deferred-readiness` — gap

> "Local-only commands never spawn a readiness probe; generation auto-selection probes the effective requested tag and never downloads"

- Met (generation half): `tests/ollama_no_pull.rs:129` (automatic selection
  with `--model requested:latest` probes exactly once, so the list marker is
  `x`, and runs). `:146` and `:168` (a missing tag is refused with
  `no-auto-pull` after `ollama list`). `:190` (a present tag runs).
  `src/agent/backend.rs:626` (the probe checks the exact requested tag).
  `src/agent/streaming.rs:437` (the streamed turn refuses to pull).
- **Gap (local-only half):** no executed test on HEAD pins "local-only
  commands never spawn a readiness probe". `tests/cli_surface.rs:600`
  (`local_verbs_need_no_executor_backend`) runs `claims`/`allowlist`/`model`/
  `doctor`/`edition` with an **empty PATH**, so a probe would be skipped
  because Ollama is absent, not because the verb is local.
  `tests/effective_model.rs` and `tests/config_report.rs` also use an empty
  PATH and no stub. `ABBEY_TEST_OLLAMA_LIST_MARKER` is used only by
  `tests/ollama_no_pull.rs`, and all four of its tests run `print`.
  `backend.rs:550` only proves that an installed Ollama does not trigger the FM
  readiness probe during default selection. A discriminating test would stage
  the `ollama` stub, run a local verb, and assert that no list marker exists.

### `voice-claim-boundary` — met (macOS host; non-macOS branch by source inspection)

> "Revalidate corrected registry and generated docs against dispatch: portable adapters are non-macOS only; macOS failure has no portable fallback"

- Registry: `src/claims/tests.rs:563` pins the note text: "voice dispatch is
  per host", "does not cross over", "no portable fallback", "dispatched only
  on non-macOS hosts", "is refused", and that the old overstatement is absent.
  The text is at `src/claims/registry.rs:427`. This test checks text only.
- Generated docs: `check_claims_sync.py` OK. The boundary text appears in
  the generated table at `docs/identity.md:38`, a `table`-mode target of
  `tools/check_claims_sync.py:30`. The `docs/claims.md` row shows no note
  column, so it does not carry the text.
- Dispatch: `src/voice.rs:676`
  (`the_macos_voice_path_never_falls_back_to_a_portable_adapter`) passed on
  this macOS host. It returns early off macOS. The portable-only-on-non-macOS
  branch is source-inspected at `voice.rs:133`, `:229-233` and `:372`, and was
  not executed here.
- Observation for Task 14: the registry's `tests:` list for
  `portable-primary-host-surfaces` (shown in `docs/claims.md:39`) names
  neither the boundary test nor `voice.rs:676`.

### `corpus-golden-vectors` — met

> "Compute and independently compare canonical bytes and expected corpus digests; altered expected values cannot return Valid"

- `src/abbey_contracts/golden_tests.rs:16` computes the JCS bytes and checks
  them against the vendored ABI `expected_hex` (`Valid`), and an altered value
  is not `Valid`. `:24` does the same for the aggregate `expected_digest`.
  `:32`: path order is normalized and duplicates are rejected. `:41`:
  UTF-16 key order and `-0` normalization are checked against a literal
  written independently of the canonicalizer.

### `synthetic-guild-correctness` — met

> "No empty advertised alternatives, overwrite absence preserved in rollback, visibility digest binds bot authority, JSON is bounded before allocation, cyclic parent graphs refused"

- `tests/guild_intelligence_regressions.rs:16`: only `do-nothing` is offered
  when no permission plan changes anything, and other selections give
  `UnknownSelection`. `:65`: `present` is kept in preconditions and the
  rollback preview (schema 2). `:85`: the evidence digest changes with bot
  role authority even when the effective bits do not. `:108`: 4 MiB is
  accepted and 4 MiB + 1 is refused with "byte limit". `:119`: self and
  two-node parent cycles are refused, and acyclic chains are accepted.
- The bound is checked before allocation in source:
  `src/app_core/guild_intelligence.rs:63` checks the length before
  `serde_json::from_str` at `:66`. The test pins the boundary, not the order.

### `accelerator-install-isolation` — met for isolation and staged failure; published-stage rollback not re-verified on HEAD

> "Safe/personal installed accelerator layouts cannot replace each other; staged/published verification and rollback fixtures preserve prior bytes"

- `tools/tests/test_install_accel_editions.py`, in both install orders:
  each edition owns a distinct dylib, and installing the other edition leaves
  its bytes unchanged. A forced `codesign` failure (a staged verification
  failure) exits non-zero, keeps every earlier file's bytes, and leaves no
  `.abbey-*` staging or partial personal files.
- Not re-verified on HEAD: the published-stage relocation rollback
  (`ABBEY_FAIL_RELOCATION`, backup comparison) and the native staged-dylib
  failure are in `tools/tests/smoke_accel_install.sh`. That script is a
  `check.sh` step (`check.sh:136`) that runs a real `--features accel` release
  install. It was not run here because of the load constraint. This half
  rests on the prior receipt
  (`docs/verification/2026-10-22-completion-review-fixes/installer-root.log`).

## Hand-off to Task 14

- Tests pass and acceptance is met: `wire-vocabularies`, `evidence-channels`,
  `provider-model-switch`, `corpus-golden-vectors`,
  `synthetic-guild-correctness`, `voice-claim-boundary` (macOS host).
- Tests pass, with a recorded acceptance gap that needs Donald's decision:
  `prompt-budget` (Unicode does not cross the supervisor) and
  `deferred-readiness` (no discriminating local-verb no-probe test).
- Tests pass, with part of the evidence only in a prior receipt:
  `accelerator-install-isolation` (the published-stage rollback smoke was
  not re-run on HEAD).
- Closing any row also needs the main-sync Task 6 root gate receipt on the
  same revision. This file does not provide one.
