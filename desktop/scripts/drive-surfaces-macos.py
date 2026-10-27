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


def env_path(name):
    """The path an environment variable names, or None when it is unset.

    An empty variable is unset: `Path("")` is the current directory, which
    always exists and must never pass for a bundle. The path is made
    absolute against the caller's directory, because the app is launched
    with its cwd set to a fresh temp directory, where a relative path
    would no longer name the bundle.
    """
    value = os.environ.get(name, "")
    return Path(value).expanduser().resolve() if value else None


def refuse(required, reason, **detail):
    """Report a run that cannot proceed: a failure when the gate requires
    this driver, the historical skip otherwise."""
    report = {"status": "failed" if required else "skipped", "reason": reason}
    report.update(detail)
    print(json.dumps(report, sort_keys=True))
    return 1 if required else 0


def edition_refusal(app_bundle):
    """Why a required run must not drive this bundle, or None.

    The bundle's own identifier decides the edition, as in the packaged
    prover; the path and the `.app` name never do.
    """
    import prove_packaged_macos as packaged

    expected = os.environ.get("ABBEY_DESKTOP_EXPECT_EDITION", "")
    if expected not in set(packaged.EDITIONS_BY_IDENTIFIER.values()):
        return {"reason": "unknown_expected_edition", "expected": expected}
    identifier = packaged.bundle_identifier(app_bundle)
    found = packaged.EDITIONS_BY_IDENTIFIER.get(identifier)
    if found != expected:
        return {
            "reason": "edition_mismatch",
            "expected": expected,
            "found": found,
            "identifier": identifier,
        }
    return None


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
    # `desktop/check.sh` sets this when the driver runs as a gate step: then
    # every reason not to drive the app is a failure, never a skip.
    required = os.environ.get("ABBEY_DESKTOP_DRIVER_REQUIRED") == "1"
    if platform.system() != "Darwin":
        return refuse(required, "not_macos")

    found_bundle = env_path("ABBEY_DESKTOP_APP")
    app_bundle = found_bundle if found_bundle and found_bundle.is_dir() else None
    found_binary = env_path("ABBEY_DESKTOP_BINARY")
    binary = found_binary if found_binary and found_binary.is_file() else None
    if required:
        # A bare binary has no Info.plist, so its edition cannot be proved:
        # a required run drives a bundle or nothing.
        if app_bundle is None:
            return refuse(required, "no_app_bundle")
        refusal = edition_refusal(app_bundle)
        if refusal is not None:
            return refuse(required, **refusal)
    elif app_bundle is None and binary is None:
        return refuse(required, "no_app_bundle")

    driver = build_driver()
    with tempfile.TemporaryDirectory(prefix="abbey-drive-") as scratch:
        root = Path(scratch)
        env = {k: os.environ[k] for k in ("HOME", "USER", "TMPDIR") if k in os.environ}
        env.update(
            PATH=f"{Path.home()}/.cargo/bin:/usr/bin:/bin:/opt/homebrew/bin",
            HOME=str(root / "home"),
        )
        (root / "home").mkdir(parents=True, exist_ok=True)
        if app_bundle is not None:
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
