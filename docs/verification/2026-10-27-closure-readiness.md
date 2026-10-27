# Abbey closure readiness inventory (Phase 11 dry run) — 2026-10-27

Status: first snapshot for task 13 of the platforms/security/closure plan
(`docs/plans/2026-10-27-abbey-platforms-security-closure-plan.md`). This is a
docs-only inventory. It changes no source, ledger, claim or status, and it promotes
nothing. Repeat it after each lane integrates into local `main`.

The plan files cited below (`docs/plans/2026-10-27-abbey-*.md`) are working drafts.
At this revision they are untracked in the canonical checkout and are not on `main`.
Task numbers refer to those drafts: "PSC Tn" means the platforms/security/closure
plan, and the correctness, authority-tools, desktop, models-media-federation and
completion-index plans are named in full. OB-n rows are the owner blocks in
`docs/completion-plan-2026-10-22.md` §3 (lines 653-671).

## Revision

| Input | Revision | State |
| --- | --- | --- |
| Abbey (`main`, clean task worktree `task/wf-platforms-security-closure-t13-20261027`) | `0ad2fef80f0c1ec38dd1367882541e6a2cdec661` | clean |
| `../abi` | `dff096415715462370a18a9ad71e5eaf098197da` | 2 untracked paths: `tools/package/`, `tools/tests/test_package_install.py` (owned by another session, not touched) |
| `../wdbx` | `93dbeab8987a06007430bf5ab17573ec5a80dc3e` | clean |

Unlanded branches are excluded. Their evidence does not count here until it is merged into local `main`:

| Branch | Head | Commits ahead of `main` |
| --- | --- | --- |
| `task/abbey-authority-20261027` | `1ca3eb3` | 4 |
| `task/abbey-correctness-20261027` | `961bdfe` | 2 |
| `task/abbey-desktop-20261027` | `08c787f` | 1 |
| `abbey/windows-foundation` | `5be3d25` | 1 |

## Ledger tools

Each tool was run from the clean worktree at the revision above:

```text
$ python3 tools/check_completion.py; echo exit=$?
completion coverage: 45 requirements, 9 milestones; coverage only, not completion evidence
exit=0
$ python3 tools/check_claims_sync.py; echo exit=$?
claims/docs: OK (60 claims, schema 3, sha256 688b6a5317304d9ba7ea3d2bf91a1b9788fb5a2cbe6b2b33e1a838d2ba6f7a7b)
exit=0
$ python3 tools/check_instructions.py; echo exit=$?
instructions: OK (AGENTS.md canonical; CLAUDE.md pointer, command table, module map)
exit=0
```

`check_completion.py` reports coverage only. A zero exit there is not completion evidence.

## Method

Row source (read-only):

```sh
python3 -I -c "import json;d=json.load(open('tasks/completion.json'));[print(r['id'],r['status'],ch,(r.get('evidence',{}).get(ch) or {}).get('status','missing')) for r in d['requirements'] for ch in r.get('required_evidence',[])]"
```

Row count check:

```sh
python3 -I -c "import json;d=json.load(open('tasks/completion.json'));print(sum(len(r.get('required_evidence',[])) for r in d['requirements']))"
```

This prints `130`, and the table below has 130 rows.

Status legend. `passed`, `required` and `blocked` are the channel statuses recorded
in `tasks/completion.json`. `required` means refs are recorded but the channel is not
marked passed. `missing` is this inventory's label for a required channel that has no
evidence entry. A `-` in the prerequisite columns means the channel is already `passed`.

Scope: this inventory covers requirement-level `required_evidence` only. Milestone-level
`required_evidence` (for example `platforms` requires `ci`) falls outside this task
and is not listed here.

## Summary

- Requirements: 45 (13 `implemented`, 5 `in_progress`, 27 `open`).
- Required channels: 130 (28 `passed`, 94 `missing`, 4 `required`, 4 `blocked`).
- Ready to close today: none of the 32 non-implemented requirements has all of its required channels `passed`.

## Findings (recorded, nothing changed)

1. **Evidence exists under keys that are not required.** These rows already carry
   `passed` evidence under a channel name that `required_evidence` does not list. The
   required channel stays open until evidence is recorded under its own key:
   - `retained-plan-reconciliation`: `historical_mapping`, `historical_gate` and `frozen_macos_gates` passed; `source`/`automated` missing.
   - `runtime-contracts`: `contract_extraction` passed.
   - `production-model`: `target_metadata` passed.
   - `model-request-validation`: `focused_tests` passed; `automated` missing.
   - `dependency-security-remediation`: `license_inventory` and `secret_directory_triage` passed on the 2026-10-22 recovery candidate, but the required keys are `license_review` and `secret_scan`. PSC T10 step 4 closes them by rerunning on the candidate revision and recording the results under the required keys. This requirement also records `consumer_strict_dependency_audit` and three historical audits as `failed` (not required channels).
2. **`linux-ci` is `implemented` while its `native_ci` channel is `blocked`.** The
   completion index's Review Focus names this pattern (an `implemented` row hiding
   unfinished work). Its open todo is `tasks/todo.md:192`. This inventory changes nothing.
3. **Channels with no external prerequisite.** Repo-side work alone can close these.
   Each one still needs Donald's approval to merge its lane into local `main`:
   `retained-plan-reconciliation`, `identity-recovery-flake`,
   `linux-service-foundation` `source`/`automated`, and
   `dependency-security-remediation` `license_review`/`secret_scan`.

## Requirement × channel table

| # | Requirement | Req. status | Channel | Channel status | External prerequisite | Closing proof |
|---:|---|---|---|---|---|---|
| 1 | `baseline-record` | implemented | `source` | passed | - | - |
| 2 | `baseline-record` | implemented | `automated` | passed | - | - |
| 3 | `requirement-coverage` | implemented | `source` | passed | - | - |
| 4 | `requirement-coverage` | implemented | `automated` | passed | - | - |
| 5 | `retained-plan-reconciliation` | open | `source` | missing | None external. Repo-side work owned by the correctness lane; landing needs Donald's approval to merge that lane into local `main` | correctness plan T15: audit record for each of the four acceptance items, then ledger `source`/`automated` with refs |
| 6 | `retained-plan-reconciliation` | open | `automated` | missing | None external. Repo-side work owned by the correctness lane; landing needs Donald's approval to merge that lane into local `main` | correctness plan T15: audit record for each of the four acceptance items, then ledger `source`/`automated` with refs |
| 7 | `native-controls-proof` | open | `source` | missing | OB-5: macOS runner/host with Accessibility (TCC) permission plus a staging bundle (Donald / runner operator) | desktop plan T2 (wire drivers into client gate), T3 (Donald runs drivers under OB-5), T4 (close); `tasks/todo.md:166` |
| 8 | `native-controls-proof` | open | `automated` | missing | OB-5: macOS runner/host with Accessibility (TCC) permission plus a staging bundle (Donald / runner operator) | desktop plan T2 (wire drivers into client gate), T3 (Donald runs drivers under OB-5), T4 (close); `tasks/todo.md:166` |
| 9 | `macos-ci` | open | `source` | missing | OB-6: registered runner `Abbey-Mac-arm64` offline since 2026-10-19; runner administrator must bring it online and enable `ABBEY_MACOS_ARM64_RUNNER` | PSC T4: exact-head run where `python3 tools/ci/require_executed_run.py <run-id>` exits 0 with the macOS job; `tasks/todo.md:182`, `:199` |
| 10 | `macos-ci` | open | `automated` | missing | OB-6: registered runner `Abbey-Mac-arm64` offline since 2026-10-19; runner administrator must bring it online and enable `ABBEY_MACOS_ARM64_RUNNER` | PSC T4: exact-head run where `python3 tools/ci/require_executed_run.py <run-id>` exits 0 with the macOS job; `tasks/todo.md:182`, `:199` |
| 11 | `macos-ci` | open | `native_ci` | missing | OB-6: registered runner `Abbey-Mac-arm64` offline since 2026-10-19; runner administrator must bring it online and enable `ABBEY_MACOS_ARM64_RUNNER` | PSC T4: exact-head run where `python3 tools/ci/require_executed_run.py <run-id>` exits 0 with the macOS job; `tasks/todo.md:182`, `:199` |
| 12 | `linux-ci` | implemented | `source` | passed | - | - |
| 13 | `linux-ci` | implemented | `automated` | passed | - | - |
| 14 | `linux-ci` | implemented | `native_ci` | blocked | OB-8: Linux ARM64 runner `self-hosted,Linux,ARM64,abbey` registered and `ABBEY_LINUX_ARM64_RUNNER` enabled (repository administrator) | PSC T5: `require_executed_run.py <run-id>` exit 0 with the Linux job; `tasks/todo.md:192` |
| 15 | `windows-foundation` | in_progress | `source` | required | Donald: release of commit `5be3d25` on unlanded branch `abbey/windows-foundation` by its writer, fresh review, and merge approval | PSC T1 (review record), T2 (integration branch + gates + Donald merge approval) |
| 16 | `windows-foundation` | in_progress | `automated` | required | Donald: release of commit `5be3d25` on unlanded branch `abbey/windows-foundation` by its writer, fresh review, and merge approval | PSC T1 (review record), T2 (integration branch + gates + Donald merge approval) |
| 17 | `windows-foundation` | in_progress | `native_ci` | blocked | OB-9 + Windows 11 ARM64 runner `self-hosted,Windows,ARM64,abbey` with `ABBEY_WINDOWS_ARM64_RUNNER` enabled (Donald / runner administrator) | PSC T3: `require_executed_run.py <run-id>` exit 0 with the Windows job; `docs/verification/YYYY-MM-DD-windows-native-ci.md` |
| 18 | `runtime-contracts` | in_progress | `source` | missing | Lane integration: authority lane (unlanded `task/abbey-authority-20261027`) must land via Donald's merge approval; authority plan T2 decision gate (migration 12 or not) | authority plan T2-T10; T10 closes `runtime-contracts`; `tasks/todo.md:220`, `:484` |
| 19 | `runtime-contracts` | in_progress | `automated` | missing | Lane integration: authority lane (unlanded `task/abbey-authority-20261027`) must land via Donald's merge approval; authority plan T2 decision gate (migration 12 or not) | authority plan T2-T10; T10 closes `runtime-contracts`; `tasks/todo.md:220`, `:484` |
| 20 | `owned-tool-host` | open | `source` | missing | Sibling owner `../abi`: ABI Phase 5 tool-runtime APIs (authority plan T15 decision gate; Branch ABI-missing = sibling request + reviewed pin) | authority plan T16-T18 (T18 = real model-produced multi-tool loop); `tasks/todo.md:264`, `:718`, `:1054` |
| 21 | `owned-tool-host` | open | `automated` | missing | Sibling owner `../abi`: ABI Phase 5 tool-runtime APIs (authority plan T15 decision gate; Branch ABI-missing = sibling request + reviewed pin) | authority plan T16-T18 (T18 = real model-produced multi-tool loop); `tasks/todo.md:264`, `:718`, `:1054` |
| 22 | `desktop-product` | open | `source` | missing | `runtime-contracts` landed first (authority T10); OB-1..OB-4 (Developer ID, notary key, updater key, published release) and OB-9 (Windows host) for packaged acceptance | desktop plan T5-T13, T15, T16; `tasks/todo.md:267`, `:935` |
| 23 | `desktop-product` | open | `automated` | missing | `runtime-contracts` landed first (authority T10); OB-1..OB-4 (Developer ID, notary key, updater key, published release) and OB-9 (Windows host) for packaged acceptance | desktop plan T5-T13, T15, T16; `tasks/todo.md:267`, `:935` |
| 24 | `production-model` | open | `source` | missing | OB-12 (ABI Gemma 4 loader, sibling owner `../abi`), OB-13 (licensed Gemma 4 12B weights + licence acceptance), OB-14 (24 GB Apple Silicon host) | models-media-federation plan T8 (inert harness), T9 (Donald/sibling steps); `tasks/todo.md:270`, `:871` |
| 25 | `production-model` | open | `automated` | missing | OB-12 (ABI Gemma 4 loader, sibling owner `../abi`), OB-13 (licensed Gemma 4 12B weights + licence acceptance), OB-14 (24 GB Apple Silicon host) | models-media-federation plan T8 (inert harness), T9 (Donald/sibling steps); `tasks/todo.md:270`, `:871` |
| 26 | `production-model` | open | `license` | missing | OB-13: licensed Gemma 4 12B IT QAT weights + accepted licence (model licence holder) | models-media-federation plan T9: artefact hash + licence in the edition-scoped ledger |
| 27 | `production-model` | open | `native_quality` | missing | OB-12 loader and OB-14 24 GB Apple Silicon host | models-media-federation plan T8 harness run on OB-14 (peak RSS <= 20 GB, `tasks/todo.md:1021-1025`) |
| 28 | `accelerator-runtime` | open | `source` | missing | Apple whole-model path is repo-side (models plan T10, lands on Donald's merge approval); non-Apple half needs OB-17 hardware (hardware operator) and the ABI accelerator owner | models-media-federation plan T10; `tasks/todo.md:273` |
| 29 | `accelerator-runtime` | open | `automated` | missing | Apple whole-model path is repo-side (models plan T10, lands on Donald's merge approval); non-Apple half needs OB-17 hardware (hardware operator) and the ABI accelerator owner | models-media-federation plan T10; `tasks/todo.md:273` |
| 30 | `accelerator-runtime` | open | `native_hardware` | missing | OB-17: non-Apple accelerator host (CUDA/Vulkan/TPU), hardware operator; Apple half needs the macOS host | models-media-federation plan T10 run per claimed device (CPU oracle, no-fallback, executed-device) |
| 31 | `production-adapters` | open | `source` | missing | Donald approval of the trainer decision (models plan T4); OB-15 (ABI trainer, sibling owner) if Branch ABI; OB-13 licensed base; dataset consent owner | models-media-federation plan T4-T6; `tasks/todo.md:300` |
| 32 | `production-adapters` | open | `automated` | missing | Donald approval of the trainer decision (models plan T4); OB-15 (ABI trainer, sibling owner) if Branch ABI; OB-13 licensed base; dataset consent owner | models-media-federation plan T4-T6; `tasks/todo.md:300` |
| 33 | `production-adapters` | open | `consent` | missing | Dataset consent owner (Donald): consented corpus | models-media-federation plan T6 consent record |
| 34 | `production-adapters` | open | `license` | missing | OB-13: licensed base model | models-media-federation plan T6 licence record |
| 35 | `production-adapters` | open | `held_out_quality` | missing | OB-15 trainer (or Branch Native approval, T4) and a real training run (Donald step) | models-media-federation plan T5 harness on T6 artefacts (held-out NLL/tool-call bars) |
| 36 | `neural-media` | open | `source` | missing | OB-16: local neural media weights + licences (model licence holders) | models-media-federation plan T11; `tasks/todo.md:303` |
| 37 | `neural-media` | open | `automated` | missing | OB-16: local neural media weights + licences (model licence holders) | models-media-federation plan T11; `tasks/todo.md:303` |
| 38 | `neural-media` | open | `license` | missing | OB-16: media model licences | models-media-federation plan T11 licence records |
| 39 | `neural-media` | open | `native_quality` | missing | OB-16 weights on a native host | models-media-federation plan T11 offline proof harness on real artefacts |
| 40 | `compute-editions` | open | `source` | missing | Aggregate: needs `personal-edition` (OB-10) and `worker-runtime` (sibling `abi-worker`) closed first | models-media-federation plan T15; `tasks/todo.md:306` |
| 41 | `compute-editions` | open | `automated` | missing | Aggregate: needs `personal-edition` (OB-10) and `worker-runtime` (sibling `abi-worker`) closed first | models-media-federation plan T15; `tasks/todo.md:306` |
| 42 | `consented-capture` | open | `source` | missing | OB-18: Discord guild manager authorization + participant consent, plus a separately reviewed retention/recording contract | models-media-federation plan T17; `tasks/todo.md:330` |
| 43 | `consented-capture` | open | `automated` | missing | OB-18: Discord guild manager authorization + participant consent, plus a separately reviewed retention/recording contract | models-media-federation plan T17; `tasks/todo.md:330` |
| 44 | `consented-capture` | open | `authorization` | missing | OB-18: guild manager authorization | models-media-federation plan T17 reviewed authorization record |
| 45 | `consented-capture` | open | `consent` | missing | OB-18: explicit participant consent | models-media-federation plan T17 reviewed consent record |
| 46 | `live-guild-validation` | open | `source` | missing | OB-18, only after `consented-capture` closes | models-media-federation plan T18; `tasks/todo.md:335` |
| 47 | `live-guild-validation` | open | `automated` | missing | OB-18, only after `consented-capture` closes | models-media-federation plan T18; `tasks/todo.md:335` |
| 48 | `live-guild-validation` | open | `authorization` | missing | OB-18: owner/admin read-only authorization | models-media-federation plan T18 authorization + revocation record |
| 49 | `live-guild-validation` | open | `consent` | missing | OB-18: participant consent | models-media-federation plan T18 consent record |
| 50 | `guild-effects` | open | `source` | missing | OB-18 plus a separate Donald approval for any desired-state effect program | models-media-federation plan T18; `tasks/todo.md:338` |
| 51 | `guild-effects` | open | `automated` | missing | OB-18 plus a separate Donald approval for any desired-state effect program | models-media-federation plan T18; `tasks/todo.md:338` |
| 52 | `guild-effects` | open | `authorization` | missing | OB-18 plus a separate Donald approval of each effect | models-media-federation plan T18 approved effect plan with rollback |
| 53 | `guild-effects` | open | `consent` | missing | OB-18: participant consent | models-media-federation plan T18 consent record |
| 54 | `identity-recovery-flake` | open | `source` | missing | None external. Repo-side correctness lane (unlanded `task/abbey-correctness-20261027`); landing needs Donald's merge approval | correctness plan T2-T4R/T4N; `tasks/todo.md:602` |
| 55 | `identity-recovery-flake` | open | `automated` | missing | None external. Repo-side correctness lane (unlanded `task/abbey-correctness-20261027`); landing needs Donald's merge approval | correctness plan T2-T4R/T4N; `tasks/todo.md:602` |
| 56 | `worker-runtime` | open | `source` | missing | Sibling owner `../abi`: `abi-worker` protocol change consumed through a reviewed pin | models-media-federation plan T13; `tasks/todo.md:879` |
| 57 | `worker-runtime` | open | `automated` | missing | Sibling owner `../abi`: `abi-worker` protocol change consumed through a reviewed pin | models-media-federation plan T13; `tasks/todo.md:879` |
| 58 | `personal-edition` | open | `source` | missing | Donald decision revising the conflicting invariants; OB-10 signed privileged helpers (authority plan T22) | authority plan T19-T22; `tasks/todo.md:882` |
| 59 | `personal-edition` | open | `automated` | missing | Donald decision revising the conflicting invariants; OB-10 signed privileged helpers (authority plan T22) | authority plan T19-T22; `tasks/todo.md:882` |
| 60 | `model-media-roadmap` | open | `source` | missing | Aggregate: `production-model`, `accelerator-runtime`, `production-adapters`, `neural-media` closed first (OB-12..OB-17) | models-media-federation plan T12; `tasks/todo.md:1038` |
| 61 | `model-media-roadmap` | open | `automated` | missing | Aggregate: `production-model`, `accelerator-runtime`, `production-adapters`, `neural-media` closed first (OB-12..OB-17) | models-media-federation plan T12; `tasks/todo.md:1038` |
| 62 | `three-vm-proof` | open | `source` | missing | OB-11: three Ubuntu ARM64 VMs with distinct IPs, state dirs, private CA (VM operator) | models-media-federation plan T14; `tasks/todo.md:1104` |
| 63 | `three-vm-proof` | open | `automated` | missing | OB-11: three Ubuntu ARM64 VMs with distinct IPs, state dirs, private CA (VM operator) | models-media-federation plan T14; `tasks/todo.md:1104` |
| 64 | `three-vm-proof` | open | `three_vm` | missing | OB-11: three Ubuntu ARM64 VMs (VM operator) | models-media-federation plan T14 scenario harness run on OB-11 |
| 65 | `final-closure` | open | `source` | missing | OB-19: independent reviewer with no write access, after every included requirement is complete | PSC T13 (this inventory, repeated per lane), T14 (sibling gates on frozen inputs), T15 (OB-19 review); `tasks/todo.md:1120` |
| 66 | `final-closure` | open | `automated` | missing | OB-19: independent reviewer with no write access, after every included requirement is complete | PSC T13 (this inventory, repeated per lane), T14 (sibling gates on frozen inputs), T15 (OB-19 review); `tasks/todo.md:1120` |
| 67 | `wire-vocabularies` | implemented | `source` | passed | - | - |
| 68 | `wire-vocabularies` | implemented | `automated` | passed | - | - |
| 69 | `evidence-channels` | implemented | `source` | passed | - | - |
| 70 | `evidence-channels` | implemented | `automated` | passed | - | - |
| 71 | `provider-model-switch` | implemented | `source` | passed | - | - |
| 72 | `provider-model-switch` | implemented | `automated` | passed | - | - |
| 73 | `prompt-budget` | implemented | `source` | passed | - | - |
| 74 | `prompt-budget` | implemented | `automated` | passed | - | - |
| 75 | `portable-ollama-probe` | open | `source` | missing | Repo-side correctness lane for the portable path; OB-9 Windows host for native acceptance | correctness plan T5, T6, T7 (Donald, OB-9); `tasks/todo.md:1541` |
| 76 | `portable-ollama-probe` | open | `automated` | missing | Repo-side correctness lane for the portable path; OB-9 Windows host for native acceptance | correctness plan T5, T6, T7 (Donald, OB-9); `tasks/todo.md:1541` |
| 77 | `deferred-readiness` | implemented | `source` | passed | - | - |
| 78 | `deferred-readiness` | implemented | `automated` | passed | - | - |
| 79 | `voice-claim-boundary` | implemented | `source` | passed | - | - |
| 80 | `voice-claim-boundary` | implemented | `automated` | passed | - | - |
| 81 | `corpus-patterns` | open | `source` | missing | Sibling owner `../abi` corpus maintainer: anchored schemas upstream, then a reviewed pin to re-vendor | correctness plan T10 (consuming regression), T11 (re-vendor); `tasks/todo.md:1615` |
| 82 | `corpus-patterns` | open | `automated` | missing | Sibling owner `../abi` corpus maintainer: anchored schemas upstream, then a reviewed pin to re-vendor | correctness plan T10 (consuming regression), T11 (re-vendor); `tasks/todo.md:1615` |
| 83 | `corpus-command` | open | `source` | missing | Sibling owner `../abi` corpus maintainer (verify first; may already be fixed upstream) | correctness plan T9, T11; `tasks/todo.md:1622` |
| 84 | `corpus-command` | open | `automated` | missing | Sibling owner `../abi` corpus maintainer (verify first; may already be fixed upstream) | correctness plan T9, T11; `tasks/todo.md:1622` |
| 85 | `corpus-golden-vectors` | implemented | `source` | passed | - | - |
| 86 | `corpus-golden-vectors` | implemented | `automated` | passed | - | - |
| 87 | `synthetic-guild-correctness` | implemented | `source` | passed | - | - |
| 88 | `synthetic-guild-correctness` | implemented | `automated` | passed | - | - |
| 89 | `accelerator-install-isolation` | implemented | `source` | passed | - | - |
| 90 | `accelerator-install-isolation` | implemented | `automated` | passed | - | - |
| 91 | `model-request-validation` | in_progress | `source` | passed | - | - |
| 92 | `model-request-validation` | in_progress | `automated` | missing | Local half: correctness lane gate evidence (Donald's merge approval). Sibling owner `../abi`: elapsed-duration repair before a qualified pin change | correctness plan T8; `tasks/todo.md:1648` |
| 93 | `model-store-isolation` | open | `source` | missing | PSC T8 decision gate: Branch A is repo-side; Branch S needs the `../abi` owner to add an edition-qualified storage variable | PSC T8: `tests/edition_isolation.rs` scoped-model-store tests pass and `./check.sh` OK; `tasks/todo.md:1658` |
| 94 | `model-store-isolation` | open | `automated` | missing | PSC T8 decision gate: Branch A is repo-side; Branch S needs the `../abi` owner to add an edition-qualified storage variable | PSC T8: `tests/edition_isolation.rs` scoped-model-store tests pass and `./check.sh` OK; `tasks/todo.md:1658` |
| 95 | `mcp-server-conformance` | open | `source` | missing | Donald decision: which MCP protocol revision is pinned (authority plan T11) | authority plan T11-T14 (T14 closes) |
| 96 | `mcp-server-conformance` | open | `automated` | missing | Donald decision: which MCP protocol revision is pinned (authority plan T11) | authority plan T11-T14 (T14 closes) |
| 97 | `mcp-server-conformance` | open | `protocol_conformance` | missing | Donald decision on the pinned revision (authority plan T11) | authority plan T12-T14 classic-client compatibility and lifecycle fixtures |
| 98 | `portable-package-services` | open | `source` | missing | OB-8 (Ubuntu ARM64), OB-9 (Windows 11 ARM), OB-10 (Windows/Linux signing keys); macOS release keys OB-1..OB-4 | desktop plan T14 (repo side), T13 and T15 (Donald steps) |
| 99 | `portable-package-services` | open | `automated` | missing | OB-8 (Ubuntu ARM64), OB-9 (Windows 11 ARM), OB-10 (Windows/Linux signing keys); macOS release keys OB-1..OB-4 | desktop plan T14 (repo side), T13 and T15 (Donald steps) |
| 100 | `portable-package-services` | open | `macos_arm64` | missing | OB-1..OB-4: Developer ID, notary key, updater key, published release | desktop plan T13 notarised macOS release journey per edition |
| 101 | `portable-package-services` | open | `ubuntu_arm64` | missing | OB-8: Ubuntu ARM64 host/runner | desktop plan T15 clean-user package journey on Ubuntu ARM64 |
| 102 | `portable-package-services` | open | `windows11_arm` | missing | OB-9: Windows 11 ARM host | desktop plan T15 PowerShell coexistence + package journey on Windows 11 ARM |
| 103 | `portable-package-services` | open | `signing` | missing | OB-10 (Windows Authenticode, Linux package key) and OB-1..OB-3 (macOS) | desktop plan T13/T15 signature verification receipts |
| 104 | `three-platform-artifact-ci` | open | `source` | missing | OB-6, OB-8 and a Windows 11 ARM64 runner online at once, plus OB-10 (and OB-1..OB-3) signing material | PSC T11 (workflow guards, repo-side) and T12 (`require_executed_run.py <run-id> --require-platforms macos,linux,windows` exit 0) |
| 105 | `three-platform-artifact-ci` | open | `automated` | missing | OB-6, OB-8 and a Windows 11 ARM64 runner online at once, plus OB-10 (and OB-1..OB-3) signing material | PSC T11 (workflow guards, repo-side) and T12 (`require_executed_run.py <run-id> --require-platforms macos,linux,windows` exit 0) |
| 106 | `three-platform-artifact-ci` | open | `macos_arm64` | missing | OB-6: macOS ARM64 runner online | PSC T12 macOS job in the executed release run |
| 107 | `three-platform-artifact-ci` | open | `ubuntu_arm64` | missing | OB-8: Linux ARM64 runner | PSC T12 Linux job in the executed release run |
| 108 | `three-platform-artifact-ci` | open | `windows11_arm` | missing | OB-9 + Windows 11 ARM64 runner | PSC T12 Windows job in the executed release run |
| 109 | `three-platform-artifact-ci` | open | `native_ci` | missing | OB-6, OB-8 and the Windows ARM64 runner online together | PSC T12: `require_executed_run.py <run-id> --require-platforms macos,linux,windows` exit 0 |
| 110 | `three-platform-artifact-ci` | open | `signing` | missing | OB-10 and OB-1..OB-3 signing material provisioned | PSC T11 signature-verification steps executed in the T12 run |
| 111 | `three-platform-artifact-ci` | open | `artifact_provenance` | missing | PSC T11 guards landed (repo-side, Donald merge approval) and the T12 run (runners above) | PSC T11 `test_release_jobs_verify_artifact_signature_and_provenance_per_edition` + T12 attestation receipts |
| 112 | `physical-federation-proof` | open | `source` | missing | Donald decision: no OB row exists for physical/geographic hosts; define one or defer (completion-index T3) | models-media-federation plan T16 |
| 113 | `physical-federation-proof` | open | `automated` | missing | Donald decision: no OB row exists for physical/geographic hosts; define one or defer (completion-index T3) | models-media-federation plan T16 |
| 114 | `physical-federation-proof` | open | `physical_hosts` | missing | Donald decision (no OB row) then physical distinct hosts | models-media-federation plan T16 step 2 |
| 115 | `physical-federation-proof` | open | `geographic_deployment` | missing | Donald decision (no OB row) then a geographic deployment | models-media-federation plan T16 step 2 |
| 116 | `physical-federation-proof` | open | `hardware` | missing | Donald decision (no OB row) then physical hardware operator | models-media-federation plan T16 step 2 |
| 117 | `linux-service-foundation` | in_progress | `source` | required | Repo-side gap mapping and tests (PSC T7 steps 1-2) land on Donald's merge approval | PSC T7: `docs/verification/YYYY-MM-DD-linux-service-gaps.md` plus TDD commits per gap |
| 118 | `linux-service-foundation` | in_progress | `automated` | required | Repo-side gap mapping and tests (PSC T7 steps 1-2) land on Donald's merge approval | PSC T7: `docs/verification/YYYY-MM-DD-linux-service-gaps.md` plus TDD commits per gap |
| 119 | `linux-service-foundation` | in_progress | `ubuntu_arm64` | blocked | OB-8: Ubuntu ARM64 host (Linux runner operator) | PSC T7 step 3: native install/start/status/cancel/restart/uninstall for both editions |
| 120 | `linux-service-foundation` | in_progress | `native_ci` | blocked | OB-8: Linux ARM64 runner | PSC T5 executed Linux job (`require_executed_run.py` exit 0) |
| 121 | `dependency-security-remediation` | in_progress | `source` | missing | Donald decision per advisory (bincode RUSTSEC-2025-0141, paste RUSTSEC-2024-0436: replace vs reviewed exception), PSC T10 step 1; Donald approval to tick compatible-security-pins boxes (PSC T9); GLib upstream stays with the GTK/Tauri owners | PSC T9, T10 steps 1-3 |
| 122 | `dependency-security-remediation` | in_progress | `automated` | missing | Donald decision per advisory (bincode RUSTSEC-2025-0141, paste RUSTSEC-2024-0436: replace vs reviewed exception), PSC T10 step 1; Donald approval to tick compatible-security-pins boxes (PSC T9); GLib upstream stays with the GTK/Tauri owners | PSC T9, T10 steps 1-3 |
| 123 | `dependency-security-remediation` | in_progress | `dependency_audit` | passed | - | - |
| 124 | `dependency-security-remediation` | in_progress | `license_review` | missing | None external: rerun the license inventory on the current revision (repo-side, PSC T10 step 4) | PSC T10 step 4 license inventory receipt on the candidate revision |
| 125 | `dependency-security-remediation` | in_progress | `secret_scan` | missing | None external: rerun the secret scan on the current revision (repo-side, PSC T10 step 4) | PSC T10 step 4 secret-scan receipt on the candidate revision |
| 126 | `dependency-security-remediation` | in_progress | `macos_arm64` | missing | OB-6 host plus OB-3/OB-4 (updater key, published release) for updater acceptance | PSC T10 step 5 via desktop plan T13 per-edition updater/install acceptance |
| 127 | `dependency-security-remediation` | in_progress | `ubuntu_arm64` | missing | OB-8: Ubuntu ARM64 host | PSC T10 step 5 via desktop plan T15 |
| 128 | `dependency-security-remediation` | in_progress | `windows11_arm` | missing | OB-9: Windows 11 ARM host (Windows update acceptance owner) | PSC T10 step 5 via desktop plan T15 |
| 129 | `completion-review-boundaries` | implemented | `source` | passed | - | - |
| 130 | `completion-review-boundaries` | implemented | `automated` | passed | - | - |
