#!/bin/sh
# Install Abbey CLI/TUI + shell completions
set -eu
cd "$(dirname "$0")"
cargo build --release
mkdir -p "${HOME}/.local/bin"
install -m 755 target/release/abbey "${HOME}/.local/bin/abbey"
echo "installed: ${HOME}/.local/bin/abbey ($("${HOME}/.local/bin/abbey" --version))"

# Zsh completions (if modular zsh dir exists)
if [ -d "${HOME}/.zsh/completions" ]; then
  "${HOME}/.local/bin/abbey" completion zsh > "${HOME}/.zsh/completions/_abbey_clap" || true
  echo "wrote ~/.zsh/completions/_abbey_clap (run: rm -f ~/.zcompdump; compinit)"
fi
if [ -d "${HOME}/.bash_completion.d" ] || mkdir -p "${HOME}/.local/share/bash-completion/completions" 2>/dev/null; then
  mkdir -p "${HOME}/.local/share/bash-completion/completions"
  "${HOME}/.local/bin/abbey" completion bash > "${HOME}/.local/share/bash-completion/completions/abbey" || true
fi
