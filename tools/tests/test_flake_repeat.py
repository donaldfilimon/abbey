import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "flake_repeat", ROOT / "tools" / "flake_repeat.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

OK_3 = "running 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
FAIL_1 = "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n"
NONE = "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out\n"


class FakeBurner:
    def __init__(self):
        self.terminated = False

    def terminate(self):
        self.terminated = True

    def join(self, timeout=None):
        pass


def scripted(results, calls=None):
    queue = list(results)

    def runner(argv):
        if calls is not None:
            calls.append(list(argv))
        item = queue.pop(0)
        if isinstance(item, Exception):
            raise item
        return item

    return runner


class FlakeRepeatTest(unittest.TestCase):
    def run_harness(self, results, **kwargs):
        calls = []
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            params = dict(test="t", runs=len(results), mode="serial", burners=0, filter=None)
            params.update(kwargs)
            status = MODULE.run_repeats(
                out=out, runner=scripted(results, calls), **params
            )
            records = [
                json.loads(line)
                for line in (out / "runs.jsonl").read_text().splitlines()
            ]
            logs = sorted(p.name for p in out.glob("run-*.log"))
        return status, records, logs, calls

    def test_records_every_run_exit_code(self):
        status, records, logs, _ = self.run_harness([(0, OK_3), (101, FAIL_1), (0, OK_3)])
        self.assertEqual(1, status)
        self.assertEqual([0, 101, 0], [r["exit"] for r in records])
        self.assertEqual([1, 2, 3], [r["run"] for r in records])
        self.assertEqual([3, 2, 3], [r["passed"] for r in records])
        self.assertEqual([0, 1, 0], [r["failed"] for r in records])
        self.assertEqual({"run", "mode", "exit", "seconds", "passed", "failed"}, set(records[0]))
        self.assertEqual(["run-1.log", "run-2.log", "run-3.log"], logs)

    def test_all_passing_runs_exit_zero(self):
        status, _, _, _ = self.run_harness([(0, OK_3), (0, OK_3)])
        self.assertEqual(0, status)

    def test_a_filter_that_matches_nothing_is_an_error(self):
        # cargo exits 0 when a filter matches no tests; that is not a pass.
        status, records, _, _ = self.run_harness([(0, NONE)], filter="no_such_test")
        self.assertEqual(0, records[0]["passed"])
        self.assertNotEqual(0, status)

    def test_load_burners_are_terminated_even_when_a_run_raises(self):
        burners = [FakeBurner(), FakeBurner()]
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(RuntimeError):
                MODULE.run_repeats(
                    test="t", runs=1, mode="load", burners=2, filter=None,
                    out=Path(directory),
                    runner=scripted([RuntimeError("boom")]),
                    burner_factory=lambda count: burners,
                )
        self.assertTrue(all(b.terminated for b in burners))

    def test_burners_only_run_in_load_mode(self):
        started = []
        self.run_harness(
            [(0, OK_3)], mode="parallel", burners=4,
            burner_factory=lambda n: started.append(n) or [],
        )
        self.assertEqual([], started)

    def test_serial_mode_passes_test_threads_one(self):
        _, _, _, calls = self.run_harness([(0, OK_3)], mode="serial", filter="name")
        self.assertEqual(
            ["./tools/cargo.sh", "test", "--locked", "--test", "t", "name", "--", "--test-threads=1"],
            calls[0],
        )

    def test_parallel_mode_does_not_pin_test_threads(self):
        _, _, _, calls = self.run_harness([(0, OK_3)], mode="parallel")
        self.assertEqual(["./tools/cargo.sh", "test", "--locked", "--test", "t"], calls[0][:5])
        self.assertNotIn("--test-threads=1", calls[0])


if __name__ == "__main__":
    unittest.main()
