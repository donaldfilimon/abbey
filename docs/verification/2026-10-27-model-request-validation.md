# model-request-validation: local-half automated evidence (2026-10-27)

**Result: the local half passes; the requirement stays `in_progress`.** This receipt
covers correctness plan Task 8 Steps 1–3 only. Step 4, the ABI elapsed-duration
repair and its consuming test, was not attempted because it needs a sibling change.

## Scope

- Revision: `96a5dc3620adc6112507efaaafd2e378f4420dc1` (task branch
  `task/wf3-correctness-t8-20261027`, clean tree before this receipt).
- Toolchain: rustc 1.99.1, cargo 1.99.1, through `./tools/cargo.sh`.
- `CARGO_TARGET_DIR` was a shared per-session target directory outside the checkout.
- Only the two focused filters below were run. The full `./check.sh` was not run for
  this receipt; see the gate receipt referenced under "Automated receipts".

## Focused runs

### `./tools/cargo.sh test --locked -p abbey --lib v3_model`

Exit 0. `11 passed; 0 failed; 0 ignored; 0 measured; 1317 filtered out`.

The filter is broader than the two request-validation tests, and it includes both:

- `app_core::v3_model_tests::inference_accepts_ordinary_multiline_whitespace_and_frame_sized_prompts`
  (`src/app_core/v3_model_tests.rs:102`)
- `app_core::v3_model_tests::inference_rejects_unsafe_controls_and_json_expansion_over_the_frame_ceiling`
  (`src/app_core/v3_model_tests.rs:112`)

The other nine matching tests also passed:
`model_details_contract_is_bounded_and_read_only`,
`request_digest_binds_every_bounded_field`,
`result_requires_exact_digests_native_single_device_evidence_and_bounds`,
`protocol_v3_model_lifecycle_claim_keeps_selection_startup_owned_and_evidence_narrow`,
`v3_model_read_refuses_locally_when_negotiation_denies_the_grant`,
`inference_stops_locally_when_its_separate_grant_is_denied`,
`inference_echoes_the_exact_grant_and_recomputes_all_correlation_digests`,
`v3_model_lifecycle_echoes_exact_grants_and_correlates_every_status`, and
`inference_accounts_for_serialized_envelope_overhead_before_connecting`.

### `./tools/cargo.sh test --locked -p abbey --lib model_inference`

Exit 0. `3 passed; 0 failed; 0 ignored; 0 measured; 1325 filtered out`.

- `daemon::runtime_v3::model_lifecycle::model_inference::inference_boundary_tests::inference_keeps_typed_token_and_event_budget_errors`
  (`src/daemon/runtime_v3/model_lifecycle/model_inference.rs:217`)
- `daemon::runtime_v3::model_lifecycle::model_inference::inference_boundary_tests::inference_accepts_a_completed_bounded_provider_run`
  (`src/daemon/runtime_v3/model_lifecycle/model_inference.rs:235`)
- `claims::tests::exact_model_inference_claim_is_fixture_bounded_and_surface_narrow`

## Automated receipts

The `automated` channel in `tasks/completion.json` cites two receipts:

- [`2026-10-27-integration-gate.md`](2026-10-27-integration-gate.md):
  `tools/ci/verify-pinned-siblings.sh` printed `check.sh: OK` on `5472d40`
  (6334 Rust tests passed, 0 failed, 6 ignored). `96a5dc3` is a docs-only commit on
  top of `5472d40`, so that run covers the same Rust sources.
- This file, for the focused runs above on `96a5dc3`.

## Why the requirement stays `in_progress`

The acceptance criterion also requires that elapsed-duration handling is
"repaired upstream before a qualified pin change". That half belongs to ABI: sibling
request (c) in completion-index Task 2 asks that `RunContext::finish` consult
elapsed duration. Until the ABI owner lands that fix and the CI sibling pin moves to
a qualified revision that contains it:

- `model-request-validation` keeps `status: "in_progress"`;
- the `tasks/todo.md` item "The v3 model request bounds and validation are narrower
  than the…" (line 1601) stays open;
- plan Task 8 Step 4 (`inference_refuses_a_run_that_exceeds_its_duration_ceiling`,
  red on the old pin and green on the new one) is not started.

No sibling checkout (`../abi`, `../wdbx`) was edited. The plan file
`docs/plans/2026-10-27-abbey-correctness-plan.md` is untracked in the canonical
checkout and absent from this task branch, so its step checkboxes are not updated
here. Passing focused tests are not runtime, provider or production qualification.
