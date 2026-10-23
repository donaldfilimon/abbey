# Abbey Completion Plan — repo + Tauri desktop

> Historical execution checklist. The active [2026-10-22 Completion Program](superpowers/plans/2026-10-22-abbey-completion.md) supersedes execution order and target-policy choices. These unchecked items are preserved, not asserted complete; distinct obligations are tracked by `retained-plan-reconciliation` in `tasks/completion.json`. Canonical workflow-ledger coverage does not claim to validate every historical plan checkbox.

> **This is a PLAN, not evidence.** It changes no capability claim and grants no
> capability: every target, bar, gate, and acceptance test below is *proposed work*,
> not a shipped fact. The authoritative sources for what Abbey can actually do are
> the capability ledger (`src/claims/registry.rs`, re-exported by `src/claims.rs`)
> and the workflow ledger (`tasks/goals.md` / `tasks/todo.md`); where this plan and
> those ledgers disagree, the ledgers win.
>
> **How to read it / status.** Every factual statement carries a `file:line`
> citation against the working tree at HEAD `ea2ae07` (dirty). Line numbers drift
> as the repo moves, so re-check any citation you rely on before acting on it.
> **Ledger-count drift.** The two ledger-count rows in §0 carry the live values
> (generated summary in `AGENTS.md`) with the 2026-10-22 authoring values beside
> them; §8 still maps only the 26 todos of that snapshot (48 live).
> Authored 2026-10-22 by a planning subagent; installed into `docs/` on 2026-10-22
> with citation corrections and this front-matter note only — no source, claim,
> ledger, or generated file was changed.
>
> **Re-check with:** `./check.sh` (the merge bar) · `desktop/check.sh` (the client
> gate) · `python3 tools/check_claims_sync.py` (claims ledger + generated regions) ·
> `python3 tools/check_instructions.py` (the AGENTS.md/CLAUDE.md split) ·
> `python3 tools/check_p3_readonly.py` (Program 3 read-only boundary).

**Repo:** `/Users/donald/sources/repos/abbey` · **Branch:** `main` · **HEAD at authoring:** `ea2ae07`
**Working tree:** dirty on purpose (workspace unification in flight: `desktop/Cargo.toml` and `desktop/Cargo.lock` deleted, root `Cargo.toml` gains `[workspace]`).
**Authoring date:** 2026-10-22 · **Author:** planning subagent (authored outside the checkout; installed into `docs/` on 2026-10-22).

---

## 0. Method, and what was verified

Every factual claim below is cited `file:line`. I read the sources listed; I did **not** run `./check.sh` (one gate at a time per checkout — `tools/gate_lock.sh:27-63` — and the tree is mid-flight). Anything I could not read directly is marked **unverified**.

Verified directly:

| Fact | Citation |
|---|---|
| Claims ledger is 42 Current · 4 Partial · 8 Proposed · 1 Blocked · 5 Out of scope, schema 2, digest `27318597…` (live; generated summary in `AGENTS.md` — at authoring: 3 Partial, digest `edb95b2b…`) | `tasks/todo.md:9`, `tasks/goals.md:9` |
| Canonical registry is `src/claims/registry.rs`, re-exported by `src/claims.rs` | `src/claims.rs:12-15` |
| 27 goals (24 done / 1 in_progress / 1 proposed / 1 blocked), 185 checked / 48 open todos (live; generated summary in `AGENTS.md` / `tasks/goals.md` — 26 open at authoring) | `tasks/todo.md:10` |
| The 26 open todos of the authoring snapshot were exactly that tree's 26 unchecked boxes (the live tree has 48) | `tasks/todo.md:164,179,189,203,247,250,253,256,283,286,289,313,318,321,376,467,585,700,853,861,864,917,1020,1036,1086,1102` (authoring-time line numbers) |
| Desktop is a workspace member; `desktop/Cargo.toml` + `desktop/Cargo.lock` deleted on purpose | `Cargo.toml:1-22` (`[workspace]`, `members = ["desktop/codegen","desktop/src-tauri"]`, `default-members`) |
| 37 Tauri commands | `desktop/src-tauri/src/main.rs:209-247` |
| `src/app_core/v4_types.rs` (763 lines) is **not declared** in `src/app_core/mod.rs` | `src/app_core/mod.rs:60-68` (declares `v4`, `v4_content`, `v4_permission`, `v4_protocol`, `v4_tests`; **not** `v4_types`) |
| `v4_types.rs` imports helpers that do not exist in `v4.rs` | `src/app_core/v4_types.rs:4-7` vs `src/app_core/v4.rs:13-20,23-51` (has `MAX_V4_TITLE_BYTES`, `MAX_V4_PAGE`, `MAX_V4_QUERY_BYTES`, `ensure`, `text`, `label`, `route`, `timestamp`, `limit`; **lacks** `MAX_V4_ITEMS`, `MAX_V4_LABEL`, `MAX_V4_TEXT`, `validate_id`, `validate_text`, `validate_conversation_id`) |
| `src/automation_schedule.rs` (253 lines) compiles with **no caller** | `src/lib.rs:72` is the only reference repo-wide |
| 12 v4 commands refused as "not served" | `src/daemon/runtime_v4.rs:141-157` |
| Degenerate v4 types | `src/app_core/v4.rs:727` `V4TaskState {}`, `src/app_core/v4.rs:730` `V4Automation {}` |
| `V4Capability` has 13 variants and is `Serialize`-only (no `Deserialize`) | `src/app_core/v4.rs:53-69` |
| 6 of 13 v4 capabilities are served | `src/daemon/runtime_v4.rs:19-25` (`SERVED`, 5) + `src/daemon/runtime_v4.rs:31` (`CancelTurns`) |
| 5 v4 store refusal stubs | `src/runtime/store/v4_admin.rs:53-82` |
| `v4_search_conversations` ignores the search field | `src/runtime/store/v4.rs:71-77` (comment: "search uses list for now; full in later") |
| v3 training family (7 commands) + `CancelJob` + `PollEvents` declared but never dispatched | `src/app_core/v3.rs:350-363`; catch-all refusal at `src/daemon/runtime_v3.rs:266`; module doc at `src/daemon/runtime_v3.rs:7` |
| v3 worker family returns always-empty / not-found | `src/daemon/runtime_v3.rs:218-251` |
| 13 authority-bearing federation methods fail closed | `src/daemon/federation.rs:176-190`; disabled catalog rows `src/daemon/federation.rs:353-358` |
| Windows: every daemon path fails closed | `src/daemon/server.rs:171-173`, `src/daemon/client.rs:59-64,113`, `src/daemon/client/v3.rs` (every method fails closed; 21 `Err(ClientError::UnsupportedPlatform)` sites from `:73` to `:860`), `src/daemon/client/v4.rs:15,24,286-289`, `src/daemon/runtime_config.rs:44`, `src/daemon/runtime_handler.rs:44`, `src/daemon/federation.rs:298-299`, `src/run_control.rs:466`, `src/commands.rs:533,545` |
| Teacher confidence 0.6 vs the `ready` threshold 0.9 | `src/distill.rs:26,196` vs `src/learn.rs:357` |
| `abbey learn lora` refuses (exit 2) | `src/commands.rs:127` → `src/deferred.rs:164`; `src/learn.rs:304,323,437,751` |
| Desktop README prose says "28" in three places while the gate-enforced prose says 37 | `desktop/README.md:41,52,178` vs `desktop/README.md:192`; checker at `tools/check_claims_sync.py:289-321` |
| The desktop "Tasks" card is a hard-coded Unavailable card | `desktop/src/views/RunsView.tsx:133-141` |
| `drive-surfaces-macos.py` / `drive-controls-macos.py` are wired into no gate | repo-wide grep (2026-10-22, excluding this document): only `tasks/todo.md:165` and `docs/verification/2026-10-21-evidence-and-appearance.md:117` mention them |
| `prove_packaged_macos.py` picks the bundle alphabetically | `desktop/scripts/prove_packaged_macos.py:85-87` (`sorted(...glob("*.app"))[-1]`) |
| No render tests for ModelsView / UpdateCheck / CommandPalette / ErrorBoundary / usePreferences | `desktop/tests/` (19 files); grep for each name in `desktop/tests/` returns nothing |
| Schema is at version 10 with migrations 1..10 | `src/runtime/migrations.rs:6,15-31,33` |
| CI runs exactly one job | `.github/workflows/rust.yml:46-56` (`gate (macOS ARM64 adjunct)`) |
| `ABBEY_LINUX_ARM64_RUNNER` is a repo variable no script reads | AGENTS.md "Traps"; `tasks/goals.md:20-27` |

**Unverified (would need a gate run):** current green/red state of `./check.sh` and `desktop/check.sh` on the dirty tree; whether the working tree compiles; the exact `Cargo.lock` delta introduced by the workspace unification (the AGENTS.md lesson at `tasks/lessons.md:346-354` requires that a unification "only *adds* packages and does not churn the versions the root had already pinned" — that check has not been run). Note also that the working tree, the git index, and `target/` are shared with every other session in this checkout (AGENTS.md "Traps"), so a re-run of any gate must be attributed to the exact bytes it saw.

---

## 1. Definition of done — "Abbey is complete"

### 1.1 The repo's own completion definition (authoritative)

`tasks/todo.md:1118-1124`, Phase 11(d), quoted verbatim:

>   - Finish-all definition: every locally implementable goal has source+tests+docs+
>     required local live evidence; todo.md has no unchecked locally actionable item;
>     remaining unchecked items name a real external prerequisite + exact proof needed;
>     goals/todo/lessons/`src/claims.rs`/AGENTS/docs pass the automated sync check; no
>     secrets/weights/credentials/signing material in either repo; local green gates vs
>     hosted CI vs runtime proof vs signing vs DAST vs external review vs multi-host
>     reported as separate facts

Phase 11 itself is the last unchecked phase (`tasks/todo.md:1102`) and its per-wave bar (`tasks/todo.md:1103-1106`) requires focused unit+integration tests, fmt, warning-denied lint, file-size and docs checks, scratch-state services only, and `ledger status+evidence updated in the same review unit`.

Note the asymmetry the definition encodes: **locally implementable** work must be *done*; **externally blocked** work must *name its exact prerequisite and the exact proof that closes it* — not be deleted, not be silently dropped, and not be relabelled finished.

### 1.2 Claim-status transitions this plan targets

| Claim id | Today | Target | What the gate proves |
|---|---|---|---|
| `surface-parity-grok-codex-claude` | Partial (`src/claims/registry.rs:78-95`) | **Current** | `tests/cli_surface.rs::compatibility_ux_acceptance_matrix` plus per-surface TUI/desktop fixtures close every documented gap; the registry note keeps the "not a reimplementation of vendor runtimes" boundary. |
| `daemon-abbey-v1-federation` | Partial (`src/claims/registry.rs:227-240`) | **Current** (one family at a time) | Each authority family gains a durable adapter and its own fail-closed-then-enabled fixture in `src/daemon/federation.rs::tests`; `tests/abbey_contracts.rs::bundled_corpus_qualifies_exact_bytes_and_every_declared_fixture` still green. |
| `edition-windows-install-naming` | Partial (`src/claims/registry.rs:619-630`) | **Current** | An **executed** `install.ps1` on a real Windows host for both editions, showing `abbey.exe`/`_abbey.ps1` and `abbey-personal.exe`/`_abbey-personal.ps1` coexist. The source-parsing test `tools/tests/test_install_ps1_editions.py` alone is explicitly *not* sufficient (the registry note says so at `src/claims/registry.rs:622`). |
| `desktop-tauri-react` | Proposed (`src/claims/registry.rs:645-656`) | **Current** | `desktop/check.sh` green + a **notarized** packaged acceptance run + Windows packaging/runtime acceptance. Today the registry names three concrete gaps: notarized distribution, governed tasks/automations, Windows packaging/runtime. |
| `runtime-provider-neutral-owned` | Proposed (`src/claims/registry.rs:658-669`) | **Current** | A provider-neutral multi-tool loop plus an external MCP/ACP client-host fixture. Serving Abbey's own read-only tools over stdio is the opposite direction and does not close it (`src/claims/registry.rs:661`). |
| `models-production-local-weights` | Proposed (`src/claims/registry.rs:671-682`) | **Current** | Real licensed weights, a reproducible quality/safety corpus, and a measured performance bar (`tasks/todo.md:1021-1025`: 4K context + 64-token smoke, peak RSS ≤ 20 GB, no sustained swap thrash, deterministic tokenizer fixtures, valid structured tool-call output, CPU/Metal oracle within documented tolerance). |
| `training-lora-pipeline` | Proposed (`src/claims/registry.rs:684-695`) | **Current** | A real weight update with the adapter bar from `tasks/todo.md:1030-1031`: ≥95% valid tool-call JSON on held-out fixtures, NLL regression ≤2% vs base, deterministic reopen/rollback, 100% pass on public-edition refusal fixtures. |
| `accelerator-execution-runtime` | Proposed (`src/claims/registry.rs:697-708`) | **Partial** (Apple devices only) | Exact requested/executed device evidence with no fallback and no mixed execution, plus residency that is *directly observable*. Non-Apple accelerators (CUDA/Vulkan/TPU) stay Proposed until real hardware exists. |
| `media-local-neural-models` | Proposed (`src/claims/registry.rs:710-721`) | **Partial** (speech first) | Offline STT/TTS with recorded device/runtime/revision/output digest; cloud SaaS stays out of scope; macOS system speech stays a separate provider. |
| `edition-personal-unrestricted` | Proposed (`src/claims/registry.rs:723-734`) | **Current** | A validated structured `shell.exec` + signed privileged helpers + audit, **in the separated personal edition only**; the shipped edition's allowlist + `--confirm` are byte-unchanged. |
| `mesh-three-vm-and-production` | Proposed (`src/claims/registry.rs:736-747`) | **Partial** (three-VM on one Mac) | An authenticated three-VM proof. Separate-physical-host / geographic-HA / multi-GPU stays Proposed (`src/claims/registry.rs:744`). |
| `ci-self-hosted-linux-proof` | Blocked (`src/claims/registry.rs:749-760`) | **Current** | `python3 tools/ci/require_executed_run.py <run-id>` prints `EVIDENCE: N job(s) executed and succeeded with M successful step(s)` and exits 0 **with the Linux job included** (`tasks/todo.md:411-415`), plus a separate Windows runtime job. |
| 5 × `oos-*` | Out of scope (`src/claims/registry.rs:762,775,788,801,814`) | **Out of scope, unchanged** | Not part of completion. See §4. |

### 1.3 New registry rows this plan requires

The 42/4/8/1/5 counts are **not** a target — they are today's state (live; generated summary in `AGENTS.md`). Closing the gaps above adds capability that no existing row covers, so `src/claims/registry.rs` must gain new rows (never edit the generated tables; `tools/check_claims_sync.py --write` regenerates them and the digest changes):

| New row (proposed id) | Substantiated by | Status today |
|---|---|---|
| `daemon-protocol-v4-task-automation-authority` | WS-6 | Proposed → Current |
| `daemon-protocol-v3-training-authority` | WS-7 | Proposed → Current (read/manage over `train_candidate`; **no** weight update) |
| `daemon-protocol-v3-event-polling` | WS-7 | Proposed → Current (fixed watermark) |
| `desktop-render-test-suite` | WS-2 | Proposed → Current (a *test-surface* claim, not a capability claim) |

---

## 2. Workstreams (ordered by dependency, not by appeal)

Legend — Size in engineer-days: **S** ≤2, **M** 3–6, **L** 7–15, **XL** 16+.
"Gate" = the narrowest command that proves the workstream.

---

### WS-1 — Qualify the unified workspace (baseline)

**Goal.** The tree carries an in-flight workspace unification (`Cargo.toml` now declares `[workspace]` with `default-members = [".", "desktop/codegen", "desktop/src-tauri"]`; `desktop/Cargo.toml` and `desktop/Cargo.lock` are deleted). Turn that into a *qualified* baseline before any new capability lands, and close the ledger-prose drift the unification exposed.

**Files.**
- `Cargo.toml:1-22` (workspace table), `Cargo.lock` (single lockfile).
- `desktop/check.sh` (now uses `--manifest-path $ABBEY_TOOLCHAIN_ROOT/Cargo.toml`; already reworked in the dirty tree).
- `desktop/README.md:41,52,178` (three stale "28" occurrences) vs `:192` ("37 commands are enumerated").
- `tools/check_claims_sync.py:289-321` — extend `_check_desktop_readme` to also police the file-tree prose ("typed wrappers over the N enumerated commands", "the N enumerated commands") and the `Task17 verification note` sentence, so the class of drift cannot recur.
- `tools/tests/test_workspace_topology.py` (untracked in the dirty tree) — assert exactly one lockfile, `desktop/` has no manifest of its own, and every member is a default member.
- `desktop/package.json` — the `codegen` script still says `--manifest-path codegen/Cargo.toml`; with `desktop/codegen` now a workspace member the script should use the root manifest or drop `--manifest-path`.

**Claims moved.** None. This workstream *protects* every claim: the unified lockfile is what makes the desktop's Rust code covered by the root gate (`AGENTS.md` "Execution path"; `tasks/lessons.md:346-354`).

**Acceptance tests.**
- `tools/tests/test_workspace_topology.py` (new).
- `tools/tests/test_check_claims_sync.py::DesktopReadmeDriftTests` — extended to the three file-tree occurrences.
- `./check.sh` exits 0 with `check.sh: OK`.
- `desktop/check.sh` exits 0 with `desktop/check.sh: OK`.
- `cargo run --locked --quiet -p abbey-desktop-codegen -- --check` reports no drift (root gate step `check.sh:112-119`).

**Gate command.** `./check.sh` then `desktop/check.sh` (both take the same per-checkout lock, so run them serially — `tools/gate_lock.sh:27-63`).

**Size.** M (2–3 d). Most of the unification is already in the tree; the residual is qualification, the README drift, and the codegen script path.

**Dependencies.** None.

**Risk.** The root gate now builds the Tauri tree; `target/debug` reaches tens of GB and **a full disk fails the build with a linker error that reads like a code fault** (`AGENTS.md` "Execution path"). Check free space before a full run. Second risk: `tasks/lessons.md:346-354` requires the unification to *add* packages and not churn versions the root had pinned — verify the `Cargo.lock` diff accordingly before accepting.

---

### WS-2 — Desktop acceptance-harness and render-test gaps

**Goal.** Close the desktop's own test debt, and fix the two proven harness defects.

**Files.**
- `desktop/tests/` — add `models-view.test.tsx`, `update-check.test.tsx`, `command-palette.test.tsx`, `error-boundary.test.tsx`, `preferences.test.tsx` (none of ModelsView, UpdateCheck, CommandPalette, ErrorBoundary, or `usePreferences` appears in `desktop/tests/` today).
- `desktop/scripts/prove_packaged_macos.py:84-87` — replace `sorted(glob("*.app"))[-1]` with an explicit `--app`/edition selection. The bundle names are `Abbey` (`desktop/src-tauri/tauri.conf.json:3`) and `Abbey Personal` (`desktop/src-tauri/tauri.personal.conf.json:3`); alphabetical order picks `Abbey.app` and therefore silently proves the **safe** edition when the caller meant the personal one.
- `desktop/scripts/drive-surfaces-macos.py`, `desktop/scripts/drive-controls-macos.py` — wire into `desktop/check.sh` behind an opt-in variable, mirroring `ABBEY_DESKTOP_ROUTES_ACCEPTANCE` / `ABBEY_DESKTOP_PACKAGED_ACCEPTANCE` / `ABBEY_DESKTOP_MENU_ACCEPTANCE` (`desktop/check.sh:105-124`). Today they are referenced only by `tasks/todo.md:165` and `docs/verification/2026-10-21-evidence-and-appearance.md:117` (and this plan) — no gate runs them.
- `desktop/check.sh` — add the two acceptance steps.

**Claims moved.** None directly; supports `desktop-tauri-react`.

**Acceptance tests.**
- `cd desktop && bun test tests` — the new files.
- `python3 -m unittest discover -s desktop/scripts/tests -p 'test_*.py'` — a new test pinning that `prove_packaged_macos.py` selects by edition, not by `sorted()[-1]`.
- `ABBEY_DESKTOP_SURFACE_ACCEPTANCE=1 ABBEY_DESKTOP_CONTROL_ACCEPTANCE=1 ./desktop/check.sh` — requires macOS Accessibility permission (owner-blocked for the *runtime* proof, but the wiring is local).

**Gate command.** `cd desktop && bun test tests && bun run typecheck && bun run build && bun run verify:bundle` (this is what `desktop/check.sh` runs at lines 51-62).

**Size.** M (3–4 d).

**Dependencies.** WS-1 (the client must compile in the unified workspace before its tests are meaningful).

**Risk.** The AX drivers need Accessibility permission, so the *runtime* proof stays owner-blocked (`tasks/todo.md:164-171` already records this honestly). Wire the scripts in anyway so the moment permission lands the proof is one command away — but do not claim the interaction is proven until it has run.

---

### WS-3 — Surface-parity polish (`surface-parity-grok-codex-claude`)

**Goal.** Move `surface-parity-grok-codex-claude` Partial → Current by closing every *documented* gap between Grok/Codex/Claude-compatible surfaces and Abbey, without claiming runtime equivalence.

**Files.** `src/slash.rs` (aliases), `src/agent/argv.rs` (per-backend argv grammar), `src/surfaces.rs`, `src/tui/`, `desktop/src/views/`, `tests/cli_surface.rs`.

**Claims moved.** `surface-parity-grok-codex-claude` Partial → Current (`src/claims/registry.rs:78-95`).

**Acceptance tests.** `tests/cli_surface.rs::compatibility_ux_acceptance_matrix` (already exists, `src/claims/registry.rs:86`) extended to the newly closed surfaces; `src/slash.rs::tests::lookup_is_case_insensitive`; per-backend argv-leak tests in `src/agent/argv.rs`.

**Gate command.** `./tools/cargo.sh test --locked --test cli_surface compatibility_ux_acceptance_matrix`.

**Size.** M (3–5 d), open-ended by nature — the definition of "documented gap" is the README + `src/surfaces.rs` catalogue, and the catalogue grows.

**Dependencies.** None. Parallel-safe with WS-1/WS-2.

**Risk.** The registry note at `src/claims/registry.rs:81` says "not a reimplementation of vendor runtimes". Polishing must never become a claim of behavioural equivalence with Cursor/Grok/Claude internals — `oos-reimplement-vendor-runtimes` (`src/claims/registry.rs:814`) stays out of scope.

---

### WS-4 — Protocol-v4 contract reconciliation (one canonical typed family)

**Goal.** Today there are **two** incompatible v4 contract families: `src/app_core/v4.rs` (tuple-style `V4Command`/`V4Event`) and the richer `src/app_core/v4_types.rs` (763 lines, struct-style, with tasks/automations/permissions/actions) that is **never compiled** and references six helpers that do not exist. The daemon, the client, the store, and the desktop codegen all consume the *live* family. Establish one canonical typed v4 set and make the dead file either part of it or gone.

**Files.**
- `src/app_core/v4.rs` (829 lines) — home of `V4Command`/`V4Event`/`V4Capability`.
- `src/app_core/v4_types.rs` (763 lines) — the richer, uncompiled set.
- `src/app_core/v4_protocol.rs` (420), `src/app_core/v4_content.rs` (327), `src/app_core/v4_permission.rs` (109).
- `src/app_core/mod.rs:60-68` — add/remove the module declaration.
- `src/app_core/v4_tests.rs` (12 tests).
- `desktop/codegen/` + `desktop/src/ipc/generated.ts`, `generated.samples.ts`.

**Concrete work.**
1. Choose the canonical family. Recommendation: keep `v4_protocol.rs`/`v4_content.rs`/`v4_permission.rs` as the *wire* family and **port the task/automation/action types out of `v4_types.rs` into a new `src/app_core/v4_admin.rs`**, then delete `v4_types.rs`; keeping a second parallel family is what produced the 104-error compile failure recorded in `.superpowers/coordination/desktop-assistant.md:89`.
2. Add the missing helpers to `v4.rs` (or the new module): `MAX_V4_ITEMS`, `MAX_V4_LABEL`, `MAX_V4_TEXT`, `validate_id`, `validate_text`, `validate_conversation_id`, `validate_id_list`, `bounded_page` — the exact set `src/app_core/v4_types.rs:4-7,585-586` names.
3. Replace the degenerate `V4TaskState {}` (`src/app_core/v4.rs:727`) and `V4Automation {}` (`:730`) with the real shapes (`src/app_core/v4_types.rs:364,500`).
4. Decide the `V4Capability` wire shape. `V4Capability` is `Serialize`-only (`src/app_core/v4.rs:53-69`) with 13 variants while only 6 are served (`src/daemon/runtime_v4.rs:19-25,31`). Either derive `Deserialize` so a client can *request* a capability set, or keep it one-way and make the request type explicit — but do not leave 7 variants that can be named on the wire and never honoured.
5. If the wire shape changes, bump `APP_SCHEMA_V4` (`src/app_core/v4.rs:12`) and re-run the codegen.

**Claims moved.** None yet — this is the prerequisite for WS-6 and WS-8.

**Acceptance tests.** `src/app_core/v4_tests.rs` (12 existing tests, all kept by name); new tests for the replaced degenerate types; `cargo run --locked --quiet -p abbey-desktop-codegen -- --check` reports no drift; `desktop/src/ipc/generated.samples.ts` still compiles under `tsc --noEmit`.

**Gate command.** `./tools/cargo.sh test --locked --lib v4_` then `cargo run --locked --quiet -p abbey-desktop-codegen -- --check`.

**Size.** L (7–12 d). WS-4 is the gate on everything downstream.

**Dependencies.** WS-1.

**Risk.** The desktop's TypeScript is *generated* from these types, so an unnoticed shape change breaks the frontend silently — that is exactly what the codegen `--check` step exists for (`check.sh:112-119`). Do not hand-edit `desktop/src/ipc/generated.ts`. Second risk: the file-size guard (`check.sh:128-146`) caps `src/**/*.rs` at 800 warn / 1000 fail — `v4.rs` at 829 lines is already over the soft threshold, and `v4_types.rs` at 763 would push any merge of the two over 1000. Split before merging, not after (`AGENTS.md` "Invariants").

---

### WS-5 — Federation authority adapters (`daemon-abbey-v1-federation`)

**Goal.** Move the federation claim Partial → Current, **one family at a time**, exactly as the registry's own `next_action` directs (`src/claims/registry.rs:238`): "integrate remaining authority method families (Cognize, changes, episodes, manifests, consent) one at a time; add WDBX/episode/consent adapters only when durable paths exist".

**Files.** `src/daemon/federation.rs` (dispatch `:154-190`; catalog `:344-360`), `contracts/abbey/`, `src/abbey_contracts.rs`, `src/daemon/runtime_handler.rs`.

**Order.** `Authorize` (currently a prepared stub at `src/daemon/federation.rs:180-183`) → `Cognize` → consent epoch family (`OpenConsentEpoch`/`AttestConsent`/`CloseConsentEpoch`/`ResumeConsentEpoch`) → episodes → manifests → changes. Each family needs its **own durable adapter**; the catalog row stays `"enabled": false` until that adapter exists.

**Claims moved.** `daemon-abbey-v1-federation` Partial → Current incrementally.

**Acceptance tests.** `src/daemon/federation.rs::tests::read_only_catalog_works_and_authority_methods_are_disabled` (`:577`) extended per family — each family gets a "disabled before the adapter, enabled after" pair; `tests/abbey_contracts.rs::bundled_corpus_qualifies_exact_bytes_and_every_declared_fixture`; `src/daemon/server.rs::tests::federation_requires_exact_digests_and_never_downgrades`.

**Gate command.** `./tools/cargo.sh test --locked --lib daemon::federation`.

**Size.** L per family (7–12 d); XL for all five families.

**Dependencies.** WS-1. Independent of WS-4 (separate protocol family — `AGENTS.md` "Execution path": "v3 and v4 never downgrade and never decode each other's JSON").

**Risk.** `tasks/lessons.md:125-128`: "A new protocol type is not daemon authority." Declaring a method without a durable adapter is exactly the failure mode the current fail-closed design prevents — do not weaken it to make a row look complete. Also: the corpus is digest-pinned (`contracts/abbey/`), so any contract change invalidates `tests/abbey_contracts.rs` digests and must be a deliberate, reviewed update.

---

### WS-6 — Durable v4 task / automation / permission authority

**Goal.** Replace the 12 refused v4 commands (`src/daemon/runtime_v4.rs:141-157`) and the 5 store stubs (`src/runtime/store/v4_admin.rs:53-82`) with real durable, grant-gated authority — and bind automations to the already-compiled-but-unused `src/automation_schedule.rs`.

**Files.**
- `src/app_core/v4_admin.rs` (new; types ported per WS-4), `src/app_core/v4.rs` (degenerate types removed).
- `src/runtime/store/v4_admin.rs:53-82` — implement `v4_tasks`, `v4_create_task`, `v4_automations`, `v4_upsert_automation`, `v4_decide_action`.
- `src/runtime/migrations.rs:6,15-31,33` — `CURRENT_SCHEMA_VERSION` is 10; add `migration_11_v4_admin.sql`, its constant, and a `MIGRATIONS` entry. **Never rewrite an applied migration**; bump the version.
- `src/daemon/runtime_v4.rs:19-25,141-157` — widen `SERVED` **only** for capabilities that are actually served; the negotiation filter (`:44-55`) must stay deny-by-default.
- `src/automation_schedule.rs` (253 lines) — `CompiledSchedule::new/next_after/preview/catch_up` (`:92,137,141,157`) become the pure calculation behind the durable occurrence reservation.
- `src/daemon/client/v4.rs` — typed client methods.
- `src/runtime/store/v4.rs:71-77` — `v4_search_conversations` must actually filter by the search field (today it silently calls `v4_list_conversations`; the command `V4Command::SearchConversations` at `src/daemon/runtime_v4.rs:79-87` therefore returns the unfiltered list while advertising a search).

**Semantics to implement.**
- Task: `{task_id, attempt, state, run_id, timestamps}`, immutable snapshot of route/profile/budget at creation; never infer a task from a run (`tasks/lessons.md:91-94`).
- Automation: occurrence identity bound to (automation revision, scheduled UTC instant); one task per occurrence; no overlapping runs; first DST fold occurrence wins; gaps advance; catch up only the latest missed occurrence and retain missed/interrupted evidence — the pure rules are already proven in `src/automation_schedule/tests.rs` (`once_is_exact_and_exclusive`, `dst_gap_fold_and_skipped_day`, `catchup_is_latest_only`, `gap_collisions_are_deduplicated`).
- Action decision: bind the exact action id + decision id + kind; revocation before effect; prepared intent before effect; ambiguous effects never auto-replay (`tasks/lessons.md:95-98`).

**Claims moved.** New row `daemon-protocol-v4-task-automation-authority` Proposed → Current; unlocks part of `desktop-tauri-react`.

**Acceptance tests.** New `tests/daemon_v4_admin.rs`; the existing `src/runtime/store/v4_unavailable_tests.rs` (2 tests, `:19,27`) becomes the *refusal* baseline and is replaced by *service* tests; `src/runtime/store/v4_profile_tests.rs` (3 tests) extended; `src/app_core/v4_tests.rs` extended.

**Gate command.** `./tools/cargo.sh test --locked --lib runtime::store` and `./tools/cargo.sh test --locked --test daemon_v4_admin`.

**Size.** XL (16–25 d).

**Dependencies.** WS-4 (contract shape), WS-1.

**Risk.** Highest-risk area in the plan. Three specific traps: (a) a `V4Capability` that is nameable but not served is exactly the "stub capability advertised" failure the plan's own global constraint forbids; (b) migrations are append-only and `CURRENT_SCHEMA_VERSION` must be bumped — a rewritten migration silently corrupts existing installs; (c) `tasks/lessons.md:144-146`: snapshots and fixed-watermark pages are different evidence — a task list must not be a mutable snapshot presented as a paged history.

---

### WS-7 — Protocol-v3 training, worker, and polling authority

**Goal.** Wire the declared-but-never-dispatched v3 families: 7 training commands (`src/app_core/v3.rs:350-356`), `CancelJob` (`:361`), and `PollEvents` (`:363`). The worker family already dispatches but always returns empty (`src/daemon/runtime_v3.rs:218-251`).

**Files.**
- `src/daemon/runtime_v3.rs:7` (module doc still says "Training, worker, and polling grants remain absent"), `:123` (`available.push(V3Capability::ReadWorkers)` is unconditional while the handlers are stubs), `:266` (catch-all refusal).
- `src/app_core/v3.rs:350-363` (commands) and `:40-45` (capabilities `ReadTraining`, `ManageTraining`, `ReadWorkers`, `CancelJobs`, `PollEvents`).
- `src/daemon/client/v3.rs` — typed client methods (none exist for training/polling today).
- `src/runtime/store/` — new training/dataset table + `migration_12_*.sql`; `src/runtime/migrations.rs:6` bump to 12.
- `src/learn.rs:328-386`, `src/distill.rs:26,196,384` — the `train_candidate` substrate the training authority reads.
- `src/runtime/store/model_operation.rs` — the model-operation ledger whose "cancel" currently flips a row without signalling the in-flight download/load.

**Concrete work.**
1. `ReadTraining`: read `train_candidate` provenance/confidence/obsolete state through the existing `MemoryStore` trait (`src/memory/mod.rs`), sanitized, bounded, fixed-watermark.
2. `ManageTraining`: dataset-status, start/cancel, promote-adapter, rollback-adapter — over a **durable training-operation ledger**, not over weights (WS-10 owns weights). A `ManageTraining` grant that cannot mutate weights must say so in its contract, or the desktop's Training view will be a picture of something that does not run (`desktop/src/surfaces.ts`, training entry).
3. `PollEvents`: a durable event cursor with a fixed first-page watermark bound to the exact query (`tasks/lessons.md:132-135`).
4. `CancelModel` (`src/daemon/runtime_v3.rs:217`) must actually signal the in-flight download/load, not just move the ledger row.

**Claims moved.** New rows `daemon-protocol-v3-training-authority` and `daemon-protocol-v3-event-polling` Proposed → Current.

**Acceptance tests.** `tests/protocol_v3_contract.rs` additions; new `tests/daemon_training.rs`; `src/daemon/runtime_v3/tests.rs` (existing fixtures at `:110,146,374,384,387,397`).

**Gate command.** `./tools/cargo.sh test --locked --test protocol_v3_contract`.

**Size.** L (8–12 d).

**Dependencies.** WS-1. WS-10 (adapter training) depends on this.

**Risk.** `src/daemon/runtime_v3.rs:123` already advertises `ReadWorkers` while the handlers return empty — an advertised-but-inert grant. Fix that in the same wave, or the desktop's Cluster view will look "available" and show nothing. Also `tasks/lessons.md:231-234`: a stable claim ID is an authority boundary — training records must use opaque, domain-separated IDs, never raw memory record IDs.

---

### WS-8 — Desktop governed surfaces (Tasks / Automations / Tools / Training / Cluster)

**Goal.** Replace the desktop's stubs and its hard-coded card with real surfaces, backed by the authority WS-6/WS-7 provide.

**Files.**
- `desktop/src/views/RunsView.tsx:133-141` — the hard-coded "Tasks / Unavailable" card.
- `desktop/src/surfaces.ts` — `automations`, `training`, `cluster` are the three `unavailable` views; `requires` is derived from live capability sets, not a hardcoded boolean (the file's own comment at lines 8-15).
- `desktop/src-tauri/src/commands.rs` + `desktop/src-tauri/src/main.rs:209-247` (the `generate_handler!` block **must stay comment-free** — `verify-bundle.mjs` parses it by splitting on commas, per `AGENTS.md` "Execution path").
- `desktop/src/ipc/client.ts` — the `COMMANDS` list must match `generate_handler!` exactly (`tools/check_claims_sync.py:246-250` fails on drift).
- `desktop/src/components/UpdateCheck.tsx`, `CommandPalette.tsx`, `ErrorBoundary.tsx`, `hooks/usePreferences.ts`, `views/ModelsView.tsx` — the untested surfaces (WS-2 adds their render tests).

**Claims moved.** `desktop-tauri-react` Proposed → Partial (governed tasks/automations closed; notarization and Windows still open), → Current only after WS-9 + WS-13.

**Acceptance tests.** `desktop/tests/` new files; `desktop/check.sh`; `ABBEY_DESKTOP_ROUTES_ACCEPTANCE=1 ./desktop/check.sh`; `desktop/scripts/prove-packaged-macos.sh` against the packaged bundle.

**Gate command.** `cd desktop && bun test tests && bun run typecheck && bun run build && bun run verify:bundle` plus `./desktop/check.sh`.

**Size.** XL (16–25 d).

**Dependencies.** WS-6 (task/automation authority), WS-7 (training/worker authority), WS-1, WS-2.

**Risk.** The desktop must never fabricate a task record — the current card is honest, and replacing it with a view that shows invented data is worse than the stub. `tasks/lessons.md:147-149`: "Frontend-neutral does not mean every frontend ships the capability… every bridge needs explicit wiring and its own runtime evidence."

---

### WS-9 — macOS notarized release + updater publication

**Goal.** Take the updater chain from "authored, credential-blocked" to "published and verifiable".

**Files.**
- `desktop/scripts/release-macos.sh`, `desktop/scripts/assemble-update-feed.sh`, `desktop/scripts/updater-keygen.sh`.
- `.github/workflows/desktop-release.yml:106-117` (fail-closed secret gate), `:170` (build both editions), `:188` (verify both updater artifacts against the pinned public key), `:201` (assemble feeds), `:238-279` (create the draft release and upload assets).
- `desktop/src-tauri/tauri.conf.json:69-72` — the pinned Ed25519 `pubkey` and the feed `endpoints`.
- `desktop/src-tauri/tests/updater_signature.rs` — the artifact verification test.

**Facts.** The pubkey is pinned in the config; the production private key must match it. Feed URLs only resolve once the release is **published** — a draft release's assets are not publicly downloadable (the workflow's own comment at `.github/workflows/desktop-release.yml:220-221` says so). Signing capabilities stay exactly `core:default` + `updater:default`; there is no in-app relaunch and no update rollback — the registry note (`src/claims/registry.rs:648`) and `tasks/todo.md:936-943` both say so.

**Claims moved.** `desktop-tauri-react` — notarized packaged distribution becomes provable (still needs WS-13 for Windows).

**Acceptance tests.** `ABBEY_DESKTOP_PACKAGED_ACCEPTANCE=1 ./desktop/check.sh`; `desktop/scripts/prove-packaged-macos.sh`; `cargo test --locked -p abbey-desktop --test updater_signature` against the released artifact; `codesign --verify --strict` + `spctl -a` on the notarized bundle; a first-launch update check that resolves the published feed.

**Gate command.** `ABBEY_DESKTOP_PACKAGED_ACCEPTANCE=1 ./desktop/check.sh`.

**Size.** M repo-side (3–4 d); the last mile is owner-blocked (see §3).

**Dependencies.** WS-1, WS-2. Owner-blocked (Apple Developer ID, App Store Connect key, updater private key, a published release).

**Risk.** "Never manufacture/embed credentials" (`tasks/todo.md:1011-1014`). The pinned pubkey must not be regenerated to match a convenience key — the production key must match the *pinned* pubkey.

---

### WS-10 — Curated dataset promotion + adapter training pipeline

**Goal.** Turn `train_candidate` curation substrate into a real training pipeline: an explicit promotion verb, a trainer, evaluation, rollback, and signed artifact provenance.

**Files.**
- `src/learn.rs:328-386` — `review_train` computes `ready = prov_ok && confidence >= 0.9` (`:357`), while teacher pairs are written at `0.6` (`src/distill.rs:26,196`). There is **no** verb that promotes a pair to `ready`; the only curation verb is `abbey memory invalidate <id>` (`src/memory/mod.rs:382`), which removes a pair from the export.
- `src/distill.rs:384` (`export_sft`), `:458-474` (round-trip tests).
- `src/memory/mod.rs:430` (`promote` — currently a *retention* promotion, not a confidence promotion).
- New `src/training/` — dataset manifest, deterministic split, trainer invocation, evaluation, rollback, adapter artifact.
- `src/daemon/runtime_v3.rs` — the `ManageTraining` dispatch from WS-7.

**Concrete work.**
1. Add an explicit, provenance-checked promotion verb that raises a reviewed pair's confidence/retention — with a distinct record so promotion is auditable and reversible. Do **not** mutate `confidence` in place silently; the review output format (`src/learn.rs:365-372`) is a human-facing contract.
2. Deterministic 80/10/10 split with a recorded seed and dataset digest; local redaction + dedup; no upload, no remote training.
3. Trainer against the **separate Gemma 4 E4B instruction** base — never advertised as 12B-compatible (`tasks/lessons.md:77-79`).
4. Evaluation against the bar (`tasks/todo.md:1030-1031`): ≥95% valid tool-call JSON on held-out fixtures, NLL regression ≤2% vs base, 100% pass on public-edition refusal fixtures, deterministic reopen/rollback.
5. Adapter manifest with base hashes, dataset digest, seed, metrics, rollback state.

**Claims moved.** `training-lora-pipeline` Proposed → Current (on the full bar).

**Acceptance tests.** New `tests/training_pipeline.rs`; `tests/learn_distill.rs` (3 existing tests, `src/claims/registry.rs:142`) extended for promotion; `src/distill.rs` tests for the promotion path.

**Gate command.** `./tools/cargo.sh test --locked --test training_pipeline`.

**Size.** XL (16–25 d).

**Dependencies.** WS-7 (the manage authority). The trainer itself depends on the ABI sibling's trainer interface (owner of `../abi`).

**Risk.** The registry must keep saying what is unproven: whether a model's terms permit training on its output is not claimed (`src/claims/registry.rs:139`). Also `tasks/lessons.md:95-98`: restart recovery must never replay an ambiguous external action — a training run interrupted mid-step reopens as interrupted, not as retried.

---

### WS-11 — Provider-neutral owned tool host + MCP/ACP client-host

**Goal.** Move `runtime-provider-neutral-owned` Proposed → Current: an Abbey-owned provider-neutral multi-tool loop, plus Abbey acting as a **client/host** of other providers' MCP/ACP servers. Also close the Phase 9 server-conformance remainder.

**Files.**
- New `src/tool_host/{mod,mcp,acp,policy}.rs` — configured stdio and authenticated-network sessions, negotiated schemas/capabilities, bounded output/deadlines.
- New `src/runtime/agent_loop.rs` — the governed loop, reusing `src/runtime/store/governed.rs`, the tool approval/execution stores, and ABI's agent-host/runtime contracts.
- `src/mcp_host/` — the *server* side stays exactly as it is: read-only registry, `core:default`, no shell (`src/mcp_host/tests.rs::no_advertised_tool_is_execution_capable`, `tests/mcp_server.rs::the_advertised_registry_contains_no_execution_capable_tool`, `tests/mcp_http.rs::the_http_advertised_tool_list_equals_the_stdio_one`).
- Phase 9 remainder (`tasks/todo.md:1065-1085`): non-loopback binding, HTTPS/TLS, OAuth 2.1 + PKCE + resource indicators + token-audience binding, SSE/`GET` streaming with resumability, `resources`/`prompts`/`tasks`, cooperative mid-flight cancellation.

**Claims moved.** `runtime-provider-neutral-owned` Proposed → Current.

**Acceptance tests.** New `tests/tool_host.rs` (malicious schemas/outputs, duplicate IDs, child leaks, disconnect during an effect, auth failure, redirects, unexpected capability expansion) against real fixture servers; the three existing MCP no-execution-capable tests stay green; new MCP conformance fixtures for the Phase 9 remainder.

**Gate command.** `./tools/cargo.sh test --locked --test tool_host` and `./tools/cargo.sh test --locked --test mcp_server`.

**Size.** XL (20–35 d; this is the largest single workstream).

**Dependencies.** WS-6 (action contracts), WS-7 (training/tooling integration), WS-1.

**Risk.** Two boundaries must not move. First: `tasks/todo.md:1074-1081` — `shell.exec` must stay absent from *remotely discoverable* MCP capabilities, enforced structurally by the `EffectClass` enum having exactly one variant (`ReadOnly`). Second: the non-loopback MCP transport is meaningless without real TLS + OAuth 2.1 resource-server semantics — `tasks/todo.md:1083-1085` says so explicitly, and `tests/mcp_http.rs::the_transport_refuses_what_it_does_not_implement` proves the current honest refusal. Do not ship a wider binding flag.

---

### WS-12 — Production-capable local model runtime

**Goal.** Move `models-production-local-weights` Proposed → Current.

**Files.** ABI `abi-models` / `abi-model-runtime` (sibling repo — owner of `../abi`), `src/app_core/v3_model.rs`, `src/daemon/runtime_v3/model_lifecycle.rs` + `model_lifecycle/model_inference.rs`, `src/daemon/runtime_v3/model_inventory.rs`, `desktop/src/views/ModelsView.tsx`.

**Bar.** `tasks/todo.md:1021-1025`: target `google/gemma-4-12B-it-qat-w4a16-ct` (24 GB Mac fit), bounded at 4K context + 64-token acceptance smoke, peak RSS ≤ 20 GB, no sustained swap thrash, deterministic tokenizer fixtures, valid structured tool-call output, CPU/Metal oracle within documented tolerance — "if the bar fails, keep the capability OFF with no silent substitution".

**Claims moved.** `models-production-local-weights` Proposed → Current.

**Acceptance tests.** `tests/daemon_model_lifecycle.rs` extended with real-licence download/load/infer/unload/reopen/device-failure and a quality/security evaluation; the existing `tests/daemon_model_lifecycle.rs::real_daemon_runs_the_exact_fixture_on_metal_without_falling_back` stays green.

**Gate command.** `./tools/cargo.sh test --locked --test daemon_model_lifecycle`.

**Size.** XL (16–25 d), plus the upstream architecture work.

**Dependencies.** Owner-blocked (weights + licence). The Gemma 4 architecture itself is an **ABI-side** task (`tasks/todo.md:853-855`: "upstream Candle catalog lacks Gemma 4"), so the sibling repo owner must land it first.

**Risk.** `tasks/lessons.md:77-79`: Gemma E4B adapters are not transferable to Gemma 12B — the training target and the inference target are different architectures and must not be conflated. `tasks/lessons.md:278-283`: a loaded model is process state, not a durable routing claim — do not install a loaded provider as a global default.

---

### WS-13 — Windows transport, packaging, and install proof

**Goal.** Move `edition-windows-install-naming` Partial → Current, and give the daemon a Windows transport.

**Files.**
- `src/daemon/server.rs:171-173`, `src/daemon/client.rs:59-64,113`, `src/daemon/client/v3.rs` (21 `UnsupportedPlatform` sites), `src/daemon/client/v4.rs:15,24,286-289`, `src/daemon/runtime_config.rs:44`, `src/daemon/runtime_handler.rs:44`, `src/daemon/federation.rs:298-299`, `src/run_control.rs:466`, `src/commands.rs:533,545` — every daemon path fails closed today.
- New `src/daemon/server/windows.rs` (named-pipe transport, ACL, peer checks, same framing/correlation/limits as Unix), new `src/daemon/client/windows.rs`, a Windows Job-Object supervisor module.
- `install.ps1` — derive the installed names from `abbey edition --name`/`--daemon-name` (already done per `src/claims/registry.rs:622`), then **execute** it.
- `desktop/src-tauri/` — Windows packaging targets.
- `tools/tests/test_install_ps1_editions.py` — source-parsing only; explicitly insufficient.

**Claims moved.** `edition-windows-install-naming` Partial → Current.

**Acceptance tests.** A real Windows host: `install.ps1` for the safe and personal feature sets, both installed sets coexisting; a real `abbeyd` over a named pipe answering `Status`/`Claims`; `install.ps1` name derivation re-proved on that host.

**Gate command.** Windows-only: `powershell -File install.ps1` on the host, then `abbey.exe --version` / `abbeyd.exe --version`; cross-compile is a *soft* check only (`sh tools/ci/cross-compile-guard.sh`, `check.sh:152`).

**Size.** XL (20–35 d), and it is owner-blocked on a Windows host.

**Dependencies.** Owner-blocked (Windows host, Windows signing key). Repo-side compile checks are local.

**Risk.** "Windows named pipes stay unsupported until implemented **and** proved" (`AGENTS.md` "Do not do", §4 below). Cross-compilation is not runtime proof (`tasks/todo.md:225-226`; `tasks/lessons.md:59-62`).

---

### WS-14 — Program 3 live Discord (explicitly consented)

**Goal.** Close the three Program 3 open todos (`tasks/todo.md:313-324`) — *after* authorization and consent exist.

**Files.** `src/app_core/guild_intelligence/` + `src/app_core/guild_intelligence.rs`, `contracts/abbey/`, new recorder/transport/effect adapters, `tools/check_p3_readonly.py`.

**Sequence (from the todo itself).** (1) A separately reviewed read-only Discord metadata adapter with explicit manager authorization, participant consent rules, least-privilege scopes, bounded freshness and contradiction handling, redaction, revocation. (2) Separate consented evidence for a real guild observation, compared against a closed replay without adding message content, member enumeration, persistence, or effects. (3) Desired-state execution — a separate effect program requiring approvals, optimistic concurrency, exact before-state checks, bounded compensation, audit, and rollback.

**Claims moved.** Program 3 goal rows only; the `program-3-guild-intelligence-synthetic` claim (`Current`) stays Current and is not widened.

**Acceptance tests.** `python3 tools/check_p3_readonly.py` must keep passing; new live-capture adapter tests; deterministic replay comparison.

**Gate command.** `python3 tools/check_p3_readonly.py` (repo-side boundary) — the live proof is owner-blocked.

**Size.** XL (16–25 d) and owner-blocked.

**Dependencies.** Owner-blocked (manager authorization + participant consent).

**Risk.** "Do not reuse the synthetic fixture authority assertion as live authorization" (`tasks/todo.md:316-317`). The synthetic engine must stay pure and read-only (`tasks/todo.md:334`).

---

### WS-15 — Accelerator inference / training integration

**Goal.** Move `accelerator-execution-runtime` Proposed → Partial (Apple devices only).

**Files.** `src/accel/` + `src/accel.rs`, `src/daemon/runtime_v3/model_lifecycle/model_inference.rs`, ABI compute/GPU/model-runtime adapters, `src/commands.rs` (`abbey accel`), `src/platform.rs`.

**Existing narrow evidence to preserve.** `accelerator-kernel-execution-metal` (Current) — three kernels (`dot`, `cosine`, `top_k` over 1024-element f32) execute through ABI's `MetalAccelerator` and are checked against the `abi-compute` `CpuBackend` oracle in the same run (`src/claims/registry.rs` row; `abbey accel verify` behind `--features accel`). `daemon-protocol-v3-exact-model-inference` (Current) — fixture-bounded inference on a real Metal device.

**Bar.** `tasks/todo.md:256-274` and `docs/superpowers/plans/2026-10-20-production-completion.md:289-291`: exact requested/executed device, no fallback, no mixed execution, bounded allocations, numerical oracle, cancellation, allocation-failure and unsupported-kernel refusal, reproducible CPU benchmark on named hardware. Residency only if directly observable (`tasks/lessons.md:80-83`).

**Claims moved.** `accelerator-execution-runtime` Proposed → Partial (Apple); CUDA/Vulkan/TPU stay Proposed.

**Acceptance tests.** `cargo test --locked -p abbey --features accel` (the accel cohort is a *separate* gate cohort — `check.sh:72-77`); `abbey accel verify --json`; `tests/daemon_model_lifecycle.rs`.

**Gate command.** `./tools/cargo.sh test --locked -p abbey --features accel`.

**Size.** L–XL (12–20 d).

**Dependencies.** WS-12 (whole-model workloads), WS-1. Owner-blocked for non-Apple hardware.

**Risk.** `tasks/lessons.md:80-83`: CoreML output-oracle success proves execution under requested compute units, **not** ANE placement or residency. And the existing registry row already refuses to claim any speedup or performance figure because nothing on that path is timed (`src/claims/registry.rs` `accelerator-kernel-execution-metal`).

---

### WS-16 — Local neural speech and media

**Goal.** Move `media-local-neural-models` Proposed → Partial (speech first).

**Files.** `src/voice.rs`, `src/voice_portable.rs`, `src/media.rs`, new local media provider runtime + desktop UI, reusing the model registry/artifact machinery from WS-12.

**Bar.** `tasks/todo.md:286-288,1032-1035`: opt-in offline Whisper large-v3-turbo STT and Parler-TTS Mini v1.1 TTS first; then SDXL image and LTX-Video; hash-verified downloads, recorded licence acceptance, fixed-seed fixtures, recorded device/runtime/revision/duration/output digest; network-disabled generation proof.

**Claims moved.** `media-local-neural-models` Proposed → Partial.

**Acceptance tests.** New offline speech acceptance suite; `cargo test --locked -p abbey` voice tests; `src/voice_portable_tests.rs`.

**Gate command.** `./tools/cargo.sh test --locked --lib voice`.

**Size.** XL (16–25 d). Owner-blocked on weights + licences.

**Dependencies.** WS-12 (registry/artifact reuse), owner-blocked.

**Risk.** Cloud TTS/STT SaaS stays out of scope (`oos-bundled-cloud-speech`, `src/claims/registry.rs:801`); macOS system speech stays a *separate* provider (`tasks/lessons.md:22`).

---

### WS-17 — Personal-unrestricted edition runtime

**Goal.** Move `edition-personal-unrestricted` Proposed → Current.

**Files.** `src/edition.rs`, `src/os_control.rs`, `src/actions.rs`, `src/deferred.rs`, new `src/tools/shell.rs` (validated structured `shell.exec`), native signed helper services (Authorization Services / SMAppService, polkit, signed Windows service/UAC), `desktop/src-tauri/tauri.personal.conf.json`, `install.sh`/`install.ps1` personal-edition paths.

**Bar.** `tasks/todo.md:289-294,864-916`: separate binary + desktop bundle (distinct name, bundle ID, config root, credential namespace, audit log); unrestricted mode always on in the personal edition with the prompt-injection consequence documented; commands execute only after a validated structured `shell.exec` tool call — never concatenating untrusted raw strings into a shell; privileged ops via signed platform helpers; never store the user password or bypass OS authorization; mandatory audit records, process-group teardown, bounded capture, timeouts, emergency kill switch; the personal shell tool only via local IPC/loopback — never remote MCP or a network worker API.

**Claims moved.** `edition-personal-unrestricted` Proposed → Current.

**Acceptance tests.** `tests/edition_isolation.rs` (existing) extended; a new personal-edition test file run under `cargo test --locked -p abbey --features personal-edition` (the personal cohort is a separate gate cohort — `check.sh:59-64`).

**Gate command.** `./tools/cargo.sh test --locked -p abbey --features personal-edition`.

**Size.** XL (16–25 d).

**Dependencies.** WS-11 (the action/tool contracts), WS-1.

**Risk.** The shipped edition's allowlist + `--confirm` never relax (`src/os_control.rs:79-82,181-190`); `oos-shipped-shell-bypass` stays out of scope (`src/claims/registry.rs:762`). `Edition::unrestricted_runtime_implemented()` is `false` in every edition today (`tasks/todo.md:901-902`) and must not become `true` for the safe edition.

---

### WS-18 — Authenticated worker control + three-VM mesh

**Goal.** Move `mesh-three-vm-and-production` Proposed → Partial (three-VM proof on one Mac), and give the desktop a real Cluster view.

**Files.** ABI `abi-worker` (sibling repo), `src/mesh.rs`, `src/daemon/runtime_v3.rs:218-251` (the always-empty worker stubs), `src/app_core/v3.rs:357-361` (worker commands), `desktop/src/views/` (the `cluster` unavailable entry in `desktop/src/surfaces.ts`).

**Bar.** `tasks/todo.md:1086-1101`: three Ubuntu ARM64 VMs on one Mac with distinct IPs, independent state dirs, worker identities/certs, separate namespaces; WDBX data plane + consensus-backed control/membership plane with an **independent** persistent store (no circular dependency on WDBX placement); ABI model/compute worker plane; private-CA mTLS, cert rotation, signed job envelopes, capability advertisement, leases/deadlines, retries/cancellation, result hashes, quotas/backpressure, resumable recovery, backup/restore, rolling-upgrade compatibility; validate worker/VM termination, leader failure, network partition, stale leader, duplicate delivery, conflicting WDBX values, read repair, reconnect, rolling restart, clean teardown.

**Claims moved.** `mesh-three-vm-and-production` Proposed → Partial.

**Acceptance tests.** `abbey mesh local-demo --nodes 3..9` (Current, same-host — a control, not the proof); a new three-VM fixture; the desktop Cluster view's render test.

**Gate command.** `./tools/cargo.sh test --locked --lib mesh` locally; the three-VM proof is an owner-blocked hardware run.

**Size.** XL (16–25 d). Owner-blocked on three VMs.

**Dependencies.** WS-7 (worker authority), WS-11, owner-blocked (VMs).

**Risk.** `tasks/lessons.md:73-76`: a three-VM demo on one Mac is multi-VM evidence, **not** physical multi-host evidence. Do not promote the production row.

---

### WS-19 — Linux / Windows CI proof

**Goal.** Move `ci-self-hosted-linux-proof` Blocked → Current.

**Files.** `.github/workflows/rust.yml` (currently exactly one job: `gate (macOS ARM64 adjunct)` at `:46-56`), `tools/ci/require_executed_run.py`, `tools/ci/cross-compile-guard.sh`, `tools/ci/checkout-public-revision.sh`, `tools/tests/test_workflow_guards.py`.

**Repo-side work (local).** Re-add the guarded Linux ARM64 job behind the `ABBEY_LINUX_ARM64_RUNNER` repository variable, using the prepared guard patterns in `tools/tests/test_workflow_guards.py`, so the moment a runner is registered the job actually executes. The Linux and hosted-fork jobs were removed on 2026-09-28 (`src/claims/registry.rs:752`).

**Owner-side work.** Provision/register a Linux ARM64 runner with labels `self-hosted,Linux,ARM64,abbey`; enable `ABBEY_LINUX_ARM64_RUNNER`; obtain a successful non-zero-job Linux run; separately obtain Windows runtime evidence.

**Claims moved.** `ci-self-hosted-linux-proof` Blocked → Current.

**Acceptance tests.** `python3 tools/ci/require_executed_run.py <run-id>` prints `EVIDENCE: N job(s) executed and succeeded with M successful step(s)` and exits 0, with the **Linux** job among them (`tasks/todo.md:411-415`); `python3 -m unittest tools.tests.test_workflow_guards`.

**Gate command.** `python3 -m unittest discover -s tools/tests -p 'test_*.py'` (repo-side, already in `check.sh:96`) then `python3 tools/ci/require_executed_run.py <run-id>` (the closing proof).

**Size.** S repo-side (1–2 d); the closing proof is owner-blocked.

**Dependencies.** Owner-blocked (runner). Independent of every other workstream — can start now.

**Risk.** "a zero-job `startup_failure` is infrastructure evidence, never source-test evidence" (`tasks/todo.md:389-391`). And `tasks/lessons.md:32-35`: runner registration and CI execution are different facts.

---

### WS-20 — Final ledger closure (Phase 11)

**Goal.** Satisfy Phase 11(d) exactly (`tasks/todo.md:1118-1124`).

**Files.** `tasks/goals.md`, `tasks/todo.md`, `tasks/lessons.md`, `src/claims/registry.rs`, and every generated region (`AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/claims.md`, `docs/*.md`, `tasks/*.md`, `desktop/README.md`).

**Concrete work.**
1. For every locally implementable goal: source + tests + docs + required local live evidence.
2. Every remaining unchecked item names a real external prerequisite **and** the exact proof that closes it.
3. `python3 tools/check_claims_sync.py --write` regenerates every generated region; the digest in the generated summary changes.
4. `python3 tools/check_instructions.py` passes.
5. `./check.sh` and `desktop/check.sh` both green on the final bytes.
6. No secrets, weights, credentials, or signing material in either repo.
7. Local green gates, hosted CI, runtime proof, signing, DAST, external review, and multi-host are reported as **separate facts** — never as one "done".

**Claims moved.** All, per the table in §1.2.

**Acceptance tests.** `python3 tools/check_claims_sync.py` (default mode *is* the check — `AGENTS.md` "Commands and the gate") and `python3 tools/check_instructions.py`, both inside `./check.sh:97-98`.

**Gate command.** `python3 tools/check_claims_sync.py && python3 tools/check_instructions.py`.

**Size.** S (1–2 d), recurring.

**Dependencies.** Every other workstream.

**Risk.** `tasks/lessons.md:63-66`: claims must be generated from a single source, never hand-copied. Editing a generated table by hand makes the gate fail — that is the desired behaviour, not a nuisance.

---

## 3. Owner-blocked work

Nothing here is implementable by the repo. Each row names exactly what is needed, from whom, and what becomes possible the moment it lands.

| # | What is needed | From whom | Exactly which credentials / hosts | What becomes possible | Workstream |
|---|---|---|---|---|---|
| OB-1 | Apple Developer ID Application identity + private key | Donald (repository/release owner) | Developer ID Application certificate + matching private key in the release keychain; the identity string for `APPLE_SIGNING_IDENTITY` (`.github/workflows/desktop-release.yml:108`) | Developer ID signing of both editions' `.app`/`.dmg`; `codesign --verify --strict` on a public-trust signature | WS-9 |
| OB-2 | App Store Connect API key + issuer + key id | Donald | `APPLE_API_KEY_B64`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER` (`.github/workflows/desktop-release.yml:109-111`); alternatively an app-specific password | `notarytool` submission, stapling, and `spctl -a` acceptance | WS-9 |
| OB-3 | Production updater Ed25519 private key (+ passphrase) | Donald | `ABBEY_UPDATER_SIGNING_KEY`, `ABBEY_UPDATER_SIGNING_KEY_PASSWORD` (`.github/workflows/desktop-release.yml:112-113`). The private key must match the **pinned** public key at `desktop/src-tauri/tauri.conf.json:70` | Signed `.app.tar.gz` + `.sig` artifacts and per-edition feeds (`latest.json` / `latest-personal.json`) | WS-9 |
| OB-4 | A **published** (not draft) GitHub release | Donald | Publishing the draft created at `.github/workflows/desktop-release.yml:274`; the feed URLs in `desktop/src-tauri/tauri.conf.json:72` only resolve after publication | The in-app updater resolves a real version; the Doctor "Updates" card can actually update | WS-9 |
| OB-5 | macOS runner with Accessibility permission | Donald / the runner operator | Automation permission for the driving process (TCC), plus a staging bundle from `desktop/scripts/release-macos.sh` | `prove-routes-macos.sh`, `prove-packaged-macos.sh`, `prove-menu-icon-macos.sh`, and the two AX drivers (`drive-surfaces-macos.py`, `drive-controls-macos.py`) actually run | WS-2, WS-8 |
| OB-6 | Self-hosted macOS ARM64 runner enabled in repository variables | Repository administrator | The runner behind `.github/workflows/rust.yml:56` (`runs-on: [self-hosted, macOS, ARM64, abbey]`) | The `gate (macOS ARM64 adjunct)` job executes on exact-head commits | WS-19 |
| OB-7 | `gh` CLI authenticated on that runner | Repository administrator | `GH_TOKEN` with `contents: write` for the draft-release step only (`.github/workflows/desktop-release.yml:241`) | The release job can create the draft and upload both editions' assets | WS-9 |
| OB-8 | Linux ARM64 self-hosted runner | Repository administrator / VM operator | A machine registered with labels `self-hosted,Linux,ARM64,abbey`; the `ABBEY_LINUX_ARM64_RUNNER` repository **variable** enabled (no script in this repo reads it) | The guarded Linux gate job executes; `require_executed_run.py` can close `ci-self-hosted-linux-proof` | WS-19 |
| OB-9 | Windows host with PowerShell | Donald / hardware owner | A Windows machine; PowerShell 5.1+ | **Executed** `install.ps1` for both editions (the exact proof `edition-windows-install-naming` needs — `src/claims/registry.rs:626`), plus Windows daemon/packaging/runtime acceptance | WS-13 |
| OB-10 | Windows code-signing key; Linux packaging signing key | Donald / release owner | Authenticode cert + Linux signing key | Signed Windows installer; signed Linux packages | WS-13 |
| OB-11 | Three Ubuntu ARM64 VMs on one Mac | VM/cluster operator | Three VMs with distinct IPs, independent state dirs, private CA + worker certs | The authenticated three-VM shared-compute proof; `mesh-three-vm-and-production` Partial | WS-18 |
| OB-12 | Gemma 4 architecture + compressed-tensor loader (Candle lacks it upstream) | Owner of the sibling `../abi` repo | An ABI-side implementation or a pinned reviewed Candle fork (`tasks/todo.md:853-855`) | Any production-weight claim: the current fixture `abi-bigram-v1` cannot promote it | WS-12 |
| OB-13 | Licensed Gemma 4 12B IT QAT w4a16 ct weights + licence acceptance | Model licence holder | The artefact, its hash, and an accepted licence in the edition-scoped ledger | `models-production-local-weights`; also the 24 GB-Mac / ≤20 GB-RSS bar | WS-12 |
| OB-14 | A 24 GB-RAM Apple Silicon host (or the named host) | Donald | The machine the RSS bar is measured on | The `peak RSS ≤ 20 GB, no sustained swap thrash` acceptance (`tasks/todo.md:1021-1025`) | WS-12 |
| OB-15 | ABI trainer + evaluation interface | Owner of the sibling `../abi` repo | A bounded trainer contract with deterministic seeds and held-out evaluation | The adapter training pipeline and its NLL/tool-call bars | WS-10 |
| OB-16 | Local neural media weights + licences | Model licence holders | Whisper large-v3-turbo, Parler-TTS Mini v1.1, SDXL, LTX-Video artefacts + licences | `media-local-neural-models` | WS-16 |
| OB-17 | Non-Apple accelerator hardware | Hardware operator | A CUDA / Vulkan / TPU host | The non-Apple half of `accelerator-execution-runtime` | WS-15 |
| OB-18 | Discord guild manager authorization + participant consent | Guild manager + participants | Explicit, revocable, least-privilege scope; a separately reviewed consent record | Program 3 live capture, live read-only validation, and the separate desired-state effect program | WS-14 |
| OB-19 | Independent final review capacity | Donald / a reviewer | A reviewer with no write access to the candidate | Phase 11's "independent final requirements/code/release review after the last repair" | WS-20 |

### 3.1 Ready-to-run checklist for the credential holder

Each item is a command or an action, in the order the plan needs them. Nothing here requires a code change.

**A. macOS release chain (OB-1 → OB-4).**
1. Put the Developer ID Application certificate + private key in the runner keychain; confirm `security find-identity -v -p codesigning` shows it.
2. Store `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY_B64`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`, `ABBEY_UPDATER_SIGNING_KEY`, `ABBEY_UPDATER_SIGNING_KEY_PASSWORD` as repository secrets.
3. Confirm the updater private key's public half matches the pinned `pubkey` in `desktop/src-tauri/tauri.conf.json:70` (the workflow verifies both artifacts against the pinned public key at `.github/workflows/desktop-release.yml:188`; it will refuse to publish otherwise).
4. Tag and dispatch `desktop-release.yml`; it fails closed at `.github/workflows/desktop-release.yml:106` while any secret is missing.
5. Publish the draft release from the Releases page. Until then the feed URL does not resolve.
6. Verify: `curl -sSfL <feed-url> | jq .version` returns the published version; `shasum -a 256` the downloaded artifact and re-verify the `.sig` against the pinned pubkey.

**B. Accessibility acceptance (OB-5).**
7. Grant Accessibility to the terminal (or the CI-driving binary) on the macOS runner: *System Settings → Privacy & Security → Accessibility*.
8. `desktop/scripts/release-macos.sh` (produces the staging bundle).
9. `ABBEY_DESKTOP_ROUTES_ACCEPTANCE=1 ./desktop/check.sh` → the seven canonical Routes scenarios.
10. `ABBEY_DESKTOP_PACKAGED_ACCEPTANCE=1 ./desktop/check.sh` → the packaged `.app` scenarios (and confirm `prove_packaged_macos.py` selects the intended edition after WS-2's fix).
11. `ABBEY_DESKTOP_MENU_ACCEPTANCE=1 ./desktop/check.sh` → the menu/icon proof.
12. `ABBEY_DESKTOP_SURFACE_ACCEPTANCE=1 ABBEY_DESKTOP_CONTROL_ACCEPTANCE=1 ./desktop/check.sh` → the two AX drivers once WS-2 has wired them.

**C. Linux CI (OB-8).**
13. Register the runner with labels `self-hosted,Linux,ARM64,abbey`.
14. Add the repository variable `ABBEY_LINUX_ARM64_RUNNER=true`.
15. Re-add the guarded Linux job to `.github/workflows/rust.yml` (WS-19 prepares it).
16. Push and capture the run id: `gh run list --workflow rust.yml --limit 1 --json databaseId`.
17. Close the claim only with: `python3 tools/ci/require_executed_run.py <run-id>` → `EVIDENCE: N job(s) executed and succeeded with M successful step(s)`, exit 0, **with the Linux job among them**.

**D. Windows install (OB-9).**
18. On the Windows host, run `install.ps1` for the default feature set, then for `ABBEY_CARGO_FEATURES=personal-edition`.
19. Confirm `abbey.exe` + `_abbey.ps1` **and** `abbey-personal.exe` + `_abbey-personal.ps1` coexist; `abbey.exe --version` and `abbey-personal.exe --version` both work.
20. Attach the output to `src/claims/registry.rs`'s `edition-windows-install-naming` local evidence, then `python3 tools/check_claims_sync.py --write`.

**E. Discord (OB-18).**
21. Obtain the guild manager's written, scoped, revocable authorization.
22. Obtain participant consent per the separately reviewed rules.
23. Only then implement the recorder adapter and record separate consented evidence — never reuse the synthetic fixture's authority assertion (`tasks/todo.md:316-317`).

---

## 4. Do not do

### 4.1 The five Out-of-scope claims — excluded from this plan by policy

| Claim id | Registry | Why it stays closed |
|---|---|---|
| `oos-shipped-shell-bypass` | `src/claims/registry.rs:762` | The shipped edition keeps the `os_control` allowlist + `--confirm`; only the separately packaged personal edition is Proposed (`edition-personal-unrestricted`). |
| `oos-owned-cot-engine` | `src/claims/registry.rs:775` | `abbey cot` is a transcript viewer; reason uses vendor thinking models. |
| `oos-fake-cost-accounting` | `src/claims/registry.rs:788` | `/cost` stays N/A for cursor-agent. |
| `oos-bundled-cloud-speech` | `src/claims/registry.rs:801` | Cloud TTS/STT SaaS is not bundled; local neural speech is Proposed separately. |
| `oos-reimplement-vendor-runtimes` | `src/claims/registry.rs:814` | Grok/Codex/Claude parity is surface parity only. |

Canceled Zig work is likewise retained history, not backlog (`REWRITE-DECISION-2026-10-19.md`; `tasks/todo.md:3-5`).

### 4.2 Invariants that must not be relaxed

1. **OS execution never runs without `--confirm` and only against the allowlist, in both editions, with no shell bypass.** `src/os_control.rs:79-82` (allowlist check), `:181-190` (`--confirm` gate, shell strings refused).
2. **`unsafe_code` is denied at the manifest level.** `Cargo.toml:45` (`unsafe_code = "deny"`), `desktop/src-tauri/Cargo.toml:16`. Every `unsafe` needs an explicit `#[allow(unsafe_code)]` with a SAFETY comment — clippy `-D warnings` alone will not catch it.
3. **Conversation identity is canonical-first.** Commit the opaque canonical save or clear tombstone to `runtime.sqlite` *first*, then recover only the exactly marked commit/creation/removal plan through the owner-only fs4-locked journal. Identity clear must never delete history, transcripts, memory, models, route/run data, or finalized migration backups.
4. **Claims honesty.** `src/claims/registry.rs` is the only source; the generated regions in `AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/*.md`, `tasks/*.md`, and `desktop/README.md` are machine-written. Edit the registry; regenerate with `tools/check_claims_sync.py --write`.
5. **Never run `cargo fmt --all`.** It formats the local path dependencies and reaches `../abi` and `../wdbx`. Use `cargo fmt --package abbey -- --check` (`check.sh:21-27`).
6. **One gate at a time per checkout.** `tools/gate_lock.sh:27-63` takes an exclusive lock; `check.sh:12-13` and `desktop/check.sh:31-32` both call it. A second gate refuses instead of interleaving.
7. **Never fabricate credentials, weights, or evidence.** No manufactured Developer ID, no self-signed "production" updater key standing in for the pinned one, no placeholder weights, no simulated CI run. A zero-job run is infrastructure evidence, never source-test evidence.
8. **Never claim runtime equivalence for vendor surfaces.** `surface-parity-grok-codex-claude` is surface parity; `oos-reimplement-vendor-runtimes` stays out of scope.
9. **Windows named pipes stay unsupported until implemented *and* proved.** Every daemon path must keep failing closed with `UnsupportedPlatform` until a real Windows host proves the transport. Cross-compilation is not runtime proof.
10. **No second execution path for a persona/role turn.** CLI, slash, TUI, daemon, and desktop all reach the executor through `actions::run_agent` → `session::hybrid_run` → `agent::run_resilient`.
11. **Migrations are append-only.** A new migration is a new `.sql` file beside `src/runtime/migrations.rs`, its constant, an entry in `MIGRATIONS`, and a `CURRENT_SCHEMA_VERSION` bump (`src/runtime/migrations.rs:6,15-33`).
12. **A feature-gated module must be gated.** Adding a `[features]` entry without the matching clippy/test lines in `check.sh` lets the code rot while CI stays green (`check.sh:47-77`).
13. **File-size guard.** `src/**/*.rs` only: `main.rs` ≤200 (hard), others ≤800 (warn) / ≤1000 (hard). `src/app_core/v4.rs` is already at 829 lines — split before merging, not after.

---

## 5. First two weeks

The sequence below is **locally implementable, unblocked, and maximally downstream-unblocking**. Gate after each item.

### Week 1

| # | Item | Workstream | Gate after it |
|---|---|---|---|
| 1 | Qualify the in-flight workspace unification on the exact current bytes; confirm the `Cargo.lock` delta only *adds* packages and does not churn versions the root had pinned | WS-1 | `./check.sh` → `check.sh: OK` |
| 2 | Same bytes, client side | WS-1 | `desktop/check.sh` → `desktop/check.sh: OK` |
| 3 | Close the three stale "28" occurrences (`desktop/README.md:41,52,178`) and extend `_check_desktop_readme` (`tools/check_claims_sync.py:289-321`) to police the file-tree prose too | WS-1 | `python3 tools/check_claims_sync.py` |
| 4 | Land `tools/tests/test_workspace_topology.py` (one lockfile; no `desktop/` manifest; every member a default member) | WS-1 | `python3 -m unittest discover -s tools/tests -p 'test_*.py'` |
| 5 | Fix `desktop/scripts/prove_packaged_macos.py:84-87` to select the bundle by edition, not `sorted(...)[-1]`; add a unit test that fails on the old behaviour | WS-2 | `python3 -m unittest discover -s desktop/scripts/tests -p 'test_*.py'` |
| 6 | Add the five missing render tests: `models-view`, `update-check`, `command-palette`, `error-boundary`, `preferences` | WS-2 | `cd desktop && bun test tests` |
| 7 | Wire `drive-surfaces-macos.py` and `drive-controls-macos.py` into `desktop/check.sh` behind `ABBEY_DESKTOP_SURFACE_ACCEPTANCE` / `ABBEY_DESKTOP_CONTROL_ACCEPTANCE` | WS-2 | `desktop/check.sh` (the new steps report "skipped — permission" rather than failing) |
| 8 | Fix `desktop/package.json`'s `codegen` script now that `desktop/codegen` is a workspace member | WS-1 | `cd desktop && bun run codegen && cargo run --locked --quiet -p abbey-desktop-codegen -- --check` |

### Week 2

| # | Item | Workstream | Gate after it |
|---|---|---|---|
| 9 | Decide the canonical v4 family; write the decision down before touching code (the 104-error failure came from two families coexisting) | WS-4 | *(decision doc; no gate)* |
| 10 | Port the task/automation/action types out of `src/app_core/v4_types.rs` into `src/app_core/v4_admin.rs`; add the six missing helpers (`MAX_V4_ITEMS`, `MAX_V4_LABEL`, `MAX_V4_TEXT`, `validate_id`, `validate_text`, `validate_conversation_id` + `validate_id_list`/`bounded_page`) | WS-4 | `./tools/cargo.sh test --locked --lib v4_` |
| 11 | Delete `src/app_core/v4_types.rs` and remove `V4TaskState {}` (`src/app_core/v4.rs:727`) and `V4Automation {}` (`:730`); bump `APP_SCHEMA_V4` (`src/app_core/v4.rs:12`) if the wire changed; regenerate the desktop types | WS-4 | `./tools/cargo.sh test --locked --lib v4_` && `cargo run --locked --quiet -p abbey-desktop-codegen -- --check` && `cd desktop && bun run typecheck` |
| 12 | Schema 11: `src/runtime/migration_11_v4_admin.sql` + constant + `MIGRATIONS` entry + `CURRENT_SCHEMA_VERSION` bump (`src/runtime/migrations.rs:6,33`) | WS-6 | `./tools/cargo.sh test --locked --lib runtime::store` |
| 13 | Implement the five store methods behind the current refusal stubs (`src/runtime/store/v4_admin.rs:53-82`); make `v4_search_conversations` actually filter (`src/runtime/store/v4.rs:71-77`) | WS-6 | `./tools/cargo.sh test --locked --lib runtime::store` |
| 14 | Dispatch the 12 refused v4 commands (`src/daemon/runtime_v4.rs:141-157`) against the new store; widen `SERVED` (`:19-25`) only for what is served | WS-6 | `./tools/cargo.sh test --locked --lib daemon::runtime_v4` |
| 15 | Bind `src/automation_schedule.rs` to the durable occurrence reservation: one task per (automation revision, scheduled instant), first DST fold wins, catch up only the latest missed occurrence | WS-6 | new `tests/daemon_v4_admin.rs` (one test per rule from `src/automation_schedule/tests.rs`) |
| 16 | Re-add the guarded Linux CI job behind `ABBEY_LINUX_ARM64_RUNNER` so it executes the moment a runner exists | WS-19 | `python3 -m unittest discover -s tools/tests -p 'test_*.py'` |
| 17 | Start the v3 training read/manage slice: `ReadTraining` over `train_candidate` (`src/learn.rs:328-386`), bounded and fixed-watermark | WS-7 | `./tools/cargo.sh test --locked --test protocol_v3_contract` |
| 18 | A first surface-parity polish slice, closing one documented gap per session | WS-3 | `./tools/cargo.sh test --locked --test cli_surface compatibility_ux_acceptance_matrix` |

**Not in the first two weeks, deliberately:** everything in §3 (owner-blocked), and WS-10/WS-11/WS-12/WS-13/WS-14/WS-15/WS-16/WS-17/WS-18, which depend on WS-4/WS-6/WS-7 landing first or on owner credentials.

---

## 6. Verification matrix

One command per workstream — the narrowest that proves it — and where it sits in the gate.

| WS | The ONE command that proves it | Fits into `./check.sh`? |
|---|---|---|
| WS-1 | `./check.sh` (root) **and** `desktop/check.sh` (client) | Yes — root gate is the merge bar; `desktop/check.sh` is separate and must be run before committing under `desktop/` |
| WS-2 | `cd desktop && bun test tests` | No — the root gate runs only the codegen `--check` (`check.sh:112-119`); the client gate owns bun |
| WS-3 | `./tools/cargo.sh test --locked --test cli_surface compatibility_ux_acceptance_matrix` | Yes (inside `cargo test --locked`, `check.sh:37`) |
| WS-4 | `./tools/cargo.sh test --locked --lib v4_` | Yes (inside `cargo test --locked`) |
| WS-5 | `./tools/cargo.sh test --locked --lib daemon::federation` | Yes |
| WS-6 | `./tools/cargo.sh test --locked --test daemon_v4_admin` | Yes (new file; add to `check.sh`'s `cargo test --locked`) |
| WS-7 | `./tools/cargo.sh test --locked --test protocol_v3_contract` | Yes (existing file) |
| WS-8 | `cd desktop && bun test tests && bun run typecheck && bun run build && bun run verify:bundle` | No — client gate only |
| WS-9 | `ABBEY_DESKTOP_PACKAGED_ACCEPTANCE=1 ./desktop/check.sh` | No — client gate, opt-in |
| WS-10 | `./tools/cargo.sh test --locked --test training_pipeline` | Yes (new file) |
| WS-11 | `./tools/cargo.sh test --locked --test tool_host` | Yes (new file) |
| WS-12 | `./tools/cargo.sh test --locked --test daemon_model_lifecycle` | Yes (existing file) |
| WS-13 | `powershell -File install.ps1` on a Windows host, then `abbey.exe --version` | No — platform-specific; the repo-side soft check is `sh tools/ci/cross-compile-guard.sh` (`check.sh:152`) |
| WS-14 | `python3 tools/check_p3_readonly.py` | Yes (`check.sh:121-122`) — but this proves the *boundary*, not the live capture |
| WS-15 | `./tools/cargo.sh test --locked -p abbey --features accel` | Yes — a separate cohort (`check.sh:72-77`) |
| WS-16 | `./tools/cargo.sh test --locked --lib voice` | Yes |
| WS-17 | `./tools/cargo.sh test --locked -p abbey --features personal-edition` | Yes — a separate cohort (`check.sh:59-64`) |
| WS-18 | `./tools/cargo.sh test --locked --lib mesh` | Yes (same-host control only; the three-VM proof is external) |
| WS-19 | `python3 tools/ci/require_executed_run.py <run-id>` | No — external; the repo-side guard tests are inside `check.sh:96` |
| WS-20 | `python3 tools/check_claims_sync.py` | Yes (`check.sh:97`) |

**Cohort discipline.** Three feature cohorts exist and are scoped to the packages that own the feature: `--features wdbx` (`-p abbey`), `--features personal-edition` (`-p abbey -p abbey-desktop`), `--features accel` (`-p abbey`). Always name the package — without `-p` the Tauri tree is rebuilt for a cohort that cannot use it (`check.sh:41-77`).

**Evidence-class discipline.** Per `tasks/lessons.md:59-62`, the following are **separate** facts and must never be reported as one: a green local `./check.sh`; a live run on a developer Mac; a successful hosted CI job; an independent audit; a signed+notarized artifact; a DAST/DAST-equivalent scan; a multi-host deployment. The `ci-self-hosted-linux-proof` claim exists precisely to keep that distinction visible.

---

## 7. Risks and traps

Seeded from `tasks/lessons.md` and the AGENTS.md "Traps" section, each with its consequence in *this* plan.

| Lesson (citation) | Consequence here |
|---|---|
| **"A claim is not evidence."** A registry row that describes intended behaviour is a false claim until the implementation matches it (`AGENTS.md` *Traps*). | `desktop-tauri-react`'s registry note already lists three concrete gaps. WS-6/WS-7/WS-8 must close the *code* before the row changes. Never write the row first. |
| **"An observer must use the same rule as the behaviour it observes."** The recovery path reads `fs::symlink_metadata` and requires a regular file; a test observer using `Path::is_file` follows symlinks (`tasks/lessons.md:334-338`). | WS-6's task/automation observers must use the production resolution rule, not `Path::is_file`. Same for WS-10's promotion reader and WS-7's training reader: the observer and the behaviour must agree on what "exists" means. |
| **"A run budget is only a bound where it is consulted."** The pinned ABI `RunContext::finish` does not consult elapsed duration; the ceiling is checked only at event checkpoints (`tasks/lessons.md:328-333`). | WS-12's whole-model inference must treat the **client's** read deadline as the effective bound, exactly as `tests/daemon_model_lifecycle.rs::METAL_READ_TIMEOUT` does today. Do not "fix" this by depending on an uncommitted sibling change — CI pins a revision. |
| **"Never branch per-call behaviour on `AgentBackend::from_env()`."** It is resolved once in a `OnceLock`; the TUI's Ctrl-B switch changes `AgentConfig::backend` at runtime (`AGENTS.md` "Traps"). | WS-8's desktop route/model selection and WS-3's parity work must thread the live `cfg.backend`. The `OnceLock` also makes `from_env()` order-dependent inside a test binary — set `ABBEY_BACKEND` before anything resolves a backend. |
| **"Ollama never auto-pulls."** The tag must already appear in `ollama list` or the turn fails (`AGENTS.md` "Traps"; `tests/ollama_no_pull.rs`). | WS-8's Chat surface routes through the same preflight. A desktop turn must never be the path that pulls a 26B model. |
| **"A report of what a turn would use must resolve the route a turn would use."** The Doctor once resolved the *first available* route while Chat submits on the *selected* route (`tasks/lessons.md:309-316`). | Already fixed and tested (`desktop/tests/doctor-effective-model.test.tsx`; `live_daemon_desktop_the_selected_route_answers_the_effective_model` at `desktop/src-tauri/src/commands.rs:619`). WS-8 must not regress it: a task/automation view that reports a route must resolve the *selected* route, and `EffectiveModel::source` (`local-config` vs `runtime-route`) stays in the typed contract, not in the shared doctor line (`tasks/lessons.md:317-322`). |
| **"A read-only surface must not borrow a session that negotiates write authority."** (`tasks/lessons.md:323-327`) | WS-8's Tasks/Automations read views must open their own read-only session, exactly as the effective-model read does. |
| **"A green `./check.sh` cannot see a check that cannot fail."** (`tasks/lessons.md:196-201`) | Every acceptance in this plan must have a demonstrated **fail-then-pass** (red/green) control. A test that has never been seen red proves nothing. |
| **"Writing complete code into a plan moves where the bug enters; it does not remove it."** (`tasks/lessons.md:202-207`) | This document specifies *files, semantics, and gates*, not code bodies. Any code fragment here is suspect and needs its own red-green proof. |
| **"A test that passes alone and fails in the suite is an isolation defect, not noise."** (`tasks/lessons.md:189-195`) | WS-6/WS-10/WS-7's new store tests must own a generated edition namespace, state root, and journal path (the pattern from `tasks/todo.md:605-614`), and must be run serially and in parallel before being called green. |
| **"Two builds in one checkout corrupt the experiment, not just the schedule."** (`tasks/lessons.md:378-388`) | Never run a bare `cargo test` while a gate is in flight. Use `tools/gate_lock.sh`; the plan's gate column assumes serial execution. |
| **"Other sessions share this checkout."** The working tree, the git index, and `target/` are shared with every other session in the repo; `tools/gate_lock.sh` serializes `./check.sh` and `desktop/check.sh` only, and does not stop another session from editing files or running `cargo build` (AGENTS.md "Traps"). | A `git status` that grows between two commands, a compile error in a file you did not touch, or a suddenly flaky integration test is an **environment race until proven otherwise**. Before treating any WS-1…WS-20 failure as a code defect, re-check whether another session changed the tree or rebuilt `target/`, and re-run the focused command before changing code. |
| **"A workspace-wide formatter reaches path dependencies, not just workspace members."** (`tasks/lessons.md:284-289`) | `cargo fmt --package abbey -- --check` only (`check.sh:21-27`). Never `--all`. |
| **"A second lockfile is a second source of truth that will drift."** (`tasks/lessons.md:346-354`) | WS-1 must verify the unification *added* packages and did not churn pinned versions. `desktop/Cargo.lock` must not come back. |
| **"A new protocol type is not daemon authority."** (`tasks/lessons.md:125-128`) | WS-4's new types are contracts only; WS-6/WS-7 add the dispatch. Keeping `APP_PROTOCOL_VERSION` and `SUPPORTED_PROTOCOL_VERSIONS` (`src/daemon/protocol.rs:17`, asserted `[1,2,3]` at `:345`) unchanged until the envelope ships. |
| **"Negotiation is not authority unless the command carries and the daemon rechecks the exact grant."** (`tasks/lessons.md:216-220`) | WS-6's `SERVED` widening must be re-derived from startup-owned authority, not from the client's request. `src/daemon/runtime_v4.rs:44-55` is the shape. |
| **"Advertised runtime capability must derive from startup-owned authority."** (`tasks/lessons.md:136-139`) | `src/daemon/runtime_v3.rs:123` pushes `ReadWorkers` unconditionally while the handlers return empty — WS-7 must make the advertisement follow the handler, not the reverse. |
| **"Inventory should project one safe registry, not copy it."** (`tasks/lessons.md:235-238`) | WS-11's external MCP/ACP tool inventory must project the *external* descriptor through the local policy, never copy a registry wholesale. |
| **"Snapshots and fixed-watermark pages are different evidence."** (`tasks/lessons.md:144-146`) | WS-6's task list and WS-7's event polling are paged histories with a fixed first-page watermark, not mutable snapshots. |
| **"Restart recovery must never replay an ambiguous external action."** (`tasks/lessons.md:95-98`) | WS-6's automations, WS-10's training, and WS-7's model cancel must reopen as *interrupted*, not retried. |
| **"A loaded model is process state, not a durable routing claim."** (`tasks/lessons.md:278-283`) | WS-12 must not install a loaded provider as a global default or route. |
| **"Gemma E4B adapters are not transferable to Gemma 12B."** (`tasks/lessons.md:77-79`) | WS-10 trains against the E4B instruction base; WS-12 serves the 12B. Two architectures, two claims, no substitution. |
| **"CoreML output-oracle success proves execution under requested compute units, not ANE placement or residency."** (`tasks/lessons.md:80-83`) | WS-15 may claim "executed under a requested compute-unit policy"; it may **not** claim placement or residency without profiler evidence. |
| **"Runner registration and CI execution are different facts."** (`tasks/lessons.md:32-35`) | WS-19: registering the Linux runner is not the proof. `require_executed_run.py` against a real run id is. |
| **"Source compilation, local runtime proof, hosted CI, and external audit are separate evidence classes."** (`tasks/lessons.md:59-62`) | §6's evidence-class discipline; also why WS-13's cross-compile check is explicitly *not* the Windows proof. |
| **"Claims must be generated or validated from a single source, never manually duplicated."** (`tasks/lessons.md:63-66`) | WS-20 never hand-edits a generated table. |
| **"A model-produced structured tool call is the execution boundary."** (`tasks/lessons.md:67-69`) | WS-11 and WS-17: raw model text never becomes shell syntax. |
| **"A three-VM demo on one Mac is multi-VM evidence, not physical multi-host evidence."** (`tasks/lessons.md:73-76`) | WS-18 stops at Partial. |
| **"A scope decision changes the roadmap status, not the evidence status."** (`tasks/lessons.md:28-31`) | Moving a Proposed item to Current must not remove any fail-closed refusal path (e.g. `abbey lora` must keep refusing until WS-10 lands; `abbey claims refuse` must keep naming both the Proposed and the OOS row — `src/claims/tests.rs::shell_refusal_surfaces_proposed_edition_and_oos_shipped_bypass`, referenced from `src/claims/registry.rs:768`). |
| **"A dropped audit record is a hole in the evidence, not a detail."** (`tasks/lessons.md:369-373`) | WS-6/WS-7's store writes are evidence: a failed write fails the command, exactly as `route.jsonl` now fails the turn (`tests/cli_surface.rs::an_unwritable_route_log_fails_the_turn_instead_of_losing_the_record`). |
| **"A liveness bound is not the behaviour under test."** (`tasks/lessons.md:355-361`) | WS-6/WS-8's automation-tick and AX-driver fixtures get their own bounds and retry while the observation stays indeterminate. |
| **"The evidence anchor must be built by the qualified toolchain."** (`tasks/lessons.md:296-299`) | Every command in §5 and §6 goes through `./tools/cargo.sh`, never a bare `cargo`, and `./tools/cargo.sh --abbey-qualify-toolchain` is a *mode switch* that runs no cargo command. |
| **"A dependency on an uncommitted sibling change is not reproducible."** (`tasks/lessons.md:339-345`) | WS-12/WS-10/WS-18 depend on ABI-side work. Until the sibling commits it, the local `../abi` working tree is not evidence. Verify with `tools/ci/verify-pinned-siblings.sh`. |

---

## 8. The 26 open todos (2026-10-22 authoring snapshot) → workstream map

Line numbers are `tasks/todo.md` at HEAD `ea2ae07`. Status vocabulary: **local** = implementable in this checkout with no owner input; **owner-blocked** = needs an owner credential, host, licence, or consent; **deferred-by-construction** = deliberately not a unit of work in this plan (it belongs to a separate program or is a policy boundary).

**Snapshot, not live.** The live tree has 48 open todos (see the generated summary in `AGENTS.md` / `tasks/goals.md`); this map covers the 26 of the 2026-10-22 authoring snapshot, and the 22 added since are not yet assigned to a workstream.

| # | todo.md:line | Item | Workstream | Status |
|---|---|---|---|---|
| 1 | `tasks/todo.md:164` | Accessibility control driver not proven by the gate | WS-2 | local (wire it in) / owner-blocked (the AX permission to run it) |
| 2 | `tasks/todo.md:179` | CI blocked external proof (soft macOS adjunct only) | WS-19 | owner-blocked |
| 3 | `tasks/todo.md:189` | CI blocked on runner provisioning (Linux ARM64) | WS-19 | owner-blocked (OB-8); repo-side job re-add is local |
| 4 | `tasks/todo.md:203` | Roadmap 1 — Runtime contracts (provider-neutral executor, tool-call, permission, cancellation, transcript, audit) | WS-11 (contracts) + WS-6 (action contracts) | local |
| 5 | `tasks/todo.md:247` | Roadmap 2 — Owned tool host (capability-scoped registry, MCP/ACP host adapters, schema validation, bounded policy, consent, audit) | WS-11 | local |
| 6 | `tasks/todo.md:250` | Roadmap 3 — Desktop GUI (Tauri 2 + React/TS over shared state; backend selection, chat, tools/consent, memory, routes, accessibility) | WS-8 (+ WS-2 for the harness) | local, except notarization (WS-9) and Windows (WS-13) |
| 7 | `tasks/todo.md:253` | Roadmap 4 — Local model runtime (production weights, manifests, integrity/licence provenance, CPU fallback, memory budgets, quality/performance bars) | WS-12 | owner-blocked (OB-12, OB-13, OB-14) |
| 8 | `tasks/todo.md:256` | Roadmap 5 — Accelerator runtime (capability negotiation + execution behind the compute abstraction; correctness and fallbacks before advertising) | WS-15 | local for Apple devices; owner-blocked (OB-17) for CUDA/Vulkan/TPU |
| 9 | `tasks/todo.md:283` | Roadmap 6 — LoRA/fine-tuning (consent/redaction/splits, reproducible adapter training, evaluation, rollback, signed provenance; no automatic weight mutation) | WS-10 (+ WS-7 for the manage authority) | local, except the ABI trainer (OB-15) and the weights (OB-13) |
| 10 | `tasks/todo.md:286` | Roadmap 7 — Local neural media (speech first, then image/video) | WS-16 | owner-blocked (OB-16) |
| 11 | `tasks/todo.md:289` | Roadmap 8 — Shared compute + separate edition (three local VMs; personal-unrestricted packaging) | WS-18 (compute) + WS-17 (edition) | local for the edition; owner-blocked for the VMs (OB-11) |
| 12 | `tasks/todo.md:313` | Program 3 — Authorized non-synthetic recording capture | WS-14 | owner-blocked (OB-18) |
| 13 | `tasks/todo.md:318` | Program 3 — Live read-only validation (after capture authority exists) | WS-14 | owner-blocked (OB-18) + sequenced after #12 |
| 14 | `tasks/todo.md:321` | Program 3 — Desired-state execution | WS-14 | deferred-by-construction (a separate effect program requiring approvals, optimistic concurrency, before-state checks, bounded compensation, audit, rollback) + owner-blocked |
| 15 | `tasks/todo.md:376` | Completion Program Phase 2 — Self-hosted runners replace the broken hosted-CI assumption | WS-19 | owner-blocked (OB-8); the repo-side guard work is local and already prepared |
| 16 | `tasks/todo.md:467` | Completion Program Phase 4 — Shared Abbey application core + durable `abbeyd` daemon | WS-5 (federation) + WS-6 (v4 task/automation authority) + WS-7 (v3 training/worker/poll authority) | local |
| 17 | `tasks/todo.md:585` | Phase 4B.5 flaky test observed 2026-08-09 (`real_cli_recovers_committed_conversation_mirrors_after_failpoint`) | WS-1 | local / **closed as a diagnosed observer defect** — the two observer defects were fixed and pinned (`tasks/todo.md:615-635`); the single historical occurrence was never reproduced and stays **unattributed**, retained as evidence rather than closed as noise |
| 18 | `tasks/todo.md:700` | Completion Program Phase 5 — Model and tool runtime ownership in ABI | WS-7 (v3 training/worker/poll) + WS-11 (tool runtime) + WS-18 (worker deployment) + WS-12 (model architecture, ABI-side) | local for the Abbey side; owner-blocked for the ABI-side architecture (OB-12) and the VMs (OB-11) |
| 19 | `tasks/todo.md:853` | Gemma 4 architecture + compressed-tensor loader in ABI or a pinned reviewed Candle fork | WS-12 | owner-blocked (OB-12 — sibling-repo owner) |
| 20 | `tasks/todo.md:861` | Deploy the authenticated worker protocol (local IPC; mTLS gRPC for VM workers) | WS-18 | local for the Abbey-side dispatch (WS-7); owner-blocked for the VM fleet (OB-11) |
| 21 | `tasks/todo.md:864` | Completion Program Phase 6 — Two deliberately separate runtime editions | WS-17 | local (the personal-unrestricted runtime); Windows packaging remains owner-blocked (OB-9, OB-10) |
| 22 | `tasks/todo.md:917` | Completion Program Phase 7 — Tauri 2 React/TypeScript desktop product | WS-8 (surfaces) + WS-9 (release) + WS-2 (harness) + WS-13 (Windows) | local for the macOS product; owner-blocked for notarization (OB-1…OB-4) and Windows (OB-9) |
| 23 | `tasks/todo.md:1020` | Completion Program Phase 8 — Local model and media roadmap | WS-12 + WS-15 + WS-16 | owner-blocked (OB-12…OB-17) |
| 24 | `tasks/todo.md:1036` | Completion Program Phase 9 — MCP hosting without exposing the personal shell | WS-11 (client-host; plus the server-side remainder: non-loopback, TLS, OAuth 2.1/PKCE, SSE streaming, `resources`/`prompts`/`tasks`, cooperative cancellation) | local |
| 25 | `tasks/todo.md:1086` | Completion Program Phase 10 — Authenticated three-VM shared-compute proof | WS-18 | owner-blocked (OB-11) |
| 26 | `tasks/todo.md:1102` | Completion Program Phase 11 — Verification and final ledger closure | WS-20 | local |

**Cross-check.** Every unchecked box in the authoring-time `tasks/todo.md` appears exactly once above, and every workstream WS-1…WS-20 appears at least once. Counts: 26 todos; 20 workstreams; owner-blocked rows OB-1…OB-19.

---

## 9. What this plan does not decide

- **Which v4 contract family wins** (WS-4). The plan states the criteria — one family, one wire shape, generated types that cannot drift — and the recommendation, but the choice needs a written decision before code changes, because the repo has already paid for the two-family state once (`.superpowers/coordination/desktop-assistant.md:89`).
- **The exact byte-level contract** for tasks, automations, and actions. The plan fixes the *semantics* and the *evidence bar*; the shapes are WS-4/WS-6's deliverable.
- **The production model host.** `tasks/todo.md:1021` names `google/gemma-4-12B-it-qat-w4a16-ct`; whether that licence is obtainable is OB-13's answer, not this plan's.
- **The order of the federation families.** WS-5 lists a recommended order (Authorize → Cognize → consent → episodes → manifests → changes) that follows the registry's own `next_action` (`src/claims/registry.rs:238`), but each family's adapter choice is its own review.
- **Whether `tasks/todo.md:585`'s historical flake gets a further root cause.** The plan's position is the repo's: the observer defects were diagnosed and fixed; the single 2026-08-09 occurrence was never reproduced and stays unattributed. Closing it as "noise" would be wrong; closing it as "fixed" would be wrong too.
