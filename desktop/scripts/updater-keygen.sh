#!/bin/sh
# Abbey desktop updater signing-key ceremony (Tauri updater artifacts).
#
# Generates the Ed25519 keypair the updater artifact pipeline signs with.
# The PRIVATE KEY must never be committed or copied into this repository:
# it lives under the owner's data directory by default and is handed to the
# build only through the environment at signing time.
#
#   default output: ~/.local/share/abbey-updater/
#     abbey-updater.key       private key (0600)
#     abbey-updater.key.pub   public key — paste into tauri.conf.json
#                             plugins.updater.pubkey
#
# A newly generated key invalidates the old one: replace the pubkey in
# tauri.conf.json BEFORE building update artifacts, or every client that
# trusts the old pinned key refuses the new artifacts.
#
# Usage:
#   ./scripts/updater-keygen.sh [output-dir]
#
# Then build with (release-macos.sh --updater-key <file> does this for you):
#   export TAURI_SIGNING_PRIVATE_KEY="$(cat <output-dir>/abbey-updater.key)"
#   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
set -eu

cd "$(dirname "$0")/../.."
ROOT=$(pwd)

OUT=${1:-${ABBEY_UPDATER_KEY_OUT:-"$HOME/.local/share/abbey-updater"}}
OUT=$(cd "$OUT" 2>/dev/null && pwd || echo "$OUT")
if [ -z "$OUT" ]; then
  echo "updater-keygen: no output directory" >&2
  exit 2
fi
case "$OUT" in
  "$ROOT"|"$ROOT"/*)
    echo "updater-keygen: refusing to write the private key inside the repository ($OUT)" >&2
    echo "               pass an owner-data path, e.g. ./scripts/updater-keygen.sh" >&2
    exit 2
    ;;
esac

mkdir -p "$OUT"
chmod 700 "$OUT"
KEY="$OUT/abbey-updater.key"
cd "$ROOT/desktop"
if [ -f "$KEY" ]; then
  echo "updater-keygen: regenerating (old key at $KEY becomes invalid)" >&2
fi
bun run --silent tauri signer generate -w "$KEY" -f -p ""

chmod 600 "$KEY"
echo ""
echo "updater-keygen: wrote $KEY (private, never commit) and $KEY.pub"
echo ""
echo "embed into tauri.conf.json -> plugins.updater.pubkey:"
echo "----------------------------------------"
cat "$KEY.pub"
echo "----------------------------------------"
echo "build with:"
echo "  export TAURI_SIGNING_PRIVATE_KEY=\"\$(cat $KEY)\""
echo "  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=\"\""
echo "  ./scripts/release-macos.sh --updater-key $KEY"
