#!/bin/sh
# Abbey desktop gate.
#
# Deliberately NOT called by the repository root `./check.sh`: `desktop/` is a
# separate cargo workspace and a bun project, and the root gate must stay a
# pure Rust-crate gate that needs neither bun nor a WebView toolchain. The root
# gate runs only the codegen drift check below, with `--locked` (it builds the
# codegen crate, never the Tauri app), so generated-type drift from
# `src/app_core/` is caught there; everything else here is caught only when
# someone runs this — so run it before committing anything under `desktop/`.
set -eu
cd "$(dirname "$0")"
if [ -z "${ABBEY_PYTHON:-}" ]; then ABBEY_PYTHON=$(command -v python3); fi
export ABBEY_PYTHON

ABBEY_TOOLCHAIN_ROOT=$(CDPATH= cd .. && pwd)
"$ABBEY_TOOLCHAIN_ROOT/tools/cargo.sh" --abbey-qualify-toolchain
. "$ABBEY_TOOLCHAIN_ROOT/tools/toolchain-env.sh"

echo "== native Routes acceptance assertion tests =="
"$ABBEY_PYTHON" -m unittest discover -s scripts/tests -p 'test_*.py'

echo "== codegen drift =="
cargo run --quiet --locked -p abbey-desktop-codegen -- --check

echo "== shell script syntax =="
sh -n scripts/release-macos.sh
sh -n scripts/prove-daemon-read.sh
sh -n scripts/assemble-update-feed.sh
sh -n scripts/prove-menu-icon-macos.sh

echo "== typecheck =="
bun run --silent typecheck

echo "== model operation state and recovery =="
bun run --silent test:models

echo "== frontend build =="
bun run --silent build

echo "== bundle security =="
bun run --silent verify:bundle

echo "== cargo test (default edition) =="
cargo test --quiet --locked -p abbey-desktop

# The personal edition is a compile-time cfg and is invisible to the run above,
# exactly as it is for the root gate. Gate it explicitly or it rots.
echo "== cargo test (--features personal-edition) =="
cargo test --quiet --locked -p abbey-desktop --features personal-edition

# `a_configured_daemon_never_falls_back_to_the_in_process_core` early-returns
# unless a bearer is exported, so the run above can never reach its assertions —
# the discriminating no-silent-fallback test was passing vacuously in every gate.
# `std::env::set_var` is `unsafe` in edition 2024 and this crate denies unsafe,
# so the variable has to come from here. No daemon is started on purpose: the
# test requires a configured bearer with nothing listening, and asserts the
# desktop fails closed instead of answering from the in-process core.
echo "== cargo test (live scratch abbeyd — desktop reads, no in-process fallback) =="
./scripts/prove-daemon-read.sh

echo "== cargo test (bearer configured — proves no silent fallback) =="
ABBEYD_BEARER_TOKEN="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')" \
  cargo test --quiet --locked -p abbey-desktop a_configured_daemon_never_falls_back

# `cargo check` does not link. A Tauri binary that type-checks can still fail to
# link against the platform WebView frameworks, so build a real binary.
echo "== cargo build (links a real binary) =="
cargo build --quiet --locked -p abbey-desktop

# When a staging bundle is present, prove that its updater artifact verifies
# against the key this client pins — the same check the release workflow runs
# before it publishes anything. No bundle means no artifact to check, so this
# is skipped rather than failed; the gate never requires a build.
if [ -f target/release/bundle/macos/Abbey.app.tar.gz ]; then
  echo "== updater artifact verifies against the pinned key =="
  ABBEY_DESKTOP_UPDATE_ARTIFACT="../target/release/bundle/macos/Abbey.app.tar.gz" \
    cargo test --quiet --locked -p abbey-desktop --test updater_signature
fi

if [ "${ABBEY_DESKTOP_ROUTES_ACCEPTANCE:-0}" = "1" ]; then
  echo "== macOS Routes window acceptance (required) =="
  ./scripts/prove-routes-macos.sh
fi

# Packaged-app acceptance requires a staging bundle from release-macos.sh; it
# launches the .app bundle binary itself, so bundle layout, packaged Info.plist
# and the hardened-runtime signature are exercised, not just the raw binary.
if [ "${ABBEY_DESKTOP_PACKAGED_ACCEPTANCE:-0}" = "1" ]; then
  echo "== macOS packaged .app Routes acceptance (required, after release-macos.sh) =="
  ./scripts/prove-packaged-macos.sh
fi

# The menu/icon proof needs a staging bundle too (it inspects the packaged
# bundle's icon and drives the packaged app's AX menu bar), and it needs the
# same Accessibility permission as the Routes acceptance.
if [ "${ABBEY_DESKTOP_MENU_ACCEPTANCE:-0}" = "1" ]; then
  echo "== macOS packaged menu/icon acceptance (required, after release-macos.sh) =="
  ./scripts/prove-menu-icon-macos.sh --app "target/release/bundle/macos/Abbey.app" --expect-edition safe
fi

echo "desktop/check.sh: OK"
