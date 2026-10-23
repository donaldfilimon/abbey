# Completion Program review fixes and continuation — 2026-10-22

The full Completion Program remains incomplete. This receipt separates the six
review fixes, subsequent locally actionable work, focused regressions, and frozen
functional gates. No capability status is promoted and no native Linux/Windows,
distribution signing, updater, model-quality, live recording, or physical-host proof is added.

## Six review findings

All six dedicated regressions failed against the original implementations
(`six-findings-red.log`). The repairs now:

- Keep pipe draining under the process deadline, including after leader exit.
  Unix readers are nonblocking and cancellable. The group leader remains
  unreaped until pipe closure or teardown, reserving its numeric identity before
  group signalling. Windows Job Objects and native Windows acceptance remain open;
  the portable branch still terminates only the direct child.
- Require valid explicit consent before rollback changes artifacts, pointers, or
  ledger rows; successful rollback records the consent digest.
- Remove a partial artifact when trainer invocation fails or times out.
- Preserve current state when baseline evaluation fails or its artifact is
  missing; record a non-promoting decision and the baseline failure.
- Detect private-key markers through bounded byte windows so adjacent UTF-8
  cannot bypass redaction.
- Propagate terminal re-entry failures while retaining nonfatal repaint handling.

The training suite passed 82 tests; the `review_` selection passed 15 tests,
including the additional PID-ownership and sampler-failure regressions.
The independent guardrail reviewer found no remaining actionable issue in the
revised source. These are fixtures and orchestration proofs, not model training
quality evidence.

## Continued baseline and contract work

`tools/check_completion.py` now enforces the 266 retained historical obligation
mappings, including new plan checkboxes, exact retained locations, duplicate
identity/location rejection, active owners/acceptance, and safe source paths.
Checked history remains mapped and fenced examples are excluded. Five new cases
failed before implementation; all 20 matrix tests now pass.

Claim-vocabulary schemas now identify v1 requests/status as schema 2 and the v3
envelope and grant negotiation as schema 4. V2 remains protocol 2/schema 3 with
unchanged request serialization. Two old-peer regressions failed before the bump.
A further direct-Claims regression proved that merely revising Status was
insufficient: v1 now requires a request schema declaration and rejects missing or
stale declarations before command dispatch. Explicit null declarations are also
rejected, preserving v2's strict admission boundary. V3 rejects stale envelope
schemas before authority dispatch. Positive v1/schema-2 and literal v2 JSON/status
fixtures protect both accepted paths. Old peers are refused; no mutation is
replayed or downgraded to work around schema incompatibility. Canonical Rust
generated the updated TypeScript sample; claims documentation was regenerated
from its registry. Native three-platform acceptance remains required separately.

## Gate failure investigation

The first captured full root gate failed after 475.68 seconds in the accelerator
cohort's metadata PTY quit scenario: the actual leaf readiness record was absent.
The source manifest did not drift. Default, WDBX, and personal cohorts completed;
this is a failed root gate, not a partial pass. Desktop was not started.
An isolated retry also failed metadata cancel readiness.

The owned metadata parent skipped Python site hooks with `-S`, but its descendant
did not. A regression injects a private slow site hook: it ran before the fix and
is skipped afterward. The descendant now uses `-S` too. All three actual PTY
scenarios passed afterward. The production 500 ms deadline was not changed.
This proves the fixture leak and repair; it does not prove site hooks were the
only possible source of scheduling delay in the original failing run.

The complete Python suite passed 209 tests. Existing subprocess ResourceWarnings
remain visible in its retained log. A concurrent ABI EventSink Send-bound compile
failure was retained separately; its owner corrected the source before the live
focused tests passed. No sibling edits were overwritten.

## Additional combination qualification and isolation incident

Clippy, binary builds, and the full Abbey suite passed for each of the four
additional combinations: `wdbx,personal-edition`, `wdbx,accel`,
`personal-edition,accel`, and `wdbx,personal-edition,accel`. Captured source did not
drift. The first personal/accelerator attempt failed three CLI assertions because
I incorrectly ran metadata validation concurrently: `check_claims_sync.py` invokes
`cargo run` and replaced the shared executable with a default build. Its failed
log/results are retained. Serialized retries passed both remaining combinations.

That collision exposed a harness isolation gap: tests supplied only the compiled
edition's state override, so the stale other-edition binary fell back to the real
safe-edition profile. Two stubbed turns appended two route-audit and two
conversation-history rows for the owned qualification workspace, created its cwd
identity, and the collision window contains four fixture memory records. A private
owner-only **post-incident** backup preserves the affected files; it is not a
pre-incident restore point and is not committed to this repository. I marked the
three still-live, exact-workspace/exact-timestamp fixture memory records obsolete
through Abbey's CLI; the fourth was already obsolete. All records and provenance
are retained. An attempted scoped identity clear did not verify success; the cwd
mirror remains, and cleanup is **not complete**. The global chat mirror's observed
mtime remained unchanged. Audit/history were not pruned; the no-argument history
compaction kept 50 while the file has 27 rows. No claim of a pristine profile or
complete rollback is made. `fixture-collision.json` records the bounded incident
without storing memory payloads or raw database contents.

The CLI harness now clears ambient application configuration/credentials and
places HOME, Windows/XDG roots, and **both** editions' state/config paths inside
scratch storage, even for a stale executable. The new real-child environment
regression failed before this change; all 17 CLI-surface tests passed afterward.
The following earlier gate and combination passes do not qualify that final
harness change; the hardened harness subsequently passed serial root/desktop gates in 612.77 s
and 20.76 s, with no captured-source drift (`harness-gate-results.json`). Final
installer qualification and full combination reruns are pending.

After qualification, old generated compiler caches were cleared to manage disk
pressure: 2,009 incremental cache directories and 776,538 retained stale object
files. Sources, product binaries, diagnostic dylibs, and receipts were preserved.
The original oversized cache directory shape is no longer retained; its earlier
loader-probe evidence remains historical.

## Qualification before final harness hardening

Both required functional gates passed serially on the new frozen source capture:

| Gate | Exit | Elapsed | Captured-source drift |
| --- | --- | --- | --- |
| `./check.sh` | 0 | 826.22 s | none |
| `./desktop/check.sh` | 0 | 30.67 s | none |

The root gate includes the default, WDBX, personal, and accelerator cohorts,
warning-denied private rustdoc, 209 Python tests, claims/instructions/history
coverage, generated IPC drift, synthetic read-only boundary checks, and actual
isolated accelerator installer publication/rollback fixtures. Every metadata PTY
scenario passed in the rerun. The unsupported cross-target soft checks were
skipped because their targets were not installed; they are not native proof.
The desktop gate's default owned scratch journeys passed. Its opt-in GUI/menu,
accessibility, signed distribution, and other-host acceptance are not claimed.

This earlier cohort is preserved as `/Users/donald/.codex/verification/abbey-completion-20261022/review-before-fixture-hardening`.
`final-source-manifest.json` identifies all captured bytes (including dirty and
untracked source); its Git HEADs alone do not identify the tested worktree. Rust
1.99.1 remains the qualified compiler. The four additional feature combinations also passed as described above. The
subsequent harness cohort is preserved as `review-harness-qualified`; its exact
bytes and serial passes are in `harness-source-manifest.json` and the harness logs.

Logs and manifests reside beside this receipt. Concurrent live sibling changes
are outside the frozen cohort. Follow-up receipt/ledger additions are evidence
metadata, not executable changes to the tested product. No release was published.

## Remaining program boundaries

Historical coverage is bookkeeping, not execution proof. Durable task/automation
and worker authority, platform transport/services, owned tools and personal shell,
production models/training/media, federation/shared compute, native packaged
acceptance, signing and strict dependency-security closure remain open. Existing
security candidate and native-host prerequisites are unchanged.

## Continued platform repair: accelerator edition isolation

The installer previously published both editions' accelerator library as
`libabi_metal_dot.dylib`. An executed stub regression reproduced replacement of
the other edition's bytes in both install orders. The personal edition now gets
`libabbey-personal-abi-metal.dylib`. Only staged copies have their dependency
commands and library ID rewritten, then are ad-hoc signed and verified before
transactional publication. The safe edition retains its existing layout.

The stub fixture isolates its target and both editions' state/config roots,
including when an ambient target redirect is supplied. Fourteen installer tests
pass. These stubs do not qualify Mach-O relocation: the extended native smoke
requires actual CLI/daemon/library signatures and dependencies, execution, and
byte-preserving coexistence in both install orders. Native execution passed on this Apple Silicon Mac: both editions executed all
three kernels on Metal, dependency identities and signatures verified, both
install orders preserved the other edition byte-for-byte, and existing rollback
faults passed. The unchanged-source root gate passed in 896.67 s followed by the
desktop gate in 21.24 s (`installer-gate-results.json`), with no source drift.
The Python suite now passes 210 tests. Full combination reruns are in progress. This is local installation, not Developer ID
signing, notarization, Windows, or Linux accelerator qualification.

## Next correctness repair: inference admission and budget failures

Three focused regressions failed against the qualified installer baseline:
ordinary multiline/larger prompts were refused; an otherwise valid request could
not reach the full-envelope bound; and ABI budget exhaustion became a generic
runtime failure. All nine focused inference tests pass after the repair. Prompt
controls are limited to ordinary CR/LF/tab, raw and serialized request JSON are
bounded by the shared 1 MiB frame ceiling, and the client checks the complete
bearer/grants/correlation envelope before connecting. Escaping and envelope
overhead cannot bypass the bound. Existing typed budget failures survive the
exact-provider boundary. No sibling pin is promoted; cooperative ABI duration
behavior still needs its separately required upstream qualification.

The installer cohort's combined-feature rerun passed WDBX/personal and WDBX/accel,
then failed personal/accel on the PID fixture reading an empty file between
creation and publication. This is retained as a failure, not a combination pass.
The observer now waits for a parsed PID and passed ten consecutive ownership
trials. The fourth combination was not started. All four will be rerun.

A separate first inference compile attempt captured concurrent runtime work
against an older ABI snapshot and failed before tests. Its compiler output is
retained; it is not RED evidence for the inference defect. The three RED cases
were subsequently demonstrated on the coherent installer baseline. The newer
local-approval test helper has since been corrected in the live checkout.
Full unchanged-source qualification of the new combined input is pending.

## Integrated qualification and additional retained failures

Qualification moved to an independent APFS copy-on-write target; no hardlinks
share mutable build products. Copied local-crate fingerprints and top-level
executables were discarded before rebuild. Live sibling/relay development is
qualified only as exact captured bytes, independently of public CI pins.

The combined input passed nine inference tests, three local-approval tests, and
the initial relay suite. A first root gate failed formatting; its unchanged-source
failed receipt is retained. A later root run failed an approved relay fixture with
`Denied` after a 6.71 s suite. Ten full traced trials then passed in 0.34–0.46 s;
the original failure remains unattributed. A controlled 1.1 s observation delay
reproduced a no-pending/CancelledBeforeDispatch/Cancelled refusal under the
fixture's 1 s real-time budget, with no effect or worker admission. This proves a
possible mechanism, not the cause of the original failure. Temporary diagnostic
code was restored before qualification.

The authorization/reopen fixture now declares a finite 10 s test source budget,
still clipped by its signed-job lifetime. Production deadlines are unchanged. A
separate 5 ms source/50 ms observation regression proves refusal, no effect,
unchanged worker journal, and no local approval. All six relay tests pass, including
crashes after preparation and after the actual file effect. Independent static
review found no actionable fixture issue.

The next root run passed relay tests and exposed the old legacy-migration fixture
expecting schema 10 after the append-only schema 11 addition. The live fixture's
explicit version-11 assertion was integrated; its real-daemon import/reopen and
backup-retention test passes. Failed root receipts are retained. Serial gates are
running again, and all four additional combinations still require their final
rerun. No failed, skipped, or pending check is counted as a pass.

Golden-vector regressions have been prepared as the next coherent correctness
change in the live worktree; they are outside this integrated inference capture
and are not yet qualified. The older patch README supplied as context is a
reference, not an instruction to apply its numeric patch sequence blindly.

### Snapshot timestamp cache invalidation

A subsequent root run failed the same schema assertion despite captured source
expecting 11: its cached integration-test binary still expected 10. Snapshot
updates had used `copy2`, preserving older source timestamps. The separately
compiled focused package variant passed, but did not invalidate the root gate's
workspace feature-unified variant. Hash and mode stability alone did not prove
Cargo rebuilt changed snapshot inputs. The failed 41.52 s receipt remains in
`review-qualified-final/stale-artifact-failure` in the external verification tree.

Before restarting serial qualification, all 4,150 captured source-file timestamps
were refreshed and 207 private Abbey/ABI fingerprint directories were removed.
The target remains independent of the shared canonical target. No source bytes,
qualified pins, or production behavior changed for this cache repair. The new
serial gate result is pending; the failed cached run is not counted as a pass.

Disk-pressure mitigation removed 766 private incremental-cache directories only
when every descendant timestamp predated this qualification start. The canonical
target, source, retained evidence, and current-run cache directories were left
in place. Additional combination runs disable incremental compilation. The
selection cutoff and removed names are recorded externally in
`review-qualified-final/stale-incremental-cleanup.json`.

### Integrated inference cohort serial qualification (2026-10-23)

After invalidating stale local artifacts, root passed in 1,716.29 s and desktop
passed next in 132.55 s, both against unchanged captured hashes and modes.
`integrated-final-results.json` and the matching source manifest identify the
exact local Abbey/ABI/WDBX inputs. These are local source results, not CI-pin,
Linux/Windows native, signing/notarization, or external-audit qualifications.
Unavailable cross-compilation targets remained explicit skips.

The installed coexistence fixture emitted three native accelerator reports;
strict post-run assertions verified initialized Metal, native execution and CPU
oracle parity for dot/cosine/top-k in all three reports. The same run proved
staged-verification and mid-publication rollback. The four extra feature
combinations are the next qualification step.

A subsequent installer candidate limits release builds to `-p abbey --bins`,
so CLI installation does not require desktop GUI/codegen build dependencies.
The executed Unix Cargo stub failed before the fix in both feature modes;
PowerShell source assertions failed as well. All 16 installer tests now pass,
and static review found no actionable issue. Full unchanged-source/native
qualification of that candidate remains pending. No Windows runtime proof is
claimed. The corpus candidate also has a pending path-alias regression from
static review; its implementation is not yet qualified.

The first three additional combinations passed clippy, bin builds, and complete
tests (284.89 s, 232.62 s, 236.41 s), with no source drift. The all-features
combination passed clippy and build but failed its Metal lifecycle fixture:
`BudgetExceeded` was a newly preserved, legitimate one-second cold-start
refusal. That fourth combination is failed, not qualified.

The fixture already allowed three explicit attempts for cold Metal setup but
matched the old generic `Internal` error. Its candidate now recognizes only
`BudgetExceeded` as the daemon budget refusal; other daemon failures remain
fatal, the three-attempt cap is unchanged, and a successful result must still
prove requested/executed Metal, positive native operations, and no fallback or
mixed execution. Production deadlines and client send-once behavior are
unchanged. Three real-daemon focused trials and new full gates are pending.

### Corpus, installer scope, and native fixture follow-up

The corpus candidate computes the independent canonical-byte and aggregate
corpus-digest vectors, rejecting changed expected commitments, duplicate paths,
noncanonical decimal lengths/hashes, unsupported numeric values, and normalized
path aliases. UTF-16 key order and negative zero have independent expected-byte
fixtures. Corpus bytes and their qualified vendor digests remain unchanged.

A concurrent canonical edit had already fixed the alias defect before the
qualification snapshot was captured. The initial expected-RED test therefore
passed. No concurrent edit was reverted. Removing only that guard in the
isolated snapshot reconstructed the defect: all three alias forms were accepted.
Restoring the captured guard passed all seven focused corpus tests. The three
full-corpus integration tests also pass. RED reconstruction provenance is
recorded separately from naturally observed failures.

All 16 installer tests pass on the captured candidate. Three all-features native
Metal fixture trials pass with exact output, nine native operations, and false
fallback/mixed-execution flags. Static follow-up review found no actionable issue.
The first serial runner invocation accidentally retained the prior cohort path;
it was stopped after about 30 seconds, its interrupted log retained, and no
qualification result credited. The corrected runner verifies the intended
candidate files and every captured digest before running serial gates. Root
and desktop qualification of this follow-up is pending.

The next root attempt failed after 319.13 s at the ordinary metadata control,
with no owned probe event. Cancellation and quit cases passed. The cause of
that unanswered 500 ms attempt remains unattributed; a production metadata
probe may legitimately time out before an interpreted fixture starts. Its
failed receipt and prior fixture source remain retained separately.

The control now permits at most three explicit read-only setup calls with new
nonces, only when neither its marker nor any execution event was observed.
Any observed wrong response is immediately fatal, and a probe that never
starts still fails after three reads. Positive proof still requires the exact
selected executable/argv, one real version event, and separate TUI teardown.
Cancellation/quit and production deadlines are unchanged. Two focused unit
failures were observed before this change; all four fixture tests pass after.
Ten native Rust/PTY cohorts then passed, executing 30 scenarios. Static review
found no weakened execution/teardown proof. Setup attempt counts are exposed
in the fixture's JSON proof; they do not establish a timeout cause.

Only the two Python fixture files changed since that root failure. Their exact
source delta is recorded; unchanged Rust artifacts are reused, with normal
per-feature bin builds still enforced by the gate. Fresh serial qualification
is running. No failed attempt is relabeled as a pass.

A later root run passed every Rust cohort but failed the Python toolchain
installer stub after 255.71 s: it expected the old workspace-wide Cargo argv.
The live checkout already had the correct package/bin-scoped expectation;
that concurrent update was preserved and captured. Its 16 focused toolchain
tests pass, as do all 215 Python tooling tests on the corrected snapshot.
The new delta remains Python-only (metadata fixture pair plus that toolchain
fixture); production Rust and dependencies are unchanged. The failed receipt
is retained under the separate toolchain-expectation prefix. Serial gates are
running again; no pending fourth combination or interrupted run is a pass.

The corrected corpus/installer snapshot passed root (528.72 s) followed by
desktop (45.21 s), with no source hash or mode drift. Three strict installed
Metal reports contain native dot/cosine/top_k execution and oracle parity;
these remain macOS evidence only. Exact source, results, provenance and logs
are retained under `corpus-installer-final-*`. The checkpoint copy has identical
manifest bytes; logs identify the original compiled workspace path.

Ruling: run all four additional feature combinations on the following snapshot
containing the desktop read responsiveness fix. That fix changes desktop Rust
only; core sources remain identical. None of those four checks is yet credited
as passing for this snapshot, and all four remain required after serial gates.

Desktop read responsiveness follow-up moves eleven blocking Tauri read bridges
to the blocking-worker pool. Names, arguments, successful payloads and read
grants are unchanged; worker-join failure returns a sanitized transport error.
A compile-time regression first refused all eleven synchronous signatures;
the new helper also proves a blocked read yields on the polling thread and
keeps panic payloads out of IPC errors. The full desktop Rust suite passes
(69 unit and 3 updater integration tests). Independent static review passed
spec compliance and code quality. Serial root/desktop qualification and the
four additional core feature combinations are running/pending; packaged
responsiveness, native Windows/Linux, accessibility and performance acceptance
remain separate requirements. Logs and scoped review use `desktop-read-*`.

The desktop-read snapshot root attempt failed after 96.86 s at the metadata
cancellation fixture, with no source drift. Desktop and extra combinations
did not run. The queued marker was absent; the screen showed the metadata
execution deadline followed by ordinary prompt execution. The fixture waits
for call and leaf readiness before submitting its queued draft, consuming the
real 500 ms deadline. Precise scheduler timing remains unattributed. The
failed receipt is retained as `desktop-read-metadata-cancel-failure-*`; it is
not a pass. Fixture-order correction is under investigation, preserving all
positive queue/cancellation/descendant teardown assertions and production bounds.

Batching corrected the demonstrated queue-input ordering defect, but its
repeat trial later failed at a different boundary: three full cohorts passed,
then the fourth cancellation had no actual metadata event. The precise cause
remains unattributed. Both the failure and partial successes are retained.

The version fixture now uses a tiny test-only POSIX C executable compiled during
setup, before the real 500 ms metadata probe begins. That path emits actual
process identities, forks and reaps a real leaf, and uses signal handlers only
to set flags. Non-version requests still execute the original Python generation
stub with unchanged arguments; positive forwarding tests protect the queued
no-contact assertion from becoming vacuous. Missing/failed compiler is a failure.
The poisoned-interpreter regression failed on the previous fixture; all nine
focused tests now pass, and independent static review found no actionable issue.
Ten native control/cancel/quit cohorts passed (30 scenarios), each ordinary
control succeeding on its first read. Compiler/source/binary identities and
results are retained. This removes interpreted startup from the timed version
path; it does not attribute the previous failure or guarantee execution under
arbitrary scheduler suspension. Production limits and positive teardown proof
remain unchanged. Full serial qualification still requires a fresh run.

The integrated desktop/Linux-controller/Windows-CI/native-metadata snapshot
passed root (546.36 s), then desktop (25.88 s), serially with no hash/mode drift.
Root ran 243 Python tooling tests. All four additional feature combinations
then passed warning-denied clippy, binary build and full tests on that identical
snapshot: WDBX+personal 96.81 s, WDBX+accelerator 98.63 s, personal+accelerator
96.59 s, all three 100.15 s. The previous all-feature failure remains historical;
this fresh run passes. Logs/manifests/results use `platform-tooling-*`.

Three strict installer-section reports prove initialized native Metal kernel
execution and oracle parity. A separate preceding Rust-unit JSON report is
not counted as installed evidence. Signatures, edition coexistence and rollback
fixtures passed. Cross targets absent on this host were explicitly skipped;
Ubuntu/Windows native, public CI-pin qualification and signed distribution remain
separate. This gate is not globally warning-free: the retained WDBX import
warning and two claims-tool timeout PIPE ResourceWarnings are visible. The latter
have a separately reviewed local fix pending the next integrated gate. No
failed, skipped or pending channel is relabeled as passing.
