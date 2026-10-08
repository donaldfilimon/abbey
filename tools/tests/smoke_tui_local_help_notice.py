#!/usr/bin/env python3
"""Actual CLI help controls and selected-ABI TUI help retained after redraw."""
import argparse
import json
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
import smoke_tui_slash_pty as harness

CASES = (
    ("subagents-empty", "/subagents run", b"not a multi-node mesh"),
    ("subagents-help", "/subagents run --help", b"not a multi-node mesh"),
    ("distill-help", "/learn distill --help", b"default chain is local first"),
    ("improve-help", "/improve run --help", b"ABBEY_CHECK_CMD overrides ./check.sh"),
)


def retained(binary, root, command, marker):
    owned = harness.Fixture(root, backend="abi")
    # CLI grammar/reporting is a positive control before the actual TUI entry.
    owned.set_control(marker="must-not-generate-cli-help")
    output, _ = owned.cli(binary, [command])
    harness.require(marker in output, "control: actual CLI help marker missing")
    harness.require(not owned.events(owned.nonce),
                    "control: CLI help/empty command contacted generation")
    session = harness.OwnedSession(binary, owned)
    try:
        owned.set_control(marker="must-not-generate-tui-help")
        prior_chat = (root / "state" / "chat-id").read_bytes()
        session.submit(command + " ")
        session.wait_join(0)
        session.redraw()
        harness.require(marker in session.screen(),
                        "expectation: actual help/empty output was lost after terminal redraw; screen=" + repr(session.screen()[-3000:]))
        harness.require(not owned.events(owned.nonce),
                        "expectation: selected ABI help/empty command contacted generation")
        harness.require((root / "state" / "chat-id").read_bytes() == prior_chat,
                        "expectation: metadata-only help changed the chat identity")
        session.quit_observed()
    finally:
        owned.cleanup(session)


def main():
    harness.OPTIONS = OPTIONS
    proofs, failures = [], []
    for name, command, marker in CASES:
        try:
            with tempfile.TemporaryDirectory(prefix="abbey-local-help-", dir="/tmp") as scratch:
                retained(Path(OPTIONS.binary).resolve(), Path(scratch), command, marker)
            proofs.append(dict(scenario=name, status="passed"))
        except Exception as error:
            failures.append(dict(scenario=name, status="failed", error=str(error)))
    print(json.dumps(dict(status="failed" if failures else "passed",
                          scenarios=proofs, failures=failures), sort_keys=True))
    return 1 if failures else 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("--state-env", required=True)
    parser.add_argument("--config-env", required=True)
    OPTIONS = parser.parse_args()
    sys.exit(main())
