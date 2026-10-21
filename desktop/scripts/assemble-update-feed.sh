#!/bin/sh
# Abbey desktop updater feed assembler (tauri-plugin-updater v2 JSON).
#
# Builds one feed file per edition from already-built release artifacts and
# the download URL they will be served at:
#
#   {"version": "<semver>", "pub_date": "<RFC 3339 UTC>",
#    "platforms": {"<platform>": {"signature": "<.sig contents>",
#                                 "url": "<https asset URL>"}}}
#
# The signature field is the byte-exact contents of the .app.tar.gz.sig the
# Tauri bundler produced next to the updater artifact — nothing is invented,
# no key material is read or echoed anywhere in this script (the signature is
# public release data; the private updater key never passes through here).
# The .sig is already base64 of the minisign signature text and the plugin
# decodes exactly one layer of base64 before verifying it, so the bytes are
# published verbatim: re-encoding them here would decode to the base64 text
# instead of the signature and every update would fail to verify.
#
# The feed is BOUND to that signature. The desktop enables the updater
# plugin's `requireSignedVersion`, so the version the feed announces has to
# be the version recorded inside the signature's trusted comment or the
# plugin rejects the update. This script therefore reads the trusted comment
# out of the .sig and refuses to publish a feed that does not match it:
#
#   * --version, when given, must equal the signed version (both are named in
#     the failure); when it is omitted the signed version is published.
#   * the signed `file:` name must equal the artifact's file name, so a
#     signature made for one edition's asset can never be published for
#     another's.
#   * a signature that records no version, or no file name, is refused rather
#     than published as a feed the updater could not verify.
#
# A platform entry carries EXACTLY the two fields the plugin reads — `url` and
# `signature`. The signed artifact name is deliberately NOT published as a
# third key: serde ignores unknown fields today, but a future plugin release
# that adds `deny_unknown_fields` to its platform struct would then reject
# every feed published with it. The name is still checked — the signed `file:`
# must equal the artifact's own file name or this script refuses to publish —
# and the name is printed to the operator's console instead. The signature
# itself carries the name anyway.
#
# Options (all required except --version and --notes):
#   --artifact PATH    the <productName>.app.tar.gz updater artifact
#   --sig PATH         its .sig file (a single base64 line)
#   --url URL          the https URL the updater will download the artifact from
#   --version X.Y.Z    the released semver; optional, and when given it must
#                      equal the version recorded inside the signature
#   --platform NAME    updater platform key; only darwin-aarch64 ships here
#   --out PATH         where to write the feed JSON
#   --notes TEXT       optional release notes string inside the feed
#
# Usage:
#   ./scripts/assemble-update-feed.sh --artifact PATH --sig PATH --url URL \
#     [--version X.Y.Z] --platform darwin-aarch64 --out PATH [--notes TEXT]
#
# Fails closed on: a missing artifact or signature, an artifact that is not
# .tar.gz, an empty / multiline / non-base64 signature, a signature with no
# trusted comment, a trusted comment with no version or no file name, a
# --version that disagrees with the signed version, a signed file name that
# disagrees with the artifact's file name, a non-https URL, a non-semver
# version, any platform other than darwin-aarch64, and unknown arguments. The
# output file always ends with exactly one newline.
set -eu

usage() {
  echo "usage: ./scripts/assemble-update-feed.sh --artifact PATH --sig PATH --url URL" >&2
  echo "          [--version X.Y.Z] --platform darwin-aarch64 --out PATH [--notes TEXT]" >&2
}

fail() {
  echo "assemble-update-feed: $1" >&2
  exit 1
}

ARTIFACT=""
SIG=""
URL=""
VERSION=""
PLATFORM=""
OUT=""
NOTES=""

while [ $# -gt 0 ]; do
  case "$1" in
    --artifact|--sig|--url|--version|--platform|--out|--notes)
      if [ $# -lt 2 ]; then
        echo "assemble-update-feed: missing value after $1" >&2
        usage
        exit 2
      fi
      case "$1" in
        --artifact) ARTIFACT=$2 ;;
        --sig) SIG=$2 ;;
        --url) URL=$2 ;;
        --version) VERSION=$2 ;;
        --platform) PLATFORM=$2 ;;
        --out) OUT=$2 ;;
        --notes) NOTES=$2 ;;
      esac
      shift 2
      ;;
    *)
      echo "assemble-update-feed: unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

[ -n "$ARTIFACT" ] || fail "--artifact is required"
[ -n "$SIG" ] || fail "--sig is required"
[ -n "$URL" ] || fail "--url is required"
[ -n "$PLATFORM" ] || fail "--platform is required"
[ -n "$OUT" ] || fail "--out is required"

[ -f "$ARTIFACT" ] || fail "artifact is not a file: $ARTIFACT"
case "$ARTIFACT" in
  *.tar.gz) ;;
  *) fail "artifact must be a .tar.gz updater artifact: $ARTIFACT" ;;
esac
[ -f "$SIG" ] || fail "signature is not a file: $SIG"
case "$URL" in
  https://*) ;;
  *) fail "url must be an https URL, not: $URL" ;;
esac
if [ -n "$VERSION" ] && ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$'; then
  fail "version must be semver X.Y.Z, not: $VERSION"
fi
[ "$PLATFORM" = "darwin-aarch64" ] || \
  fail "platform must be darwin-aarch64 (this pipeline ships macOS Apple silicon only), not: $PLATFORM"

# The bundler writes exactly one base64 line plus a trailing newline into the
# .sig. Whitespace-stripped content must still be a single non-blank line,
# and the plugin's base64 decoder rejects whitespace outright, so the feed
# carries the stripped bytes.
SIG_LINES=$(grep -c '[^[:space:]]' "$SIG" || true)
[ "$SIG_LINES" = 1 ] || fail "signature must be exactly one non-blank base64 line: $SIG"
SIGNATURE=$(tr -d '[:space:]' < "$SIG")
[ -n "$SIGNATURE" ] || fail "signature file is empty: $SIG"

# json.dumps does the escaping; the signature's trusted comment is read here
# so the feed can never announce a version, or an artifact, that the
# signature does not cover.
python3 - "$OUT" "$PLATFORM" "$VERSION" "$URL" "$NOTES" "$SIGNATURE" "$ARTIFACT" <<'PY'
import base64
import json
import os
import re
import sys
from datetime import datetime, timezone

out, platform, requested, url, notes, signature, artifact = sys.argv[1:8]

# The official semver grammar. The plugin compares the signed version with
# the announced one as semver when both parse, so this has to be as strict as
# the `semver` crate the plugin uses.
SEMVER = re.compile(
    r"^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)"
    r"(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)


def fail(message):
    raise SystemExit("assemble-update-feed: " + message)


def versions_match(signed, announced):
    """Mirror the plugin's `verify_signed_version` comparison.

    It strips a leading "v" from both sides, compares them as semver when
    both parse, and falls back to a literal comparison otherwise.
    """
    left = signed.lstrip("v")
    right = announced.lstrip("v")
    if SEMVER.match(left) and SEMVER.match(right):
        return left == right
    return signed == announced


if not signature:
    fail("empty signature")
if not url.startswith("https://"):
    fail("url must be https")
try:
    decoded = base64.b64decode(signature, validate=True)
except Exception:
    fail("signature is not valid base64")
try:
    minisign = decoded.decode("utf-8")
except UnicodeDecodeError:
    fail("signature does not decode to UTF-8 minisign text")

# The trusted comment is only trustworthy because the plugin verifies the
# signature over it before reading it; here it only decides what may be
# published, and a mismatch fails the whole feed. The minisign crate the
# plugin uses keeps the whole line and hands out everything after the
# 17-character "trusted comment: " prefix, so this reads it the same way.
trusted = None
for line in minisign.split("\n"):
    if line.startswith("trusted comment: "):
        trusted = line[17:]
        break
if trusted is None:
    fail(
        "signature carries no 'trusted comment: ' line; "
        "it is not a Tauri minisign signature"
    )

# Tab separated key:value pairs, exactly as the plugin reads them:
# timestamp:<unix>\tfile:<name>\tversion:<semver>. The values are taken
# verbatim — the plugin does not trim them, so neither may this.
signed_version = None
signed_file = None
for field in trusted.split("\t"):
    if field.startswith("version:"):
        if signed_version is None:
            signed_version = field[len("version:"):]
    elif field.startswith("file:"):
        if signed_file is None:
            signed_file = field[len("file:"):]

if not signed_version:
    fail(
        "the signed trusted comment records no version: "
        "requireSignedVersion is enabled, so the artifact must be re-signed by a "
        "CLI that records the version (tauri signer sign --app-version X.Y.Z)"
    )
if not SEMVER.match(signed_version.lstrip("v")):
    fail(f"the signed version is not a semver: '{signed_version}'")

if requested and not versions_match(signed_version, requested):
    fail(
        f"version mismatch: the feed would announce '{requested}' but the "
        f"signature was made for version '{signed_version}'"
    )

if not signed_file:
    fail(
        "the signed trusted comment records no file: name; "
        "the artifact must be re-signed so the signature names the artifact it covers"
    )

artifact_name = os.path.basename(artifact)
if signed_file != artifact_name:
    fail(
        f"signed artifact name mismatch: the signature covers '{signed_file}' "
        f"but the artifact being published is '{artifact_name}'"
    )

feed = {
    "version": signed_version,
    "pub_date": datetime.now(timezone.utc)
    .isoformat(timespec="seconds")
    .replace("+00:00", "Z"),
    "platforms": {
        platform: {"signature": signature, "url": url}
    },
}
if notes:
    feed["notes"] = notes
# Write through a sibling temporary file: a half-written feed must never be
# the file a release step uploads.
staged = f"{out}.tmp"
try:
    with open(staged, "w", encoding="utf-8") as stream:
        json.dump(feed, stream, indent=2)
        stream.write("\n")
    os.replace(staged, out)
except BaseException:
    if os.path.exists(staged):
        os.remove(staged)
    raise
print(
    f"assemble-update-feed: wrote {out} (platform: {platform}, "
    f"version: {signed_version}, signed file: {signed_file})"
)
PY
