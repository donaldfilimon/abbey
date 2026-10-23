# Completion Program baseline — 2026-10-22

**Outcome: unqualified.** No root or desktop gate completed. This receipt is a baseline of actual attempts, not release evidence. This task changed no Abbey product source before the attempts below ended. Subsequent shared sibling and lockfile drift is recorded separately; no unchanged-cohort qualification is claimed.

## Inputs

Captured at 2026-10-22T22:31:48.130296+00:00. Rust `1.99.1 (58cf8f9c3 2026-10-13)`, Cargo `1.99.1`, macOS ARM64. All three repositories were clean at capture. The complete per-file SHA-256 maps are retained in [baseline.json](2026-10-22-completion-baseline/baseline.json). Symlinks hash their link text prefixed by `symlink:`; absent tracked paths hash `<missing>`; other files hash their bytes. The aggregate hashes SHA-256 of the compact, sorted-key UTF-8 JSON filename-to-digest map.

| Repository | HEAD | Files | Aggregate SHA-256 |
| --- | --- | --- | --- |
| abbey | `19d934d88ac1527e3de38d2541e784d2b52a3a10` | 910 | `3cd2ddb6d7d10b52737a8f3469c35f748d14b89902c60f645bb951747eb41638` |
| abi | `69910bf61205458dec27dd4b616201e0a5f3898a` | 1277 | `90e52193102aaa86f1c2c91aac2bcdb8dc1919a5dfb3653eca6952918067848a` |
| wdbx | `36bc99b699de60be6c07b2838d74580084fb5d4c` | 1218 | `39f3f12f57fb3b08f13cfaf220bda6925407608e90670fcb1f304a3b8d722373` |

CI pins remain ABI `7b9519b6cfb25cc9bc9db2982e0a901318ba039b` and WDBX `5182bc3a60300e1dd152611025e774c4b906638b`. They differ from the local source inputs above. No pinned-sibling gate was completed, and no pin was changed.

## Attempts

| Command | Result | Evidence boundary |
| --- | --- | --- |
| `./check.sh` | exit 101 after agent terminated stalled compiler with SIGTERM | Stopped in initial `cargo check`; no correctness diagnostic or test result |
| `CARGO_INCREMENTAL=0 ./check.sh` | exit 101 after agent terminated stalled compilers with SIGTERM | Also stopped in initial compilation; disabling incremental did not establish a passing baseline |
| `./desktop/check.sh` | wrapper exit 124 at 60 seconds | 100 native-harness Python assertion tests passed; stopped at codegen compilation, without completing the client gate |
| `python3 -m unittest discover -s tools/tests -p 'test_workflow_guards.py'` | exit 0; 59 tests passed | Workflow source guard tests only; no CI job execution |

The desktop timeout wrapper launched its own process session and terminated only that session on timeout. Root interruption targeted the compiler children owned by these attempts. No source or compiler pins were changed to bypass the failure.

Workspace compiler processes showed negligible CPU progress while procedural-macro dylibs were open. A standalone trivial program compiled with the pinned compiler. A separate `codesign -v` probe on a cached macro dylib also stalled. These observations do **not** establish the cause of the workspace stall. Another ungated Cargo build was observed; the checkout gate lock does not serialize unrelated builds. Retry qualification only after the build environment is healthy, with fresh input manifests.

## External prerequisites observed

`gh api repos/donaldfilimon/abbey/actions/runners` reported exactly one runner: `Abbey-Mac-arm64`, labels `self-hosted, macOS, ARM64, abbey`, status `offline`, not busy. No Linux or Windows runner was registered. No PowerShell or common VM-manager CLI was found on PATH. Those are observed availability limits, not proof that machines or credentials cannot exist elsewhere. Runner/VM administrators own provisioning; actual native runtime, signing, model-license, accelerator and three-VM acceptance remain unproved.

## Retained artifacts

| Artifact | SHA-256 |
| --- | --- |
| [baseline.json](2026-10-22-completion-baseline/baseline.json) | `94b1401d82bee09c084f59eb347d003e5032d13ac83d2a4ff5db0e95bf972fad` |
| [baseline-root.log](2026-10-22-completion-baseline/baseline-root.log) | `e86f9c51ae60a9598bfe44ad9660ef381f52b217128996ea2894e638e708fe24` |
| [baseline-root-no-incremental.log](2026-10-22-completion-baseline/baseline-root-no-incremental.log) | `d19b8a8e60e9f48553fa6d052a8818d389a85aed8bb3daebe97a3d960e974349` |
| [baseline-desktop.log](2026-10-22-completion-baseline/baseline-desktop.log) | `b9a90c328db3626603f4f4787d9593e6901f1976b3ef024e537f6615240b9197` |
| [baseline-workflow-tests.log](2026-10-22-completion-baseline/baseline-workflow-tests.log) | `aed905a2ff0e66d456134cdcac7aca183f0fd888b4ce32e04c7b39e9348735f0` |

This baseline does not close any Completion Program milestone or change a capability claim. Subsequent edits require their own source manifests and validation.
