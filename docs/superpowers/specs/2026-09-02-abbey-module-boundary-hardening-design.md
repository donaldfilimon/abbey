# Abbey module-boundary hardening design

Status: Approved on 2026-09-02

## Purpose

This change is a behavior-preserving modularization of the canonical Abbey
checkout. It reduces four oversized source and test modules without changing
Abbey's public contracts, durable data, command-line behavior, or claims.

This record implements the approved ABI, Abbey, and abbey-bot delivery plan. It
does not introduce additional product behavior or design decisions.

## Invariants

- Preserve every existing public module path.
- Preserve every CLI command, flag, alias, default, help string, parser rule,
  and exit behavior.
- Preserve serialized store bytes, schema behavior, precedence, fallback, and
  corruption handling.
- Preserve claim IDs, ordering, text, evidence, and digest.
- Keep the `CLAIMS` literal authoritative and byte-for-byte stable.
- Preserve the existing test inventory and behavior.
- Regenerate the claims ledger only with
  `python3 tools/check_claims_sync.py --write`, after the source refactors are
  complete.
- Use only the canonical checkout on `main`; create no branch or worktree.
- Stage only the exact files owned by each cycle.

## Boundary 1: claims registry types and lifecycle tests

Move `Claim`, `ClaimEvidence`, and `EvidenceState` from
`src/claims/registry.rs` to `src/claims/registry/types.rs`, then re-export them
through the existing registry path. The claim macro, evidence constants, and
the complete `CLAIMS` literal remain in place and unchanged.

Move the lifecycle-heavy test slice from `src/claims/tests.rs` to
`src/claims/tests/lifecycle.rs`. Use the repository's established `include!`
split pattern so the tests remain in the same logical Rust module and keep
their exact fully qualified identities.

## Boundary 2: runtime store codec

Move the row codecs, enum/string projections, identifier parsing, sequence
conversion, and SQLite error adapter from `src/runtime/store.rs` to
`src/runtime/store/codec.rs`. Rebind those helpers privately in the parent
module so existing store child modules retain the same names and behavior.

This boundary does not change SQL, migrations, schemas, byte layout,
transactions, recovery, fallback, or public paths.

## Boundary 3: CLI leaf argument definitions

Move the leaf argument enums and structs from `src/cli.rs` to
`src/cli/args.rs`, then re-export them through `crate::cli`. Keep `Cli`,
`Commands`, `ExecMode`, and the version constant in the front-door module.

All Clap derives, attributes, variant ordering, aliases, range checks, field
visibility, and parser error text remain unchanged.

## Boundary 4: protocol-v3 client tool tests

Move the seven protocol-v3 tool inventory, invocation, pending-request, and
decision tests from `src/daemon/client/v3_tests.rs` to
`src/daemon/client/v3_tool_tests.rs`.

Include the extracted file from the existing `v3_tests` module. That keeps the
test names and claim evidence identities stable while making the physical test
boundary smaller. Production code is untouched.

## Boundary 5: protocol-v3 approval tests

Move the four approval, decision, cancellation, and durable-authorization
tests from `src/daemon/runtime_v3/tests.rs` to
`src/daemon/runtime_v3/tests/approval_tests.rs`.

Include the extracted file from the existing runtime-v3 test module. This
preserves default- and personal-edition test identity, conditional compilation,
and the claims digest. After the extraction, run the canonical claims
generator, inspect its output, and require the checker to pass without a
hand-edited ledger.

## Acceptance

Each boundary is one coherent cycle: baseline, one extraction, focused tests,
and one exact-path commit. The closeout requires all focused suites, the Python
claims-policy suite, the canonical claims generator/checker, `./check.sh`, a
file-size and ledger-consistency review, a complete base-to-head diff review,
normal push to `origin/main`, and exact-head GitHub checks.
