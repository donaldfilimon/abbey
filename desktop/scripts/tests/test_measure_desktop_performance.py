"""Evaluate recorded desktop performance samples against the plan's bars.

Only the evaluator's pure logic is exercised over owned sample files written
to a temporary directory; no app is launched and no live process is sampled.
A pass here is not performance qualification: that needs a run on named
hardware (plan Task 12 Step 2).
"""

from __future__ import annotations

import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import measure_desktop_performance as perf  # noqa: E402


def sample(metric: str, value: float, edition: str = "safe") -> str:
    return json.dumps({"metric": metric, "value": value, "edition": edition})


class EvaluatorTest(unittest.TestCase):
    def run_main(self, lines: list[str]) -> tuple[int, str, str]:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "samples.jsonl"
            path.write_text("\n".join(lines) + "\n", encoding="utf-8")
            out, err = io.StringIO(), io.StringIO()
            with redirect_stdout(out), redirect_stderr(err):
                code = perf.main([str(path)])
        return code, out.getvalue(), err.getvalue()

    def test_p95_is_computed_over_recorded_samples(self) -> None:
        # Nearest rank: ceil(0.95 * 20) = 19th smallest of 1..20.
        self.assertEqual(perf.p95([float(v) for v in range(20, 0, -1)]), 19.0)
        # ceil(0.95 * 10) = 10th smallest: the maximum.
        self.assertEqual(perf.p95([float(v) for v in range(1, 11)]), 10.0)
        self.assertEqual(perf.p95([0.7]), 0.7)

        lines = [sample("warm_interaction_s", v / 10) for v in range(1, 21)]
        code, out, _ = self.run_main(lines)
        verdict = json.loads(out)["editions"]["safe"]["warm_interaction_s"]
        self.assertEqual(verdict["statistic"], "p95")
        self.assertEqual(verdict["value"], 1.9)
        self.assertEqual(verdict["samples"], 20)
        self.assertEqual(verdict["status"], "pass")

    def test_a_metric_with_no_samples_is_reported_missing_not_passed(self) -> None:
        lines = [
            sample("cold_start_s", 3.0),
            sample("warm_interaction_s", 1.0),
            sample("navigation_feedback_ms", 50.0),
        ]
        code, out, _ = self.run_main(lines)
        report = json.loads(out)
        idle = report["editions"]["safe"]["idle_cpu_pct"]
        self.assertEqual(idle["status"], "missing")
        self.assertEqual(idle["samples"], 0)
        self.assertIsNone(idle["value"])
        self.assertEqual(report["editions"]["safe"]["overall"], "incomplete")
        self.assertNotEqual(code, 0)

    def test_editions_are_evaluated_separately(self) -> None:
        lines = []
        for edition, cold in (("safe", 4.0), ("personal", 6.5)):
            lines += [
                sample("cold_start_s", cold, edition),
                sample("warm_interaction_s", 1.0, edition),
                sample("navigation_feedback_ms", 80.0, edition),
                sample("idle_cpu_pct", 1.5, edition),
            ]
        code, out, _ = self.run_main(lines)
        editions = json.loads(out)["editions"]
        self.assertEqual(editions["safe"]["cold_start_s"]["status"], "pass")
        self.assertEqual(editions["safe"]["cold_start_s"]["value"], 4.0)
        self.assertEqual(editions["safe"]["overall"], "pass")
        self.assertEqual(editions["personal"]["cold_start_s"]["status"], "fail")
        self.assertEqual(editions["personal"]["cold_start_s"]["value"], 6.5)
        self.assertEqual(editions["personal"]["overall"], "fail")
        self.assertNotEqual(code, 0)

    def test_a_malformed_line_fails_the_run(self) -> None:
        good = sample("cold_start_s", 3.0)
        for bad in (
            "{not json",
            sample("unknown_metric", 1.0),
            sample("cold_start_s", 1.0, "enterprise"),
            json.dumps({"metric": "cold_start_s", "value": "3", "edition": "safe"}),
            json.dumps({"metric": "cold_start_s", "value": True, "edition": "safe"}),
            json.dumps({"metric": "cold_start_s", "edition": "safe"}),
            '{"metric": "cold_start_s", "value": NaN, "edition": "safe"}',
            sample("cold_start_s", -1.0),
        ):
            with self.subTest(bad=bad):
                code, out, err = self.run_main([good, bad])
                self.assertNotEqual(code, 0)
                self.assertEqual(out, "")
                self.assertIn("line 2", err)

    def test_output_labels_itself_non_qualifying(self) -> None:
        code, out, _ = self.run_main([sample("cold_start_s", 3.0)])
        report = json.loads(out)
        self.assertIs(report["qualifying"], False)
        self.assertIn("non-qualifying", report["label"])
        self.assertIn("named hardware", report["label"])


if __name__ == "__main__":
    unittest.main()
