# Compatible security pins across product consumers

**Goal:** Remove the observed Tokio and ctutils vulnerabilities from every affected captured lockfile with no unrelated dependency changes.
**Architecture:** One coordinated, exact sibling cohort; each repository retains its independently generated lockfile and existing package identities.
**Spec:** Approved modernization milestone 2; Abbey `dependency-security-remediation` and `baseline`; ABI C01/C42 contribution; WDBX source-specific dependency qualification. This packet does not resolve the separately recorded ZIP, GLib or macro-package findings.
**Tech stack:** Rust 1.99.1, repository wrappers, Cargo locked graphs and existing security policies.

## Global Constraints

- Preserve every existing source, vendor attribution, package identity, historical receipt and security exception. No advisory suppression or source-version relabeling.
- Use isolated task worktrees and build targets. No shared-main edits, live stores or external deployment. ABI is a pinned read-only sibling.
- Restore unrelated Cargo resolver churn and validate the narrowed lockfiles with `metadata --locked`; no copied replacement lockfiles.
- Report scanners and checks by exact scope and exit. Remaining ZIP/GLib/proc-macro-error findings are open, not waived by this compatible patch packet.
- Test persistence uses owned scratch stores: leave ABI_WDBX_PERSIST unset and retain memory fallback. Build caches may be reused only as independent writable clones; preserve provenance and run real checks.

## Task 1: Update all compatible vulnerable pins

**Owned files:** This Abbey checkout `Cargo.lock`; adjacent WDBX `Cargo.lock`, `fuzz/Cargo.lock`, `specimen-studio/Cargo.lock`. No manifest, source, instruction, exception or vendor edits. Commit exact files separately in their owning repositories. The controller owns this plan.

**Exact updates:** Abbey and WDBX Studio Tokio `1.54.0` -> `1.54.1` (RUSTSEC-2026-0384). All four listed lockfiles ctutils `0.4.3` -> `0.4.4` (RUSTSEC-2026-0385). Existing ABI sibling already has both safe versions; Bot Tokio1.53.3 is outside the Interval advisory's affected range and is not changed. Current local advisory records are the source for patched version ranges; if a new scanner finding changes this, report to the controller instead of suppressing it.

- [ ] Read both repository entrypoints and Studio/fuzz guidance applicable to lockfile verification. Record initial commits/lock hashes; inspect exact vulnerable package blocks and advisory fixes.
- [ ] Run targeted `cargo update -p ... --precise ...` in each owning workspace using wrappers/toolchain rules. Restore unrelated Windows dependency-edge churn when existing ranges remain valid. Confirm only the listed package version/checksum entries and strictly required resolution changes remain; run full `metadata --locked` for each workspace.
- [ ] Run actual dependency scans for each lockfile. Abbey uses its required repository scanner with its existing exception policy. For WDBX workspaces, use cargo-audit without adding ignores. Preserve any unrelated failed advisories/warnings explicitly and verify the two targeted advisory IDs are gone.
- [ ] Run Abbey locked library and desktop tests, WDBX locked default workspace tests, WDBX fuzz locked check, and Studio locked native tests (`specimen-kernel`, `specimen-core`, `specimen-wasm`, `specimen-tui`). Run expensive builds serially with separate targets and stdin closed. No mirror tests that simply assert version strings are needed.
- [ ] Run diff checks; compare package identities/versions/dependency edges across each final lock. Preserve old failed security receipts; do not label the complete cohort secure or full root/desktop/Studio qualified from these focused checks.
- [ ] Self-review, commit owned lockfiles, and report source IDs, exact diff, commands, outputs, package/lock hashes, failures and remaining ZIP/GTK repair obligations.

**Rollback:** Revert each owning repository's task commit; the observed vulnerable pins return. No runtime state is migrated.
