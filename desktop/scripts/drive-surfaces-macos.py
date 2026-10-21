#!/usr/bin/env python3
"""Drive every surface of the real desktop window with macOS Accessibility.

The unit tests render components; this drives the *app* — a real Tauri
window, a real webview, a real Accessibility tree — and reports what each
surface actually put on screen. A surface that renders but says the wrong
thing, or a navigation entry that goes nowhere, is what this finds and the
unit tests cannot.

Every observation is read-only: the driver presses a navigation button and
reads the tree. It never submits, downloads, loads, or approves anything.
"""
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time

DESKTOP = Path(__file__).resolve().parents[1]
ROOT = DESKTOP.parent
AX_DRIVER = DESKTOP / "scripts" / "routes-ax.swift"

# The surface's navigation label, and what that surface must say when it
# is opened. Each expectation is a string that must appear in the
# accessibility tree of the window after the navigation button is pressed.
# Each entry is (driver action, navigation label, what the surface must
# say). The expectations are the surface's *own* words, so a surface that
# renders but says the wrong thing fails here.
SURFACES = [
    ("doctor", "Settings", ["MAKE YOURSELF AT HOME", "Appearance", "Connection"]),
    ("routes", "Routes", ["routing decisions", "app_routes"]),
    ("claims", "Capabilities", ["canonical capability ledger", "app_claims"]),
    # These three need a capability the connected runtime has not granted —
    # this window runs without a configured daemon — so the surface must say
    # so rather than render an empty view. That refusal *is* the contract.
    ("runs", "Tasks", ["needs a capability your connected runtime hasn"]),
    ("tools", "Tools", ["app_tools_list"]),
    ("memory", "Memory", ["needs a capability your connected runtime hasn"]),
    ("models", "Models", ["needs a capability your connected runtime hasn"]),
    # The surfaces with nothing behind them. Each must *say* so rather
    # than show an empty page.
    ("training", "Training", ["not implemented anywhere in Abbey yet"]),
    ("cluster", "Cluster", ["does not expose this contract yet"]),
    ("automations", "Automations", ["does not expose this contract yet"]),
]


def build_driver():
    """Compile the Accessibility driver."""
    binary = Path(tempfile.mkdtemp(prefix="abbey-ax-")) / "ax"
    result = subprocess.run(
        ["swiftc", "-O", "-o", str(binary), str(AX_DRIVER)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit("the Accessibility driver did not compile")
    return binary


def observable_strings(payload):
    """Every string the driver observed, flattened.

    The driver reports the accessibility tree as nodes whose attributes map
    to lists of strings, plus the flat `observedStrings` it collected on the
    way. Both are read: a surface's *label* is an attribute, and its
    *content* is the observed text.
    """
    out = []
    for snapshot in payload.get("snapshots", []):
        for node in snapshot.get("nodes", []):
            for values in (node.get("attributes") or {}).values():
                if isinstance(values, list):
                    out.extend(value for value in values if isinstance(value, str))
    for value in payload.get("observedStrings", []) or []:
        if isinstance(value, str):
            out.append(value)
    return out


def visible_text(payload):
    """The text a person would read on the surface.

    The webview's content is exposed as `AXValue` on static-text and
    heading nodes; `AXTitle` carries the *control* names (a button's
    label, a menu item's title). A surface's own words are its values, and
    a surface's controls are its titles — both are worth reading, and a
    navigation button is what the driver presses.
    """
    values = []
    titles = []
    for snapshot in payload.get("snapshots", []):
        for node in snapshot.get("nodes", []):
            attributes = node.get("attributes") or {}
            for value in attributes.get("AXValue", []) or []:
                if isinstance(value, str) and value.strip():
                    values.append(value)
            for title in attributes.get("AXTitle", []) or []:
                if isinstance(title, str) and title.strip():
                    titles.append(title)
    return " ".join(values + titles)


def run_surface(driver, pid, surface):
    result = subprocess.run(
        [str(driver), str(pid), surface],
        capture_output=True,
        text=True,
        timeout=60,
    )
    if result.returncode != 0:
        return None, (result.stderr or "").strip() or "driver_failed"
    try:
        return json.loads(result.stdout), None
    except json.JSONDecodeError:
        return None, "driver_output_not_json"


def main():
    if platform.system() != "Darwin":
        print(json.dumps({"status": "skipped", "reason": "not_macos"}))
        return 0

    app_bundle = Path(
        os.environ.get("ABBEY_DESKTOP_APP", "")
    ).expanduser()
    binary = Path(
        os.environ.get("ABBEY_DESKTOP_BINARY", "")
    ).expanduser()
    if not app_bundle.exists() and not binary.exists():
        print(json.dumps({"status": "skipped", "reason": "no_app_bundle"}))
        return 0

    driver = build_driver()
    with tempfile.TemporaryDirectory(prefix="abbey-drive-") as scratch:
        root = Path(scratch)
        env = {k: os.environ[k] for k in ("HOME", "USER", "TMPDIR") if k in os.environ}
        env.update(
            PATH=f"{Path.home()}/.cargo/bin:/usr/bin:/bin:/opt/homebrew/bin",
            HOME=str(root / "home"),
        )
        (root / "home").mkdir(parents=True, exist_ok=True)
        if app_bundle.exists():
            # The executable inside the bundle carries the crate's binary
            # name, not the product name.
            macos = app_bundle / "Contents" / "MacOS"
            executables = sorted(entry for entry in macos.iterdir() if entry.is_file())
            if not executables:
                raise SystemExit("the app bundle has no executable")
            command = [str(executables[0])]
        else:
            command = [str(binary)]

        log_path = root / "app.log"
        log = log_path.open("wb")
        process = subprocess.Popen(
            command,
            cwd=root,
            env=env,
            stdout=log,
            stderr=log,
            start_new_session=True,
        )
        observations = []
        failures = []
        try:
            time.sleep(2.5)
            for surface, label, expected in SURFACES:
                payload, error = run_surface(driver, process.pid, surface)
                if payload is None:
                    failures.append(
                        {"surface": surface, "reason": error}
                    )
                    continue
                seen = visible_text(payload)
                missing = [want for want in expected if want.lower() not in seen.lower()]
                observations.append(
                    {
                        "surface": surface,
                        "label": label,
                        "ok": not missing,
                        "missing": missing,
                    }
                )
                if missing:
                    failures.append(
                        {
                            "surface": surface,
                            "reason": "missing_text",
                            "missing": missing,
                            "seen": seen[:400],
                        }
                    )
        finally:
            try:
                process.terminate()
                process.wait(timeout=5)
            except Exception:
                process.kill()
            log.close()
            app_log = log_path.read_text(errors="replace")[-400:]

    print(
        json.dumps(
            {
                "status": "failed" if failures else "passed",
                "surfaces": observations,
                "failures": failures,
                "app_log": app_log,
            },
            sort_keys=True,
        )
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
