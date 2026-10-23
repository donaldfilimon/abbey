# Completion Program: compilation recovery and correctness continuation

The program remains incomplete. No milestone is closed and no native Linux or
Windows, signing, production-model, or shared-compute evidence is asserted.
Earlier failed attempts remain in the baseline and first-wave receipts.

## Compilation recovery

The qualified Rust 1.99.1 compiler could compile standalone code but stalled
loading cached proc-macro libraries. `codesign -d` also timed out on a library
in `target/debug/deps`; an identical copy in scratch or a smaller sibling
directory responded immediately with its existing linker signature. Replacing
one file in the original directory did not resolve the behavior.

The directory contained 795,242 entries, including 776,538 object files.
Retaining it as `target/debug/deps-retained-20261022` and reconstructing `deps`
with its non-object artifacts allowed Cargo check, compilation, and the gates
to finish. This supports a cache-directory pathology; it does not establish a
unique filesystem or code-signing implementation cause. No signature, compiler
pin, security setting, or source dependency was changed as a workaround.
Retained and reconstructed artifacts use hardlinks and are not an immutable
byte backup. They are disposable generated cache entries, not user data.
The repeat loader probes are in `2026-10-22-completion-recovery/loader-probes.json`.

## Correctness changes and focused acceptance

- Automatic Ollama selection resolves the installed executable without probing
  an unrelated default model. Generation retains the bounded exact requested-tag
  check and refuses missing weights without pulling. The new process regression
  observed two probes before the repair and one afterward. All four no-auto-pull
  process tests passed. Windows portable readiness and process-tree supervision
  remain open; the Unix supervisor is not relabeled portable.
- Repair the claims registry's reference to the removed fake worker-inventory
  test. It now names the real no-worker-authority regression. The CI description
  recognizes the authored guarded Linux job while retaining Blocked status for
  absent Linux/Windows executed proof. Generated claim regions were regenerated.
- The synthetic guild engine admits at most 4 MiB before deserialization, rejects
  self/multi-node parent cycles iteratively, offers only applicable permission
  alternatives, and skips unchanged overwrites. Only explicit `do-nothing` can
  carry an intentionally empty plan.
- Replay schema 2 adds `present` to permission preconditions, postconditions,
  and rollback previews. An absent overwrite can therefore be distinguished
  from a present zero-bit overwrite, including in the plan digest. Recording
  input schema 1 remains supported. Visibility evidence binds the bot identity,
  role membership, owner identity, and role permissions as well as the channel.
  This is data-only synthetic replay; no transport or effect authority is added.

All five synthetic regressions failed against the prior implementation. The
repaired focused suite passed 26 tests: 11 permission, 5 regression, and 10 replay
tests. An older regression that explicitly expected ineffective empty plans was
updated to require refusal. Independent static guardrails review found no
actionable issue; it ran no builds and does not replace execution evidence.
RED and GREEN logs are retained in the adjacent evidence directory.

## Gate boundaries

The first recovered full root run passed all Rust cohorts but failed claims
synchronization on the stale worker test reference. After the repair, full
`./check.sh` passed, followed serially by `./desktop/check.sh`. These runs precede
the synthetic-engine edits. The root run includes default, WDBX, personal, and
accelerator lint/tests/docs, Python checks, IPC generation, and isolated native
accelerator installer/rollback fixtures. Desktop adds frontend and bundle checks,
edition tests, scratch-daemon reads, configured-daemon refusal, and native linking.
Opt-in packaged GUI, accessibility, signing, and external acceptance were not run.

Abbey and ABI bytes remained stable during that root run, but seven WDBX paths
changed concurrently, including a Rust module. Consequently its exit-zero log
is valid observed local evidence, not the required frozen-source qualification.
The source manifest and both gate logs are retained alongside this receipt.
An isolated three-repository candidate is used for the post-repair serial gates;
their exact outcomes must be recorded separately before qualification is credited.

The post-repair frozen-2 root gate subsequently passed in 789.80 seconds with no
source or mode drift. Desktop's first isolated run failed five receipt-harness
tests because the copy lacked Git metadata, before product checks. Separate Git
metadata was initialized to reference the existing base commit through the
original object database; no commit was created and no original ref was changed.
The desktop retry passed, again with no source/mode drift. Its base Git HEAD is
not the identity of the modified tested bytes: `frozen-2-source-manifest.json`
is that identity. The original failed desktop run and successful retry remain
separate logs/results. Both complete gates therefore pass for the repaired
product source under Rust 1.99.1 on this macOS ARM64 host.

A byte comparison afterward found canonical Abbey's product code, manifests and
lockfile identical to the tested snapshot. Three captured documents differed:
the active plan, this evolving receipt, and the requirement matrix. The historical
mapping and model metadata were added outside the frozen cohort. Their direct
coverage/structural checks are supplemental evidence, not a rewritten root log.

## Continuing the requirement and model work

`tasks/completion-historical.json` maps all 266 unchecked occurrences across five
retained plans without editing their checkboxes. Each occurrence names a current
requirement, milestone, acceptance criteria and blocker owner. Independent review
checked exact source/path/line/text coverage and identified distinct obligations
that now have dedicated requirements: served MCP conformance, portable packages
and services, three-platform artifact CI, and physical/geographic federation.
Linux service foundation has a separate minimal-install requirement. Final
installed-product journeys belong after runtime/tool authority, avoiding a
semantic dependency cycle in the platform foundation. The active matrix now has
44 requirements across nine milestones; its canonical TODO/goal coverage checker
passes. Historical mapping remains an audit artifact, not a new gate or proof
that any historical implementation is complete.

The exact model target is recorded in `2026-10-22-gemma12b-feasibility.json`.
The publisher API supplied immutable revision, file sizes and large-artifact
SHA-256 identities; small configuration files were fetched at that revision and
verified against their published Git blob identities. Large weights/tokenizer
were not downloaded, hashed locally, accepted in a license ledger, or loaded.
The publisher declares Apache-2.0; this is not owner acceptance or a legal review.
ABI's current provider accepts only its bigram fixture architecture, so the
Gemma4Unified/compressed-tensors target remains unavailable with no substitution.
The actual 24 GiB Apple M5 host exists, but no production-model residency,
4K/64-token, quality, performance, or sustained-swap result has been measured.

## Security channels and unpromoted dependency candidate

`cargo audit --deny warnings` with only the repository's existing two exceptions
failed for the frozen lockfile: Tokio 1.54.0 (`RUSTSEC-2026-0384`) and ZIP 4.6.1
(`RUSTSEC-2026-0367`, `0368`, `0369`). ZIP enters through the Windows Tauri updater;
source inspection confirms its extraction call, but no native Windows exploit or
updated-package acceptance was executed. These actual scan failures block release
qualification independently of successful functional gates.

`security-candidate.patch` is prepared in a separate qualification copy. It selects
Tokio 1.54.1 and the exact upstream updater ZIP-9 repair commit
`ba98bfadc4c868c9a7e1a226c83af528e1f11b89`. The full Cargo-generated lock delta is
retained: it removes ZIP 4 and its arbitrary dependencies, uses the existing ZIP
9 version, and re-resolves some `windows-sys` and `syn` edges. Qualification must
judge the full graph, not only two named version changes. Canonical pins remain
unchanged while this candidate is unqualified. Candidate functional gates are
recorded separately when finished; native Windows update/install proof remains
required before promotion.

The candidate audit has zero vulnerability-category findings but still exits 1
for denied `RUSTSEC-2024-0370` (unmaintained proc-macro-error) and
`RUSTSEC-2024-0429` (GLib VariantStrIter unsoundness). No new ignore was added.
This is not a passing dependency/security scan or a completed remediation.

Gitleaks 8.30.1 ran a redacted directory scan with default rules and no added
allowlist. Its raw exit was 1 with 71 matches. Exact per-location triage classified
69 as source-manifest SHA-256 values, one as a presentation storage namespace,
and one as a literal secret-redaction test fixture; none remained unclassified.
The sanitized triage is retained. This is not a Git-history scan or independent
external security review, and the raw scanner result is not relabeled exit zero.

Fresh infrastructure reads found only an offline macOS ARM64 GitHub runner and
no available Linux/Windows host in the connected-device inventory. No CI run,
runner provisioning, signing credential change, publisher action, live recording,
or personal-shell authority was performed.
