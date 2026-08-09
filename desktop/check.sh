#!/bin/sh
# Abbey desktop gate.
#
# Deliberately NOT called by the repository root `./check.sh`: `desktop/` is a
# separate cargo workspace and a bun project, and the root gate must stay a
# pure Rust-crate gate that needs neither bun nor a WebView toolchain. The cost
# is that generated-type drift is only caught when someone runs this — so run it
# before committing anything under `desktop/`.
set -eu
cd "$(dirname "$0")"

echo "== codegen drift =="
cargo run --quiet -p abbey-desktop-codegen -- --check

echo "== typecheck =="
bun run --silent typecheck

echo "== frontend build =="
bun run --silent build

echo "== bundle security =="
bun run --silent verify:bundle

echo "== cargo test (default edition) =="
cargo test --quiet -p abbey-desktop

# The personal edition is a compile-time cfg and is invisible to the run above,
# exactly as it is for the root gate. Gate it explicitly or it rots.
echo "== cargo test (--features personal-edition) =="
cargo test --quiet -p abbey-desktop --features personal-edition

# `cargo check` does not link. A Tauri binary that type-checks can still fail to
# link against the platform WebView frameworks, so build a real binary.
echo "== cargo build (links a real binary) =="
cargo build --quiet -p abbey-desktop

echo "desktop/check.sh: OK"
