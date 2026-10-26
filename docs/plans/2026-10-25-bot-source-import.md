# Bot source custody and library prerequisite (M9.1a)

Authority: approved company modernization master section 9, `MOD-BOT-EXTENSION`,
and its Bot ownership amendment. This is an import/library prerequisite, not
managed-extension enablement or runtime parity. Original documents remain intact.

## Global constraints

Use reviewed Abbey `80187be345828ed966374c5cd6880eb40fe62bec` and only Bot
`6eb5cff114c22dd7a2d0f78f98c2a1c2df8bf215`. Preserve source histories,
authorship, licenses, fixtures, scripts, deployment, Activity and local patches.
Capture all source refs in a checksummed, verified, restoration-tested private
bundle before an unsquashed subtree import; do not copy the dirty source overlay.
No remote transfer, history rewrite, nested Git, gitlink, original deletion,
service execution, credential reads or production state migration.

## Task 1: Preserve attributed source and extract its actual library

Own `extensions/bot/**`, root workspace exclusion, this plan, import receipt and
focused boundary checks. Preserve existing `crate::` meaning by making Bot's
modules and Data belong to its library; retain a thin standalone compatibility
binary and explicitly named process-global compatibility entry. Preserve all
standalone process behavior; do not invent a host API, enable switch or adapter.

Exclude Bot from Abbey's workspace; retain Bot's own lock, 1.98.0 source contract
and Serenity/OpenMLS patches. Do not add a root dependency/feature, modify root
Cargo.lock or reconcile the currently unqualified shared security graph.
M9.1b owns that later cohort. M9.2 owns runtime/configuration/service injection.
No changes to app_core, store schemas, transport, provider or authority behavior.

Acceptance: imported tree exactly equals source tree in the separate import
commit; source SHA remains an ancestor; all refs restore from private bundle;
original docs and all 64 requirement IDs remain attributed; default Abbey graph
has no Bot member/dependency; real Bot library compiles through Abbey's 1.99.1
wrapper with locked own dependency graph; focused existing token-free tests and
diagnostics preserve command behavior. Keep independent source-1.98 evidence
separate. No full root/desktop gates in this shared-machine packet.

Rollback: before runtime adoption, revert the library-boundary commit and the
subtree merge with mainline parent 1, preserving this branch and private bundle.
Never delete the retained source repository or its non-ancestor/archive refs.
Import acceptance closes no runtime, native-platform, deployment, model-quality
or sustained-operation requirement. Parent coordinates independent review.
