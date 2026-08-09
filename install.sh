#!/bin/sh
# Install Abbey CLI/TUI, Unix daemon, and shell completions.
set -eu
cd "$(dirname "$0")"

# Extra cargo features, e.g. `ABBEY_CARGO_FEATURES=personal-edition ./install.sh`
# to build and install the separately named personal edition (src/edition.rs).
if [ -n "${ABBEY_CARGO_FEATURES:-}" ]; then
  cargo build --release --locked --features "$ABBEY_CARGO_FEATURES"
else
  cargo build --release --locked
fi
# Honour CARGO_TARGET_DIR (Cursor sandboxes redirect it); fall back to ./target.
BIN="${CARGO_TARGET_DIR:-target}/release/abbey"
if [ ! -f "$BIN" ] && [ -f "${CARGO_TARGET_DIR:-target}/release/abbey.exe" ]; then
  BIN="${CARGO_TARGET_DIR:-target}/release/abbey.exe"
fi

# Install under the *compiled edition's* names. A personal-edition build
# (`cargo build --release --features personal-edition`) must never overwrite the
# safe edition's binary, completions, or daemon — the names come from the binary
# itself (`src/edition.rs`), not from a literal repeated here.
# No fallback literal. Falling back to 'abbey' on a failed probe is precisely
# the clobber this derivation exists to prevent: a personal-edition build whose
# probe failed would install over the safe edition under its name. Fail the
# install instead. install.ps1 throws for the same reason.
EDITION_BIN=$("$BIN" edition --name 2>/dev/null) || {
    printf 'install.sh: could not read the edition name from %s\n' "$BIN" >&2
    exit 1
}
EDITION_DAEMON=$("$BIN" edition --daemon-name 2>/dev/null) || {
    printf 'install.sh: could not read the daemon name from %s\n' "$BIN" >&2
    exit 1
}
[ -n "$EDITION_BIN" ] && [ -n "$EDITION_DAEMON" ] || {
    printf 'install.sh: edition probe returned an empty name\n' >&2
    exit 1
}

DEST_DIR="${ABBEY_INSTALL_DIR:-${HOME}/.local/bin}"
COMPLETION_HOME="${ABBEY_COMPLETION_HOME:-${HOME}}"
mkdir -p "$DEST_DIR"
# Stage beside the destination so the final rename is atomic on one filesystem.
# `install(1)` is not assumed because minimal images and Git Bash may omit it.
STAGED_BIN=$(mktemp "$DEST_DIR/.abbey.XXXXXX")
cleanup_staged() {
  [ -z "${STAGED_BIN:-}" ] || rm -f -- "$STAGED_BIN"
  [ -z "${STAGED_DAEMON:-}" ] || rm -f -- "$STAGED_DAEMON"
  [ -z "${STAGED_COMPLETION:-}" ] || rm -f -- "$STAGED_COMPLETION"
}
trap cleanup_staged EXIT HUP INT TERM
cp "$BIN" "$STAGED_BIN"
chmod 755 "$STAGED_BIN"
"$STAGED_BIN" --version >/dev/null
mv -f "$STAGED_BIN" "$DEST_DIR/$EDITION_BIN"
STAGED_BIN=""
echo "installed: $DEST_DIR/$EDITION_BIN ($("$DEST_DIR/$EDITION_BIN" --version))"

# The daemon is Unix-only until the named-pipe transport lands. It is staged
# independently so an interrupted install never truncates either good binary.
DAEMON_BIN="${CARGO_TARGET_DIR:-target}/release/abbeyd"
if [ -f "$DAEMON_BIN" ]; then
  STAGED_DAEMON=$(mktemp "$DEST_DIR/.abbeyd.XXXXXX")
  cp "$DAEMON_BIN" "$STAGED_DAEMON"
  chmod 755 "$STAGED_DAEMON"
  mv -f "$STAGED_DAEMON" "$DEST_DIR/$EDITION_DAEMON"
  STAGED_DAEMON=""
  echo "installed: $DEST_DIR/$EDITION_DAEMON (authenticated Unix daemon; bounded run control)"
fi

write_completion() {
  shell_name="$1"
  destination="$2"
  completion_dir=$(dirname "$destination")
  mkdir -p "$completion_dir"
  STAGED_COMPLETION=$(mktemp "$completion_dir/.abbey-completion.XXXXXX")
  if "$DEST_DIR/$EDITION_BIN" completion "$shell_name" > "$STAGED_COMPLETION"; then
    mv -f "$STAGED_COMPLETION" "$destination"
    STAGED_COMPLETION=""
    return 0
  fi
  echo "warning: could not generate $shell_name completion; existing file preserved" >&2
  return 1
}

# Zsh completions (if modular zsh dir exists)
if [ -d "${COMPLETION_HOME}/.zsh/completions" ]; then
  if write_completion zsh "${COMPLETION_HOME}/.zsh/completions/_${EDITION_BIN}_clap"; then
    echo "wrote ${COMPLETION_HOME}/.zsh/completions/_${EDITION_BIN}_clap (refresh compinit cache if needed)"
  fi
fi
if [ -d "${COMPLETION_HOME}/.bash_completion.d" ] || mkdir -p "${COMPLETION_HOME}/.local/share/bash-completion/completions" 2>/dev/null; then
  if write_completion bash "${COMPLETION_HOME}/.local/share/bash-completion/completions/${EDITION_BIN}"; then
    echo "wrote ${COMPLETION_HOME}/.local/share/bash-completion/completions/${EDITION_BIN}"
  fi
fi
