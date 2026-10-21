#!/bin/sh
# Packaged .app menu-bar, icon, and window-title acceptance; never
# substitutes the unpackaged proof.
set -eu
cd "$(dirname "$0")/.."
exec python3 scripts/prove_menu_icon_macos.py "$@"
