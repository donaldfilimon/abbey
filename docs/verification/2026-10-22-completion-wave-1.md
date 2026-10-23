# Completion Program — first implementation wave

**Program status: incomplete. No milestone or capability is promoted.**
The active plan is `docs/superpowers/plans/2026-10-22-abbey-completion.md`.
`tasks/completion.json` maps all 48 open canonical workflow-ledger checkboxes to
38 requirements across nine milestones, including separate final qualification.
Historical plan checklists remain intact and explicitly superseded for execution;
their distinct remaining obligations have an open reconciliation requirement.

## Implemented source

- Add a coverage/evidence checker and required gate step. Duplicate historical
  reports have one logical owner. Every unfinished goal and open todo is mapped.
  Required evidence channels are explicit: failed signing/CI/hardware/license/
  consent evidence cannot be ignored when completing a requirement or milestone.
  This is bookkeeping, not independent proof of referenced evidence or capability
  implementation. All milestones remain open or in progress.
- Restore the guarded Linux ARM64 primary workflow with the same trusted-head,
  immutable-pin, read-only-token, scratch-state and cleanup protections as macOS.
  Both Unix jobs now require `desktop/check.sh`; the executed-run verifier requires
  that step too. No job was dispatched, runner registered, or availability variable
  enabled by this task. Windows native CI and transport remain unimplemented.
- Move pure argument budgeting into `src/argv_budget.rs`, shared by Doctor's
  reported ceiling, process supervision, and all backend builders. The Unix
  prompt ceiling is now 32 KiB; the supervisor stays at 32 KiB per argument and
  64 KiB aggregate. Fixed flags/context consume the aggregate before prompts;
  fixed configuration is never silently truncated. UTF-8 truncation preserves
  the existing notice. Windows retains its 24 KiB per-prompt ceiling; Windows
  command-line quoting and native process acceptance remain unproved.
- Cross-provider TUI changes persist `system` for FM and `auto` for other
  destinations, clearing the previous explicit-model flag only after successful
  persistence. Same-provider selection preserves its explicit model. Regressions
  cover every ordered provider pair and failed-persistence identity preservation.
- Stop advertising `ReadWorkers` without a startup-owned worker adapter. Remove
  the always-empty worker/cluster dispatch; worker requests now fail closed.
  The regression requires denied negotiation and direct-command refusal in both
  editions. No worker implementation or new authority is claimed.

## Verification actually observed

| Check | Result and scope |
| --- | --- |
| `python3 -m unittest discover -s tools/tests -p 'test_*.py'` | 202 passed; final recheck also passed 202 tests after the desktop-step guard extension |
| Completion checker regressions | 14 passed; the two required-evidence regressions were observed failing before the repair |
| Workflow guards | 60 passed; Linux-job and required-desktop-step regressions each failed before their implementation |
| Executed-run verifier tests | 16 passed after requiring the desktop step |
| `rustc --edition=2024 --test src/argv_budget.rs` with the pinned compiler | 5 passed; pure production helper code only, not application/process integration |
| Original HEAD host-ceiling component | Extracted actual original `max_prompt_argv_bytes` into a standalone pinned-compiler test; the <=32 KiB assertion failed (exit 101), confirming the old 96 KiB mismatch |
| Frontend `bun run typecheck`, `bun test tests`, `bun run build`, `bun run verify:bundle` | All exited 0; 178 tests, 2,079 assertions, 22 files; bundle verifier checked four built assets. This is existing frontend acceptance, not a native WebView/package proof |
| Instruction split, Program 3 read-only boundary, completion coverage, diff whitespace | Passed during the wave; final recheck recorded below |

The full Python run emitted existing ResourceWarnings about unclosed file handles
but exited 0. No warning was relabeled as a failure or silently repaired here.

## Unqualified or blocked evidence

The pre-edit root/desktop attempts and full input manifests are retained in
`2026-10-22-completion-baseline.md`. Root compilation stalled; desktop passed
100 Python harness tests but stopped at Cargo codegen. Neither gate passed.

Early application regression commands using `--locked` were refused because
shared sibling manifests and lockfiles were changing. No assertion result was
obtained. A later worker-negotiation attempt compiled dependencies, then stalled
in the Abbey test crate and was explicitly terminated; the before-assertion RED
test was **not** observed. Its source repair and application regressions remain
unqualified. The provider application test similarly has no passing result yet.
The old-ceiling standalone RED result does not stand in for either application
test. These are explicit deviations from full application-level RED/GREEN proof.

ABI and WDBX gained concurrent edits after the clean baseline. Abbey's lockfile
gained `libc`/`nix` edges for `abi-worker` outside this task; preserve those changes
and do not attribute them to this patch. No sibling source or dependency pin was
edited by this task. A guardrail reviewer accidentally started claims manifest
generation during the focused Rust attempt, then interrupted it and terminated
its orphaned Cargo process. That attempt produced no validation result; do not
qualify overlapping build evidence. Subsequent final gates must be separate.

## Rulings and next qualification

- The requested full program needs durable tracking before authority expands;
  coverage is limited to the canonical workflow ledger. Distinct historical-plan
  obligations remain an explicit open requirement, including source modes,
  isolated-copy equivalence, stale-executable negative controls and content bounds.
- Factor argv ceilings/truncation into one pure production module so its actual
  arithmetic can be tested without procedural-macro dependencies. Those tests
  cannot qualify backend spawning or Windows quoting; application fixtures remain
  required. The cost if this boundary is misunderstood is an overstated proof,
  prevented by keeping application evidence blocked and milestones open.
- Keep the supervisor ceilings unchanged and lower prompt admission, as approved;
  oversized multi-argument input may be truncated sooner, with the existing notice.
- Independent static guardrail review found no capability promotion, generated
  region edit, or new rule violation after the tracker repairs. Full root/desktop
  gates, native platforms, signing, real CI, model/training/media hardware and VM
  proofs remain required. No commit, push, publication or live-service action occurred.

## Final serial qualification attempts

Both attempts used the exact Rust 1.99.1 toolchain and `CARGO_BUILD_JOBS=1`.
The wrapper bounded root to 180 seconds and desktop to 120 seconds, creating a
separate process session for each and terminating only that owned session on
expiry. **Both wrapper exits were 124; neither gate completed or passed.** Root
stopped in initial compilation; desktop passed 100 Python harness assertions and
stopped during Cargo codegen. These operational timeouts are not failed source
assertions or product performance measurements.

The complete before/after file-byte and permission-mode manifests and outcomes
are retained below. Each aggregate hashes compact sorted-key UTF-8 JSON of its
filename-to-`{sha256,mode}` map (absent tracked paths use `{missing:true}`;
symlinks hash `symlink:` plus their link text).

| Gate | Abbey unchanged | ABI unchanged | WDBX unchanged |
| --- | --- | --- | --- |
| Root | yes | yes | no |
| Desktop | no | no | no |

During desktop qualification the only Abbey source-tree drift was two blank
lines removed from the generated region of `desktop/README.md`, outside this
writer's edits. ABI documentation changed; WDBX source and execution evidence
also changed. Preserve all concurrent work. Even an eventual green command
would need its source-cohort limitations stated rather than silently promoted.

After the last source repair, the complete Python suite passed **202 tests**;
the production pure argv component passed **5 tests with `-D warnings`**; package
format checking, instruction split, Program 3 boundary, completion coverage and
`git diff --check` passed. The earlier frontend run passed 178 tests plus typecheck,
build and bundle security. The edited generated claim regions in AGENTS.md,
CLAUDE.md and tasks/goals.md were byte-compared with HEAD and remain unchanged.
Claims-manifest/codegen execution, application-level Rust regressions and complete
root/desktop gates remain unqualified. Receipt-only additions after the frozen
attempts are separate from the source manifests they describe.

The independent static reviewer found no remaining blocking or should-fix issue
in this wave, including worker denial and the required desktop CI step. This
review is not native/runtime/signing evidence. All nine milestones remain open
or in progress; the full program is not complete.

## Retained final artifacts

| Artifact | SHA-256 |
| --- | --- |

| [final-before.json](2026-10-22-completion-wave-1/final-before.json) | `d78aef77c3e8927c986c7d1716cf2a4d3a415c69af97242328fbd18bae45231e` |
| [final-after-root.json](2026-10-22-completion-wave-1/final-after-root.json) | `680b8e9c7f596a5df150a7813b43e1306793576c5a03fcfb057b0dd7dd7447f3` |
| [final-after-desktop.json](2026-10-22-completion-wave-1/final-after-desktop.json) | `0f13d32c3ab2b1bc478356e05ca4e471aec93a112ab1ef9fd63c599338af71e2` |
| [final-results.json](2026-10-22-completion-wave-1/final-results.json) | `b222b6ce280d7bada4dedb8269905f197861efde312c6cfef0a9b5a4823d5df2` |
| [final-root.log](2026-10-22-completion-wave-1/final-root.log) | `c0d1d4a57d1e663ce88c762b03d4e2b44c6ed1e91a37a6e87af2c6a9ec81a6f4` |
| [final-desktop.log](2026-10-22-completion-wave-1/final-desktop.log) | `cd40f1dceaa91fc1492a9cd4d68e81fd0edff1d699b009f0934918ddda999c68` |
| [final-python.log](2026-10-22-completion-wave-1/final-python.log) | `c0efd5550b444f18c45310bebc489adb00bbd29743d8ad50cc9a854ca84a347a` |
| [final-budget.log](2026-10-22-completion-wave-1/final-budget.log) | `6defd495ed89df1cba17cb2b23f0f3e03f5d52a246c5f7a859c066871e6a3f13` |
| [original-prompt-ceiling.rs](2026-10-22-completion-wave-1/original-prompt-ceiling.rs) | `73166eb20882a0073d06c3dc6fe18d1d20185585a9be2e725230b4d57dd31106` |
| [original-prompt-ceiling-red.log](2026-10-22-completion-wave-1/original-prompt-ceiling-red.log) | `07edc2966488513d1a3bc27e236ad7e4ddec2db3c1492ea124b081190caa01f7` |
| [provider-regression-before.log](2026-10-22-completion-wave-1/provider-regression-before.log) | `b31b7bb1bc9977303338ce6a5bbee9f6ce64b8ded070ce33976dc1bb38e2de70` |
| [linux-workflow-red.log](2026-10-22-completion-wave-1/linux-workflow-red.log) | `13cbdbe76323601725bb5e80a5e262c666298dd048c155b7de040a299c33e135` |
