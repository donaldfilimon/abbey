# Program 3 live capture contract

**Status: Draft for independent review — grants no authority; no adapter may be implemented until OB-18 evidence exists.**

This document drafts the contract for a separately reviewed, read-only Discord
guild metadata adapter. It is the first step of the remaining live Discord work
recorded in `tasks/todo.md` (section "Remaining live Discord work", lines
328-341; the plan cites them as 330-342) and of Task 17 in
`docs/plans/2026-10-27-abbey-models-media-federation-plan.md`. Task 18 of that
plan (`live-guild-validation` and `guild-effects`) depends on this contract
being reviewed and authorised first.

Nothing here is implemented. The draft changes no claim, registry entry, task
ledger or completion record. It authorises no capture, connection, recording or
effect.

## Scope

The adapter may observe only read-only Discord guild **metadata** needed by the
existing Program 3 engine. That means the guild identity, the channel and
category structure, the role definitions, permission overwrites, and the bot's
own membership and roles. Its only output is one bounded recording in the same
shape as the closed synthetic recordings that the engine already validates.

Messages, voice, member lists, presence, user profiles, audit logs, and any
other content or activity data are outside scope.

## Manager authorisation

- A named guild owner or administrator ("manager") authorises capture in
  writing for one specific guild ID. The authorisation names the guild, the
  bot application, the permitted scopes, the freshness window, the retention
  period, and an expiry date.
- The authorisation is recorded as evidence
  (`docs/verification/YYYY-MM-DD-p3-capture-authorisation.md`) before any
  capture code exists. An authorisation for one guild never extends to another.
- The adapter must refuse to run when no matching, unexpired authorisation
  exists. It must never infer authorisation from the bot's permissions or from
  the synthetic fixture claim.

## Participant consent rules

- Guild participants are told, in a channel the manager selects, that metadata
  capture is planned, what it covers, and how to object, before the first
  capture.
- Captured metadata never identifies individual non-bot members. Role and
  overwrite targets that refer to individual members are reduced to an opaque
  per-recording reference (see Redaction).
- A credible participant objection pauses capture until the manager and the
  reviewer resolve it. The resolution is recorded with the authorisation
  evidence.

## Least-privilege scopes

- The bot uses the smallest set of read-only gateway intents and permissions
  that can read guild, channel and role structure. It needs no message-content
  intent, no privileged member intent, no presence intent, and no write,
  manage, moderate or administrator permission.
- Any requested scope beyond that set needs a separate review. The granted
  scope set is pinned in the authorisation evidence and checked at startup. A
  token with more scope than that set is refused, not narrowed silently.
- Credentials stay in a supported local secret store. They never go into the
  repository, recordings, logs or evidence.

## Bounded freshness

- Each recording carries its capture time and the authorisation it was taken
  under. A recording older than the authorised freshness window is stale. Stale
  recordings feed only the stale, read-only status the engine already reports.
- Capture is a bounded one-shot read with an explicit timeout. There is no
  standing subscription, polling loop or background worker.

## Contradiction handling

- If the observed metadata contradicts itself (for example, overwrites aimed
  at unknown targets, duplicate semantic targets, parent cycles, or limits
  exceeded), the recording is refused before replay. This uses the same
  validation rules as the closed synthetic path.
- If a recording contradicts an earlier recording from the same guild, the
  adapter reports both digests and the differing objects. It does not merge
  them or choose a winner.

## Redaction

- Usernames, nicknames, avatars and display names are never captured. Member
  identifiers that appear as overwrite targets are replaced with opaque
  references scoped to one recording, so they cannot be joined across
  recordings.
- Channel and role names are captured only when the manager's authorisation
  allows it. Otherwise they are replaced with opaque references.
- Redaction is applied before anything is written or emitted, never after.

## Retention

- A recording is kept only as long as the authorisation allows, with a fixed
  maximum set at review. It lives in one owner-only location named in the
  authorisation, and nowhere else.
- When the period expires the recording is deleted, and the deletion is noted
  in the evidence. Derived findings and plan digests carry no raw metadata.

## Revocation

- The manager can revoke authorisation at any time. A participant objection
  that is upheld revokes it too. After revocation the adapter refuses to
  capture, and the retained recordings for that guild are deleted.
- Revocation is recorded in the same evidence file as the authorisation,
  together with proof of the deletion.

## Never

- No message content: no message bodies, attachments, embeds, reactions,
  thread content, voice, audio or transcripts.
- No member enumeration: no member lists, presence, or per-user profile data.
- No persistence beyond this contract: nothing is written outside the single
  authorised recording location and retention window. Program 3 runtime code
  gets no durable store.
- No effects: no message sends, edits, deletions, role or channel changes,
  permission writes, approvals or compensations. Desired-state execution is a
  separate effect program (`guild-effects`) with its own approval.
- No reuse of the `synthetic_fixture_claim` authority as live authorisation.
  The synthetic fixture claim describes closed fixtures only. It can never
  stand in for manager authorisation or participant consent.

## Relationship to the synthetic claim

The `program-3-guild-intelligence-synthetic` claim stays **Current** and keeps
its present scope: closed synthetic replay over already-loaded metadata
recordings, with no non-synthetic capture, live Discord access, participant
consent, persistence or effect surface. This contract does not widen that
claim. A live capture capability, if it is ever authorised, would be a separate
claim with its own evidence.

## Checker coverage

`tools/check_p3_readonly.py` now applies the full Program 3 read-only rules to
any live-capture module that appears at:

- `src/app_core/guild_live_capture.rs`
- `src/app_core/guild_live_capture/**/*.rs`
- `src/guild_live_capture.rs`
- `src/guild_live_capture/**/*.rs`

These rules forbid network clients, process execution, filesystem access,
durable stores, runtime tools, Program 5 types, effect operations, and private
content. The network rule is applied too, which is deliberately fail-closed.
Any network allowance for an approved adapter belongs to the later reviewed
adapter task after OB-18, not to this draft. The checker runs inside
`./check.sh`. `tools/tests/test_check_p3_readonly.py` covers it
(`test_live_capture_modules_cannot_import_effect_paths` and
`test_checker_scans_live_capture_surfaces_alongside_program_three`).

## Next step (Donald-gated, OB-18)

An independent review of this contract, the guild manager's authorisation, and
participant consent records must all exist as evidence before anyone implements
an adapter. That later work is its own TDD task
(`live_capture_records_only_authorised_metadata`).
