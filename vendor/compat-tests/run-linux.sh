#!/bin/sh
# Native Linux qualification: requires GTK3 SDK and an explicit virtual display.
set -eu
cd "$(dirname "$0")/../.."
[ "$(uname -s)" = Linux ] || { echo 'native Linux GTK3 runner requires Linux' >&2; exit 2; }
pkg-config --modversion glib-2.0 gobject-2.0 gtk+-3.0
command -v xvfb-run >/dev/null
: "${CARGO_TARGET_DIR:=$PWD/.work/gtk/target-vendor}"
export CARGO_TARGET_DIR CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
unset ABI_WDBX_PERSIST
export ABI_WDBX_PATH=:memory:
for profile in dev release; do
    set --
    [ "$profile" = dev ] || set -- --release
    python3 tools/security/check_gtk_vendor.py
    xvfb-run -a ./tools/cargo.sh test --locked --manifest-path vendor/Cargo.toml -p gtk-compat-tests --features glib-runtime,gtk-runtime "$@" < /dev/null
    ./tools/cargo.sh test --locked --manifest-path vendor/Cargo.toml -p glib --lib variant_iter "$@" < /dev/null
done
python3 tools/security/check_gtk_vendor.py
