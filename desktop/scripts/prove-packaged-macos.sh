#!/bin/sh
# Packaged .app Routes acceptance; never substitutes the unpackaged proof.
set -eu
cd "$(dirname "$0")/.."
exec python3 scripts/prove_packaged_macos.py "$@"
