#!/usr/bin/env python3
"""Seed and verify a deterministic synthetic v4 conversation turn."""
import json
import os
from pathlib import Path
import socket
import stat
import struct
import sys
import time
import uuid


def seed():
    if os.environ.get("ABBEY_DESKTOP_CONVERSATION_FIXTURE") != "1":
        raise RuntimeError("explicit scratch-fixture marker required")
    path = Path(os.environ["ABBEYD_SOCKET_PATH"]).resolve()
    parent = path.parent
    if parent.parent != Path("/tmp").resolve() or not parent.name.startswith("abbey-desktop-live."):
        raise RuntimeError("fixture must use the owner-only scratch harness")
    if stat.S_IMODE(parent.stat().st_mode) != 0o700 or stat.S_IMODE(path.stat().st_mode) != 0o600:
        raise RuntimeError("fixture socket or parent permissions are invalid")
    bearer = os.environ["ABBEYD_BEARER_TOKEN"]
    grants = {"capabilities": []}

    def request(kind, content):
        request_id = str(uuid.uuid4())
        command = {"command": kind}
        if content is not None:
            command["data"] = content
        raw = json.dumps({"version": 4, "schema_version": 1, "request_id": request_id,
                          "bearer": bearer, "grants": grants,
                          "command": command}).encode()
        with socket.socket(socket.AF_UNIX) as peer:
            peer.settimeout(3)
            peer.connect(str(path))
            peer.sendall(struct.pack(">I", len(raw)) + raw)

            def exact(count):
                result = b""
                while len(result) < count:
                    chunk = peer.recv(count - len(result))
                    if not chunk:
                        raise RuntimeError("scratch fixture response interrupted")
                    result += chunk
                return result

            length = struct.unpack(">I", exact(4))[0]
            if length > 1024 * 1024:
                raise RuntimeError("scratch fixture frame too large")
            response = json.loads(exact(length))
        if response.get("request_id") != request_id or response.get("version") != 4:
            print("fixture envelope diagnostics:", {key: value for key, value in json.loads(raw).items() if key != "bearer"}, file=sys.stderr)
            print("fixture response diagnostics:", response, file=sys.stderr)
            raise RuntimeError("scratch fixture response correlation failed")
        payload = response["payload"]
        if payload.get("outcome") != "ok":
            # error payload has outcome error
            raise RuntimeError("scratch fixture command rejected: " + str(payload))
        ev = payload.get("event", {})
        if isinstance(ev, dict) and ev.get("event") == "error":
            raise RuntimeError("scratch fixture command rejected")
        # return the inner data for callers expecting content-like
        if isinstance(ev, dict):
            return ev.get("data", ev)
        return ev

    requested = {"capabilities": ["read_conversations", "write_conversations", "submit_turns", "read_routes"]}
    event = request("negotiate", {"supported_versions": [4], "requested": requested})
    grants = event.get("granted", event.get("content", event))
    if grants != requested:
        raise RuntimeError("scratch fixture grants unavailable")
    conversation = request("create_conversation", "Seeded desktop history")
    routes = request("list_routes", None)
    route = next((row for row in routes if row.get("available") is True), None)
    if route is None:
        raise RuntimeError("scratch fixture requires one startup-bound available route")
    # This fixture can use only the fake provider created by the scratch harness.
    submission = request("submit_turn", {"conversation_id": conversation["conversation_id"],
            "idempotency_key": str(uuid.uuid4()), "route_id": route["route_id"],
            "input": "Authorized desktop history fixture"})
    deadline = time.monotonic() + 5
    messages = []
    while time.monotonic() < deadline:
        event = request("list_messages", {"conversation_id": conversation["conversation_id"], "after": None, "limit": 16})
        messages = event.get("messages", []) if isinstance(event, dict) else []
        user_saved = any(m.get("text") == "Authorized desktop history fixture" and m.get("role") == "user" for m in messages)
        assistant_saved = any(m.get("text") == "desktop-live-provider-output\n" and m.get("role") == "assistant" and m.get("text_complete") is True for m in messages)
        if user_saved and assistant_saved:
            break
        time.sleep(0.05)
    if not user_saved or not assistant_saved:
        raise RuntimeError("scratch canonical turn did not persist both messages")
    print("seed-conversation-fixture: deterministic v4 submit produced a complete assistant reply")


if __name__ == "__main__":
    seed()
