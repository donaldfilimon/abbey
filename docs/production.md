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

## Runtime deps

| Dep | Required? | Notes |
|-----|-----------|-------|
| `cursor-agent` | Yes (default) | LLM executor |
| `abi` | Optional | Prefer for `os` / `plugin` / WDBX CLI |
| git | Optional | diff/commit/pr/branch |

## Config

- `$ABBEY_CONFIG` or `~/.config/abbey/config.toml`
- Env: `ABBEY_MODEL`, `ABBEY_ROLE`, `ABBEY_PERSONA`, `ABBEY_MEMORY_BACKEND`, `ABBEY_AGENT`, `ABBEY_BACKEND`, `ABBEY_FORCE`, `ABBEY_PER_CWD`, `ABBEY_STATE_DIR`

## State locations

- Chat/model/history/routes: `$XDG_STATE_HOME/abbey` (or `~/.local/state/abbey`)
- Memory SQLite: `…/abbey/memory.sqlite`
- Memory WDBX (feature `wdbx`): `…/abbey/wdbx/` (segments + WAL)
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

## Checklist before tagging a release

- [ ] `./check.sh` green
- [ ] `abbey doctor` shows expected stamp/persona/role/memory lines
- [ ] `docs/architecture.md` + `docs/identity.md` claims match code
- [ ] `AGENTS.md` claims gate updated
- [ ] `./install.sh` installs runnable binary
