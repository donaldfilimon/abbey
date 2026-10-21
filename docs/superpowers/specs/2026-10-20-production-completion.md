# Abbey production completion requirements — 2026-10-20

Authority: Donald requested the full approved desktop plan, then all next steps to finish Abbey for production and continued desktop implementation. This specification consolidates those requests with `AGENTS.md`, `REWRITE-DECISION-2026-10-19.md`, `tasks/goals.md`, `tasks/todo.md`, `src/claims/registry.rs`, and the existing desktop plan. Historical checked boxes do not establish present acceptance. This document adds no live external access, publishing, spending, signing credential, or destructive data authorization.

## Required end state

Abbey is a usable, installable, supportable local assistant on macOS, Windows, and Linux. Its desktop manages conversations, actual agent work, permitted tools, recurring automations, memory, models, providers and permissions through the same application/runtime authorities as CLI/TUI. The default route remains local-first and requires no paid subscription. The complete approved broader roadmap includes owned runtime/tool hosting, production local models, accelerator execution, curated adapter training, local neural media, edition separation, federation and shared-compute proofs. Those programs remain open until their own requirements and evidence are met; a desktop release is an intermediate milestone, not a redefinition of project completion.

“Perfect” means a complete requirement matrix, zero unresolved release-blocking defects, tested recovery, accessible polished interactions, measured resource budgets, reviewed dependencies, and reproducible signed packages. It does not mean an unverifiable promise of zero future defects. Every unsupported route/platform/device remains explicit and unavailable until proved.

## Invariants

- Retain Rust `1.99.1` and the exact qualified repository pins until a new candidate passes its own gates. Zig rewrite and cutover are canceled; preserve retained history.
- Work in the canonical checkout/default branch, preserve dirty work, and coordinate file ownership. Do not commit/push/publish unless requested. No deletion of preserved migration sources or user data as cleanup.
- One shared agent/session execution path; backend-specific argv, stream and cancellation semantics remain in their existing adapters. Desktop must not become a second agent engine.
- v1/v2/v3 and the read-only MCP server keep their existing authority boundaries. Desktop content/effects use separately negotiated narrow v4 commands. Capability negotiation advertises only implemented dispatch.
- Credentials remain in Rust/platform credential storage, outside WebView IPC, transcript output and diagnostics. Grants are edition-, workspace-, route-, profile- and operation-bound.
- Both editions preserve existing OS command confirmation and allowlist rules. Separate personal-edition proposals cannot silently override the safe edition or current rules. Signed privileged helpers require OS authorization and never store user passwords.
- Every action is a validated structured request. External content cannot grant authority. Approval binds exact immutable input and current target. Persist intent before an effect, never replay an ambiguous mutation after a crash/reconnect, and expose interruption/reconciliation.
- Conversation clear, identity clear, transcript deletion, semantic-memory obsoletion and model removal are separate operations with explicit retention rules.
- Raw authorized chat text is separate from sanitized audit evidence. No hidden chain-of-thought or invented cost/token/device-residency claims.
- All source/test/live/CI/package/signing/device/cluster evidence names exact inputs and limitations. Skipped checks and empty CI jobs are not passes.

## Desktop completeness

D1. Real onboarding: discover/connect/start an edition-matched daemon, provision owner-only local credentials, select available providers/models/workspace/profile, and explain missing prerequisites with repair actions. No manual bearer pasting into a WebView.
D2. Chat: multi-turn context, bounded durable history/search, new/rename/pin/archive/unarchive/export/delete, route/persona/model selection, multiline composition, attachments, markdown/code/copy, visible streaming, Stop/retry, honest usage and failure state. Drafts and selections survive failures; ambiguous sends do not duplicate turns.
D3. Tasks: actual persisted goal/attempt/state/budget/route/workspace/profile, queue/detail/activity/artifacts, conversation links, pause/approve/deny/cancel/retry and recovery. Vendor delegation is labeled.
D4. Governed tools: scoped file operations, dedicated-profile browser, allowed typed commands, native desktop inspection/actions, external MCP/ACP integrations. UI displays preview, current target, confirmation, outcome and emergency stop.
D5. Automations: one-time/hourly/daily/weekly/cron schedules, IANA timezone and preview, enable/disable/run-now, exact occurrence de-duplication, no overlap, latest-only catch-up, DST behavior, revoked-profile refusal, service/continue-on-close and notifications.
D6. Management: functional Training/adapters and Cluster/workers views as required by the existing Completion Program; exact-approved memory obsoletion, provider/persona/role configuration, immutable model revision/license/download/load/unload/progress/device evidence, permission profiles and revocation, diagnostic export, backup/restore and data retention.
D7. Native product: proper icons, menu/shortcuts, tray/background semantics, window state, keyboard/screen-reader/reduced-motion/high-contrast support, safe external links/downloads, bounded long-history rendering and measured startup/resource use.
D8. Distribution: reproducible packages for supported hosts and editions, daemon/runtime installation, code signing/notarization where required, verified updates and rollback, fresh-account offline/local-provider startup, upgrade/restart/uninstall without unintended data loss.

## Project completion beyond the desktop

P1. Close retained CLI/TUI/provider parity and strict `abbey.v1` federation authority adapters with exact grants, limits, backward compatibility and recovery.
P2. Qualify production local model architecture/tokenizer/artifacts/license and actual inference/tool quality. Existing bigram fixtures, model aliases and artifact presence do not prove this.
P3. Qualify supported accelerator inference/training execution with correctness and measured device evidence. Detection and Metal numerical-kernel parity remain separate bounded claims.
P4. Curate/export/train/evaluate/rollback model adapters reproducibly with consent/provenance. No automatic weight updates or cross-architecture adapter claims.
P5. Qualify offline local speech, then image/video generation with license, budgets, cancellation and quality evidence; distinguish existing platform I/O and delegated media.
P6. Complete edition-specific consent/audit/helper designs and authenticated three-VM compute proof, then separately prove physical-host/HA/multi-device production operation.
P7. Provision working Linux/Windows CI, preserve trusted-runner policy, reproduce from immutable public dependency pins, and validate every release artifact on its actual host.
P8. Preserve synthetic guild-intelligence boundaries; non-synthetic recording/live Discord/effects require separate consent/authorization and exact acceptance. No external messaging or deployment follows from this plan alone.

## Release evidence and quality budgets

- Correctness: root and desktop gates plus feature-combination, contract, crash/restart, cross-edition and installed-artifact tests; zero ignored required cases; no unresolved Critical/High defects or Important correctness/authority findings.
- UX: full keyboard and screen-reader flows; 200% zoom, reduced motion, long Unicode text, 10,000-message history and empty/offline/permission-denied states. No fabricated records or enabled dead controls.
- Provisional performance acceptance (measure on named minimum hardware before adjusting): warm window interactive p95 <=2 seconds, cold start <=5 seconds, nonblocking navigation feedback <=100ms; desktop idle CPU <=2% of one core averaged over 60 seconds; no unbounded heap/event/poll growth in a 24-hour soak. Record baseline, dataset, OS, device, provider and sampling method; do not claim model speed from UI timing.
- Durability: power-loss/process-kill at every persisted-intent/effect/result boundary; no unintended replay, cross-conversation contamination or lost committed history; complete backup/restore plus forward migration and safe old-binary refusal.
- Supply chain: exact Abbey/ABI/WDBX source manifests, locked Rust/Bun inputs, reproducible artifact hashes, dependency/license/SBOM checks, secret scanning, signed update metadata and provenance. Report unavailable external audit/DAST explicitly.
- Launch: signed installed artifacts on clean supported accounts, safe/personal coexistence, provider optionality, service lifecycle, update interruption/rollback and uninstall retention. Native-host proof is required for each platform; compilation is insufficient.

## External prerequisites tracked, not assumed

Apple Developer ID/notarization credentials; Windows signing identity; actual Linux/Windows runners and minimum hardware; approved model licenses/artifacts and suitable compute/storage; explicit consent for live third-party services/Discord recording; VM/worker infrastructure and reviewed trust keys. Prepare concrete artifacts/configuration before requesting the final external action. Continue independent local work while any one prerequisite is missing.
