#!/usr/bin/env python3
"""Actual provider/model changes -> FM argv; private fixtures, no model service."""

import argparse
import json
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
import smoke_tui_slash_pty as harness


def scenario(binary, root):
    fixture = harness.Fixture(root, backend="abi", teacher=True)
    fixture.env["ABBEY_MODEL"] = "pcc"
    session = harness.OwnedSession(binary, fixture)
    try:
        session.send(b"\x13")  # Ctrl-S: explicit provider selection.
        session.wait_output(b"Provider", timeout=harness.WAIT)
        # The active ABI provider is fourth; FM is immediately above it.
        session.send(b"\x1b[A")
        session.send(b"\r")
        session.wait_output(b"provider", timeout=harness.WAIT)
        session.wait_output(b"Apple Foundation Models", timeout=harness.WAIT)
        fixture.set_control(marker="unrelated-help-must-not-generate")
        session.submit("/help ")
        session.wait_join(0)
        harness.require(not fixture.events(), "local help unexpectedly generated")

        # A new prompt is the real shared worker/argv path, not an App mutation
        # injected by the test. Ambient PCC must not replace the live system
        # selection after the local command completes.
        fixture.set_control("teacher", marker="provider-system-after-help")
        session.submit("hello world")
        try:
            call = fixture.wait_call(session, "teacher")
        except AssertionError as error:
            raise AssertionError(f"{error}; screen={session.screen()!r}; calls={fixture.events()!r}") from error
        args = call["argv"]
        harness.require(call["backend"] == "fm" and args[0] == "respond",
                        "selected FM was not the actual responding executor")
        harness.require(args.count("--model") == 1 and
                        args[args.index("--model") + 1] == "system" and
                        "pcc" not in args,
                        f"ambient PCC replaced explicit system selection: {args!r}")
        session.wait_join(0)
        session.redraw()
        harness.require(fixture.marker.encode() in session.screen(),
                        "FM response did not survive actual redraw")
        harness.require(len(fixture.events()) == 1, "unexpected additional provider call")
        fixture.assert_closed(call, session)
        session.quit_observed()
        return dict(status="passed", model="system", calls=1)
    finally:
        fixture.cleanup(session)


def startup_override(binary, root):
    fixture = harness.Fixture(root, backend="fm", teacher=True)
    fixture.env["ABBEY_MODEL"] = "pcc"
    session = harness.OwnedSession(binary, fixture, ["--model", "system", "tui"])
    try:
        fixture.set_control("teacher", marker="explicit-startup-system")
        session.submit("hello world")
        call = fixture.wait_call(session, "teacher")
        args = call["argv"]
        harness.require(call["backend"] == "fm" and args[0] == "respond" and
                        args.count("--model") == 1 and
                        args[args.index("--model") + 1] == "system" and
                        "pcc" not in args,
                        f"ambient PCC replaced explicit startup model: {args!r}")
        session.wait_join(0)
        fixture.assert_closed(call, session)
        session.quit_observed()
    finally:
        fixture.cleanup(session)


def require_fm_model(call, model):
    args = call["argv"]
    harness.require(call["backend"] == "fm" and args[0] == "respond" and
                    args.count("--model") == 1 and
                    args[args.index("--model") + 1] == model,
                    f"expected actual FM respond with {model}: {args!r}")


def queued_model(binary, root, fail=False):
    fixture = harness.Fixture(root, backend="fm", teacher=True)
    fixture.env["ABBEY_MODEL"] = "pcc"
    fixture.set_control("teacher", hold=True, marker="held-system-answer")
    session = harness.OwnedSession(binary, fixture, ["--model", "system", "tui"])
    model_file = root / "state" / "model"
    try:
        session.submit("hello world")
        first = fixture.wait_call(session, "teacher")
        require_fm_model(first, "system")
        harness.require(harness.exists(first["pid"]), "first FM call was not held")
        session.submit("/model pcc ")
        session.wait_output(b"queued (1)", timeout=harness.WAIT)
        session.submit("hello world")
        session.wait_output(b"queued (2)", timeout=harness.WAIT)
        harness.require(len(fixture.events()) == 1,
                        "queued work reached FM before the first response completed")
        harness.require(not model_file.exists(),
                        "explicit startup unexpectedly persisted a model")
        if fail:
            # This private destination cannot be replaced by model persistence.
            # No permission assumptions (or mutation of existing files) needed.
            model_file.mkdir()
        fixture.set_control("teacher", marker="queued-pcc-answer")
        fixture.release()
        if fail:
            session.wait_join(1)
            session.wait_output(b"discarded 1 queued prompt", timeout=harness.WAIT)
            session.redraw()
            harness.require(model_file.is_dir(), "failed mutation replaced its destination")
            harness.require(len(fixture.events()) == 1,
                            "failed model mutation dispatched the following queued prompt")
            fixture.assert_closed(first, session)
            session.quit_observed()
            harness.require(len(fixture.events()) == 1,
                            "discarded prompt reached FM during teardown")
            return 1

        second = fixture.wait_call(session, "teacher")
        require_fm_model(second, "pcc")
        session.wait_join(0)
        harness.require(model_file.read_text().strip() == "pcc",
                        "queued model selection did not persist successfully")
        harness.require(len(fixture.events()) == 2,
                        "queued prompt did not dispatch exactly once")
        fixture.assert_closed(first, session)
        fixture.assert_closed(second, session)

        # Clear a genuinely selected PCC model, not an already-default model.
        # Ambient PCC must not be re-imported after this successful mutation.
        session.submit("/model clear ")
        session.wait_join(0)
        session.redraw()
        harness.require(not model_file.exists(), "model clear did not remove selection")
        harness.require(len(fixture.events()) == 2, "model clear generated a response")
        fixture.set_control("teacher", marker="cleared-system-answer")
        session.submit("hello world")
        third = fixture.wait_call(session, "teacher")
        require_fm_model(third, "system")
        session.wait_join(0)
        session.redraw()
        harness.require(fixture.marker.encode() in session.screen(),
                        "cleared-model response did not survive actual redraw")
        fixture.assert_closed(third, session)
        session.quit_observed()
        harness.require(len(fixture.events()) == 3,
                        "unexpected additional provider call after model clear")
        return 3
    finally:
        fixture.cleanup(session)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary")
    parser.add_argument("--state-env", default="ABBEY_STATE_DIR")
    parser.add_argument("--config-env", default="ABBEY_CONFIG")
    options = parser.parse_args()
    harness.OPTIONS = options
    try:
        with tempfile.TemporaryDirectory(prefix="abbey-provider-pty-") as scratch:
            binary = Path(options.binary).resolve()
            root = Path(scratch)
            scenario(binary, root / "switch")
            startup_override(binary, root / "startup")
            queued_calls = queued_model(binary, root / "queued")
            failed_calls = queued_model(binary, root / "failed", fail=True)
            proof = dict(status="passed", model="system", calls=2 + queued_calls + failed_calls,
                         scenarios=["provider-switch-local-help", "explicit-startup-model",
                                    "queued-model-selection", "model-clear-ambient-pcc",
                                    "failed-queued-model-discards-prompt"],
                         scenario_calls=[1, 1, 2, 1, failed_calls])
    except Exception as error:
        proof = dict(status="failed", error=str(error))
    print(json.dumps(proof, sort_keys=True))
    return 0 if proof["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
