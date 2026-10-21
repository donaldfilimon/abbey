# Abbey production continuation — 2026-10-21

This receipt records implementation across the retained Rust runtime and desktop
program. It does not close the entire production roadmap or promote a capability.
Rust 1.99.1, dependency pins, the canceled Zig decision, edition boundaries and
protocol-v1/v2/v3 authority remain unchanged.

## Implemented and integrated

- Durable stateless conversation context: exact admitted run/conversation/route,
  input and idempotency digest binding; only earlier successful same-route turns;
  full event reconstruction rather than preview text; chronological whole-pair
  selection bounded to 32 pairs / 24 KiB, with explicit truncation metadata. Legacy
  serialized byte counts/preview undercounts, genuine marker-prefixed replies,
  missing payloads and successful empty replies receive dedicated fixtures.
  CLI ABI/Ollama and the durable provider share context framing. This does not
  establish native vendor resume, production model quality, or new providers.
- Read-only protocol-v4 LookupTurn is protected by ReadTurns and correlates the
  exact conversation and idempotency key, including absent receipts. The client
  sends once; Tauri exposes only this typed lookup. It neither admits work nor
  replays a submission. Existing write authority serves the bounded metadata patch.
- Retry requires confirmed terminal attempt state, complete original input,
  an available explicitly selected route and a fresh key. Window-only lineage is
  labeled. Lost acknowledgments for normal sends and retries reconcile through
  exact-key lookup; plaintext similarity is not acceptance evidence. Unknown
  results block their conversation, preserve drafts and survive other navigation.
- Safe Markdown/code presentation includes explicit clipboard failure handling.
  Raw HTML is escaped, remote assets are not fetched, and links remain selectable
  text. Large/complex replies fall back to one selectable plain-text block.
- Pin/unpin, archived-history filtering and unarchive use canonical metadata
  patches, with immutable mutation targets and read-only unknown-result recovery.
- Message history uses 32-row windows, Earlier/Later/Latest controls, browse/follow
  modes and a bounded cursor stack with cancellable reconstruction after eviction.
  Historical views pause polling; visible content caches are pruned. Bounded exact
  attempt evidence preserves retry when a turn straddles pages. Fixtures traverse
  10,000 sparse messages and variable-sized pages without retaining/rendering all.
- Permission-profile persistence is real validated JSON. UPSERT preserves referenced
  identity; missing, corrupt, mismatched and oversized records fail closed. The
  active type has redacted Debug and strict fields/bounds. Dormant historical types
  remain preserved. No permission/task/tool execution capability was advertised.
- Unserved task/automation lists and action decisions now refuse explicitly; they no longer fabricate empty success or accepted decisions. No corresponding grants were added.
- Conversation cursors reject missing and mismatched title/archive selections.
  Doctor preserves automatic-backend reporting alongside configuration provenance.

## Verification and review

Each behavioral repair has focused regression evidence. Independent review found
and drove fixes for zero-output retry, cross-conversation blocking, Markdown node
explosion, empty successful histories, legacy marker handling split-page retry, historical multipage content loading, and Stop/content-load races.
The final truncation-notice empty-history regression passed before gate restart.

Focused results before final gates: desktop 83 tests / 1,644 assertions,
TypeScript/build/bundle verification passed; context 10 tests passed; native default
Rust bridge 32 tests plus 3 updater tests passed. Live-daemon and rendered acceptance
are separate from these unit results.

Intermediate full-root attempts failed on module formatting and on Doctor's
changed automatic-selection label. Both were corrected without reverting the
concurrent configuration work; the automatic-fallback process regression passed.
A native bridge compile failure from a missing patch-type import was also repaired.
A subsequent root run passed all default, WDBX, personal and accelerator Rust cohorts, then failed the final generated-claims documentation check because desktop/README.md was stale. The generator repaired that drift. This intermediate run does not qualify later source changes or the historical-content repair. The intermediate desktop gate passed process/backend tests and linking, but native Routes acceptance failed before the first scenario with `ax_window_missing`; the failed receipt and synthetic scratch diagnostics were retained. Full-root and full-desktop outcomes are recorded below after completion.

## Source provenance and remaining work

A concurrent writer committed shared changes as ca834d0 during this continuation;
this executor made no commit/push/publication. Qualification captures complete
Abbey/ABI/WDBX file hashes, not only HEAD. Execution logs and manifests remain in
`.superpowers/sdd/2026-10-20-production-completion/`.

The broader program remains open: exact native provider sessions and complete
provider coverage, durable governed tasks/actions/tools, automations/services,
onboarding, complete transcript export/attachments, native accessibility/soak
acceptance, production model/training/media workloads, additional platform
packages and distributed proofs. Existing signing, host/hardware, artifact/license
and participant-consent prerequisites remain distinct. An Ollama CLI list-then-run
preflight cannot establish race-free no-pull authority; that next provider slice
requires an explicitly bounded local inference transport rather than a silent
transport substitution or a false no-download claim.
