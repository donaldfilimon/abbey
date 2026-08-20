#!/usr/bin/env sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"

if command -v cargo-audit >/dev/null 2>&1; then
    echo "dependency-scan: RUN cargo-audit ($(cargo-audit --version))"
    echo "dependency-scan: scoped exception RUSTSEC-2025-0141 (bincode 1.3.3 via syntect's compiled syntax/theme data)"
    echo "dependency-scan: scoped exception RUSTSEC-2024-0436 (paste 1.0.15 via sibling ABI's current Candle/tokenizers graph)"
    exec cargo audit --deny warnings \
        --ignore RUSTSEC-2025-0141 \
        --ignore RUSTSEC-2024-0436
fi

if command -v cargo-deny >/dev/null 2>&1; then
    echo "dependency-scan: RUN cargo-deny ($(cargo-deny --version))"
    exec cargo deny check advisories bans licenses sources
fi

echo "dependency-scan: SKIP — install cargo-audit or cargo-deny; no dependency scanner ran" >&2
echo "dependency-scan: DAST, Semgrep, Guardian, and hosted code scanning are separate evidence" >&2

if [ "${ABBEY_DEP_SCAN_REQUIRE:-0}" = "1" ]; then
    echo "dependency-scan: FAIL — ABBEY_DEP_SCAN_REQUIRE=1" >&2
    exit 2
fi

exit 0
