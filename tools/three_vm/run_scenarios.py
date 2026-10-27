#!/usr/bin/env python3
"""Run the three-VM shared-compute failure/recovery scenarios and record evidence.

    python3 tools/three_vm/run_scenarios.py --inventory vms.json --out evidence-dir

Inert until OB-11: the operator provisions three VMs and supplies, per scenario,
the commands that exercise it. The inventory is

    {"vms": [{"name", "ip", "identity", "state_dir", "host"} x 3],
     "scenarios": {"<scenario id>": {"commands": [{"vm": "<name>", "argv": [...]}]}}}

The inventory is refused (non-zero exit, nothing run, nothing written) unless it
names exactly three VMs with distinct names, IPs, identities and state dirs and
covers exactly the fixed scenario ids. A scenario passes only when every command
exits 0; a runner exception is a failure, never a skip. Three VMs on one host are
labelled multi-VM evidence, never physical multi-host evidence, and this harness
never claims multi-host evidence for any inventory.
"""
import argparse
import hashlib
import json
import os
import shlex
import subprocess
import sys
from pathlib import Path

SCENARIOS = (
    "worker_vm_termination",
    "leader_failure",
    "network_partition",
    "stale_leader",
    "duplicate_delivery",
    "conflicting_wdbx_values",
    "read_repair",
    "reconnect",
    "rolling_restart",
    "certificate_rotation",
    "clean_teardown",
)
VM_FIELDS = ("name", "ip", "identity", "state_dir", "host")
DISTINCT_FIELDS = ("name", "ip", "identity", "state_dir")
SAME_HOST_LABEL = "three-VM on one host: multi-VM evidence, not physical multi-host"
DISTINCT_HOST_LABEL = (
    "three-VM across declared hosts: multi-VM evidence; "
    "physical multi-host is not established by this harness"
)
ENV_PREFIX = "ABBEY_"
CREDENTIAL_MARKERS = ("TOKEN", "KEY", "SECRET", "BEARER", "PASSWORD")
REDACTED = "<redacted>"
OUTPUT_LIMIT = 64 * 1024


class InventoryError(ValueError):
    """The inventory cannot be run; nothing was executed."""


def is_credential_name(name):
    upper = name.upper()
    return any(marker in upper for marker in CREDENTIAL_MARKERS)


def capture_env(environ):
    return dict(
        sorted(
            (name, value)
            for name, value in environ.items()
            if name.startswith(ENV_PREFIX) and not is_credential_name(name)
        )
    )


def redact_argv(argv):
    redacted = []
    for arg in argv:
        name, sep, _ = arg.partition("=")
        redacted.append(f"{name}={REDACTED}" if sep and is_credential_name(name) else arg)
    return redacted


def bounded(text):
    text = text if isinstance(text, str) else str(text)
    return text if len(text) <= OUTPUT_LIMIT else text[-OUTPUT_LIMIT:]


def validate(inventory):
    if not isinstance(inventory, dict):
        raise InventoryError("inventory must be a JSON object")
    vms = inventory.get("vms")
    if not isinstance(vms, list) or len(vms) != 3:
        raise InventoryError("inventory must name exactly three VMs")
    for vm in vms:
        if not isinstance(vm, dict):
            raise InventoryError("each VM must be a JSON object")
        for field in VM_FIELDS:
            if not isinstance(vm.get(field), str) or not vm[field]:
                raise InventoryError(f"every VM needs a non-empty '{field}'")
    for field in DISTINCT_FIELDS:
        values = [vm[field] for vm in vms]
        if len(set(values)) != len(values):
            raise InventoryError(f"two VMs share the same {field}")
    scenarios = inventory.get("scenarios")
    if not isinstance(scenarios, dict):
        raise InventoryError("inventory needs a 'scenarios' object")
    unknown = sorted(set(scenarios) - set(SCENARIOS))
    missing = [s for s in SCENARIOS if s not in scenarios]
    if unknown:
        raise InventoryError(f"unknown scenario ids: {', '.join(unknown)}")
    if missing:
        raise InventoryError(f"missing scenario ids: {', '.join(missing)}")
    names = {vm["name"] for vm in vms}
    for scenario in SCENARIOS:
        commands = (scenarios[scenario] or {}).get("commands")
        if not isinstance(commands, list) or not commands:
            raise InventoryError(f"scenario {scenario} needs at least one command")
        for command in commands:
            if not isinstance(command, dict) or command.get("vm") not in names:
                raise InventoryError(f"scenario {scenario} names an unknown VM")
            argv = command.get("argv")
            if not isinstance(argv, list) or not argv or not all(isinstance(a, str) for a in argv):
                raise InventoryError(f"scenario {scenario} has a command without an argv list")
    return vms


def inventory_digest(inventory):
    canonical = json.dumps(inventory, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def evidence_label(vms):
    return SAME_HOST_LABEL if len({vm["host"] for vm in vms}) == 1 else DISTINCT_HOST_LABEL


def run_scenario(scenario, commands, vms_by_name, runner):
    records = []
    status = "pass"
    for command in commands:
        vm = vms_by_name[command["vm"]]
        record = {"vm": vm["name"], "argv": redact_argv(command["argv"])}
        try:
            returncode, stdout, stderr = runner(vm, list(command["argv"]))
        except Exception as error:  # a runner failure is evidence of failure, not a skip
            record["error"] = f"{type(error).__name__}: {error}"
            records.append(record)
            status = "fail"
            break
        record.update(returncode=returncode, stdout=bounded(stdout), stderr=bounded(stderr))
        records.append(record)
        if returncode != 0:
            status = "fail"
            break
    return status, records


def write_json(path, payload):
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def run_scenarios(inventory, out_dir, runner, environ=None):
    vms = validate(inventory)
    environ = os.environ if environ is None else environ
    out_dir = Path(out_dir)
    digest = inventory_digest(inventory)
    label = evidence_label(vms)
    env = capture_env(environ)
    vms_by_name = {vm["name"]: vm for vm in vms}
    out_dir.mkdir(parents=True, exist_ok=True)
    results = {}
    for scenario in SCENARIOS:
        status, records = run_scenario(
            scenario, inventory["scenarios"][scenario]["commands"], vms_by_name, runner
        )
        evidence = f"{scenario}.json"
        write_json(
            out_dir / evidence,
            {
                "scenario": scenario,
                "status": status,
                "label": label,
                "inventory_sha256": digest,
                "env": env,
                "commands": records,
            },
        )
        results[scenario] = {"status": status, "evidence": evidence, "label": label}
    summary = {
        "status": "pass" if all(r["status"] == "pass" for r in results.values()) else "fail",
        "label": label,
        "inventory_sha256": digest,
        "scenarios": results,
    }
    write_json(out_dir / "summary.json", summary)
    return summary


def ssh_runner(vm, argv):
    """Run argv on the VM over ssh. Never exercised by the tests."""
    completed = subprocess.run(
        ["ssh", "-o", "BatchMode=yes", "--", vm["ip"], shlex.join(argv)],
        capture_output=True,
        text=True,
        check=False,
    )
    return completed.returncode, completed.stdout, completed.stderr


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--inventory", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        inventory = json.loads(args.inventory.read_text())
        summary = run_scenarios(inventory, args.out, ssh_runner)
    except (OSError, json.JSONDecodeError, InventoryError) as error:
        print(f"refused: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"status": summary["status"], "label": summary["label"]}))
    return 0 if summary["status"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
