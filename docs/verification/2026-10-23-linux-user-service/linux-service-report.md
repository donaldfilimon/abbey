# Linux user-service bootstrap implementation report

Implemented source only in canonical shared checkout, 2026-10-23. Ownership:
- `tools/linux_user_service.py` (new)
- `tools/tests/test_linux_user_service.py` (new)
- `docs/linux-user-service.md` (new)

Read linux-service-brief.md and repository AGENTS.md. Independently inspected
`src/edition.rs`, `src/app_core/contracts.rs`, `src/commands.rs`, daemon config
and daemon executable startup contract. No Rust/Cargo builds, installed service
operations, profile writes, external services, commits, pushes, subagents or
completion-ledger changes performed.

TDD receipt: first added behavioral tests, then ran
`python3 -m unittest discover -s tools/tests -p test_linux_user_service.py`.
Expected RED: import fails because implementation file did not exist. Recorded
in `linux-service-red.log`. Implemented after that failure. Latest same command
passes 10 focused tests; full output in `linux-service-green.log`. All writes
and service commands in tests use temporary HOME trees and fake executables.
Tests include both edition paths, authenticated edition mismatch, platform
refusal, token retention, unrelated-unit refusal, insecure file/directory and
foreign-owner refusal, missing daemon, unknown identity, output ceiling,
publication failure preserving previous unit, symlink refusal, quote escaping,
no enable/start during install, uninstall state retention, and a real `/tmp`
scratch directory whose resolved root-owned sticky ancestor is allowed.

Implementation uses explicit install/start/status/stop/restart/uninstall. Only
`install` creates credentials/unit; it does not start or enable. Credentials
remain owner-only and retained. Exact generated-unit comparison refuses unknown
units and relocation without explicit removal. Shell-free subprocesses have
three-second deadline and 64 KiB stdout bound using selector-driven pipes;
child stderr never propagates. Child holding a pipe through a descendant still
hits the deadline; finally kills/reaps the direct child, not a descendant
process tree. A fixture is trusted executable test input, not arbitrary process
supervision authority. No bearer bytes go into unit/argv/log output. Daemon
runs via env -i with edition-specific bearer-file/socket/state and fixed HOME,
PATH; no inline credential or model settings inherit. Unit quoting distinguishes
systemd specifier escaping from ExecStart dollar escaping.

Readiness checks existing authenticated CLI status JSON (event=status,
state=ready, protocol=1, schema=2, exact standard/personal edition), up to three
bounded attempts; failure requests stop and reports failure. No is-active proof.
Uninstall disables/removes only exact unit; token and all runtime state remain.

Limits and pending evidence:
- Native Ubuntu ARM64/systemd acceptance remains pending. macOS Python fixtures
  establish controller behavior only. Windows remains unavailable.
- No live scratch Rust daemon was started under this assignment's fixture-only
  restrictions; real transport assertions remain in existing Rust daemon suites.
- Main refuses macOS before operations; explicit platform injection is Python
  constructor-only test seam, unavailable through CLI/env.
- HOME-based paths only; XDG config/state overrides are intentionally unused.
- Conservative real-path/ownership/mode requirements may reject symlink installs
  or existing permissive state roots. No silent chmod/adoption.
- Exact unit content binds binary/workspace paths. Reinstall with changed paths
  is refused; stale configuration must be explicitly uninstalled with its old
  invocation. No dynamic discovery.
- Atomic staging preserves previous content if publication fails; daemon-reload
  failure leaves published unit for inspection/retry and reports failure. New
  credentials persist after failures. No enablement, login startup, package
  distribution, desktop discovery, provider authority, or production hardening
  claim added.
- Root/desktop gates and Abbey guardrails review belong to controller; this slice
  does not claim those gates passed or linux-service-foundation completed.

## Round 1 failure-atomicity review correction (supersedes reload limitation above)

Read linux-service-task-review.md and fixed only the reviewed installation
failure contract within the original three-file ownership. Pre-fix copies are
in linux-service-round1-before/{tools/linux_user_service.py,
tools/tests/test_linux_user_service.py,docs/linux-user-service.md}; scoped
unified diff is linux-service-round1.patch.

Behavioral RED was observed before implementation: fake-systemctl daemon-reload
failure left a new unit present and replaced previous mode 0400 with 0600.
Command: `python3 -m unittest discover -s tools/tests -p test_linux_user_service.py`.
Result: 12 tests, 2 failures (linux-service-round1-red.log).

Installation now snapshots the validated existing unit bytes/mode, atomically
publishes, then validates/reloads. Post-publication validation/reload failure
restores the exact previous bytes/mode or removes the newly created unit; it
retains bearer and runtime state and attempts daemon-reload again. A failed
file rollback explicitly reports unit rollback failure; a failed rollback reload
explicitly reports that filesystem state was restored but manager reload failed.
Successful rollback/reload still reports installation failure. Documentation
no longer describes leaving a published unit on reload failure as acceptable.
No change to process/time/output limits or unrelated functionality.

First post-fix run observed one identity fixture command hitting the existing
three-second timeout: 12 tests in 34.501s, 1 error. Cause unestablished; no host
load attribution is claimed. Transcribed original tool output is retained in
linux-service-round1-first-postfix-failure.log. The later suite added two
diagnostic branches and then one successful rollback/reload test; it is not an
unchanged full-suite rerun. Intermediate full run: 14 tests in 11.455s, OK.

Final full command above: 15 tests in 9.385s, OK, exact output retained in
linux-service-round1-green.log. Coverage now includes first-install removal,
existing bytes/mode restoration, credential/runtime retention, failed rollback,
failed rollback reload, and successful rollback reload. Fixtures alone run; no
Cargo, actual service, real profile, agent, commit or external service actions.
Native Ubuntu/systemd acceptance and independent controller rereview remain
pending. Edits stopped after this report for scoped rereview.

## Identity-probe isolation correction

Read linux-service-identity-investigation.md. Pre-fix3file copies retained under
linux-service-identity-before/; scoped diff linux-service-identity.patch. Only
tools/linux_user_service.py, tools/tests/test_linux_user_service.py and
docs/linux-user-service.md edited.

Before implementation, new eager-initialization fixtures reproduced actual CLI
startup behavior: mkdir edition state/by-cwd0755 before printing identity.
Full focused command `python3 -m unittest discover -s tools/tests -p
test_linux_user_service.py` observed RED18tests6.685s4failures: both editions
polluted intended service roots, failed identity used targetHOME, and preexisting
state gained by-cwd. Exact log linux-service-identity-red.log.

Controller now wraps both probes in one TemporaryDirectory with resolved private
root, private HOME/USERPROFILE, XDG config/state/data/cache/runtime directories,
and explicit separate safe/personal state/config paths. Probe env is separate
from normal control/service env and contains fixedPATH only beyond those roots.
Temporary roots clean up after successful probes, command errors, and unknown
identity rejection. No service state chmod/adoption. Documentation explains
initialization and containment.

Initial18test post-fix full run passed8.126s (observed original tool output).
After adding unknown-identity cleanup and opposite-edition env assertions,
expanded19test run failed10errors48.663s; unchanged rerun failed1failure13errors
56.381s. Logs preserved as linux-service-identity-expanded-first-failure.log
and linux-service-identity-unchanged-failure.log. Errors were bounded command
timeouts, sometimes wrapped in expected install rollback failure diagnostics.
No host-load cause is asserted.

Fixture interpreter inspection: `/usr/bin/python3 -S` reports executable
/Applications/Xcode.app/Contents/Developer/usr/bin/python3,3.9.6, while harness
uses HomebrewPython3.15. Fixtures previously selected that Xcode interpreter
through env fixedPATH. Changed only generated Python fixture shebangs to
explicit current sys.executable with -S, isolating fixture interpreter/startup
hooks. This does not establish the precise timeout cause. Production child
three-second/64KiB limits remain unchanged.

Final same focused command:19tests5.562sOK, linux-service-identity-green.log.
Coverage includes eager startup for both editions, no intended-state creation
before install, private0700 service state after install, removed probe roots,
command-error and invalid-identity cleanup, no scratch config/state leakage into
service env, unchanged/refused preexisting0755 state, and prior lifecycle and
rollback cases. All fixtures use scratch directories.

No Cargo, actual installed service operations, agents, commits or real profile
writes. Actual scratch daemon proof is delegated to controller after report;
this fix claims fixture evidence only. Native Ubuntu/systemd acceptance remains
pending. Edits stopped for controller proof and independent rereview.

## Exact supported status-version pair correction

This section supersedes earlier readiness assertions limited to protocol1/
schema2. Read-only investigation linux-service-status-investigation.md verifies
current DaemonClient prefers protocol2/schema3 and accepts readonly compatibility
protocol1/schema2 only after explicit unsupported_version. Controller-captured
linux-service-actual-status.json is a ready standard event with2/3; previous
Python check incorrectly rejected it.

Saved3file preimages under linux-service-status-before/ and scoped diff
linux-service-status.patch. Added exact-pair behavioral tests for both editions,
plus crosspair1/3 and2/2, legacy1/1, unknown2/4 and3/3, version0, strings,
booleans and wrong edition negatives. Before correction full focused command
`python3 -m unittest discover -s tools/tests -p test_linux_user_service.py`
observed RED21tests6.104s2errors1failure: both current2/3 edition cases rejected,
and True/2 improperly accepted due Python bool/int equality.
Exact log linux-service-status-red.log.

Fix requires exact integer types (excluding JSON booleans) and tuple membership
in {(1,2),(2,3)}, preserving status kind, ready state and edition checks. No
transport negotiation, fallback, extra authority, retries or bound changes.
Existing typed CLI remains responsible for negotiated capability validation.
Docs list exact accepted pairs and describe existing typed-client behavior.
No identity or Windows changes.

Final full focused command above:21tests6.265sOK, exact log
linux-service-status-green.log. Only original3owned files edited. No Cargo,
actual daemon/service operations, agents, commits or real profile writes.
Controller real scratch daemon proof and independent rereview follow; native
Ubuntu/systemd acceptance remains pending. Edits stopped after this report.
