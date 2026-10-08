# Zig toolchain

Donald authorized the master upgrade on 2026-10-08. The maintained compiler is `0.18.0-dev.120+9fe22a29b`; `build.zig.zon` records its minimum version and `tools/check.sh` rejects a different executing version. The older compiler reference in `AGENTS.md` predates this explicit upgrade.

The official aarch64 macOS archive SHA256 is `b6225af37ce3700dae0326af70d44414bb7b85aaf846243122d945016077c60c`. Use the versioned installation by prepending `~/.local/share/zig/toolchains/0.18.0-dev.120+9fe22a29b` to PATH, then run `./zig/tools/check.sh` from the repository root.

Current local qualification on 2026-10-08: the full Zig gate exited 0 with `check.sh: OK`. Both editions passed 109 library tests, 15 help goldens, contract and claims checks, Rust route and daemon compatibility, terminal restoration, and real ABI execution in temporary isolated state. No runtime source migration was necessary. Rust production launchers retain their separately verified installed artifacts; this compiler upgrade does not select a new daemon implementation.
