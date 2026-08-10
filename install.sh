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

# Cargo accepts comma- or space-separated feature lists. Keep accelerator
# packaging explicit: a build that links the ABI Metal dylib must never be
# published without the dylib that its @loader_path dependency requires.
FEATURE_WORDS=$(printf '%s' "${ABBEY_CARGO_FEATURES:-}" | tr ',' ' ')
case " $FEATURE_WORDS " in
  *" accel "*) ACCEL_REQUESTED=1 ;;
  *) ACCEL_REQUESTED=0 ;;
esac

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

ACCEL_DYLIB_NAME="libabi_metal_dot.dylib"
ACCEL_INSTALL_NAME="@loader_path/$ACCEL_DYLIB_NAME"
ACCEL_DYLIB="$(dirname "$BIN")/$ACCEL_DYLIB_NAME"
DAEMON_BIN="${CARGO_TARGET_DIR:-target}/release/abbeyd"

verify_accel_dylib() {
  candidate="$1"
  otool -D "$candidate" | grep -Fqx "$ACCEL_INSTALL_NAME"
}

verify_accel_binary() {
  candidate="$1"
  otool -L "$candidate" | grep -Fq "$ACCEL_INSTALL_NAME ("
}

smoke_daemon_without_starting() {
  candidate="$1"
  if daemon_output=$(
    unset ABBEYD_BEARER_TOKEN ABBEYD_BEARER_TOKEN_FILE \
      ABBEY_PERSONAL_DAEMON_BEARER_TOKEN \
      ABBEY_PERSONAL_DAEMON_BEARER_TOKEN_FILE
    "$candidate" 2>&1
  ); then
    return 1
  fi
  printf '%s\n' "$daemon_output" |
    grep -Eq '^abbeyd: set exactly one of [A-Z0-9_]+_BEARER_TOKEN or [A-Z0-9_]+_BEARER_TOKEN_FILE$'
}

smoke_accel_binary() {
  candidate="$1"
  if accel_output=$("$candidate" accel verify --json 2>/dev/null); then
    :
  else
    accel_status=$?
    # Exit 1 is the honest CPU-fallback/no-native-device report. A loader
    # abort, malformed invocation, or other process failure is not acceptable.
    [ "$accel_status" -eq 1 ] || return 1
  fi
  printf '%s\n' "$accel_output" | grep -Fq '"kernels_linked": true'
}

# All platform, source-artifact, install-name, and linked-dependency checks run
# before the destination is created or touched. An unsupported accelerator
# layout is therefore a refusal, never a partially installed application.
if [ "$ACCEL_REQUESTED" -eq 1 ]; then
  [ "$(uname -s 2>/dev/null || printf unknown)" = "Darwin" ] || {
    printf 'install.sh: accelerator installation requires a verified macOS loader layout\n' >&2
    exit 1
  }
  command -v otool >/dev/null 2>&1 || {
    printf 'install.sh: accelerator installation requires otool verification\n' >&2
    exit 1
  }
  [ -f "$ACCEL_DYLIB" ] && [ ! -L "$ACCEL_DYLIB" ] || {
    printf 'install.sh: missing regular ABI Metal dylib beside %s\n' "$BIN" >&2
    exit 1
  }
  verify_accel_dylib "$ACCEL_DYLIB" || {
    printf 'install.sh: ABI Metal dylib identity is not %s\n' "$ACCEL_INSTALL_NAME" >&2
    exit 1
  }
  verify_accel_binary "$BIN" || {
    printf 'install.sh: Abbey binary does not use %s\n' "$ACCEL_INSTALL_NAME" >&2
    exit 1
  }
  if [ -f "$DAEMON_BIN" ]; then
    verify_accel_binary "$DAEMON_BIN" || {
      printf 'install.sh: Abbey daemon does not use %s\n' "$ACCEL_INSTALL_NAME" >&2
      exit 1
    }
  fi
fi

DEST_DIR_CREATED=0
if [ ! -d "$DEST_DIR" ]; then
  mkdir -p "$DEST_DIR"
  DEST_DIR_CREATED=1
fi
# Stage the exact final layout beside the destination so every validation runs
# against @loader_path as installed. Publication backs up all replaced files
# and the EXIT/signal trap restores them unless the whole unit commits.
STAGE_DIR=$(mktemp -d "$DEST_DIR/.abbey-install.XXXXXX")
STAGED_BIN="$STAGE_DIR/$EDITION_BIN"
STAGED_DAEMON=""
STAGED_DYLIB=""
BACKUP_DIR=""
PUBLISH_IN_PROGRESS=0
PUBLISH_COMMITTED=0
HAD_BIN=0
HAD_DAEMON=0
HAD_DYLIB=0
INSTALL_DAEMON=0
DEST_BIN="$DEST_DIR/$EDITION_BIN"
DEST_DAEMON="$DEST_DIR/$EDITION_DAEMON"
DEST_DYLIB="$DEST_DIR/$ACCEL_DYLIB_NAME"
STAGED_COMPLETION=""

rollback_publish() {
  if [ "$PUBLISH_IN_PROGRESS" -eq 1 ] && [ "$PUBLISH_COMMITTED" -eq 0 ]; then
    rm -f -- "$DEST_BIN"
    if [ "$INSTALL_DAEMON" -eq 1 ]; then
      rm -f -- "$DEST_DAEMON"
    fi
    if [ "$ACCEL_REQUESTED" -eq 1 ]; then
      rm -f -- "$DEST_DYLIB"
    fi
    if [ "$HAD_BIN" -eq 1 ]; then
      mv "$BACKUP_DIR/binary" "$DEST_BIN"
    fi
    if [ "$HAD_DAEMON" -eq 1 ]; then
      mv "$BACKUP_DIR/daemon" "$DEST_DAEMON"
    fi
    if [ "$HAD_DYLIB" -eq 1 ]; then
      mv "$BACKUP_DIR/dylib" "$DEST_DYLIB"
    fi
    PUBLISH_IN_PROGRESS=0
  fi
}

cleanup_staged() {
  rollback_publish
  [ -z "${STAGE_DIR:-}" ] || rm -rf -- "$STAGE_DIR"
  [ -z "${BACKUP_DIR:-}" ] || rm -rf -- "$BACKUP_DIR"
  [ -z "${STAGED_COMPLETION:-}" ] || rm -f -- "$STAGED_COMPLETION"
  if [ "${DEST_DIR_CREATED:-0}" -eq 1 ] && [ "$PUBLISH_COMMITTED" -eq 0 ]; then
    rmdir "$DEST_DIR" 2>/dev/null || true
  fi
}
trap cleanup_staged EXIT HUP INT TERM

cp "$BIN" "$STAGED_BIN"
chmod 755 "$STAGED_BIN"

if [ -f "$DAEMON_BIN" ]; then
  INSTALL_DAEMON=1
  STAGED_DAEMON="$STAGE_DIR/$EDITION_DAEMON"
  cp "$DAEMON_BIN" "$STAGED_DAEMON"
  chmod 755 "$STAGED_DAEMON"
fi

if [ "$ACCEL_REQUESTED" -eq 1 ]; then
  STAGED_DYLIB="$STAGE_DIR/$ACCEL_DYLIB_NAME"
  cp "$ACCEL_DYLIB" "$STAGED_DYLIB"
  chmod 755 "$STAGED_DYLIB"
  verify_accel_dylib "$STAGED_DYLIB" || {
    printf 'install.sh: staged ABI Metal dylib identity verification failed\n' >&2
    exit 1
  }
  verify_accel_binary "$STAGED_BIN" || {
    printf 'install.sh: staged Abbey accelerator linkage verification failed\n' >&2
    exit 1
  }
  if [ -n "$STAGED_DAEMON" ]; then
    verify_accel_binary "$STAGED_DAEMON" || {
      printf 'install.sh: staged Abbey daemon accelerator linkage verification failed\n' >&2
      exit 1
    }
  fi
fi

# These executions are against the staged install directory, not target/release.
# For accelerator builds the dynamic loader must resolve the copied dylib before
# either process can even enter its argument parser.
"$STAGED_BIN" --version >/dev/null
if [ -n "$STAGED_DAEMON" ]; then
  smoke_daemon_without_starting "$STAGED_DAEMON"
fi
if [ "$ACCEL_REQUESTED" -eq 1 ]; then
  smoke_accel_binary "$STAGED_BIN"
fi

BACKUP_DIR=$(mktemp -d "$DEST_DIR/.abbey-backup.XXXXXX")
PUBLISH_IN_PROGRESS=1
if [ -e "$DEST_BIN" ] || [ -L "$DEST_BIN" ]; then
  mv "$DEST_BIN" "$BACKUP_DIR/binary"
  HAD_BIN=1
fi
if [ -n "$STAGED_DAEMON" ] && { [ -e "$DEST_DAEMON" ] || [ -L "$DEST_DAEMON" ]; }; then
  mv "$DEST_DAEMON" "$BACKUP_DIR/daemon"
  HAD_DAEMON=1
fi
if [ "$ACCEL_REQUESTED" -eq 1 ] && { [ -e "$DEST_DYLIB" ] || [ -L "$DEST_DYLIB" ]; }; then
  mv "$DEST_DYLIB" "$BACKUP_DIR/dylib"
  HAD_DYLIB=1
fi

if [ "$ACCEL_REQUESTED" -eq 1 ]; then
  mv "$STAGED_DYLIB" "$DEST_DYLIB"
  STAGED_DYLIB=""
fi
if [ -n "$STAGED_DAEMON" ]; then
  mv "$STAGED_DAEMON" "$DEST_DAEMON"
  STAGED_DAEMON=""
fi
mv "$STAGED_BIN" "$DEST_BIN"
STAGED_BIN=""

# Post-publication verification is inside the rollback window. A loader,
# identity, or malformed accelerator-report failure restores every prior file.
"$DEST_BIN" --version >/dev/null
if [ "$INSTALL_DAEMON" -eq 1 ]; then
  smoke_daemon_without_starting "$DEST_DAEMON"
fi
if [ "$ACCEL_REQUESTED" -eq 1 ]; then
  verify_accel_dylib "$DEST_DYLIB"
  verify_accel_binary "$DEST_BIN"
  if [ "$INSTALL_DAEMON" -eq 1 ]; then
    verify_accel_binary "$DEST_DAEMON"
  fi
  smoke_accel_binary "$DEST_BIN"
fi

PUBLISH_COMMITTED=1
rm -rf -- "$BACKUP_DIR"
BACKUP_DIR=""
rm -rf -- "$STAGE_DIR"
STAGE_DIR=""

echo "installed: $DEST_BIN ($("$DEST_BIN" --version))"
if [ "$INSTALL_DAEMON" -eq 1 ]; then
  echo "installed: $DEST_DAEMON (authenticated Unix daemon; bounded run control)"
fi
if [ "$ACCEL_REQUESTED" -eq 1 ]; then
  echo "installed: $DEST_DYLIB ($ACCEL_INSTALL_NAME; verified accelerator layout)"
fi

write_completion() {
  shell_name="$1"
  destination="$2"
  completion_dir=$(dirname "$destination")
  mkdir -p "$completion_dir"
  STAGED_COMPLETION=$(mktemp "$completion_dir/.abbey-completion.XXXXXX")
  if "$DEST_BIN" completion "$shell_name" > "$STAGED_COMPLETION"; then
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
