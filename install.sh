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
mkdir -p "$DEST_DIR"
# Stage beside the destination so the final rename is atomic on one filesystem.
# `install(1)` is not assumed because minimal images and Git Bash may omit it.
STAGED_BIN=$(mktemp "$DEST_DIR/.abbey.XXXXXX")
cleanup_staged() {
  [ -z "${STAGED_BIN:-}" ] || rm -f -- "$STAGED_BIN"
  [ -z "${STAGED_COMPLETION:-}" ] || rm -f -- "$STAGED_COMPLETION"
}
trap cleanup_staged EXIT HUP INT TERM
cp "$BIN" "$STAGED_BIN"
chmod 755 "$STAGED_BIN"
"$STAGED_BIN" --version >/dev/null
mv -f "$STAGED_BIN" "$DEST_DIR/abbey"
STAGED_BIN=""
echo "installed: $DEST_DIR/abbey ($("$DEST_DIR/abbey" --version))"

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
if [ -d "${HOME}/.zsh/completions" ]; then
  if write_completion zsh "${HOME}/.zsh/completions/_abbey_clap"; then
    echo "wrote ~/.zsh/completions/_abbey_clap (run: rm -f ~/.zcompdump; compinit)"
  fi
fi
if [ -d "${HOME}/.bash_completion.d" ] || mkdir -p "${HOME}/.local/share/bash-completion/completions" 2>/dev/null; then
  if write_completion bash "${HOME}/.local/share/bash-completion/completions/abbey"; then
    echo "wrote ~/.local/share/bash-completion/completions/abbey"
  fi
fi
