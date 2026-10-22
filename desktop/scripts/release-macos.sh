#!/bin/sh
# Abbey desktop macOS release pipeline: build -> sign -> (notarize) -> verify.
#
# Modes, decided entirely by the owner environment (credentials are never
# stored in this repository and never fabricated):
#
#   staging   no Apple environment at all. Builds the .app (+ .dmg), applies a
#             local ad-hoc signature with hardened runtime and the shipped
#             entitlements, and verifies the signature. Proves the whole
#             bundling/signing pipeline without any Apple account.
#   signing   APPLE_SIGNING_IDENTITY set (Developer ID Application identity,
#             name or SHA-1). The Tauri bundler code-signs the artifacts.
#             Not yet notarized: Gatekeeper still refuses fresh downloads.
#   publish   additionally one notarization credential set:
#               App Store Connect API key:
#                 APPLE_API_KEY_PATH / APPLE_API_KEY_ID / APPLE_API_ISSUER
#               app-specific password:
#                 APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID
#             The bundler then notarizes and staples the .app and .dmg; this
#             script verifies that with spctl + stapler afterwards.
#
# Flags:
#   --personal   build the personal edition overlay
#                (tauri.personal.conf.json + cargo personal-edition feature)
#                instead of the safe edition.
#   --skip-dmg   bundle the .app only (faster staging iterating).
#
# Usage:
#   ./scripts/release-macos.sh [--personal] [--skip-dmg]
#   APPLE_SIGNING_IDENTITY="Developer ID Application: ..." ./scripts/release-macos.sh
set -eu

cd "$(dirname "$0")/.."
DESKTOP=$(pwd)

# Same toolchain environment the acceptance proof uses: the pinned 1.99.1
# toolchain wins over any shadowing Homebrew rust on PATH.
export PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin"
export RUSTUP_TOOLCHAIN=1.99.1
export CC=/usr/bin/clang
export CXX=/usr/bin/clang++

EDITION_ARGS=""
SKIP_DMG=0
UPDATER_KEY_FILE=""
# Bundler product name: the bundle identity from the edition config (safe
# "Abbey", --personal "Abbey Personal"). With two editions' bundles sitting in
# the same bundle directory, artifact selection MUST be by name, not by find
# order: a find would nondeterministically mix one edition's .app with the
# other edition's .dmg.
PRINT=${PRINT:-Abbey}
while [ $# -gt 0 ]; do
  case "$1" in
    --personal)
      EDITION_ARGS="--config src-tauri/tauri.personal.conf.json --features personal-edition"
      PRINT="Abbey Personal"
      ;;
    --skip-dmg)
      SKIP_DMG=1
      ;;
    --updater-key)
      if [ $# -lt 2 ]; then
        echo "release-macos: --updater-key needs a file argument" >&2
        exit 2
      fi
      UPDATER_KEY_FILE=$2
      shift
      ;;
    *)
      echo "usage: ./scripts/release-macos.sh [--personal] [--skip-dmg] [--updater-key <file>]" >&2
      exit 2
      ;;
  esac
  shift
done

IDENTITY_HERE=${APPLE_SIGNING_IDENTITY:-}
NOTARY_API=${APPLE_API_KEY_PATH:+${APPLE_API_KEY_ID:+${APPLE_API_ISSUER:+set}}}
NOTARY_PASSWORD=${APPLE_ID:+${APPLE_PASSWORD:+${APPLE_TEAM_ID:+set}}}

if [ -n "${NOTARY_API:-}" ] && [ -n "${NOTARY_PASSWORD:-}" ]; then
  echo "release-macos: both notarization credential sets are set; pick one" >&2
  exit 2
fi
if [ -n "${NOTARY_API:-}" ] || [ -n "${NOTARY_PASSWORD:-}" ]; then
  if [ -z "$IDENTITY_HERE" ]; then
    echo "release-macos: notarization credentials require APPLE_SIGNING_IDENTITY" >&2
    exit 2
  fi
fi
if [ -n "$IDENTITY_HERE" ]; then
  echo "release-macos: signing with APPLE_SIGNING_IDENTITY (bundler-managed)"
else
  echo "release-macos: no APPLE_SIGNING_IDENTITY -> staging (ad-hoc signature)"
fi

# Updater artifact signing: `createUpdaterArtifacts: true` in tauri.conf.json
# means every build must sign the .app.tar.gz updater artifact. Precedence:
# TAURI_SIGNING_PRIVATE_KEY env (contents), --updater-key <file>, then the
# owner-path staging key written by ./scripts/updater-keygen.sh. The key is
# never stored in this repository.
UPDKEY=""
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  echo "release-macos: updater key from TAURI_SIGNING_PRIVATE_KEY"
elif [ -n "$UPDATER_KEY_FILE" ]; then
  if [ ! -f "$UPDATER_KEY_FILE" ]; then
    echo "release-macos: --updater-key is not a file: $UPDATER_KEY_FILE" >&2
    exit 2
  fi
  UPDKEY=$(cat "$UPDATER_KEY_FILE")
  echo "release-macos: updater key from $UPDATER_KEY_FILE"
elif [ "${ABBEY_NO_UPDATER_KEY:-0}" = 1 ]; then
  echo "release-macos: ABBEY_NO_UPDATER_KEY=1 -> skipping updater key (build will fail fast)"
  exit 2
else
  STAGING_KEY="$HOME/.local/share/abbey-updater/abbey-updater.key"
  if [ -f "$STAGING_KEY" ]; then
    UPDKEY=$(cat "$STAGING_KEY")
    echo "release-macos: updater key from the owner-path staging key ($STAGING_KEY)"
  else
    echo "release-macos: no updater signing key found; run:" >&2
    echo "               ./scripts/updater-keygen.sh" >&2
    echo "               then rerun with --updater-key <file>" >&2
    exit 2
  fi
fi
export TAURI_SIGNING_PRIVATE_KEY="${TAURI_SIGNING_PRIVATE_KEY:-$UPDKEY}"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

echo "== frontend security gate =="
bun run --silent build
bun run --silent verify:bundle

BUILD_ARGS="build $EDITION_ARGS"
if [ "$SKIP_DMG" = 1 ]; then
  BUILD_ARGS="$BUILD_ARGS --bundles app"
fi
echo "== tauri build: $BUILD_ARGS =="
# `bundle_dmg.sh` detaches its staging image on a timer; a Finder/Spotlight
# worker that grabbed the freshly attached volume makes `hdiutil detach` fail
# with "Resource busy" (a known first-run macOS bundling race). Retryable:
# eject any stuck `/Volumes/dmg.*` interstitial and rebuild; the cargo work is
# cached, so retries repeat only the bundling step.
BUILD_OK=1
for BUILD_ATTEMPT in 1 2 3; do
  if bun run --silent tauri $BUILD_ARGS; then
    BUILD_OK=0
    break
  fi
  [ "$BUILD_ATTEMPT" = 3 ] && break
  echo "release-macos: bundling attempt $BUILD_ATTEMPT failed — ejecting stuck DMG staging mounts and retrying" >&2
  mount | awk '/on \/Volumes\/dmg\./ { print $1 }' | while read -r dev; do
    echo "release-macos: ejecting stuck staging mount $dev" >&2
    diskutil eject "$dev" >/dev/null 2>&1 || hdiutil detach -force "$dev" >/dev/null 2>&1 || true
  done
  sleep 10
done
if [ "$BUILD_OK" != 0 ]; then
  echo "release-macos: tauri build failed 3 times" >&2
  exit 1
fi

# The bundler writes into the workspace-level target dir (the desktop client
# is a member of the repository's cargo workspace, so that is the repository
# root's target dir), not src-tauri/target and not desktop/target.
BUNDLE_DIR="../target/release/bundle"
APP="$BUNDLE_DIR/macos/$PRINT.app"
require_file() { [ -e "$1" ] || { echo "release-macos: missing artifact: $1" >&2; exit 1; }; }
require_file "$APP"
if [ "$SKIP_DMG" = 1 ]; then
  DMG=""
else
  DMG=$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name "${PRINT}_*.dmg" 2>/dev/null | head -1)
fi
echo "artifact app: $APP"
if [ -n "$DMG" ]; then echo "artifact dmg: $DMG"; fi

if [ -n "$IDENTITY_HERE" ]; then
  echo "release-macos: Developer ID signing is bundler-managed (applied during the build)"
else
  echo "== staging: deterministic ad-hoc signature with hardened runtime =="
  # A linker-signed Mach-O inside an .app is not a signed bundle (`Sealed
  # Resources=none`, no _CodeSignature) and fails strict verification as
  # "no resources but signature indicates they must be present". In staging,
  # always force-sign the bundle from scratch rather than probing for an
  # existing signature — a linker-signed binary would fool the probe.
  codesign --force -s - --deep \
    --options runtime \
    --entitlements src-tauri/entitlements.plist \
    "$APP"
  if [ -n "${DMG:-}" ]; then
    codesign --force -s - "$DMG" >/dev/null 2>&1 || echo "release-macos: dmg ad-hoc sign skipped" >&2
  fi
fi

echo "== verify signature (strict) =="
codesign --verify --strict --verbose=1 "$APP"
codesign --display --verbose=2 "$APP" 2>&1 | grep -E "Identifier=|TeamIdentifier=|flags=" || true

if [ -n "$IDENTITY_HERE" ] && { [ -n "${NOTARY_API:-}" ] || [ -n "${NOTARY_PASSWORD:-}" ]; }; then
  echo "== verify notarization + staple =="
  spctl -a -vv -t exec "$APP"
  xcrun stapler validate "$APP" >/dev/null && echo "staple: app OK"
  if [ -n "${DMG:-}" ]; then
    xcrun stapler validate "$DMG" >/dev/null && echo "staple: dmg OK"
  fi
elif [ -n "$IDENTITY_HERE" ]; then
  echo "release-macos: signed but NOT notarized — add APPLE_API_KEY_PATH/APPLE_API_KEY_ID/APPLE_API_ISSUER"
  echo "               (or APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID) to notarize; Gatekeeper"
  echo "               rejects fresh downloads of signed-but-unnotarized apps."
fi

# Match the updater artifact by the exact shipped app name, not by glob
# order — a second edition built into the same bundle dir must never let a
# previous edition's stale tarball ride into its checksums.
UPDTAR="$BUNDLE_DIR/macos/$(basename "$APP").tar.gz"
if [ -f "$UPDTAR" ] && [ -f "$UPDTAR.sig" ] && [ -s "$UPDTAR.sig" ]; then
  echo "updater artifact: $UPDTAR (+ .sig, verified non-empty; signature checks against the"
  echo "                  pinned plugins.updater.pubkey happen in the app when it updates)"
else
  echo "release-macos: updater artifact missing — createUpdaterArtifacts is on in tauri.conf.json" >&2
  exit 1
fi

OUT_DIR=$(dirname "$APP")
echo "== sha256 =="
{
  if [ -n "${DMG:-}" ]; then
    printf '%s  %s\n' "$(shasum -a 256 "$DMG" | cut -d' ' -f1)" "$(basename "$DMG")"
  fi
  printf '%s  %s\n' "$(shasum -a 256 "$APP/Contents/MacOS/abbey-desktop" | cut -d' ' -f1)" "$(basename "$APP")/Contents/MacOS/abbey-desktop"
  printf '%s  %s\n' "$(shasum -a 256 "$UPDTAR" | cut -d' ' -f1)" "$(basename "$UPDTAR")"
  printf '%s  %s\n' "$(shasum -a 256 "$UPDTAR.sig" | cut -d' ' -f1)" "$(basename "$UPDTAR.sig")"
} | tee "$OUT_DIR/SHA256SUMS.txt"
echo "release-macos: OK ($([ -n "${DMG:-}" ] && echo 'app + dmg' || echo 'app only'), updater artifact signed)"
