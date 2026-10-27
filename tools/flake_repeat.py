#!/usr/bin/env python3
"""Repeat one cargo integration test to diagnose intermittent failures.

    python3 tools/flake_repeat.py --test conversation_identity_cutover --runs 20 \
        --mode {serial,parallel,load} [--burners K] [--filter NAME] --out DIR

Each run writes DIR/run-<i>.log and appends one JSON line to DIR/runs.jsonl.
A run whose cargo summary shows zero passed tests is an error (a filter that
matches nothing makes cargo exit 0). Exit status is 0 only if every run passed.
"""
import argparse
import json
import multiprocessing
import re
import subprocess
import sys
import time
from pathlib import Path

SUMMARY = re.compile(r"test result: \w+\. (\d+) passed; (\d+) failed;")


def parse_counts(output):
    passed = failed = 0
    for match in SUMMARY.finditer(output):
        passed += int(match.group(1))
        failed += int(match.group(2))
    return passed, failed


def cargo_runner(argv):
    result = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    return result.returncode, result.stdout


def _burn():
    while True:
        pass


def start_burners(count):
    processes = [multiprocessing.Process(target=_burn, daemon=True) for _ in range(count)]
    for process in processes:
        process.start()
    return processes


def stop_burners(processes):
    for process in processes:
        process.terminate()
    for process in processes:
        process.join(timeout=5)


def build_argv(test, mode, filter_name):
    argv = ["./tools/cargo.sh", "test", "--locked", "--test", test]
    if filter_name:
        argv.append(filter_name)
    if mode == "serial":
        argv += ["--", "--test-threads=1"]
    return argv


def run_repeats(test, runs, mode, burners, filter, out, runner=cargo_runner,
                burner_factory=start_burners):
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    ok = True
    with open(out / "runs.jsonl", "a", encoding="utf-8") as ledger:
        for index in range(1, runs + 1):
            procs = []
            started = time.monotonic()
            try:
                if mode == "load" and burners > 0:
                    procs = burner_factory(burners)
                code, output = runner(build_argv(test, mode, filter))
            finally:
                stop_burners(procs)
            seconds = round(time.monotonic() - started, 3)
            (out / f"run-{index}.log").write_text(output, encoding="utf-8")
            passed, failed = parse_counts(output)
            ledger.write(json.dumps({
                "run": index, "mode": mode, "exit": code, "seconds": seconds,
                "passed": passed, "failed": failed,
            }) + "\n")
            ledger.flush()
            if code != 0 or failed > 0 or passed == 0:
                ok = False
    return 0 if ok else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--test", required=True)
    parser.add_argument("--runs", type=int, required=True)
    parser.add_argument("--mode", choices=("serial", "parallel", "load"), required=True)
    parser.add_argument("--burners", type=int, default=0)
    parser.add_argument("--filter")
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)
    if args.runs < 1:
        parser.error("--runs must be at least 1")
    return run_repeats(args.test, args.runs, args.mode, args.burners, args.filter, args.out)


if __name__ == "__main__":
    sys.exit(main())
