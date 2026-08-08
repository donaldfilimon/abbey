#!/bin/sh
# Install Abbey CLI/TUI + shell completions (Unix / macOS / Git Bash).
set -eu
cd "$(dirname "$0")"

cargo build --release --locked
# Honour CARGO_TARGET_DIR (Cursor sandboxes redirect it); fall back to ./target.
BIN="${CARGO_TARGET_DIR:-target}/release/abbey"
if [ ! -f "$BIN" ] && [ -f "${CARGO_TARGET_DIR:-target}/release/abbey.exe" ]; then
  BIN="${CARGO_TARGET_DIR:-target}/release/abbey.exe"
fi

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
mv -f "$STAGED_BIN" "$DEST_DIR/abbey"
STAGED_BIN=""
echo "installed: $DEST_DIR/abbey ($("$DEST_DIR/abbey" --version))"

# The daemon is Unix-only until the named-pipe transport lands. It is staged
# independently so an interrupted install never truncates either good binary.
DAEMON_BIN="${CARGO_TARGET_DIR:-target}/release/abbeyd"
if [ -f "$DAEMON_BIN" ]; then
  STAGED_DAEMON=$(mktemp "$DEST_DIR/.abbeyd.XXXXXX")
  cp "$DAEMON_BIN" "$STAGED_DAEMON"
  chmod 755 "$STAGED_DAEMON"
  mv -f "$STAGED_DAEMON" "$DEST_DIR/abbeyd"
  STAGED_DAEMON=""
  echo "installed: $DEST_DIR/abbeyd (read-only Unix daemon; explicit bearer required)"
fi

write_completion() {
  shell_name="$1"
  destination="$2"
  completion_dir=$(dirname "$destination")
  mkdir -p "$completion_dir"
  STAGED_COMPLETION=$(mktemp "$completion_dir/.abbey-completion.XXXXXX")
  if "$DEST_DIR/abbey" completion "$shell_name" > "$STAGED_COMPLETION"; then
    mv -f "$STAGED_COMPLETION" "$destination"
    STAGED_COMPLETION=""
    return 0
  fi
  echo "warning: could not generate $shell_name completion; existing file preserved" >&2
  return 1
}

# Zsh completions (if modular zsh dir exists)
if [ -d "${COMPLETION_HOME}/.zsh/completions" ]; then
  if write_completion zsh "${COMPLETION_HOME}/.zsh/completions/_abbey_clap"; then
    echo "wrote ${COMPLETION_HOME}/.zsh/completions/_abbey_clap (refresh compinit cache if needed)"
  fi
fi
if [ -d "${COMPLETION_HOME}/.bash_completion.d" ] || mkdir -p "${COMPLETION_HOME}/.local/share/bash-completion/completions" 2>/dev/null; then
  if write_completion bash "${COMPLETION_HOME}/.local/share/bash-completion/completions/abbey"; then
    echo "wrote ${COMPLETION_HOME}/.local/share/bash-completion/completions/abbey"
  fi
fi
