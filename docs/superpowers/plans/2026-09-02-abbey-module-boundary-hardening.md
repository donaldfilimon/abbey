# Abbey module-boundary hardening implementation plan

Status: Approved on 2026-09-02

## Delivery rules

1. Work in the canonical checkout on `main` with no branch or worktree.
2. Before edits, fetch remote metadata and freeze the dirty-path manifest.
3. Run at most five implementation cycles.
4. For each cycle: preserve the baseline, make one extraction, run its focused
   verification, stage exact paths only, inspect the staged diff, and commit.
5. Never use broad staging, reset, stash, force-push, or history rewriting.
6. Before publication, run the full gate, inspect the complete diff, fetch
   again, and require a non-divergent `origin/main`.

## Baseline evidence

- Claims: 33 focused tests; 57 claims; schema 2; digest
  `08172064e491e8a7d4c66724db0fd86288e66956819676a44fcb7f3337471cc0`.
- Store: 49 focused unit tests and one `daemon_runtime` integration test.
- CLI: 13 `cli_surface`, 7 `slash_parse`, and 8 `daemon_cli` tests.
- Client v3: 29 client tests, including the 13-test `v3_tests` inventory.
- Runtime v3: 7 default-edition and 4 personal-edition focused tests.
- Python claims policy: 16 tests.

## Cycle 1: claims boundaries

Commit: `refactor(claims): split registry types and lifecycle tests`

Exact paths:

- `src/claims/registry.rs`
- `src/claims/registry/types.rs`
- `src/claims/tests.rs`
- `src/claims/tests/lifecycle.rs`

Actions:

- Extract `Claim`, `ClaimEvidence`, and `EvidenceState` and re-export them from
  `registry`.
- Extract the lifecycle test slice through `include!` so test identities remain
  unchanged.
- Compare the `CLAIMS` literal bytes and generated digest to the baseline.

Verification:

```sh
cargo fmt --all -- --check
cargo test claims::tests
python3 tools/check_claims_sync.py
```

## Cycle 2: store codec boundary

Commit: `refactor(runtime): extract store codec`

Exact paths:

- `src/runtime/store.rs`
- `src/runtime/store/codec.rs`

Actions:

- Extract row decoding, status/backend encoding, identifier parsing, sequence
  conversion, and SQLite error conversion.
- Preserve the parent-module bindings consumed by store child modules.

Verification:

```sh
cargo fmt --all -- --check
cargo test runtime::store
cargo test --test daemon_runtime
```

## Cycle 3: CLI argument boundary

Commit: `refactor(cli): extract leaf argument definitions`

Exact paths:

- `src/cli.rs`
- `src/cli/args.rs`

Actions:

- Extract `Shell`, `GenerateCmd`, `MemoryCmd`, `MeshCmd`, `DaemonCmd`,
  `DaemonClaimStatus`, `MemoryFilterArgs`, and the private mesh-node parser.
- Re-export all public leaf types through their existing `crate::cli` paths.

Verification:

```sh
cargo fmt --all -- --check
cargo test --test cli_surface
cargo test --test slash_parse
cargo test --test daemon_cli
```

## Cycle 4: v3 client tool-test boundary

Commit: `refactor(daemon): split v3 client tool fixtures`

Exact paths:

- `src/daemon/client/v3_tests.rs`
- `src/daemon/client/v3_tool_tests.rs`

Actions:

- Extract the seven tool-specific tests into the dedicated file.
- Include the file from `v3_tests.rs` to preserve exact logical test names and
  claim evidence identity.
- Keep production behavior and the before/after test inventory unchanged.

Verification:

```sh
cargo fmt --all -- --check
cargo test --lib daemon::client::tests::v3_tests
cargo test --lib daemon::client::tests::v3_cancellation_tests
cargo test --test daemon_cli protocol_v3_safe_tool_inventory_and_audited_status_invocation_round_trip
cargo test --test protocol_v3_contract
```

## Cycle 5: v3 approval boundary and ledger closeout

Commit: `refactor(daemon): split approval tests and refresh claims ledger`

Exact source paths:

- `src/daemon/runtime_v3/tests.rs`
- `src/daemon/runtime_v3/tests/approval_tests.rs`

Actions:

- Extract the four approval-specific tests through `include!` so their logical
  names and claims references remain stable.
- Run the approval inventory in default and personal editions.
- Run `python3 tools/check_claims_sync.py --write`, inspect every generated
  path, then rerun the checker without `--write`.
- The generator must be the only writer of generated claims content; a digest
  change is a failure of the behavior-preserving invariant.

Verification:

```sh
cargo fmt --all -- --check
cargo test --lib daemon::runtime_v3::tests
cargo test --features personal-edition --lib daemon::runtime_v3::tests
cargo test --test daemon_tool_execution
python3 tools/check_claims_sync.py --write
python3 tools/check_claims_sync.py
python3 -B -m unittest tools.tests.test_check_claims_sync
```

## Closeout

Run every focused suite again, full Python claims-policy discovery, and
`./check.sh`. Confirm the `CLAIMS` literal and digest match baseline, review
file sizes and the complete base-to-head diff, fetch `origin`, reconcile only a
clean non-overlapping advance, push normally to `origin main`, resolve the
remote SHA, and wait for exact-head GitHub checks.
