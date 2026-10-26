# Vendor compile-test harness maintenance

Own only vendor dev manifests, original test harness imports, vendor lock, narrow
provenance verifier/source inventories/tests and this report. Base80187be is the
reviewed Abbey cohort; root lock, workspace, runtime and Studio remain untouched.

Replace dev-only trybuild2/basic-toml with maintained trybuild=1.0.122. User has
explicitly allowed public dependency downloads; compute stays local. Keep all
upstream compile-pass/fail cases, original supplied inline diagnostic expectations
and archived snapshot bytes. Maintained trybuild lacks inline APIs, so stage the
same inline sources as file cases with exact source comparisons and reviewed
strict stderr snapshots. Never set TRYBUILD=overwrite or accept generated stderr
without comparing each case to its retained upstream expectation. Preserve all
production macro/iterator fixes, proc-macro-error3=3.1.1 and Syn2 feature pins.

Add regression controls that reject retired harness packages and substitution.
Observe RED before implementing the verifier and migration. Reconstruct pristine
archives plus the enumerated patch set and compare complete file inventories.
Update independent review pins only for that reviewed transition; mutation tests
must still reject self-blessed drift. Run vendor metadata, strict security audit,
upstream clone and subclass compile tests, existing diagnostics/runtime controls
and provenance/mutation tests using tools/cargo.sh with Rust1.99.1, jobs1 and an
isolated target. No root gate can be claimed from vendor-only evidence; mandatory
adjacent root dependencies and other modernization work remain owner-controlled.
Independent final review precedes handing the commit to that owner for integration.

Resource floor10GiB. Do not start a second heavy graph while Bot qualification runs.
