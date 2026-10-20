# Rust 1.99.1 migration and latest dependency qualification

Authority: Donald requested latest ABI/WDBX qualification, repair of LLVM stripping warnings, and Abbey's completed stable migration with full gates. Preserve retained Zig evidence, defaults, formats and runtime authority boundaries. No commits, push, CI execution, dependency upgrades or live stores.

## Task 1: Compiler and stripping integration

In this isolated candidate, pin Rust 1.99.1 and select its matched Cargo/compiler/documentation/format/lint tools regardless of hostile PATH. Reuse the existing ABI toolchain-loader approach narrowly, preserving caller directory and linker behavior. Correct the LLVM fallback library path without copying libraries or changing global environment. Make root gate, desktop gate and Unix installer use this compiler environment. Add focused behavioral regressions including loader-path preservation and unrelated working directories. Reject unqualified compiler substitution/bootstrap during qualification. Preserve normal explicit developer overrides if existing conventions allow them. Update local workflow toolchain/dependency pins to the exact qualified current sibling revisions; do not execute CI. Do not change registry dependency pins or product behavior.

## Task 2: Latest ABI/WDBX audit

Record exact full source/index identities and recent producer changes, relevant producer receipts, and consumer/API compatibility. Freeze complete current inputs, including any dirty files. Identify additional focused checks required by new changes. Read-only producer audit; fixes to producer contracts require coordination, not silent consumer substitution.

## Task 3: Full qualification and canonical cutover

Run focused regressions, then authoritative ./check.sh and separate desktop/check.sh on exact stable 1.99.1 with unchanged full three-repository manifests. Require no LLVM stripping warnings and verify actual scratch-installed CLI, daemon, Metal library layout and rollback. Record all exit statuses, counts, skips and artifact/source hashes. Independently review each repair and final evidence. Only after passing gates copy exact candidate-owned changes to canonical checkout, verify source equality, reconcile current instructions/goals/receipt with historical evidence preserved, and validate any documentation-only delta separately. Archive temporary worktree and preserve evidence externally. Broader Proposed capabilities stay open.
