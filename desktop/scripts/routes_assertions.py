"""Pure assertions over native AX snapshots; never include observed data in errors."""
import hashlib
import re

WORKSPACES = ["/Users/routes-smoke/SECRET_WORKSPACE_A", "C:\\Users\\routes-smoke\\SECRET_WORKSPACE_B", "/private/routes-smoke/SECRET_WORKSPACE_C"]
FORBIDDEN = WORKSPACES + ["SECRET_WORKSPACE_", "SECRET_UNIX_PATH", "SECRET_WINDOWS_PATH", "SECRET_HOME_PATH", "LOCAL_FALLBACK_CANARY"]
MARKER = re.compile(r"ROUTE_ROW_\d{3}")


class ProofFailure(Exception):
    pass


def require(condition, assertion):
    if not condition:
        raise ProofFailure(assertion)


def values(snapshot):
    require(snapshot.get("complete") is True, "ax_traversal_incomplete")
    nodes = snapshot.get("nodes")
    require(isinstance(nodes, list) and len(nodes) > 0, "ax_tree_empty")
    found = []
    for node in nodes:
        require(isinstance(node.get("attributes"), dict), "ax_attributes_invalid")
        for strings in node["attributes"].values():
            require(isinstance(strings, list) and all(isinstance(s, str) for s in strings), "ax_strings_invalid")
            found.extend(strings)
    return found


def inspect(snapshot, secrets):
    text = "\n".join(values(snapshot))
    for forbidden in FORBIDDEN + list(secrets):
        require(bool(forbidden) and forbidden not in text, "rendered_sensitive_data")
    return text


def leaf_texts(snapshot):
    result = []
    # Use leaf text once, not container descriptions that repeat descendant text.
    for node in snapshot["nodes"]:
        if node["role"] != "AXStaticText":
            continue
        attrs = node["attributes"]
        text = next((" ".join(attrs[key]) for key in ("AXValue", "AXTitle", "AXDescription") if attrs.get(key)), "")
        result.append(text)
    return result


def markers(snapshot):
    return MARKER.findall("\n".join(leaf_texts(snapshot)))

def populated(snapshot, limit, secrets):
    text = inspect(snapshot, secrets)
    require(markers(snapshot) == [f"ROUTE_ROW_{i:03}" for i in reversed(range(55 - limit, 55))], "route_rows_or_order")
    require(f"{limit} decision(s) across 3 workspace(s)" in text, "route_summary")
    leaves = "\n".join(leaf_texts(snapshot))
    require(leaves.count("82%") == limit and leaves.count("[path]") == limit * 3, "route_projection")
    expected = {"ws-" + hashlib.sha256(b"abbey:route-audit-workspace:v1\0" + p.encode()).hexdigest()[:12] for p in WORKSPACES}
    require(set(re.findall(r"ws-[a-zA-Z0-9]+", text)) == expected, "workspace_digests")
    require("route-stage" in text and "route-correlation" in text and "route-tool" in text, "route_optional_fields")


def empty(snapshot, secrets):
    text = inspect(snapshot, secrets)
    require(not markers(snapshot), "empty_has_stale_rows")
    require("No routing has been audited" in text, "empty_message_missing")


def rejected(snapshot, secrets, expected_label=None):
    text = inspect(snapshot, secrets)
    require(not markers(snapshot), "error_has_stale_rows")
    labels = (expected_label,) if expected_label else ("Cannot reach Abbey", "Request rejected", "Daemon configuration error", "Protocol mismatch")
    require(any(label in text for label in labels), "error_message_missing")
