# Abbey production readiness

## Toolchain

- Rust **nightly** (`rust-toolchain.toml`), edition **2024**
- Components: `rustfmt`, `clippy`
- Pin: whatever `rustup show` reports for this directory

## Release gate

```bash
./check.sh          # fmt + clippy -D warnings + test, for BOTH feature sets
./install.sh        # release binary → ~/.local/bin/abbey
abbey doctor        # build stamp + persona/role/memory/os honesty
```

`check.sh` runs clippy/test twice: default features, then `--features wdbx`. A bare
`cargo test` never compiles `src/memory/wdbx.rs`, so it can pass while that code is broken.

`check.sh` is the production bar. Do not ship if it fails.

## Primary host targets

| Target | Portable CLI/TUI/memory/subagents | Notes |
|--------|-----------------------------------|-------|
| Linux | ✓ | primary |
| macOS | ✓ | + voice · `fm` |
| Windows | ✓ | WDBX lock via `fs2` / LockFileEx |
| other Unix | ✓ | same as Linux portable set |

Accelerator **host detection** (`abbey platform compute`) is Current. GPU/NPU/TPU
**runtimes inside Abbey** are Out of scope — see `abbey claims oos`.

## Runtime deps

| Dep | Required? | Notes |
|-----|-----------|-------|
| `cursor-agent` | Yes (default) | LLM executor |
| `fm` (`/usr/bin/fm`) | Only for `ABBEY_BACKEND=fm` | On-device Apple Foundation Model, macOS 26+ |
| `abi` | Optional | Prefer for `os` / `plugin` / WDBX CLI |
| git | Optional | diff/commit/pr/branch |
| `nvidia-smi` / ROCm / TPU tools | Optional | improve `abbey platform` detect only |

## Config

- `$ABBEY_CONFIG` or `~/.config/abbey/config.toml`
- Env: `ABBEY_MODEL`, `ABBEY_ROLE`, `ABBEY_PERSONA`, `ABBEY_MEMORY_BACKEND`, `ABBEY_AGENT`, `ABBEY_BACKEND`, `ABBEY_FORCE`, `ABBEY_PER_CWD`, `ABBEY_STATE_DIR`

## State locations

- Chat/model/history/routes: `$XDG_STATE_HOME/abbey` (or `~/.local/state/abbey`)
- Memory SQLite: `…/abbey/memory.sqlite`
- Memory WDBX (feature `wdbx`): `…/abbey/wdbx/` (segments + WAL)
- `fm` transcripts (backend `fm`): `…/abbey/fm/<chat-id>.transcript`
- Never commit state dirs

## Safety

- OS execute requires `--confirm`
- Allowlist only (see `abbey os allowlist`)
- No fake cost/token claims
- Self-learn train layer requires provenance

## Versioning

- Semver in `Cargo.toml`
- Unique stamp: `ABBEY_BUILD_STAMP` from `build.rs` (git short hash + target + profile + time)
- Surface: `abbey doctor`, `abbey agents`

## API docs

```bash
cargo doc --no-deps --document-private-items
# open: target/doc/abbey/index.html  (or $CARGO_TARGET_DIR/doc/abbey/)
```

## Checklist before tagging a release

- [ ] `./check.sh` green
- [ ] `abbey doctor` shows expected stamp/persona/role/memory/routing/learn lines
- [ ] `docs/architecture.md` + `docs/identity.md` claims match code
- [ ] `AGENTS.md` claims gate updated
- [ ] `./install.sh` installs runnable binary
- [ ] `cargo doc --no-deps --document-private-items` builds
