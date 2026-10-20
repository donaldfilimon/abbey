# Abbey active-work consolidation — 2026-10-20

The frozen exact-pin candidate passed root `./check.sh` (exit 0: 3,831 Rust executions, 98 Python tests) and separate `./desktop/check.sh` (exit 0: 49 Rust executions, 18 Python tests). This is local qualification; hosted CI was not run.

## Exact candidate

- Abbey base: 2ee9f7964428314c558bd43d70170eaecddb73c5.
- Local delta: `.github/workflows/rust.yml` and `tools/tests/test_workflow_guards.py` advance/assert ABI 7b9519b6cfb25cc9bc9db2982e0a901318ba039b.
- ABI: exact Git export 7b9519b6cfb25cc9bc9db2982e0a901318ba039b.
- WDBX: exact Git export 5182bc3a60300e1dd152611025e774c4b906638b.
- Rust 1.99.1. Root and desktop locks were not edited. The integrated refresh changes 66 package version sets relative to 3014434.

Candidate input bytes and modes were recorded and compared after the gates; no recorded source changed. The before manifest was captured shortly after the exact-pin root gate started, during its initial compilation, and before desktop gate start. Initial sibling copies were checked against their pre-copy manifest and retained separately.

## Repair and review

Independent review found that the committed CI ABI pin 6322c0f lacked DurableLifecycle::refuse_replay, now implemented by Abbey. The selected immutable ABI revision contains the trait and host change and pins Rust 1.99.1. The workflow guard failed with the expected stale-pin assertion (23 tests, one failure), then passed all 23 after the workflow repair. Independent follow-up review accepted the narrow repair without additional findings. WDBX pin and checkout security controls are unchanged.

## Failed and limited evidence

An earlier full root attempt against initially captured current siblings failed in personal-edition owned_metadata_overflow_reports_its_effective_cap_in_error_and_notice: cancel token was unset. An unchanged focused rerun passed. The later complete exact-pin gate passed, including this test. The first failure remains unexplained; no cancellation source or fixture assertion was weakened. Initial-sibling desktop passed separately and is not substituted for exact-pin desktop evidence.

The final root gate includes isolated loader-relative installation and rollback checks. This receipt makes no separate native accelerator, performance, production-model, Windows runtime or packaged desktop-product claim. Windows GNU and Linux GNU cross-checks were skipped because targets were not installed. Existing soft size warnings remain visible in the root log.

## Preservation and integration boundary

No commits, pushes, hosted CI, deployment, branch deletion, staging, resets or reverts were performed by this lane. Protected source files matched their initial hashes when checked. Other lanes modified canonical runtime/config/route/host tests after capture; those changes and current dirty sibling work are excluded. Current main already contains the earlier integration; it was not recreated. The two-file pin repair, plan and this receipt remain local for review.

Evidence directory: /Users/donald/.codex/verification/abbey-active-work-consolidation-20261020/ — initial-source.json, exact-ci-revisions.json, exact-ci-source-before.json, exact-ci-result.json, dependency-changes.json, protected-source-check.json, review.md, workflow-red.log, workflow-green.log, root-gate.log, metadata-focused.log, exact-ci-root-gate.log, exact-ci-desktop-gate.log.
