# Source with ABBEY_TOOLCHAIN_ROOT set to the repository root.
# Capture Python before system linker precedence and preserve it in children.
if [ -z "${ABBEY_PYTHON:-}" ]; then ABBEY_PYTHON=$(command -v python3); fi
export ABBEY_PYTHON
# Ensure common toolchain directories are in PATH if rustup is not already resolvable
if ! command -v rustup >/dev/null 2>&1; then
  for p in /opt/homebrew/bin /opt/homebrew/sbin "$HOME/.cargo/bin" /usr/local/bin; do
    if [ -d "$p" ]; then
      PATH="$p:$PATH"
    fi
  done
  export PATH
fi

# Resolve only in the repository; preserve the caller's working directory.
ABBEY_TOOLCHAIN_CARGO=$(cd "$ABBEY_TOOLCHAIN_ROOT" && rustup which cargo) || return 1
ABBEY_TOOLCHAIN_BIN=$(dirname "$ABBEY_TOOLCHAIN_CARGO")
export PATH="$ABBEY_TOOLCHAIN_BIN:/usr/bin:$PATH"
if [ -z "${CC:-}" ] && [ -x /usr/bin/cc ]; then export CC=/usr/bin/cc; fi
if [ -z "${CXX:-}" ] && [ -x /usr/bin/c++ ]; then export CXX=/usr/bin/c++; fi
# Direct Cargo bypasses rustup's loader setup. Release rust-objcopy and rust-lld
# need libLLVM in this toolchain's lib directory. Source this helper to preserve
# a live DYLD fallback. On macOS protected shell interpreters strip incoming
# DYLD_* before executable scripts run: callers crossing that boundary can set
# ABBEY_DYLD_FALLBACK_LIBRARY_PATH instead. Export that bridge for nested scripts.
ABBEY_TOOLCHAIN_LIB="${ABBEY_TOOLCHAIN_BIN%/bin}/lib"
ABBEY_LOADER_FALLBACK=${DYLD_FALLBACK_LIBRARY_PATH:-${ABBEY_DYLD_FALLBACK_LIBRARY_PATH:-$HOME/lib:/usr/local/lib:/usr/lib}}
case "$ABBEY_LOADER_FALLBACK" in
  "$ABBEY_TOOLCHAIN_LIB"|"$ABBEY_TOOLCHAIN_LIB":*) ;;
  *) ABBEY_LOADER_FALLBACK="$ABBEY_TOOLCHAIN_LIB:$ABBEY_LOADER_FALLBACK" ;;
esac
export DYLD_FALLBACK_LIBRARY_PATH="$ABBEY_LOADER_FALLBACK"
export ABBEY_DYLD_FALLBACK_LIBRARY_PATH="$ABBEY_LOADER_FALLBACK"
