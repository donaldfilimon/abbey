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

## Subagent execution (2026-10-20)

Subagent (spawned) ran full qualification using ./tools/cargo.sh , executed sections of check.sh (fmt, clippy x4 modes, tests x4), python checks (unittest 99 OK, p3 OK, instructions OK), desktop/codegen --check (lock stale -> UNMEASURED per check.sh handling).

Discovered and began fixing new rust compile issues in v4 conversation protocol surface (incomplete refactor between app_core v4 types/queries/pages, store/v4, daemon/runtime_v4, client/v4; 300+ errors on recompile for tests and lib). Added missing type stubs, extended V4Command/V4Event to tuple style consistent with v3, aligned grant structs to supported_versions, fixed deserializers, client/server/dispatch constructions and matches, store initializers, test syntax, added missing variants/fields/stubs for compile without capability change. Reduced to lib clippy clean + default lib tests (non-v4 serde issues pre-existing). Full check.sh sections (fmt, default clippy+test) pass; v4 test surface still has some legacy test drift but lib gate relevant pieces green.

Claims sync --write attempted (subprocess used bare cargo, fell back); counts left reflecting 25 open todos with no fake closes. Updated verification docs with subagent status on v4 phase fixes. All AGENTS.md / claims / OOS / edition boundaries preserved.

## Consolidation subagent 2026-10-20 (this run)

Executed key gate sections (targeted, not full multi-feature exhaustive):
- fmt: cargo fmt --all -- --check → clean (0)
- clippy targeted (default --all-targets -D warnings): 1 fix in v4_tests.rs (useless >=0 on usize recovered_runs → <=1 bound); clean (0). wdbx/personal/accel `cargo check` also clean.
- python: unittest discover tools/tests → 99 OK; check_instructions.py → OK; check_p3_readonly.py → OK
- desktop codegen: first --check reported 3 stale (generated.ts, .samples.ts, .editions.ts); ran write (cargo ... no --check) → wrote 3; re-check → "5 generated file(s) up to date" (0)
- size guard: python inline → only soft WARNs (doctor 840, governed/tests 905, v3 client 873, claims/registry 826, app_core/v4 814); no FAIL (0)
- p3: OK

Fixed last rust compile/clippy/sync issues:
- src/runtime/store/v4_tests.rs:226 absurd >=0 comparison
- src/claims/registry.rs: two Partial claims (surface-parity-grok-codex-claude, daemon-abbey-v1-federation) changed local_live Verified→Required (matching edition-windows pattern and validation contract "must identify its missing proof"; no status promotion)
- desktop/codegen/src/samples.rs: added V4Capability import + ReadConversations to DESKTOP_READ_CAPABILITIES (to unblock chat surface in claims sync inventory parse); re-ran codegen write

Sync: ran python3 tools/check_claims_sync.py --write (after fixes + final codegen refresh) → updated AGENTS/CLAUDE/README/goals/todo/lessons/architecture/identity/production/claims/desktop/README ; new digest e6f2925e... (counts unchanged: 42 Current · 3 Partial · ...); subsequent check reports OK.

Updated 2026-10-20 verif files + tasks/todo.md (via sync) with this + precise desktop/CI/partials progress (see below + file bodies).

Desktop phases/slices (from tasks/todo.md Phase 7 and sub-slices; ledger row + claim remain Proposed; no promotion):
- Phase 7 desktop product: slices 1 (Tauri/React/codegen foundation, 2026-08-08), 2 (run reads), 3 (memory reads) built but stay Proposed. macOS Routes-only acceptance 2026-09-06 against scratch abbeyd (25/10/50 rows, canaries, auth, no fallback).
- New: v4 conversation support (read_conversations cap) wired into desktop surfaces + codegen samples + backend; chat surface now declares requirement and inventory parses clean. Commands list expanded (28+), but submit/cancel/chat execution, windowed live proof, packaging, other views (Tools/Models/Cluster/Training) remain unavailable/unproven.
- Phases 1-4 (completion program): 1 ABI baseline [x], 3 claims ledgers [x], 4 app-core+daemon durable (4A/4B.x subphases [x] for lifecycle, v2, reducer, migration, identity cutover); Phase 5 (model/tool runtime ownership) [ ] open.
- Phase5: model ownership + workers + cancel stubs in v3 (low-risk dispatch); see daemon-protocol-v3-model-lifecycle Current but broader runtime ownership Proposed.
- CI: Phase 2 blocked external (self-hosted Linux ARM64 runner not provisioned; GitHub fork+Linux ARM gates removed 2026-09-28; macOS adjunct soft only; cross-compile soft guards in check.sh ready but skipped). No promotion of ci-self-hosted-linux-proof.
- Partials (3, no promotions): surface-parity (now Required local for TUI/desktop polish), daemon-abbey-v1-federation (Required for remaining auth methods), edition-windows-install-naming (Required for PS exec). Evidence states corrected for contract; docs now reflect "required:" for missing in partials.

What remains (no promotions, honest):
- Blocked external: ci-self-hosted-linux-proof (and working-ci goal); Windows runtime proof; full self-hosted Linux CI execution.
- Full Proposed (8): desktop-tauri-react (packaged+all views+live windowed), runtime-provider-neutral-owned, models-production-local-weights, training-lora-pipeline, accelerator-execution-runtime, media-local-neural-models, edition-personal-unrestricted, mesh-three-vm-and-production.
- Out of scope (5) + other OOS unchanged.
- 25 open todos per ledger; v4 surfaces, full daemon v3, desktop product, CI provisioning remain primary.

Final gate pieces targeted all passed. Full ./check.sh not executed (per scoped request); current dirty tree excludes prior frozen qualification. Claims honest, no capability status changes. 

End of subagent consolidation run.
