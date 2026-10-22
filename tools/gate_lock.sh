#!/bin/sh
# One gate at a time per checkout.
#
# `cargo test` releases the build lock before it runs the tests, and Abbey's
# integration tests spawn `CARGO_BIN_EXE_abbey` — a fixed path in the shared
# target directory. A second build in the same checkout can therefore replace
# that path while the first run's tests are still executing, and the tests
# then exercise a binary that is not the one they were built with. The
# symptom is a failure that has nothing to do with the code under test: the
# personal edition resolves `ABBEY_PERSONAL_STATE_DIR` and the safe edition
# resolves `ABBEY_STATE_DIR`, so a swapped binary resolves a different state
# root and the assertion about where the state landed fails.
#
# `check.sh` builds the binaries before it tests them, which protects against
# a *stale* binary left by a previous feature cohort. Only a lock protects
# against a *concurrent* one. `tools/ci/verify-pinned-siblings.sh` solves the
# same problem the other way round, by building in its own target directory.
#
# Usage: . tools/gate_lock.sh && abbey_gate_lock <repository-root>
#
# The lock is advisory and per checkout: it keeps two gates in the same
# checkout from interleaving. It is not a security boundary.

# Take the gate lock for a checkout, or fail if another live gate holds it.
# The lock directory carries the owner's pid, so a lock left behind by a
# killed gate is taken over rather than blocking the next run forever.
abbey_gate_lock() {
    abbey_gate_lock_root=$1
    abbey_gate_lock_key=$(printf '%s' "$abbey_gate_lock_root" | cksum | cut -d ' ' -f 1)
    abbey_gate_lock_dir="${TMPDIR:-/tmp}/abbey-gate-lock-$abbey_gate_lock_key"
    abbey_gate_lock_tries=0
    while ! mkdir "$abbey_gate_lock_dir" 2>/dev/null; do
        abbey_gate_lock_owner=$(cat "$abbey_gate_lock_dir/owner" 2>/dev/null || true)
        case "$abbey_gate_lock_owner" in
            '' | *[!0-9]*)
                # No readable owner: the lock is either being created right
                # now or is debris. Retry, then report it.
                ;;
            *)
                if kill -0 "$abbey_gate_lock_owner" 2>/dev/null; then
                    echo "gate: another gate is running in this checkout (pid $abbey_gate_lock_owner);" >&2
                    echo "gate: one gate at a time per checkout — a concurrent build replaces the" >&2
                    echo "gate: binaries the running tests spawn." >&2
                    return 1
                fi
                # The owner is gone: the lock is stale debris.
                rm -rf "$abbey_gate_lock_dir" 2>/dev/null || true
                ;;
        esac
        abbey_gate_lock_tries=$((abbey_gate_lock_tries + 1))
        if [ "$abbey_gate_lock_tries" -ge 3 ]; then
            echo "gate: could not take the gate lock ($abbey_gate_lock_dir); remove it by hand if it is debris." >&2
            return 1
        fi
    done
    echo $$ > "$abbey_gate_lock_dir/owner"
    # Each signal exits with the conventional status, and the exit trap
    # releases the lock on the way out.
    trap 'rm -rf "$abbey_gate_lock_dir" 2>/dev/null || true' EXIT
    trap 'rm -rf "$abbey_gate_lock_dir" 2>/dev/null || true; exit 129' HUP
    trap 'rm -rf "$abbey_gate_lock_dir" 2>/dev/null || true; exit 130' INT
    trap 'rm -rf "$abbey_gate_lock_dir" 2>/dev/null || true; exit 143' TERM
}
