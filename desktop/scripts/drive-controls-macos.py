#!/usr/bin/env python3
"""Drive the desktop window's *controls* with macOS Accessibility.

The surface sweep opens each page and reads what it says. This presses the
buttons inside a page and reads what *changed* — because a control that
renders but does nothing is the failure a static snapshot cannot find.

Every observation is read-only with respect to Abbey: the controls pressed
here are the window's own presentation controls (appearance, material,
density, sidebar, palette) and the draft button. Nothing here submits a
turn, downloads a model, or grants a capability.
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
AX_DRIVER = DESKTOP / "scripts" / "routes-ax.swift"

# Each entry is (action, what must be true afterwards). The check is on the
# accessibility *state* — a pressed button, a collapsed sidebar, an open
# palette — because that is the observable effect of the control.
CHECKS = [
    # The appearance segmented control: pressing "Light" must mark Light as
    # pressed and leave the others unpressed.
    ("appearance-light", "pressed:Light"),
    ("appearance-dark", "pressed:Dark"),
    ("appearance-system", "pressed:System"),
    # The sidebar collapse and expand.
    ("collapse-sidebar", "title:Expand sidebar"),
    ("expand-sidebar", "title:Collapse sidebar"),
    # The command palette opens over the current surface.
    ("palette", "title:Search pages"),
]


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


def build_driver():
    binary = Path(tempfile.mkdtemp(prefix="abbey-ax-")) / "ax"
    result = subprocess.run(
        ["swiftc", "-O", "-o", str(binary), str(AX_DRIVER)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit("the Accessibility driver did not compile")
    return binary


def state(payload):
    """The pressed buttons and the control titles the window reports."""
    pressed = []
    titles = []
    for snapshot in payload.get("snapshots", []):
        for node in snapshot.get("nodes", []):
            attributes = node.get("attributes") or {}
            value = attributes.get("AXValue", []) or []
            title = attributes.get("AXTitle", []) or []
            if node.get("role") == "AXButton":
                for t in title:
                    titles.append(t)
                # A pressed button reports "1" (or true) as its value.
                if any(v in ("1", "true", "YES", "1.0") for v in value):
                    pressed.extend(t for t in title)
    return pressed, titles


def run_action(driver, pid, action):
    result = subprocess.run(
        [str(driver), str(pid), action], capture_output=True, text=True, timeout=60
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

    app_bundle = env_path("ABBEY_DESKTOP_APP")
    if app_bundle is None or not app_bundle.is_dir():
        return refuse(required, "no_app_bundle")
    if required:
        refusal = edition_refusal(app_bundle)
        if refusal is not None:
            return refuse(required, **refusal)

    driver = build_driver()
    with tempfile.TemporaryDirectory(prefix="abbey-controls-") as scratch:
        root = Path(scratch)
        env = {k: os.environ[k] for k in ("HOME", "USER", "TMPDIR") if k in os.environ}
        env.update(PATH=f"{Path.home()}/.cargo/bin:/usr/bin:/bin:/opt/homebrew/bin",
                   HOME=str(root / "home"))
        (root / "home").mkdir(parents=True, exist_ok=True)
        macos = app_bundle / "Contents" / "MacOS"
        executable = sorted(e for e in macos.iterdir() if e.is_file())[0]

        log = (root / "app.log").open("wb")
        process = subprocess.Popen(
            [str(executable)], cwd=root, env=env,
            stdout=log, stderr=log, start_new_session=True,
        )
        observations = []
        failures = []
        try:
            time.sleep(3.0)
            for action, expectation in CHECKS:
                payload, error = run_action(driver, process.pid, action)
                if payload is None:
                    failures.append({"action": action, "reason": error})
                    continue
                pressed, titles = state(payload)
                kind, _, want = expectation.partition(":")
                seen = pressed if kind == "pressed" else titles
                ok = want in seen
                observations.append(
                    {"action": action, "ok": ok, "expectation": expectation}
                )
                if not ok:
                    failures.append(
                        {
                            "action": action,
                            "reason": "state_not_observed",
                            "expected": expectation,
                            "pressed": pressed[:12],
                            "titles": titles[:20],
                        }
                    )
        finally:
            try:
                process.terminate()
                process.wait(timeout=5)
            except Exception:
                process.kill()
            log.close()

    print(
        json.dumps(
            {
                "status": "failed" if failures else "passed",
                "controls": observations,
                "failures": failures,
            },
            sort_keys=True,
        )
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
