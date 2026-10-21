#!/bin/sh
# Run the root gate against the sibling revisions that CI pins.
#
# The local `../abi` and `../wdbx` checkouts are separate repositories, with
# their own gates and their own in-flight work. When one of them is mid-change
# the root gate can fail for reasons that have nothing to do with Abbey — and
# the sibling's sources are not ours to reformat, adjust, or commit. This
# script reproduces the gate CI actually runs: the sibling *path dependencies*
# are overridden to clean archives of the pinned revisions, so the gate judges
# Abbey against the same siblings CI builds against.
#
# It changes nothing: no file in either sibling checkout is written, no file in
# this repository is written, and the archives live in a temporary directory
# that is removed on exit. The crates.io cache is reused through a symlink, so
# the run does not re-download the dependency graph.
#
# The build also runs in its own target directory. A shared target directory is
# shared with every other session working in this checkout, and a concurrent
# build replaces an artifact while a running rustdoc or test still refers to
# it — which surfaces as "can't find crate" for every dependency rather than
# as a real failure.
#
# usage: tools/ci/verify-pinned-siblings.sh [--keep | --print-pins | --shared-target]
set -eu

keep=0
print_pins=0
shared_target=0
for argument in "$@"; do
    case "$argument" in
        --keep) keep=1 ;;
        --print-pins) print_pins=1 ;;
        --shared-target) shared_target=1 ;;
        *)
            echo "usage: tools/ci/verify-pinned-siblings.sh [--keep | --print-pins | --shared-target]" >&2
            exit 2
            ;;
    esac
done

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
workflow="$root/.github/workflows/rust.yml"

if [ ! -f "$workflow" ]; then
    echo "pinned-siblings: $workflow is missing" >&2
    exit 1
fi

# The workflow is the single source of truth for the pins. Reading them here
# means this script cannot drift from what CI builds against.
read_pin() {
    sed -n "s/^  $1: //p" "$workflow" | head -1
}
abi_revision=$(read_pin ABI_REVISION)
wdbx_revision=$(read_pin WDBX_REVISION)

case "$abi_revision" in
    "" | *[!0-9a-f]*)
        echo "pinned-siblings: ABI_REVISION in $workflow is not a 40-hex revision" >&2
        exit 1
        ;;
esac
case "$wdbx_revision" in
    "" | *[!0-9a-f]*)
        echo "pinned-siblings: WDBX_REVISION in $workflow is not a 40-hex revision" >&2
        exit 1
        ;;
esac
if [ "${#abi_revision}" -ne 40 ] || [ "${#wdbx_revision}" -ne 40 ]; then
    echo "pinned-siblings: a pinned revision is not exactly 40 hexadecimal characters" >&2
    exit 1
fi

if [ "$print_pins" -eq 1 ]; then
    echo "ABI_REVISION=$abi_revision"
    echo "WDBX_REVISION=$wdbx_revision"
    exit 0
fi

for sibling in abi wdbx; do
    if [ ! -e "$root/../$sibling/.git" ]; then
        echo "pinned-siblings: ../$sibling is not a git checkout" >&2
        exit 1
    fi
done

python=${ABBEY_PYTHON:-}
if [ -z "$python" ]; then
    python=$(command -v python3 || true)
fi
if [ -z "$python" ]; then
    echo "pinned-siblings: python3 is required" >&2
    exit 1
fi

workspace=$(mktemp -d "${TMPDIR:-/tmp}/abbey-pinned-siblings.XXXXXX")
cleanup() {
    if [ "$keep" -eq 1 ]; then
        echo "pinned-siblings: kept $workspace" >&2
        return 0
    fi
    rm -rf "$workspace"
}
# The signal handlers must exit: a handler that only cleans up lets the
# script continue into the gate with the workspace — and therefore the
# pinned override — already deleted.
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

mkdir -p "$workspace/abi" "$workspace/wdbx" "$workspace/cargo-home"

archive_sibling() {
    sibling=$1
    revision=$2
    # `-c gc.auto=0` keeps git from running auto-maintenance, which would
    # write into the sibling's `.git` directory.
    if ! git -C "$root/../$sibling" -c gc.auto=0 cat-file -e "$revision^{commit}" 2>/dev/null; then
        echo "pinned-siblings: ../$sibling does not contain $revision" >&2
        echo "pinned-siblings: fetch it there first (git -C ../$sibling fetch origin $revision)" >&2
        exit 1
    fi
    # `git archive -o` then `tar -xf` keeps the archive step's own status:
    # a pipeline would only report tar's, and a failed archive would leave a
    # truncated sibling tree behind.
    if ! git -C "$root/../$sibling" -c gc.auto=0 archive --format=tar \
        -o "$workspace/$sibling.tar" "$revision"; then
        echo "pinned-siblings: could not archive ../$sibling at $revision" >&2
        exit 1
    fi
    tar -xf "$workspace/$sibling.tar" -C "$workspace/$sibling"
    rm -f "$workspace/$sibling.tar"
}

archive_sibling abi "$abi_revision"
archive_sibling wdbx "$wdbx_revision"

# A Cargo `paths` override replaces a path dependency with a package of the
# same name and version. Listing every sibling crate in the archives overrides
# the direct and the transitive sibling path dependencies alike.
#
# The manifests are read with the repository's own structural TOML reader
# (`tools/toolchain_policy.py`), so the helper works on the same interpreters
# the rest of the tooling supports.
"$python" - "$workspace" "$root" <<'PY'
import pathlib
import sys

workspace = pathlib.Path(sys.argv[1])
root = pathlib.Path(sys.argv[2])
sys.path.insert(0, str(root / "tools"))

import toolchain_policy  # noqa: E402  (path is set above)

entries = []
seen = set()
for sibling in ("abi", "wdbx"):
    crates = workspace / sibling / "crates"
    if not crates.is_dir():
        continue
    for manifest in sorted(crates.glob("*/Cargo.toml")):
        data = toolchain_policy.tomllib.loads(manifest.read_text(encoding="utf-8"))
        name = data.get("package", {}).get("name")
        if not name or name in seen:
            continue
        seen.add(name)
        entries.append(manifest.parent)

if not entries:
    raise SystemExit("pinned-siblings: the archives contain no sibling crates")

config = workspace / "cargo-home" / "config.toml"
config.write_text(
    "paths = [\n" + "".join(f'    "{path}",\n' for path in entries) + "]\n",
    encoding="utf-8",
)
print(f"pinned-siblings: {len(entries)} sibling packages overridden")
PY

# Reuse the crates.io cache so the run does not re-download the graph.
if [ -d "$HOME/.cargo/registry" ] && [ ! -e "$workspace/cargo-home/registry" ]; then
    ln -s "$HOME/.cargo/registry" "$workspace/cargo-home/registry"
fi

# An isolated target directory by default. `--shared-target` opts back into
# the caller's directory, which is only safe when no other session is
# building in this checkout.
if [ "$shared_target" -eq 1 ]; then
    echo "pinned-siblings: using the shared target directory on request" >&2
else
    CARGO_TARGET_DIR="$workspace/target"
    export CARGO_TARGET_DIR
fi

echo "pinned-siblings: abi@$abi_revision wdbx@$wdbx_revision"
echo "pinned-siblings: target directory ${CARGO_TARGET_DIR:-<shared>}"

CARGO_HOME="$workspace/cargo-home"
export CARGO_HOME
cd "$root"
./check.sh
