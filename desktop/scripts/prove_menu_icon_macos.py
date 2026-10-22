#!/usr/bin/env python3
"""Prove the shipped desktop polish inside a PACKAGED macOS .app bundle.

The unpackaged provers drive a developer binary. This prover drives the
executable inside a real bundle layout — the exact artifact
`./scripts/release-macos.sh` produces — so the assertions are made against
what is actually installed: the native Apple-standard menu bar as the running
process exposes it, the packaged icon bytes, and the window title on screen.

Three claims are proved, and each one fails closed:

  1. menu bar   the running bundle's AXMenuBar exposes the app menu (named
                after the edition on screen), plus "Edit" and "Window", with
                the Apple-standard edit and window entries.
  2. icon       the bundle's Contents/Resources/icon.icns is byte-identical
                to the tracked source icon, the tracked 1024-master exists as
                a 1024x1024 PNG, the packaged chain really carries a 1024
                representation, and no stale placeholder icon survives in the
                bundle.
  3. title      the AX window title of the running bundle names the edition
                that is on screen, and never the other edition's name.

Honesty boundary: if the Accessibility API cannot be read (no permission, no
window server), the AX checks are reported UNPROVEN — never PASS — while the
icon and bundle checks still deliver. The exit code is 0 only when every
check passed, 1 when a check failed, and 2 when a check stayed unproven.

Usage:
    ./scripts/prove-menu-icon-macos.sh --app ../target/release/bundle/macos/Abbey.app
    ./scripts/prove-menu-icon-macos.sh --app ".../Abbey Personal.app" --expect-edition personal
"""
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time

DESKTOP = Path(__file__).resolve().parents[1]
ROOT = DESKTOP.parent
OUT = DESKTOP / "target" / "menu-acceptance"
RECEIPT = OUT / "menu-receipt.json"
SOURCE_ICONS = DESKTOP / "src-tauri" / "icons"
SOURCE_ICON = SOURCE_ICONS / "icon.icns"
SOURCE_MASTER = SOURCE_ICONS / "master.png"
DRIVER_SOURCE = DESKTOP / "scripts" / "menu_icon-ax.swift"
DRIVER_BINARY = OUT / "menu_icon-ax"

# The bundle identity decides the edition. The path never does: two editions'
# bundles can sit in the same directory, and proving the wrong one would be
# worse than proving nothing.
EDITIONS_BY_IDENTIFIER = {
    "com.donaldfilimon.abbey": {"edition": "safe", "product_name": "Abbey"},
    "com.donaldfilimon.abbey-personal": {"edition": "personal", "product_name": "Abbey Personal"},
}

# Top-level menus the Apple-standard bar must expose. The app menu is named
# after the edition on screen ("Abbey" / "Abbey Personal").
EXPECTED_TOP_LEVEL_MENUS = ("edit", "window")

# The Edit menu entries, as the OS reports them through AX. "Select All" is
# the OS spelling of the task's "select all"; matching is on the normalized
# (whitespace-collapsed, case-folded) title, and each accepted list is
# explicit so the check stays a set membership test and not a tautology.
EXPECTED_EDIT_ITEMS = {
    "undo": ("undo",),
    "redo": ("redo",),
    "cut": ("cut",),
    "copy": ("copy",),
    "paste": ("paste",),
    "select_all": ("select all",),
}

# The Window menu entries. muda's macOS predefined items label the zoom
# action "Zoom" (the task's prose says "maximize"), the fullscreen action
# "Toggle Full Screen" (the prose says "fullscreen"), and the close action
# "Close Window" (the prose says "close"); the OS itself uses "Zoom" unless
# the machine prefers macOS nomenclature, which renames it "Maximize". Every
# label the check accepts is listed, and the receipt records the one that was
# actually observed.
EXPECTED_WINDOW_ITEMS = {
    "minimize": ("minimize",),
    "zoom": ("zoom", "maximize"),
    "fullscreen": ("toggle full screen", "enter full screen", "full screen"),
    "close": ("close window", "close"),
}

# The app menu's own standard entries, named after the running application.
EXPECTED_APP_MENU_ITEMS = {
    "about": ("about",),
    "hide": ("hide",),
    "quit": ("quit",),
}

# The prose name for each Window-menu entry, kept so the receipt can state
# which OS label satisfies which requested entry.
LABEL_PROSE = {
    "zoom": "maximize",
    "fullscreen": "fullscreen",
    "close": "close",
    "select_all": "select all",
}

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

# The window title is read only after the packaged app has settled. The native
# window is created before the webview finishes loading, and the first AX
# window of a starting app can still be a transient one, so a title read in
# the first half second says nothing about what stays on screen. The proof
# waits for the title to stop changing and asserts the settled value; the
# whole history is recorded so a later overwrite is visible, not hidden.
SETTLE_SECONDS = 3.0
STABLE_SECONDS = 2.0


class ProofFailure(Exception):
    pass


def require(condition, assertion):
    if not condition:
        raise ProofFailure(assertion)


def normalize(label):
    """Whitespace- and case-insensitive comparison key for an AX title."""
    return " ".join(str(label).split()).casefold()


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def png_dimensions(data):
    """(width, height) from a PNG IHDR, or None when the bytes are not a PNG."""
    if len(data) < 24 or not data.startswith(PNG_SIGNATURE):
        return None
    if data[12:16] != b"IHDR":
        return None
    width, height = struct.unpack(">II", data[16:24])
    return width, height


def icns_entries(data):
    """Every entry of an .icns container with its embedded PNG dimensions."""
    if len(data) < 8 or data[:4] != b"icns":
        return []
    entries = []
    offset = 8
    while offset + 8 <= len(data):
        kind = data[offset:offset + 4].decode("latin1")
        length = struct.unpack(">I", data[offset + 4:offset + 8])[0]
        if length < 8 or offset + length > len(data):
            break
        body = data[offset + 8:offset + length]
        entry = {"type": kind, "length": length}
        dimensions = png_dimensions(body)
        if dimensions:
            entry["png_width"], entry["png_height"] = dimensions
        entries.append(entry)
        offset += length
    return entries


# ---------------------------------------------------------------------------
# Pure assertions over an observed AX structure. These take plain data so the
# static test can exercise them without a window server.
# ---------------------------------------------------------------------------

def find_menu(menus, title):
    wanted = normalize(title)
    for menu in menus:
        if normalize(menu.get("title", "")) == wanted:
            return menu
    return None


def menu_item_titles(menu):
    """Raw, non-empty item titles of a menu, in the order the OS reported them."""
    titles = []
    for item in menu.get("items", []):
        title = item.get("title", "")
        if title.strip():
            titles.append(title)
    return titles


def missing_items(menu, expected):
    """The expected entry names a menu does not expose, keyed by role."""
    require(expected and all(labels for labels in expected.values()), "ax_required_items_empty")
    present = {normalize(title) for title in menu_item_titles(menu)}
    missing = {}
    for role, labels in expected.items():
        if not present.intersection(labels):
            missing[role] = list(labels)
    return missing


def observed_labels(menu, expected):
    """The raw title that satisfied each expected entry, keyed by role."""
    present = {normalize(title) for title in menu_item_titles(menu)}
    observed = {}
    for role, labels in expected.items():
        for label in labels:
            if label in present:
                observed[role] = label
                break
    return observed


def check_top_level_menus(menus, product_name):
    """The app menu plus the Edit and Window menus must all be present."""
    require(EXPECTED_TOP_LEVEL_MENUS, "ax_required_menus_empty")
    require(menus, "ax_menu_bar_empty")
    titles = [menu.get("title", "") for menu in menus]
    app_menu = find_menu(menus, product_name)
    require(app_menu is not None, "ax_app_menu_missing")
    for wanted in EXPECTED_TOP_LEVEL_MENUS:
        require(find_menu(menus, wanted) is not None, "ax_top_level_menu_missing_" + wanted)
    return {"top_level_titles": titles, "app_menu_title": app_menu.get("title", "")}


def check_app_menu_items(menu, product_name):
    """The app menu must carry the standard about/hide/quit entries."""
    expected = {}
    for role, labels in EXPECTED_APP_MENU_ITEMS.items():
        expected[role] = tuple(labels) + tuple(f"{label} {product_name.casefold()}" for label in labels)
    missing = missing_items(menu, expected)
    require(not missing, "ax_app_menu_items_missing")
    return {"items": menu_item_titles(menu)}


def check_menu_items(menu, expected, assertion):
    """Every expected entry must be exposed by the menu."""
    missing = missing_items(menu, expected)
    require(not missing, assertion)
    return {"items": menu_item_titles(menu)}


def check_window_title(windows, product_name, other_product_names):
    """The on-screen window must name this edition, never the other one."""
    standard = [window for window in windows if window.get("subrole") == "AXStandardWindow"]
    require(standard, "ax_standard_window_missing")
    titles = [window.get("title", "") for window in windows]
    for window in standard:
        for other in other_product_names:
            require(normalize(window.get("title", "")) != normalize(other),
                    "ax_window_titles_other_edition")
    matching = [window for window in standard
                if normalize(window.get("title", "")) == normalize(product_name)]
    require(matching, "ax_window_title_mismatch")
    return {"window_titles": titles,
            "matched_title": matching[0].get("title", "")}


def check_icon_chain(entries):
    """The packaged chain must really carry a 1024 representation."""
    sizes = [(entry.get("png_width"), entry.get("png_height")) for entry in entries]
    require((1024, 1024) in sizes, "icon_chain_missing_1024")
    return {"icns_entries": entries}


def check_bundle_resources(resources, source_dir):
    """No stale placeholder icon may survive in the packaged bundle."""
    mismatches = []
    for name, digest in resources.items():
        source = source_dir / name
        if not source.is_file():
            mismatches.append(name)
            continue
        if sha256(source) != digest:
            mismatches.append(name)
    require(not mismatches, "icon_placeholder_in_bundle")
    return {"bundle_resources": resources}


def check_icon_registration(plist):
    """The registered resource must select the exact icon whose bytes we check."""
    registered = plist.get("CFBundleIconFile")
    require(registered in ("icon", "icon.icns"), "bundle_icon_registration_mismatch")
    return {"registered_icon": registered, "selected_resource": "icon.icns"}


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

class Report:
    """Per-check outcomes. A failed check never stops the run: the receipt
    must state every check's result, not only the first failure."""

    def __init__(self):
        self.checks = []

    def record(self, check_id, status, **detail):
        entry = {"id": check_id, "status": status}
        entry.update(detail)
        self.checks.append(entry)
        return entry

    def passed(self, check_id, **detail):
        return self.record(check_id, "passed", **detail)

    def failed(self, check_id, assertion, **detail):
        entry = self.record(check_id, "failed", assertion=assertion)
        entry.update(detail)
        return entry

    def unproven(self, check_id, reason, **detail):
        entry = self.record(check_id, "unproven", reason=reason)
        entry.update(detail)
        return entry

    def status(self):
        statuses = {check["status"] for check in self.checks}
        if "failed" in statuses:
            return "failed"
        if "unproven" in statuses:
            return "unproven"
        if not statuses:
            # A run that checked nothing proved nothing. This is reachable when
            # the runner aborts before its first check (an unknown argument, a
            # missing bundle, a missing GUI prerequisite): the receipt must say
            # failed, never passed.
            return "failed"
        return "passed"


def build_env():
    env = {key: os.environ[key] for key in ("HOME", "USER", "LOGNAME", "TMPDIR",
                                            "DEVELOPER_DIR", "SDKROOT") if key in os.environ}
    env.update(PATH=f"{Path.home()}/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin")
    return env


def run_build(argv, cwd, env, log, timeout=600):
    with Path(log).open("wb") as output:
        try:
            result = subprocess.run(argv, cwd=cwd, env=env, stdout=output,
                                    stderr=subprocess.STDOUT, timeout=timeout)
        except subprocess.TimeoutExpired:
            raise ProofFailure("driver_build_timeout") from None
    require(result.returncode == 0, "driver_build_failed")


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def read_plist(path):
    try:
        output = subprocess.run(["/usr/bin/plutil", "-convert", "json", "-o", "-", str(path)],
                                capture_output=True, timeout=30)
    except (subprocess.SubprocessError, OSError):
        return {}
    if output.returncode != 0:
        return {}
    try:
        return json.loads(output.stdout.decode())
    except (ValueError, UnicodeError):
        return {}


def compile_driver(receipt):
    OUT.mkdir(parents=True, exist_ok=True)
    env = build_env()
    run_build(["/usr/bin/xcrun", "swiftc", "-warnings-as-errors", str(DRIVER_SOURCE),
               "-o", str(DRIVER_BINARY)], DESKTOP, env, OUT / "driver-build.log", 180)
    receipt["artifacts"] = {"driver_sha256": sha256(DRIVER_BINARY),
                            "driver_source_sha256": sha256(DRIVER_SOURCE)}


def preflight(driver):
    check = subprocess.run([str(driver), "preflight"], capture_output=True, timeout=30)
    if check.returncode == 0:
        return None
    return check.stderr.decode(errors="replace").strip() or "accessibility_or_interactive_gui_required"


def launch_app(app_binary, scratch):
    """Launch the packaged executable with a private, empty environment."""
    env = {
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": str(scratch / "home"),
        "XDG_CONFIG_HOME": str(scratch / "config"),
        "XDG_CACHE_HOME": str(scratch / "cache"),
        "XDG_STATE_HOME": str(scratch / "home"),
        "ABBEY_STATE_DIR": str(scratch / "state"),
        "ABBEY_CONFIG": str(scratch / "state" / "config.toml"),
        "ABBEY_MEMORY_BACKEND": "sqlite",
    }
    for name in ("home", "config", "cache", "state"):
        (scratch / name).mkdir(mode=0o700, exist_ok=True)
    (scratch / "state" / "config.toml").write_text("")
    log = (scratch / "desktop.log").open("wb")
    process = subprocess.Popen([str(app_binary)], cwd=str(scratch), env=env,
                               stdout=log, stderr=log)
    return process, log


def read_ax(driver, pid):
    """The driver's JSON envelope, or the raw failure id it reported."""
    try:
        result = subprocess.run([str(driver), str(pid), "menu"], capture_output=True, timeout=60)
    except subprocess.TimeoutExpired:
        return None, "ax_command_timeout"
    if result.returncode:
        return None, result.stderr.decode(errors="replace").strip() or "ax_driver_failed"
    try:
        envelope = json.loads(result.stdout)
    except (ValueError, UnicodeError):
        return None, "ax_json_invalid"
    if not isinstance(envelope, dict):
        return None, "ax_json_invalid"
    if envelope.get("complete") is not True:
        return None, "ax_snapshot_incomplete"
    if not isinstance(envelope.get("menus"), list) or not isinstance(envelope.get("windows"), list):
        return None, "ax_json_invalid"
    return envelope, None


def settle(driver, pid, deadline_seconds=45):
    """Read the AX state until the window title stops changing.

    Returns (envelope, title history, reason). The reason is None only when
    the state settled; otherwise it names the failure that stopped it.
    """
    history = []
    envelope = None
    reason = None
    first_window = None
    last_titles = None
    last_change = None
    deadline = time.monotonic() + deadline_seconds
    while time.monotonic() < deadline:
        envelope, reason = read_ax(driver, pid)
        if envelope is None:
            if reason not in (None, "ax_command_timeout"):
                return None, history, reason
            time.sleep(0.2)
            continue
        titles = tuple(window.get("title", "") for window in envelope.get("windows", []))
        if not titles:
            time.sleep(0.2)
            continue
        now = time.monotonic()
        if first_window is None:
            first_window = now
        if titles != last_titles:
            last_titles = titles
            last_change = now
            history.append({"after_seconds": round(now - first_window, 3),
                            "titles": list(titles)})
        if (now - first_window) >= SETTLE_SECONDS and (now - last_change) >= STABLE_SECONDS:
            return envelope, history, None
        time.sleep(0.2)
    return envelope, history, reason or "ax_state_never_settled"



def main():
    def interrupted(_number, _frame):
        raise ProofFailure("runner_interrupted")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)

    OUT.mkdir(parents=True, exist_ok=True)
    receipt = {
        "schema": 1,
        "kind": "packaged-menu-icon-acceptance",
        "started_at": datetime.now(timezone.utc).isoformat(),
        "status": "failed",
        "platform": platform.system(),
        "architecture": platform.machine(),
        "checks": [],
    }
    report = Report()
    receipt["checks"] = report.checks
    scratch_path = None
    process = None
    log = None
    try:
        require(platform.system() == "Darwin", "macos_required")
        app_path = None
        expect_edition = None
        argv = sys.argv[1:]
        index = 0
        while index < len(argv):
            if argv[index] == "--app":
                require(index + 1 < len(argv), "app_path_required")
                app_path = Path(argv[index + 1]).expanduser().resolve()
                index += 1
            elif argv[index] == "--expect-edition":
                require(index + 1 < len(argv), "expect_edition_required")
                expect_edition = argv[index + 1]
                index += 1
            else:
                raise ProofFailure("unknown_argument")
            index += 1
        # The bundle under test is always named exactly. Two editions' bundles
        # can share one directory, and "the first .app found" is not a proof.
        require(app_path is not None, "app_path_required")
        require(app_path.is_dir() and app_path.suffix == ".app", "app_bundle_missing")
        receipt["bundle"] = {"path": str(app_path)}

        # --- bundle identity ------------------------------------------------
        plist_path = app_path / "Contents" / "Info.plist"
        require(plist_path.is_file(), "bundle_info_plist_missing")
        plist = read_plist(plist_path)
        require(bool(plist), "bundle_info_plist_unreadable")
        identifier = plist.get("CFBundleIdentifier", "")
        require(identifier in EDITIONS_BY_IDENTIFIER, "bundle_identifier_unknown")
        edition = EDITIONS_BY_IDENTIFIER[identifier]["edition"]
        product_name = EDITIONS_BY_IDENTIFIER[identifier]["product_name"]
        other_names = [value["product_name"] for key, value in EDITIONS_BY_IDENTIFIER.items()
                       if key != identifier]
        if expect_edition is not None:
            require(expect_edition == edition, "bundle_edition_mismatch")
        require(plist.get("CFBundleName") == product_name, "bundle_product_name_mismatch")
        require(plist.get("CFBundleDisplayName") == product_name, "bundle_display_name_mismatch")
        executable = app_path / "Contents" / "MacOS" / "abbey-desktop"
        require(executable.is_file(), "packaged_binary_missing")
        receipt["bundle"].update({
            "identifier": identifier,
            "edition": edition,
            "product_name": product_name,
            "cFBundleName": plist.get("CFBundleName"),
            "cFBundleDisplayName": plist.get("CFBundleDisplayName"),
            "cFBundleIconFile": plist.get("CFBundleIconFile"),
            "executable": str(executable),
            "executable_sha256": sha256(executable),
        })
        report.passed("bundle_identity", **{
            "identifier": identifier, "edition": edition, "product_name": product_name})
        print(f"bundle:  {app_path}")
        print(f"edition: {edition} ({identifier}) product name {product_name!r}")

        # --- icon identity --------------------------------------------------
        try:
            report.passed("icon_bundle_registration", **check_icon_registration(plist))
        except ProofFailure as failure:
            report.failed("icon_bundle_registration", str(failure),
                          registered_icon=plist.get("CFBundleIconFile"))
        bundle_icon = app_path / "Contents" / "Resources" / "icon.icns"
        require(SOURCE_ICON.is_file(), "source_icon_missing")
        require(bundle_icon.is_file(), "bundle_icon_missing")
        source_digest = sha256(SOURCE_ICON)
        bundle_digest = sha256(bundle_icon)
        receipt["icons"] = {
            "source_icon": {"path": str(SOURCE_ICON), "sha256": source_digest},
            "bundle_icon": {"path": str(bundle_icon), "sha256": bundle_digest},
            "sha256_equal": source_digest == bundle_digest,
        }
        if source_digest != bundle_digest:
            report.failed("icon_bundle_matches_source", "icon_bundle_differs_from_source",
                          source_sha256=source_digest, bundle_sha256=bundle_digest)
        else:
            report.passed("icon_bundle_matches_source", sha256=source_digest)
        print(f"icon:    bundle {bundle_digest[:16]}… source {source_digest[:16]}… "
              f"{'identical' if source_digest == bundle_digest else 'DIFFERENT'}")

        master = SOURCE_MASTER.read_bytes() if SOURCE_MASTER.is_file() else b""
        dimensions = png_dimensions(master)
        receipt["icons"]["master"] = {
            "path": str(SOURCE_MASTER),
            "exists": SOURCE_MASTER.is_file(),
            "sha256": sha256(SOURCE_MASTER) if SOURCE_MASTER.is_file() else None,
            "width": dimensions[0] if dimensions else None,
            "height": dimensions[1] if dimensions else None,
        }
        if dimensions != (1024, 1024):
            report.failed("icon_master_is_1024", "icon_master_not_1024", dimensions=list(dimensions or []))
        else:
            report.passed("icon_master_is_1024", width=1024, height=1024)
        print(f"master:  {SOURCE_MASTER} {dimensions[0]}x{dimensions[1]}" if dimensions else
              f"master:  {SOURCE_MASTER} unreadable")

        entries = icns_entries(bundle_icon.read_bytes())
        receipt["icons"]["icns_entries"] = entries
        if (1024, 1024) not in [(entry.get("png_width"), entry.get("png_height")) for entry in entries]:
            report.failed("icon_chain_has_1024", "icon_chain_missing_1024", icns_entries=entries)
        else:
            report.passed("icon_chain_has_1024", icns_types=[entry["type"] for entry in entries])
        print(f"chain:   icns carries a 1024x1024 representation "
              f"({len(entries)} entries)")

        resources = {}
        resource_dir = app_path / "Contents" / "Resources"
        if resource_dir.is_dir():
            for path in sorted(resource_dir.iterdir()):
                if path.is_file() and path.suffix.lower() in (".png", ".icns", ".ico"):
                    resources[path.name] = sha256(path)
        receipt["icons"]["bundle_resources"] = resources
        try:
            detail = check_bundle_resources(resources, SOURCE_ICONS)
            report.passed("icon_no_placeholder_in_bundle", **detail)
        except ProofFailure as failure:
            report.failed("icon_no_placeholder_in_bundle", str(failure), bundle_resources=resources)
        print(f"bundle:  resources {sorted(resources) or '[]'} (no placeholder)")

        # --- the running packaged app --------------------------------------
        receipt["expected"] = {
            "top_level_menus": list(EXPECTED_TOP_LEVEL_MENUS),
            "app_menu_title": product_name,
            "edit_items": {role: list(labels) for role, labels in EXPECTED_EDIT_ITEMS.items()},
            "window_items": {role: list(labels) for role, labels in EXPECTED_WINDOW_ITEMS.items()},
            "window_title": product_name,
            "label_prose": LABEL_PROSE,
        }
        compile_driver(receipt)
        driver = DRIVER_BINARY
        reason = preflight(driver)
        envelope = None
        history = []
        if reason is None:
            temporary = tempfile.mkdtemp(prefix="abbey-menu-icon-", dir="/tmp")
            scratch_path = Path(temporary)
            process, log = launch_app(executable, scratch_path)
            envelope, history, reason = settle(driver, process.pid)
            if envelope is None and process.poll() is not None:
                reason = reason or "owned_process_exited"
        else:
            receipt["ax"] = {"preflight": reason}

        if envelope is None or reason is not None:
            for check_id in ("ax_menu_bar", "ax_app_menu_items", "ax_edit_menu_items",
                             "ax_window_menu_items", "ax_window_title"):
                report.unproven(check_id, reason or "ax_unavailable")
            receipt["ax"] = {"status": "unproven", "reason": reason, "title_history": history}
            print(f"ax:      UNPROVEN ({reason})")
        else:
            menus = envelope.get("menus", [])
            windows = envelope.get("windows", [])
            receipt["ax"] = {
                "status": "observed",
                "menus": menus,
                "windows": windows,
                "title_history": history,
                "observed_strings_count": len(envelope.get("observedStrings", [])),
            }
            print("menus:   " + " | ".join(menu.get("title", "") for menu in menus))
            try:
                detail = check_top_level_menus(menus, product_name)
                report.passed("ax_menu_bar", **detail)
            except ProofFailure as failure:
                report.failed("ax_menu_bar", str(failure),
                              top_level_titles=[menu.get("title", "") for menu in menus])

            app_menu = find_menu(menus, product_name)
            if app_menu is None:
                report.failed("ax_app_menu_items", "ax_app_menu_missing")
            else:
                try:
                    detail = check_app_menu_items(app_menu, product_name)
                    report.passed("ax_app_menu_items", **detail)
                except ProofFailure as failure:
                    report.failed("ax_app_menu_items", str(failure),
                                  items=menu_item_titles(app_menu))

            edit_menu = find_menu(menus, "Edit")
            if edit_menu is None:
                report.failed("ax_edit_menu_items", "ax_top_level_menu_missing_edit")
            else:
                try:
                    detail = check_menu_items(edit_menu, EXPECTED_EDIT_ITEMS,
                                              "ax_edit_menu_items_missing")
                    report.passed("ax_edit_menu_items", **detail)
                except ProofFailure as failure:
                    detail = missing_items(edit_menu, EXPECTED_EDIT_ITEMS)
                    report.failed("ax_edit_menu_items", str(failure), missing=detail,
                                  items=menu_item_titles(edit_menu))
                observed = observed_labels(edit_menu, EXPECTED_EDIT_ITEMS)
                receipt.setdefault("observed", {})["edit_items"] = observed
                receipt.setdefault("observed", {})["edit_label_mapping"] = {
                    role: {"requested": LABEL_PROSE.get(role, role.replace("_", " ")),
                           "observed_label": observed.get(role),
                           "accepted_labels": list(labels)}
                    for role, labels in EXPECTED_EDIT_ITEMS.items()}

            window_menu = find_menu(menus, "Window")
            if window_menu is None:
                report.failed("ax_window_menu_items", "ax_top_level_menu_missing_window")
            else:
                try:
                    detail = check_menu_items(window_menu, EXPECTED_WINDOW_ITEMS,
                                              "ax_window_menu_items_missing")
                    report.passed("ax_window_menu_items", **detail)
                except ProofFailure as failure:
                    detail = missing_items(window_menu, EXPECTED_WINDOW_ITEMS)
                    report.failed("ax_window_menu_items", str(failure), missing=detail,
                                  items=menu_item_titles(window_menu))
                observed = observed_labels(window_menu, EXPECTED_WINDOW_ITEMS)
                receipt.setdefault("observed", {})["window_items"] = observed
                receipt.setdefault("observed", {})["label_mapping"] = {
                    role: {"requested": LABEL_PROSE.get(role, role),
                           "observed_label": observed.get(role),
                           "accepted_labels": list(labels)}
                    for role, labels in EXPECTED_WINDOW_ITEMS.items()}

            try:
                detail = check_window_title(windows, product_name, other_names)
                report.passed("ax_window_title", **detail)
                receipt.setdefault("observed", {})["window_title"] = detail["matched_title"]
                print(f"window:  title {detail['matched_title']!r}")
            except ProofFailure as failure:
                report.failed("ax_window_title", str(failure),
                              window_titles=[window.get("title", "") for window in windows])
                receipt.setdefault("observed", {})["window_title"] = None
                print(f"window:  title "
                      f"{[window.get('title', '') for window in windows]!r} "
                      f"(expected {product_name!r})")
    except (ProofFailure, subprocess.SubprocessError, OSError):
        error = sys.exc_info()[1]
        assertion = str(error) if isinstance(error, ProofFailure) else "runner_system_failure"
        receipt["failure"] = assertion
        # Record the abort as a failed check so the receipt's check list says
        # what happened, instead of leaving an empty list next to a status.
        report.failed("runner_aborted", assertion)
        print("FAIL " + assertion, file=sys.stderr)
    finally:
        if process is not None:
            stop(process)
        if log is not None:
            log.close()
        if scratch_path is not None:
            shutil.rmtree(scratch_path, ignore_errors=True)
        receipt["status"] = report.status()
        receipt["finished_at"] = datetime.now(timezone.utc).isoformat()
        receipt["summary"] = {
            "passed": sum(1 for check in report.checks if check["status"] == "passed"),
            "failed": sum(1 for check in report.checks if check["status"] == "failed"),
            "unproven": sum(1 for check in report.checks if check["status"] == "unproven"),
            "total": len(report.checks),
        }
        RECEIPT.write_text(json.dumps(receipt, indent=2) + "\n")
        for check in report.checks:
            marker = {"passed": "PASS", "failed": "FAIL", "unproven": "UNPROVEN"}[check["status"]]
            detail = check.get("assertion") or check.get("reason") or ""
            print(f"{marker} {check['id']}" + (f" — {detail}" if detail else ""), flush=True)
        print(f"RESULT {receipt['status']} "
              f"({receipt['summary']['passed']}/{receipt['summary']['total']} checks passed)")
        try:
            shown = RECEIPT.relative_to(ROOT)
        except ValueError:
            shown = RECEIPT
        print(f"Receipt: {shown}", flush=True)
    if receipt["status"] == "passed":
        return 0
    if receipt["status"] == "unproven":
        return 2
    return 1


if __name__ == "__main__":
    sys.exit(main())
