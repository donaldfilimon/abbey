# Portable packages inventory — 2026-10-27

Status: Step 1 of desktop-plan Task 14
(`docs/plans/2026-10-27-abbey-desktop-plan.md`, Task 14 Step 1). That plan
file is still untracked in the canonical checkout, so it is named here but
not cited by line. This is an inventory. It changes no code, runs no gate,
edits no ledger, and promotes no claim. It records where the
`portable-package-services` goal stands on `main` at `96a5dc3`.

## Method

- **Scope.** One section per acceptance bullet of the
  `portable-package-services` goal (`tasks/completion.json:1247-1256`; the
  bullets are lines 1252, 1253 and 1254).
- **Statuses.**
  - `present`: the item exists on `main` for both editions where editions
    apply.
  - `partial`: the item exists for some platforms or editions only, or
    exists without the packaging that the bullet asks for.
  - `missing`: nothing on `main` provides the item.
- **Evidence.** Every row cites `path:line` on `main` at `96a5dc3`. An
  absence is cited at the place where the item would have to appear.
- **Owner step.** The Task 14 or Task 15 step that closes the row. Task 15
  steps are Donald steps (OB-8, OB-9, OB-10). "—" means nothing remains for
  that row inside this goal.

## 1. Edition-isolated packages, installers, service, updater and retained-state uninstall

Bullet: `tasks/completion.json:1252`.

### 1a. Bundle targets and identities

| Item | Status | Evidence file:line on main | Owner step |
|---|---|---|---|
| Safe edition identifier `com.donaldfilimon.abbey` | present | `desktop/src-tauri/tauri.conf.json:5`; source of truth `src/edition.rs:83`, written by codegen `desktop/codegen/src/main.rs:96-103`, `desktop/codegen/src/editions.rs:65-82` | — |
| Personal edition identifier `com.donaldfilimon.abbey-personal` and product name `Abbey Personal` | present | `desktop/src-tauri/tauri.personal.conf.json:3-4`; source of truth `src/edition.rs:100` | — |
| Bundle targets | partial: macOS only | `desktop/src-tauri/tauri.conf.json:43-46` lists only `"app"` and `"dmg"`. The personal config does not override `bundle` (`desktop/src-tauri/tauri.personal.conf.json:1-28`), so it inherits the same two targets | Task 14 Step 2 |
| macOS app and DMG for both editions | present (source) | `desktop/src-tauri/tauri.conf.json:62-66` (hardened runtime, entitlements); `desktop/scripts/release-macos.sh:55` (`--config src-tauri/tauri.personal.conf.json --features personal-edition`); `.github/workflows/desktop-release.yml:256-257` uploads both DMGs. Signing and notarisation are Task 13 (OB-1…OB-4) | Task 13 (outside this goal's repo steps) |
| Windows installer section (`bundle.windows`: WiX `upgradeCode`, NSIS) | missing | No `windows` key under `bundle` in `desktop/src-tauri/tauri.conf.json:41-67` and no `bundle` key at all in `desktop/src-tauri/tauri.personal.conf.json:1-28` | Task 14 Step 2 |
| Linux native package section (`bundle.linux`: deb, AppImage, rpm, `depends`) | missing | No `linux` key under `bundle` in `desktop/src-tauri/tauri.conf.json:41-67`; none in `desktop/src-tauri/tauri.personal.conf.json:1-28` | Task 14 Step 2 |
| Windows icon | present | `desktop/src-tauri/tauri.conf.json:53` (`icons/icon.ico`) | — |
| Linux package dependencies | partial: CI host check only | `.github/workflows/rust.yml:196-197` checks `gtk+-3.0 webkit2gtk-4.1 librsvg-2.0` on the runner; no package declares them as `depends` | Task 14 Step 2 |
| Codegen keeps new bundle keys | present | `desktop/codegen/src/editions.rs:65-66` rewrites only `identifier`/`productName` (and `mainBinaryName` if present), "leaving every other key exactly as authored" | — |

### 1b. Release pipeline

| Item | Status | Evidence file:line on main | Owner step |
|---|---|---|---|
| Release job runners | partial: macOS ARM64 only | `.github/workflows/desktop-release.yml:49-54` defines a single job `release-macos` on `[self-hosted, macOS, ARM64, abbey]` | Task 14 Step 2 (targets); Task 15 Step 1 (OB-10) for signing |
| Release artifacts | partial: macOS only | `.github/workflows/desktop-release.yml:256-263` uploads two DMGs, two `.app.tar.gz` updater archives with `.sig`, and two checksum files | Task 15 Step 1 (OB-10) |
| Linux and Windows ARM64 CI | partial: build/test gates, no bundling | `.github/workflows/rust.yml:170` (Linux ARM64) and `:298` (Windows ARM64) run gates; `.github/workflows/rust.yml:221-223` runs `./desktop/check.sh`, which does not run `tauri build` | Task 14 Step 2 |
| Windows/Linux package signing | missing | No Authenticode or Linux package signing step in `.github/workflows/desktop-release.yml:106` (the fail-closed secret check covers the macOS secrets only) | Task 15 Step 1 (OB-10) |

### 1c. Updater

| Item | Status | Evidence file:line on main | Owner step |
|---|---|---|---|
| Safe edition updater feed and pinned key | present | `desktop/src-tauri/tauri.conf.json:50` (`createUpdaterArtifacts`), `:69-74` (pubkey, `requireSignedVersion`, `latest.json`) | — |
| Personal edition updater feed | present | `desktop/src-tauri/tauri.personal.conf.json:21-26` (`latest-personal.json`) | — |
| Feed assembly for both editions | partial: macOS platform only | `.github/workflows/desktop-release.yml:201-236` assembles `latest.json` and `latest-personal.json` from `bundle/macos` artifacts only | Task 14 Step 2 / Task 15 |
| Windows updater qualification | partial: script only, unexecuted here | `desktop/scripts/qualify-updater-windows.ps1:1-18` (native Windows only; `InstallerAcceptance` mode expects an operator-supplied NSIS/MSI acceptance script) | Task 15 Step 2 (OB-9) |

### 1d. Installers and service

| Item | Status | Evidence file:line on main | Owner step |
|---|---|---|---|
| Unix CLI + daemon installer, edition-named | present | `install.sh:2`, `:46-57` (names from `abbey edition --name/--daemon-name`, no fallback), `:66-70` (two known identities), `:226-231` (daemon staged when built), `:333-336` | — |
| Windows CLI installer, edition-named, no daemon | partial | `install.ps1:51-61` (edition-derived names), `:90-93` ("not installed … the authenticated daemon is Unix-socket-only"); transport stub at `src/daemon/client.rs:55-57` | Task 15 Step 2 (OB-9) for the journey; no 2026-10-27 plan step adds a Windows daemon transport |
| Shell completions per edition | present | `install.sh:356-364`; `install.ps1:105-122` | — |
| Linux systemd user service, per edition | present | `tools/linux_user_service.py:2`, `:123-136` (edition probe; unit named after the edition's daemon), `:225-245` (start/stop/restart/uninstall); tests `tools/tests/test_linux_user_service.py:124`, `:215` | Task 15 Step 3 (OB-8) for the runtime journey |
| macOS service (launchd) | missing | No LaunchAgent in `install.sh:1-366` or `tools/` | No 2026-10-27 plan step |
| Windows service | missing | `install.ps1:90-93` installs no daemon | No 2026-10-27 plan step |

### 1e. Retained-state uninstall

| Item | Status | Evidence file:line on main | Owner step |
|---|---|---|---|
| `install.sh` uninstall | missing | No uninstall mode in `install.sh:1-366`; `rm` calls at `install.sh:184-208` are rollback of a failed publish only | Task 14 Step 2 (package uninstall) / Task 15 Step 3 |
| `install.ps1` uninstall | missing | No uninstall mode in `install.ps1:1-122`; `Remove-Item` at `:79-81` and `:117-119` only removes staged temporaries | Task 15 Step 2 (OB-9) |
| Linux service uninstall retains state and bearer | present | `tools/linux_user_service.py:241-245` stops, disables and removes only the unit; `tools/tests/test_linux_user_service.py:124-135` asserts the bearer and `runtime.sqlite` survive and the unit is gone | — |
| Package-level (MSI/NSIS/deb/AppImage) uninstall with retained state | missing | Depends on the missing bundle sections above (`desktop/src-tauri/tauri.conf.json:41-67`) | Task 14 Step 2, Task 15 Steps 2-3 |

## 2. Windows 11 ARM and Ubuntu ARM64 package journeys

Bullet: `tasks/completion.json:1253`. Nothing in the repository can satisfy
this bullet. It needs real PowerShell, native WebView sessions, service
restarts and clean-user installs on physical or VM hosts.

| Item | Status | Evidence file:line on main | Owner step |
|---|---|---|---|
| Windows 11 ARM: both editions via real PowerShell, coexistence, WebView chat/tasks/tools/automations, service restart, retained-state uninstall | missing | Required channel `windows11_arm` (`tasks/completion.json:1258-1265`); goal has no evidence (`tasks/completion.json:1257`). Installer exists (`install.ps1:1-7`); no Windows bundle (`desktop/src-tauri/tauri.conf.json:43-46`) | Task 15 Step 2 (OB-9, Donald) |
| Ubuntu ARM64: same journeys with native packages and the user service | missing | Required channel `ubuntu_arm64` (`tasks/completion.json:1258-1265`); service controller `tools/linux_user_service.py:249`; no Linux bundle (`desktop/src-tauri/tauri.conf.json:43-46`) | Task 15 Step 3 (OB-8, Donald) |
| `edition-windows-install-naming` claim | partial (claim stays Partial) | `src/claims/registry.rs:619`; promotion needs Task 15 Step 2's executed `install.ps1` for both editions | Task 15 Step 4 |

## 3. Explicit X11/Wayland and portal capability detection

Bullet: `tasks/completion.json:1254`.

`git grep -n -i -E "wayland|x11|xdg-desktop-portal" main -- src desktop tools`
matches only `src/tui/clipboard.rs:64`. That file picks a clipboard command
by `PATH` lookup alone (`src/tui/clipboard.rs:66`, `:213-224`); it does not
read `WAYLAND_DISPLAY`, `DISPLAY` or `XDG_SESSION_TYPE`, and it detects no
portal. The word "portals" also appears in two doc comments that state the
adapters are unavailable (`src/platform.rs:380`,
`desktop/src-tauri/src/backend/mod.rs:14-20`).

| Item | Status | Evidence file:line on main | Owner step |
|---|---|---|---|
| X11 session detection | missing | Only a comment: `src/tui/clipboard.rs:64-66` | Task 14 Step 3 |
| Wayland session detection | missing | Only a comment: `src/tui/clipboard.rs:64-66` | Task 14 Step 3 |
| `xdg-desktop-portal` capability detection (ScreenCast, RemoteDesktop) | missing | No match outside comments; `src/platform.rs:380` | Task 14 Step 3 |
| Linux GUI inventory | partial: AT-SPI presence only | `src/platform.rs:405-411` checks `/usr/lib/at-spi2-core` or `busctl` and reports "inventory only" | Task 14 Step 3 |
| Capture/control stays unavailable | present (by absence) | `desktop/src-tauri/src/backend/mod.rs:14-20` (adapters "explicitly unavailable"); `src/platform.rs:429-434` ("unavailable for effects") | Task 14 Step 3 must keep this |

**Owning module for Step 3.** No module on `main` detects capture or control
capabilities. The nearest home is the report-only Linux arm of
`src/platform.rs::print_ui_surfaces` (`src/platform.rs:385`, `:405-411`),
which already holds the AT-SPI inventory. Step 3 should either extend that
module with a pure, testable detection function or add a new module beside
it. Choosing between the two is a Step 3 decision. The clipboard picker in
`src/tui/clipboard.rs` should not become the owner.

## Gaps feeding Task 14 Steps 2-3

1. **Windows bundle section per edition (Step 2).** Add `bundle.windows`
   (MSI/WiX and NSIS) to both Tauri configs. `tauri.personal.conf.json` has
   no `bundle` object today, so it inherits the safe edition's targets.
   **A per-edition WiX `upgradeCode` GUID is a durable install identity.**
   Once an MSI ships with it, Windows uses it to match upgrades and
   uninstalls for the life of the product, and changing it later orphans
   installs. The GUID for each edition needs owner (Donald) review before
   it is committed. The two editions must never share one.
2. **Linux bundle section per edition (Step 2).** Add `bundle.linux` (deb,
   AppImage, optionally rpm) with `depends` matching the CI host check at
   `.github/workflows/rust.yml:196-197`, and edition-distinct package names.
3. **Bundle targets (Step 2).** Extend `bundle.targets`
   (`desktop/src-tauri/tauri.conf.json:43-46`) or select per platform at
   build time. The planned test
   `test_each_edition_declares_distinct_identifiers_for_every_platform_bundle`
   should check both configs, because codegen writes only the identifier
   and product name (`desktop/codegen/src/editions.rs:65-82`).
4. **Release pipeline (Step 2 / OB-10).** `.github/workflows/desktop-release.yml:49-54`
   has only a macOS job. Windows and Linux release jobs, their signing
   secrets and their updater feed platforms are absent.
5. **Uninstall with retained state (Step 2 / OB-8 / OB-9).** Neither
   `install.sh` nor `install.ps1` has an uninstall mode. Only
   `tools/linux_user_service.py:241-245` uninstalls, and it retains state.
6. **Services outside Linux.** No macOS LaunchAgent and no Windows service
   exist. The Windows daemon is blocked on the named-pipe transport
   (`src/daemon/client.rs:55-57`). No 2026-10-27 plan step covers either.
7. **X11/Wayland/portal detection (Step 3).** Nothing detects the session
   type or portal availability. Step 3 owns a pure detector, a
   `#[cfg(target_os = "linux")]` test
   `linux_capture_is_unavailable_without_a_detected_portal`, and keeping
   capture and control unavailable when no portal is detected.
8. **Journeys (Task 15).** OB-9 (Windows 11 ARM) and OB-8 (Ubuntu ARM64)
   are Donald steps. No repository change can satisfy them.
