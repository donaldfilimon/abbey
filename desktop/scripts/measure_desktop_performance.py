#!/usr/bin/env python3
"""Evaluate recorded desktop performance samples against the plan's bars.

Usage: measure_desktop_performance.py SAMPLES.jsonl

SAMPLES.jsonl holds one JSON object per line:
    {"metric": METRIC, "value": NUMBER, "edition": "safe" | "personal"}
Blank lines are ignored. Any other malformed line fails the whole run with
exit 2 and no verdict on stdout, so a partial file never yields a partial
verdict.

The verdict is one JSON object on stdout, evaluated per edition. A metric
with no samples is reported "missing", never "pass". Exit 0 only when every
metric of every edition passes; exit 1 when any fails or is missing.

This script only evaluates recorded samples: it does not launch the app or
sample a live process. Its output is non-qualifying until the samples come
from a run on named hardware (plan Task 12 Step 2).
"""

from __future__ import annotations

import json
import math
from pathlib import Path
import sys
from typing import Callable, Sequence

EDITIONS = ("safe", "personal")

LABEL = (
    "non-qualifying: evaluated from recorded samples only; performance "
    "qualification requires a run on named hardware for both editions"
)


def p95(values: Sequence[float]) -> float:
    """Nearest-rank 95th percentile: the ceil(0.95 * n)-th smallest value."""
    if not values:
        raise ValueError("p95 of no samples")
    ordered = sorted(values)
    rank = math.ceil(0.95 * len(ordered))
    return ordered[rank - 1]


# metric -> (statistic name, statistic, inclusive upper bar, unit).
# Bars without a stated percentile are held at the worst observed sample.
BARS: dict[str, tuple[str, Callable[[Sequence[float]], float], float, str]] = {
    "cold_start_s": ("max", max, 5.0, "s"),
    "warm_interaction_s": ("p95", p95, 2.0, "s"),
    "navigation_feedback_ms": ("max", max, 100.0, "ms"),
    "idle_cpu_pct": ("max", max, 2.0, "% of one core"),
}


class SampleError(ValueError):
    """A samples line that cannot be evaluated."""


def parse_line(line: str, number: int) -> tuple[str, str, float] | None:
    """Parse one samples line into (edition, metric, value); None if blank."""
    if not line.strip():
        return None
    try:
        record = json.loads(line, parse_constant=_reject_constant)
    except (json.JSONDecodeError, SampleError) as error:
        raise SampleError(f"line {number}: not valid JSON: {error}") from None
    if not isinstance(record, dict) or set(record) != {"metric", "value", "edition"}:
        raise SampleError(
            f"line {number}: expected exactly the keys metric, value, edition"
        )
    metric, value, edition = record["metric"], record["value"], record["edition"]
    if metric not in BARS:
        raise SampleError(f"line {number}: unknown metric {metric!r}")
    if edition not in EDITIONS:
        raise SampleError(f"line {number}: unknown edition {edition!r}")
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise SampleError(f"line {number}: value must be a number")
    if not math.isfinite(value) or value < 0:
        raise SampleError(f"line {number}: value must be finite and non-negative")
    return edition, metric, float(value)


def _reject_constant(name: str) -> float:
    raise SampleError(f"non-finite constant {name}")


def parse_samples(text: str) -> dict[str, dict[str, list[float]]]:
    samples: dict[str, dict[str, list[float]]] = {
        edition: {metric: [] for metric in BARS} for edition in EDITIONS
    }
    for number, line in enumerate(text.splitlines(), start=1):
        parsed = parse_line(line, number)
        if parsed is not None:
            edition, metric, value = parsed
            samples[edition][metric].append(value)
    return samples


def evaluate(samples: dict[str, dict[str, list[float]]]) -> dict:
    editions: dict[str, dict] = {}
    for edition in EDITIONS:
        verdicts: dict[str, object] = {}
        statuses = []
        for metric, (name, statistic, bar, unit) in BARS.items():
            values = samples[edition][metric]
            if values:
                value = statistic(values)
                status = "pass" if value <= bar else "fail"
            else:
                value, status = None, "missing"
            statuses.append(status)
            verdicts[metric] = {
                "status": status,
                "statistic": name,
                "value": value,
                "bar": bar,
                "unit": unit,
                "samples": len(values),
            }
        if "fail" in statuses:
            overall = "fail"
        elif "missing" in statuses:
            overall = "incomplete"
        else:
            overall = "pass"
        verdicts["overall"] = overall
        editions[edition] = verdicts
    return {"qualifying": False, "label": LABEL, "editions": editions}


def main(argv: Sequence[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if len(args) != 1:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    try:
        text = Path(args[0]).read_text(encoding="utf-8")
        samples = parse_samples(text)
    except (OSError, UnicodeDecodeError, SampleError) as error:
        print(f"measure_desktop_performance: {error}", file=sys.stderr)
        return 2
    report = evaluate(samples)
    print(json.dumps(report, indent=2, sort_keys=True))
    passed = all(v["overall"] == "pass" for v in report["editions"].values())
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
