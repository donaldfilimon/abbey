# Desktop presentation and configuration — 2026-10-21

This slice is presentation work, not a promotion of `desktop-tauri-react`.

## Implemented

- Version-1 local presentation preferences preserve prior appearance/sidebar
  settings and add material (`auto`, `glass`, `solid`) and density
  (`comfortable`, `compact`). Invalid or unavailable storage falls back to system
  appearance, automatic material, comfortable spacing, and an expanded sidebar.
- macOS gets glass-inspired **webview chrome**, not Apple's native Liquid Glass.
  Only the sidebar and toolbar receive the material; reading surfaces stay opaque.
  Other or unrecognized platforms use solid surfaces. No private APIs, transparent
  native window, new plugin, or permission grant is introduced.
- Reduced transparency, forced colors, and increased contrast override the
  material. Unsupported backdrop filtering falls back to solid; live media-query
  listeners are removed when the hook is disposed. CSS independently protects
  overlays and chrome when those accessibility preferences apply.
- Reduced motion disables presentation animations and transitions. The explicit
  dark theme no longer receives system-theme overrides.
- Settings exposes material and density choices, explains startup/background/tool
  boundaries, and removes the stale appearance “stub” label.
- The generated Windows `.ico` is now listed in the bundle configuration. This
  is configuration preparation only, not Windows packaging/runtime proof.

## Authority boundaries

Opening Settings does not register login startup, install a background service,
change daemon-owned routes, grant tools, or select a different provider. Closing
a window does not promise cancellation of daemon-owned work. Desktop model
download/load operations are not a default-chat-model selector. CLI/TUI model
selection and daemon startup-owned routes remain separate.

## Validation

The CLI/TUI `/config` report now separates the current session's live backend
and model from local file/default/environment diagnostics. It reads the passed
session configuration rather than cached backend resolution, and malformed
local configuration produces a visible error instead of a substituted default
report. Four isolated process tests cover CLI diagnostic parity, missing and
malformed configuration, and live TUI recipe selection against conflicting
ambient environment. They passed in default and personal-edition modes. This
does not introduce a writable settings editor or a desktop daemon-config API.

`desktop/check.sh` passed after the prover hardening settled: 78 Python tests,
53 frontend tests (including eight preference-policy tests for legacy/corrupt
storage and the platform/accessibility matrix), codegen drift, frontend build,
bundle security, 31 unit tests plus three signature tests in each edition,
live-daemon reads, no-fallback checks, native linking, and the existing built
artifact's signature verification. The initial in-flight run exposed two
abort-check-ID assertion mismatches; the completed regression suite reconciles
those names and the later full run is the accepted result.

The hardened prover rejects unsettled/partial AX observations and a bundle whose
registered icon does not select the checked resource. Its tests and Swift driver
compile passed. A subsequent live run against the existing safe and personal
staging bundles passed 11/11 checks each, including icon registration, settled
AX state, and edition-specific titles. This qualifies those menu/icon behaviors
in the selected bundles, not visual acceptance of this new CSS.

Native visual acceptance on updated bundles, native Apple material, startup and
background-management integrations, writable shared model configuration, and
Windows/Linux runtime qualification remain unproven or unimplemented.
