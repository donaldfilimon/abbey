# Refactor and completion baseline — 2026-10-23

The inherited source snapshot passed `./check.sh` and then `./desktop/check.sh`, serially. The root gate includes default, WDBX, personal and accelerator tests, four rustdoc cohorts, codegen, claims/instructions, coverage bookkeeping and actual isolated Metal installer verification/coexistence/rollback. The separately run Python tooling suite passed 244 tests. These are local macOS ARM64 results, not three-platform release qualification.

The initial formatting check failed on import ordering in `src/daemon/config/bearer_tests.rs`; changing that order made the gate pass. No other product edit preceded this baseline. Before edits, all tracked/nonignored files in Abbey, ABI and WDBX were recorded with SHA-256, byte count, kind and mode in the adjacent manifests. Original staged/unstaged patches and dirty/untracked bytes from the primary checkout and other registered worktrees are retained privately in `.superpowers/sdd/2026-10-23-abbey-refactor-and-finish/baseline/`; those backups must not be removed while this program is incomplete.

Initial HEADs: Abbey `30e3b8456ff7735891138db59f384d990bb68a2f`, ABI `69910bf61205458dec27dd4b616201e0a5f3898a`, WDBX `36bc99b699de60be6c07b2838d74580084fb5d4c`. The gate used qualified Rust 1.99.1. Local sibling revisions differ from CI pins; no pinned-sibling qualification is claimed.

Another session committed and pushed the already captured dirty work as Abbey `8e724a8587051c010c538873aad1091e788c08c8` while the root gate ran. Comparison against the initial manifests found only the intentional pre-gate formatting change; no additional source-byte drift was observed. The changing HEAD/build metadata means this is source-snapshot evidence, not an exact clean-commit release build. Both sibling source manifests remained unchanged at comparison.

After both gates passed, plugin validation passed and the only outstanding branch commit `bd00ca3` was merged locally as `ec324972a6f194546824298dcc38c38e268622ba`. Other existing branch tips were already ancestors. Worktrees and their dirty files remain preserved. Merged-result qualification is a subsequent result, not covered by these pre-merge logs. This session has not pushed the merge.

Strict dependency audit exited 1: four vulnerabilities (Tokio RUSTSEC-2026-0384 and ZIP RUSTSEC-2026-0367/0368/0369), plus denied proc-macro-error maintenance, GLib unsoundness and yanked-Tokio warnings. No pins or advisory exceptions were changed. The root gate also retained an upstream WDBX unused-import warning.

Live GitHub runner inventory contained one offline macOS ARM64 runner and no registered Linux/Windows runners. Native Ubuntu/Windows, executed CI, signing, production models/media, physical-host deployment and live participant consent are not proved. Optional packaged/native GUI acceptance flags were not enabled in the desktop gate. Cross-target compiler checks were skipped for absent x86_64 targets; these skips are not runtime evidence.

Logs and per-artifact hashes are in [the receipt directory](2026-10-23-refactor-baseline/artifacts.json). The program remains incomplete. The baseline-record requirement may record implemented evidence acquisition; no capability or native milestone is promoted by this baseline.
