#!/bin/sh
# Install Abbey CLI/TUI + shell completions (Unix / macOS / Git Bash).
set -eu
cd "$(dirname "$0")"

cargo build --release
# Honour CARGO_TARGET_DIR (Cursor sandboxes redirect it); fall back to ./target.
BIN="${CARGO_TARGET_DIR:-target}/release/abbey"
if [ ! -f "$BIN" ] && [ -f "${CARGO_TARGET_DIR:-target}/release/abbey.exe" ]; then
  BIN="${CARGO_TARGET_DIR:-target}/release/abbey.exe"
fi

DEST_DIR="${ABBEY_INSTALL_DIR:-${HOME}/.local/bin}"
mkdir -p "$DEST_DIR"
# Prefer portable cp+chmod; GNU/BSD `install` is not always present (e.g. minimal images).
cp "$BIN" "$DEST_DIR/abbey"
chmod 755 "$DEST_DIR/abbey"
echo "installed: $DEST_DIR/abbey ($("$DEST_DIR/abbey" --version))"

# Zsh completions (if modular zsh dir exists)
if [ -d "${HOME}/.zsh/completions" ]; then
  "$DEST_DIR/abbey" completion zsh > "${HOME}/.zsh/completions/_abbey_clap" || true
  echo "wrote ~/.zsh/completions/_abbey_clap (run: rm -f ~/.zcompdump; compinit)"
fi
if [ -d "${HOME}/.bash_completion.d" ] || mkdir -p "${HOME}/.local/share/bash-completion/completions" 2>/dev/null; then
  mkdir -p "${HOME}/.local/share/bash-completion/completions"
  "$DEST_DIR/abbey" completion bash > "${HOME}/.local/share/bash-completion/completions/abbey" || true
fi
