# Abbey Completion Program — active execution plan

Authority: Donald approved implementation of this program on 2026-10-22,
including unrestricted personal-edition structured execution and native
acceptance on macOS ARM64, Ubuntu ARM64, and Windows 11 ARM at every milestone.
This document supersedes earlier completion plans **for execution order and
policy choices**. Earlier plans, specifications, source, fixtures, and receipts
remain historical evidence. A plan is not capability evidence or permission to
contact live services, publish, accept model terms, or use absent credentials.

## Execution contract

- Retain Rust 1.99.1, the single Cargo workspace/lockfile, exact qualified
  dependency pins, existing user state, and canceled Zig history.
- Work in the canonical checkout. Preserve other work; do not commit, push, or
  publish unless requested. Gate executions are serial and bind exact complete
  Abbey/ABI/WDBX source manifests; local siblings are not necessarily CI pins.
- `tasks/completion.json` is the active requirement-to-milestone matrix.
  `python3 tools/check_completion.py` checks every open checkbox in `tasks/todo.md` and
  unfinished goal in `tasks/goals.md`. Historical plan checklists are preserved
  but are not themselves live ledger inputs. `tasks/completion-historical.json`
  maps all 266 unchecked occurrences in five retained plans to requirements,
  acceptance criteria and blocker owners, including superseded process steps.
  It is a reviewed audit artifact, not yet enforced by the coverage checker;
  `retained-plan-reconciliation` owns further source/evidence revalidation.
  Duplicate reports share one requirement. Checked history
  may retain its binding. Adding or changing a checkbox requires reconciliation.
- The matrix records source, automated tests, native platforms, CI, signing,
  and external evidence separately. Its checker validates bookkeeping, not the
  truth or freshness of a referenced receipt. `src/claims/registry.rs` remains
  the capability authority; existing goals/todos remain the workflow ledger.
- `implemented` is not `complete`. A requirement closes on source and test
  receipts plus its explicit acceptance criteria; a milestone additionally
  needs all three native-platform receipts and completed dependencies. Required
  signing, hardware, CI, license, or consent proof cannot be replaced by a green
  coverage check. A failed external channel preserves valid local evidence.
- Missing hosts, keys, licensed artifacts, or consent block their acceptance
  only. Continue independent implementation without declaring the parent done.

## Ordered milestones

### 1. Baseline and coverage

Capture complete source manifests, revisions, compiler and dependency pins,
lockfiles, generated IPC, and root/desktop results before product edits. Map all
open ledger entries and review findings to an owner, acceptance criteria, and
milestone. Preserve historical duplicate reports without duplicating their work.
The first receipt is `docs/verification/2026-10-22-completion-baseline.md`.

### 2. Three-platform foundation

Implement authenticated owner-restricted Windows named pipes and Job Object
process-tree supervision with the Unix framing/correlation/deadline/cancellation
contract. Complete Linux daemon/service and desktop integration. Restore guarded
Linux ARM64 CI and add Windows ARM runtime qualification; preserve trusted-head
guards, read-only tokens, scratch state, cleanup, and actual executed-job proof.
Isolate installed accelerator libraries, model stores, credentials, services,
state, and updater paths by edition. Close only after native installer/process
and CI evidence exists on all three hosts.

### 3. Existing correctness and contract defects

Recompute and persist a valid model on every provider transition; preserve an
explicit model when reselecting the same provider. Probe Ollama only when a
generation route and requested tag are known; use bounded portable capture off
Unix. Match prompt/Doctor admission to the supervisor's 32 KiB per-argument and
64 KiB aggregate ceilings, including non-prompt arguments.

Version affected v1/v3 vocabularies and refuse incompatible schemas before
serving changed values. Bound inference by serialized frame size, accept normal
multiline whitespace, and preserve typed budget failures. Repair upstream ABI
duration handling and corpus schemas/docs before qualified re-vendoring or pin
updates; compute golden vectors instead of trusting expected values. Repair the
synthetic engine's alternatives, rollback absence, authority binding, allocation
bounds, and cycles. Recheck already-fixed findings and the historical recovery
flake; never invent a root cause for an unreproduced failure.

### 4. Durable shared authority

Keep the active v4 protocol family; port required task/automation types from the
disconnected prototype into compiled modules, then remove that duplicate family.
Implement real durable tasks, attempts, profiles, actions, automations, training
operations, worker inventory, and fixed-watermark event polling. Advertise only
functioning dispatch. CLI/TUI/daemon/desktop use the same session/provider and
lifecycle authorities. Persist intent before effects; ambiguous recovery is
interrupted, and retry creates a new attempt without replaying prior effects.

Connect the existing pure schedule calculator to occurrence reservation bound to
automation revision and UTC instant, with no overlap, latest-only catch-up,
existing DST behavior, and revocation. Migrations are append-only; preserve
canonical conversation identity, compatibility recovery, and finalized backups.

### 5. Owned tools and execution policies

Connect ABI structured tool calls to schema validation, current policy, durable
authorization/intent, bounded execution, cancellation, and audit. Implement scoped
files, registered commands, dedicated-profile browser, native desktop actions,
and external MCP/ACP client hosting. Raw model prose is never shell syntax.

The safe edition retains its allowlist and explicit protected-effect approvals.
The personal edition gains local validated structured `shell.exec` without that
command allowlist or per-command Abbey confirmation. This explicitly supersedes
older requirements applying the same policy to both editions; it does **not**
remove the safe edition's policy or bypass OS authorization. Signed privileged
helpers, audit, deadlines/capture bounds, process-tree teardown, and emergency
stop remain required. Keep personal execution out of served MCP and network
workers. Existing `os_control` remains allowlisted/confirmed in both editions;
the new personal authority is a separate, locally scoped structured path.

### 6. Complete desktop product

Finish edition-matched onboarding/daemon startup, credentials outside WebView IPC,
provider/workspace/profile configuration, durable multi-turn streamed chat,
attachments/search/pin/unarchive/export, explicit retry and exact Stop recovery.
Wire Tasks/Tools/Approvals/Automations/Training/Cluster only after runtime authority
exists. Complete memory curation, models/roles/permissions, diagnostics,
backup/restore/retention, services/notifications, native menus/window state,
accessibility and bounded history. Both editions must pass packaged native user
journeys and signed distribution/update/rollback/uninstall acceptance on all hosts.

### 7. Production models, accelerators, adapters, and neural media

Validate roadmap Gemma 12B architecture/tokenizer/license and immutable artifacts
before commitment, without substituting a different model. Require 4K context
plus 64-token smoke, RSS <=20 GB on the named 24 GB Mac, no sustained swap thrash,
correct tokenizer fixtures, structured-call quality and documented oracle
tolerances. Connect actual accelerator model/training workloads with requested
and executed device evidence and no silent fallback. Qualify hardware separately.

Extend existing consent-gated orchestration to actual separate E4B adapters with
provenance/redaction/deduplication, deterministic 80/10/10 splits, immutable base
identity, actual weight changes, and rollback. Require >=95% valid held-out
tool-call JSON, <=2% NLL regression, and 100% public-edition refusal success;
demo training fidelity is not held-out quality. Deliver offline Whisper/Parler
speech, then SDXL/LTX images/video with accepted licenses, verified artifacts,
budgets, cancellation, quality, and output provenance. Unavailable devices stay
explicitly unavailable; kernel parity is not production model evidence.

### 8. Federation and shared compute

Implement authorization, consent, cognition, episodes, manifests, and changes in
that order, enabling each only after its durable adapter and refusal tests exist.
Implement workers, quotas, leases/deadlines, signed jobs/results, mTLS, certificate
rotation and cancellation. Prove three Ubuntu ARM64 VMs with independent IPs,
identities/state and membership/control storage independent of WDBX placement.
Exercise partitions, leader/worker death, stale leadership, duplicates,
conflicts/read repair, restore, upgrades and teardown. Preserve the distinction
from physical multi-host production evidence. Live Discord capture/effects remain
separately consent-gated; prepare contracts and fixtures without live access.

### Final qualification

The matrix gives final qualification its own milestone so baseline closure does
not depend circularly on finishing the whole program. Require all included
requirements, root/desktop/ABI/WDBX gates, supported feature combinations,
installed packages and platform/device proofs, independent review, signed
provenance, dependency/license/SBOM/secret checks, and support/recovery runbooks.

Measure keyboard/screen-reader flows, 200% zoom, reduced motion and a
10,000-message history. Require cold start <=5s, warm interaction p95 <=2s,
navigation feedback <=100ms, idle CPU <=2% of one core and bounded resources in
a 24-hour soak. Record named hardware, datasets, sampling methods and limitations.
No missing/skipped DAST, CI, signing, native or external proof counts as passed.
The full program stays open until every required proof exists; canceled Zig and
deliberate Out-of-scope capabilities remain excluded.

## Shared interfaces and testing

One Rust contract family generates TypeScript and actual serialization samples.
Tasks bind goal/conversation/route/workspace/profile version/budget/attempt/run/
state/timestamps; actions additionally bind exact input, observation, policy,
decision and outcome. Automations expose revision/schedule/timezone/enablement/
next occurrence and task identity. Model/training/worker operations use immutable
IDs, explicit grants, bounded pages and typed status/cancellation. Version all
incompatible changes, preserve unaffected protocols and never replay mutations
on downgrade/reconnect.

Use focused discriminating regressions, then serial root and desktop gates on
unchanged source manifests. Crash/restart, migration, cross-edition isolation,
revocation, stale targets, hostile input, process teardown and mutation non-replay
are mandatory. Native fixtures are required on their actual hosts; compilation
and fixture bigram inference are insufficient for stronger claims.
