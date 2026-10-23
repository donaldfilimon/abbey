# MLAI release baseline execution — 2026-10-22

**Status: Partial; full release not accepted.** Requirement coverage and external
prerequisites are in the [release matrix](2026-10-22-mlai-release-matrix.md).

## Source and publication

Entry HEAD: `7b1b3f0b4ec8640e6c18c3ecb6d4e291d1d783dd`, clean branch `pr190`.
Remote main observed through `git ls-remote`:
`1e272bf393e14486078c08f407bb1149161e0640`. PR #190 remains the separate
MLX-VLM dependency upgrade. This execution uses `abbey/mlai-release-baseline`;
its parent includes the three unmerged MLX/lock/merge commits. Publication must
preserve that dependency and qualify the final main SHA separately.

The current canonical macOS Gate succeeded for the observed main revision:
[run 39314808196](https://github.com/donaldfilimon/abbey-bot/actions/runs/39314808196).
That run does not qualify this branch or its pending changes.

## Confirmed repair

The first `ABBEY_REQUIRE_WDBX_CONFORMANCE=1 ./check.sh` exited 1 at the
RustSec inventory gate: unexpected RUSTSEC-2026-0381 and RUSTSEC-2026-0382.
Earlier stages passed; downstream stages were not reached.

The exact root and standalone patched-Serenity lockfiles now select
`rustls-webpki 0.103.16` and `webpki-roots 1.0.10`, fixing the newly reported
emSign trust defects. The compatibility roots package remains 0.26.11 and
re-exports the patched 1.x roots. Targeted lock regeneration changed no manifests,
production source or accepted-debt policy. Builds and validation retain `--locked`.

References: [roots advisory](https://rustsec.org/advisories/RUSTSEC-2026-0381.html),
[verification advisory](https://rustsec.org/advisories/RUSTSEC-2026-0382.html).

`python3 scripts/check-rustsec-debt.py` now exits 0: one previously accepted
ringbuf vulnerability remains, plus three informational unmaintained warnings.
This is not a clean audit. `python3 scripts/check-linux-tls-tree.py` exits 0;
Rustls/WebPKI remains selected and native TLS/OpenSSL absent.

README now reflects macOS-only CI and the MLX-VLM 0.7.8 deployment lock;
`.env.example` no longer names the obsolete service version. Public APIs,
commands, provider routing and persistence formats are unchanged.

## Current checks and sibling evidence

- `npm --prefix activity test`: exit 0; 20 passed, zero failed/skipped.
- `python3 scripts/check-pages-liquid.py`: exit 0 before the new release records;
  final documentation checks must cover the final diff.
- Strict repaired source gate: pending; no current complete source pass claimed.
- WDBX current HEAD: `36bc99b699de60be6c07b2838d74580084fb5d4c`.
  Current sibling, Abbey and CI-pinned revision
  `7ddeb3d1389ec0e9fab5ac5397acf0da91a01174` projection fixtures all have
  SHA-256 `a4ec232c6980e009b77936386c9b233b864abb2d6b66b6253624d2f7a474be90`.
  Fixture equality is not qualification of all current sibling source.
- ABI current HEAD: `69910bf61205458dec27dd4b616201e0a5f3898a`.
  Its October 8 ledger names the foundation and aggregate family-isolation
  qualification, published as `5c275dd`; later ABI work is outside that receipt.
  No new full ABI/WDBX source qualification or trained-model claim is made here.

## Managed host diagnosis and external prerequisites

`python3 -I deploy/service-status.py` exits 1 with unavailable/unverified status.
The wrapper imports successfully. No matching Abbey launchd label appeared;
`~/.config/abbey-bot/env` and `~/.local/libexec/abbey-bot/abbey-bot` are absent.
The entry checkout also had no built `target/release/abbey-bot`.
This host lacks the managed installation/configuration evidence; it is not
established that a running service has failed. Do not synthesize credentials,
install a second gateway, or infer readiness from a plist.

Next operator input: identify/provision the intended managed Mac and its private
environment. Provider qualification, candidate/installed comparison and the
transactional deployment depend on that prerequisite and final-main CI.
Live voice V1–V8, 100-label human adjudication, text/ordinary-member journeys,
up-to-three-recipient initiative pilot, independent moderation assessment and
Activity host/TLS/Portal/two-participant witnesses remain open. No deployment,
restart, live Discord message, moderation activation or Portal change occurred.
