import importlib.util
import io
import json
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "run_scenarios", ROOT / "tools" / "three_vm" / "run_scenarios.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def inventory(hosts=("mac-a", "mac-b", "mac-c"), overrides=None):
    vms = [
        {
            "name": f"vm{i}",
            "ip": f"10.0.0.{i}",
            "identity": f"worker-{i}",
            "state_dir": f"/var/lib/abbey/vm{i}",
            "host": hosts[i - 1],
        }
        for i in (1, 2, 3)
    ]
    for (index, field), value in (overrides or {}).items():
        vms[index][field] = value
    scenarios = {
        scenario: {"commands": [{"vm": "vm1", "argv": ["true", scenario]}]}
        for scenario in MODULE.SCENARIOS
    }
    return {"vms": vms, "scenarios": scenarios}


class RecordingRunner:
    def __init__(self, fail=(), raise_on=()):
        self.calls = []
        self.fail = set(fail)
        self.raise_on = set(raise_on)

    def __call__(self, vm, argv):
        self.calls.append((vm["name"], list(argv)))
        scenario = argv[-1]
        if scenario in self.raise_on:
            raise OSError("ssh: connect to host refused")
        if scenario in self.fail:
            return 1, "", "assertion failed"
        return 0, "ok", ""


class InventoryRefusalTests(unittest.TestCase):
    def assert_refused(self, inv):
        runner = RecordingRunner()
        with tempfile.TemporaryDirectory() as out:
            with self.assertRaises(MODULE.InventoryError):
                MODULE.run_scenarios(inv, Path(out), runner)
            self.assertEqual(list(Path(out).iterdir()), [])
        self.assertEqual(runner.calls, [])

    def test_inventory_with_duplicate_ips_is_refused(self):
        self.assert_refused(inventory(overrides={(1, "ip"): "10.0.0.1"}))

    def test_inventory_with_duplicate_identity_is_refused(self):
        self.assert_refused(inventory(overrides={(2, "identity"): "worker-1"}))

    def test_inventory_with_duplicate_state_dir_is_refused(self):
        self.assert_refused(inventory(overrides={(2, "state_dir"): "/var/lib/abbey/vm2"}))

    def test_inventory_without_exactly_three_vms_is_refused(self):
        inv = inventory()
        inv["vms"] = inv["vms"][:2]
        self.assert_refused(inv)

    def test_unknown_or_missing_scenario_is_refused(self):
        inv = inventory()
        inv["scenarios"]["bogus"] = {"commands": [{"vm": "vm1", "argv": ["true"]}]}
        self.assert_refused(inv)
        inv = inventory()
        del inv["scenarios"]["read_repair"]
        self.assert_refused(inv)

    def test_command_naming_an_unknown_vm_is_refused(self):
        inv = inventory()
        inv["scenarios"]["reconnect"]["commands"][0]["vm"] = "vm9"
        self.assert_refused(inv)

    def test_cli_refusal_exits_non_zero_without_running(self):
        inv = inventory(overrides={(1, "ip"): "10.0.0.1"})
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "vms.json"
            path.write_text(json.dumps(inv))
            out = Path(tmp) / "out"
            with redirect_stderr(io.StringIO()):
                code = MODULE.main(["--inventory", str(path), "--out", str(out)])
            self.assertNotEqual(code, 0)
            self.assertFalse(out.exists())


class ScenarioEvidenceTests(unittest.TestCase):
    def test_every_scenario_records_pass_fail_and_evidence_path(self):
        runner = RecordingRunner(fail={"leader_failure"}, raise_on={"reconnect"})
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            summary = MODULE.run_scenarios(inventory(), out, runner)
            on_disk = json.loads((out / "summary.json").read_text())
            self.assertEqual(on_disk, summary)
            self.assertEqual(set(summary["scenarios"]), set(MODULE.SCENARIOS))
            self.assertEqual(len(MODULE.SCENARIOS), 11)
            for scenario, entry in summary["scenarios"].items():
                expected = "fail" if scenario in {"leader_failure", "reconnect"} else "pass"
                self.assertEqual(entry["status"], expected, scenario)
                self.assertEqual(entry["evidence"], f"{scenario}.json")
                evidence = json.loads((out / entry["evidence"]).read_text())
                self.assertEqual(evidence["scenario"], scenario)
                self.assertEqual(evidence["status"], expected)
                self.assertEqual(evidence["inventory_sha256"], summary["inventory_sha256"])
            raised = json.loads((out / "reconnect.json").read_text())
            self.assertIn("OSError", raised["commands"][0]["error"])
            self.assertEqual(len(summary["inventory_sha256"]), 64)
            self.assertEqual(summary["label"], MODULE.DISTINCT_HOST_LABEL)
            self.assertNotEqual(summary["status"], "pass")
        self.assertEqual(len(runner.calls), 11)

    def test_a_same_host_inventory_is_labelled_not_multi_vm(self):
        with tempfile.TemporaryDirectory() as tmp:
            summary = MODULE.run_scenarios(
                inventory(hosts=("mac", "mac", "mac")), Path(tmp), RecordingRunner()
            )
            self.assertEqual(
                summary["label"],
                "three-VM on one host: multi-VM evidence, not physical multi-host",
            )
            for entry in summary["scenarios"].values():
                self.assertEqual(entry["label"], summary["label"])
            self.assertEqual(summary["status"], "pass")

    def test_recorded_env_and_argv_omit_credentials(self):
        inv = inventory()
        inv["scenarios"]["clean_teardown"]["commands"][0]["argv"] = [
            "env", "ABBEY_TOKEN=hunter2", "clean_teardown",
        ]
        environ = {"ABBEY_STATE": "x", "ABBEY_API_KEY": "hunter2", "HOME": "/h"}
        with tempfile.TemporaryDirectory() as tmp:
            MODULE.run_scenarios(inv, Path(tmp), RecordingRunner(), environ=environ)
            text = "".join(p.read_text() for p in Path(tmp).iterdir())
            evidence = json.loads((Path(tmp) / "clean_teardown.json").read_text())
        self.assertNotIn("hunter2", text)
        self.assertEqual(evidence["env"], {"ABBEY_STATE": "x"})


if __name__ == "__main__":
    unittest.main()
