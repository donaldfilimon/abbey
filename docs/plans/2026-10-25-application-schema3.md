# Negotiated application schema 3 contracts

Approved company master M2/M8 and code-first addendum govern this packet.
Requirement owners: Abbey `runtime-contracts` (authority), production D3–D6,
P1/P6/P7; ABI C03/C04/C06/C07/C08/C21/C35/C42 remain separate runtime evidence.

## Global constraints

- Protocol v4 schema 3 publishes bounded administrative contracts, preserving
  schema 2 conversation wire behavior through an explicit compatibility boundary.
- Control frames are at most 64 KiB before allocation; larger artifacts require
  a separately authenticated stream. This packet adds no transfer authority.
- Server derives principal/edition and task/attempt/run identities. Wire profile,
  workspace, preview and artifact references grant no authority.
- Administrative capabilities remain unserved until durable runtime packets pass.
  No store, manager, provider or local/remote permission behavior changes here.
- Preserve updater/GTK repairs, source pins, old protocols and original work.
  Use one isolated task checkout and source-bound tests; independent review follows.

## Task 1

Implement strict bounded task/attempt/run/event/profile-revision/action and typed
schedule/automation/occurrence records in the active contract family. Require
idempotent mutations, CAS revisions and exact action preview digests, fixed event
watermarks, closed states, safe integers and explicit uncertain outcomes.
Negotiate supported schema 2/3 envelopes with schema-specific capabilities and
commands; schema 2 must refuse admin payloads. Retain conversation serialization.
Generate TypeScript plus real Rust samples and compatibility fixtures. Verify
malformed/unknown/boundary/correlation cases, live scratch-daemon old/new client
negotiation and oversized announced frames. Root and desktop gates run serially
within this checkout. Full runtime/stream activation is deferred, not simulated.

## Rollback

Revert this unactivated contract/negotiation packet and coordinated generated
bindings. No store migration, live effect, task admission or model is changed.

## Approved transport ruling

Schema 3 uses bytes `41 42 53 33` (`ABS3`), then u32 big-endian JSON length
then JSON, on the existing authenticated socket. Schema 2 and older protocols
retain their original u32 length framing and limits. The framing kind is retained
through dispatch and errors. Schema 3 length above 65536 fails after eight bytes
before body allocation. A framing/schema mismatch fails closed. No content
sniffing, automatic downgrade or resend is permitted. Windows transport remains
explicitly unavailable until its owned adapter ships.

## Bounded page and desktop rollout ruling

Existing desktop conversation and route sessions explicitly retain schema 2;
SDK schema 3 remains available. Legal indivisible text items can exceed 64 KiB
after JSON escaping. Schema 3 refuses those explicitly; an owned chunk/artifact
reader plus frontend consumer remains required before desktop/browser migration.
Generated metadata must distinguish supported schema 3 from selected schema 2.

Schema 3 budgets complete encoded envelopes. Only typed conversation/message/
content page tails may be deferred, publishing exact last-retained continuation.
Never truncate text, rewrite items, remove fixed watermarks or claim completeness
when items remain. Schema 2 bytes stay unchanged. Existing live content has no
explicit through field; do not invent that claim. No new authority is served.
