#!/bin/sh
# Real macOS Tauri window acceptance; never substitutes the backend-only proof.
set -eu
cd "$(dirname "$0")/.."
exec python3 scripts/prove_routes_macos.py "$@"
