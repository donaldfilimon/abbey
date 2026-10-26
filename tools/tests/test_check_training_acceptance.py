"""Synthetic schema/gate contracts only; these are not model-quality evidence."""

from contextlib import redirect_stderr, redirect_stdout
import copy
import io
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_training_acceptance as checker


def digest(character):
    return "sha256:" + character * 64


def report():
    identity = dict(zip(checker.IDENTITIES, map(digest, "0123467")))
    base = {
        "artifact_digest": digest("0"), "identity": identity,
        "held_out": [{"id": "nll-1", "mean_nll": 1, "token_count": 10}],
        "tool_json": [{"id": f"tool-{i}", "tool_json_valid": True} for i in range(20)],
        "public_refusal": [{"id": "refusal-1", "refusal_success": True,
                            "unauthorized_effects": False}],
        "categories": {name: [{"id": name + "-1", "success": True}]
                       for name in checker.CATEGORIES},
    }
    candidate = copy.deepcopy(base)
    candidate["artifact_digest"] = digest("5")
    return {"schema_version": 1, "base": base, "candidate": candidate}


class TrainingAcceptanceTests(unittest.TestCase):
    def invalid(self, value):
        with self.assertRaises(checker.InvalidReport):
            checker.validate_report(value)

    def test_valid_report_and_derived_integer_counts(self):
        result = checker.validate_report(checker.parse_report(json.dumps(report()).encode()))
        self.assertTrue(result["accepted"])
        self.assertEqual(result["counts"]["tool_json"],
                         {"samples": 20, "base_success": 20, "candidate_success": 20})
        self.assertIn("not authenticated", result["boundary"])

    def test_malformed_roots_and_versions(self):
        for value in (None, [], True, 1, "report", {}):
            with self.subTest(value=value):
                self.invalid(value)
        for version in (True, False, 1.0, "1", 0, 2, None):
            value = report()
            value["schema_version"] = version
            self.invalid(value)

    def test_missing_and_unknown_fields_at_every_level(self):
        paths = [(), ("base",), ("candidate", "identity"),
                 ("base", "categories"), ("candidate", "held_out", 0),
                 ("candidate", "tool_json", 0), ("base", "public_refusal", 0),
                 ("candidate", "categories", "coding", 0)]
        for path in paths:
            for mode in ("missing", "unknown"):
                with self.subTest(path=path, mode=mode):
                    value = report()
                    target = value
                    for key in path:
                        target = target[key]
                    if mode == "missing":
                        del target[next(iter(target))]
                    else:
                        target["free_total"] = 100
                    self.invalid(value)

    def test_duplicate_json_keys_and_malformed_bytes(self):
        for raw in (b'{"x":1,"x":2}', b'{"nested":{"x":1,"x":2}}',
                    b'{', b'\xff', b'{} {}', b'[' * 2000):
            with self.subTest(raw=raw[:50]):
                with self.assertRaises(checker.InvalidReport):
                    checker.parse_report(raw)
        private_key = "synthetic-private-marker" * 1000
        raw = ('{"' + private_key + '":1,"' + private_key + '":2}').encode()
        status, output, error = self.cli(raw)
        self.assertEqual((status, output), (2, ""))
        self.assertNotIn("synthetic-private-marker", error)
        self.assertLess(len(error), 1000)

    def test_nonfinite_json_and_nll_types(self):
        for token in ("NaN", "Infinity", "-Infinity", "1e999"):
            raw = json.dumps(report()).replace('"mean_nll": 1', '"mean_nll": ' + token).encode()
            with self.assertRaises(checker.InvalidReport):
                checker.validate_report(checker.parse_report(raw))
        for number in (True, False, -1, float("nan"), float("inf"), "1", None, []):
            value = report()
            value["candidate"]["held_out"][0]["mean_nll"] = number
            self.invalid(value)
        value = report()
        value["base"]["held_out"][0]["mean_nll"] = 0
        self.invalid(value)

    def test_exact_nll_threshold_and_regression(self):
        for nll, accepted in ((0, True), (1.02, True), (1.0200000000000002, False)):
            value = report()
            value["candidate"]["held_out"][0]["mean_nll"] = nll
            self.assertEqual(checker.validate_report(value)["accepted"], accepted)
        value = report()
        value["candidate"]["held_out"][0]["mean_nll"] = "DECIMAL"
        for number, accepted in (("1.0200000000000000000000000001", False),
                                 ("1.0200000000000000000000000000", True)):
            raw = json.dumps(value).replace('"DECIMAL"', number).encode()
            self.assertEqual(checker.validate_report(checker.parse_report(raw))["accepted"],
                             accepted)

    def test_nll_mean_not_each_example_and_large_finite_values(self):
        value = report()
        for run in ("base", "candidate"):
            value[run]["held_out"] = [{"id": "a", "mean_nll": 1e308, "token_count": 1},
                                      {"id": "b", "mean_nll": 1e308, "token_count": 1}]
        self.assertTrue(checker.validate_report(value)["accepted"])
        value["base"]["held_out"] = [{"id": "a", "mean_nll": 1, "token_count": 1},
                                     {"id": "b", "mean_nll": 3, "token_count": 1}]
        value["candidate"]["held_out"] = [{"id": "b", "mean_nll": 0, "token_count": 1},
                                          {"id": "a", "mean_nll": 4.08, "token_count": 1}]
        self.assertTrue(checker.validate_report(value)["accepted"])

    def test_exact_tool_threshold_and_refusal_failure(self):
        value = report()
        value["candidate"]["tool_json"][0]["tool_json_valid"] = False
        self.assertTrue(checker.validate_report(value)["accepted"])
        value["candidate"]["tool_json"][1]["tool_json_valid"] = False
        self.assertFalse(checker.validate_report(value)["gates"]["tool_json"])
        value = report()
        value["candidate"]["public_refusal"][0]["refusal_success"] = False
        self.assertFalse(checker.validate_report(value)["accepted"])

    def test_token_weighting_can_reverse_unweighted_decision(self):
        # Equal-example averaging would pass the first case and fail the second.
        for losses, accepted in (((0, 2), False), ((3, 0), True)):
            value = report()
            value["base"]["held_out"] = [
                {"id": "short", "mean_nll": 1, "token_count": 1},
                {"id": "long", "mean_nll": 1, "token_count": 99}]
            value["candidate"]["held_out"] = [
                {"id": "short", "mean_nll": losses[0], "token_count": 1},
                {"id": "long", "mean_nll": losses[1], "token_count": 99}]
            result = checker.validate_report(value)
            self.assertEqual(result["gates"]["held_out_nll"], accepted)
            self.assertEqual(result["counts"]["held_out"]["scored_tokens"], 100)

    def test_token_counts_required_positive_integer_and_match_per_example(self):
        for count in (True, False, 0, -1, 10.0, "10", None, 11):
            value = report()
            value["candidate"]["held_out"][0]["token_count"] = count
            self.invalid(value)
        for run in ("base", "candidate"):
            value = report()
            del value[run]["held_out"][0]["token_count"]
            self.invalid(value)
        value = report()
        for run, counts in (("base", (1, 99)), ("candidate", (99, 1))):
            value[run]["held_out"] = [
                {"id": "a", "mean_nll": 1, "token_count": counts[0]},
                {"id": "b", "mean_nll": 1, "token_count": counts[1]}]
        self.invalid(value)  # Same total does not establish matching scored tokens.

    def test_safety_requires_refusal_and_no_unauthorized_effects(self):
        for refusal in (True, False):
            for effects in (True, False):
                value = report()
                case = value["candidate"]["public_refusal"][0]
                case.update(refusal_success=refusal, unauthorized_effects=effects)
                result = checker.validate_report(value)
                self.assertEqual(result["gates"]["public_refusal"], refusal and not effects)
                self.assertEqual(result["counts"]["public_refusal"]["candidate_success"],
                                 int(refusal and not effects))
        for run in ("base", "candidate"):
            for field in ("refusal_success", "unauthorized_effects"):
                for invalid in (None, 0, 1, "false"):
                    value = report()
                    value[run]["public_refusal"][0][field] = invalid
                    self.invalid(value)
                value = report()
                del value[run]["public_refusal"][0][field]
                self.invalid(value)

    def test_each_category_regression_and_improvement(self):
        for category in checker.CATEGORIES:
            value = report()
            value["candidate"]["categories"][category][0]["success"] = False
            self.assertFalse(checker.validate_report(value)["gates"][category])
            value["base"]["categories"][category][0]["success"] = False
            self.assertTrue(checker.validate_report(value)["accepted"])
            value["candidate"]["categories"][category][0]["success"] = True
            self.assertTrue(checker.validate_report(value)["accepted"])

    def test_identity_and_artifact_checks(self):
        for key in checker.IDENTITIES:
            value = report()
            value["candidate"]["identity"][key] = digest("a")
            self.invalid(value)
        value = report()
        value["candidate"]["artifact_digest"] = value["base"]["artifact_digest"]
        self.invalid(value)
        value = report()
        for run in ("base", "candidate"):
            value[run]["identity"]["base_digest"] = digest("b")
        self.invalid(value)

    def test_bad_digests(self):
        for bad in ("sha256:" + "A" * 64, "sha256:" + "a" * 63,
                    digest("g"), digest("a") + "\n", "a" * 64, True, None):
            for path in (("artifact_digest",), ("identity", "tokenizer_digest")):
                value = report()
                target = value["candidate"]
                for key in path[:-1]:
                    target = target[key]
                target[path[-1]] = bad
                self.invalid(value)

    def test_all_lists_require_matching_nonempty_unique_ids_and_strict_outcomes(self):
        paths = [("held_out",), ("tool_json",), ("public_refusal",)]
        paths.extend(("categories", category) for category in checker.CATEGORIES)
        for path in paths:
            for mode in ("empty", "not_list", "duplicate", "mismatch", "extra",
                         "blank_id", "long_id", "int_id", "bool_outcome"):
                with self.subTest(path=path, mode=mode):
                    value = report()
                    target = value["candidate"]
                    for key in path[:-1]:
                        target = target[key]
                    records = target[path[-1]]
                    if mode == "empty":
                        target[path[-1]] = []
                    elif mode == "not_list":
                        target[path[-1]] = {}
                    elif mode in ("duplicate", "extra"):
                        records.append(copy.deepcopy(records[0]))
                        if mode == "extra":
                            records[-1]["id"] = "additional"
                    elif mode == "mismatch":
                        records[0]["id"] = "different"
                    elif mode.endswith("id"):
                        records[0]["id"] = {"blank_id": " ", "long_id": "x" * 129,
                                            "int_id": 1}[mode]
                    else:
                        outcome = next(key for key in records[0] if key != "id")
                        records[0][outcome] = True if outcome == "mean_nll" else 1
                    self.invalid(value)

    def test_oversized_input_and_records(self):
        with self.assertRaises(checker.InvalidReport):
            checker.parse_report(b" " * (checker.MAX_INPUT_BYTES + 1))
        value = report()
        value["base"]["held_out"] = [{"id": str(i), "mean_nll": 1, "token_count": 1}
                                       for i in range(checker.MAX_RECORDS + 1)]
        self.invalid(value)
        with patch.object(checker, "MAX_RECORDS", 50):
            self.assertTrue(checker.validate_report(report())["accepted"])
        with patch.object(checker, "MAX_RECORDS", 49):
            self.invalid(report())

    def cli(self, raw):
        stdin = io.TextIOWrapper(io.BytesIO(raw), encoding="utf-8")
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.object(sys, "stdin", stdin), redirect_stdout(stdout), redirect_stderr(stderr):
            status = checker.main(["-"])
        return status, stdout.getvalue(), stderr.getvalue()

    def test_cli_pass_fail_invalid_and_help(self):
        status, output, error = self.cli(json.dumps(report()).encode())
        self.assertEqual(status, 0)
        self.assertTrue(json.loads(output)["accepted"])
        self.assertEqual(error, "")
        value = report()
        value["candidate"]["public_refusal"][0]["refusal_success"] = False
        status, output, _ = self.cli(json.dumps(value).encode())
        self.assertEqual(status, 1)
        self.assertFalse(json.loads(output)["accepted"])
        for raw in (b"null", b"{}", b" " * (checker.MAX_INPUT_BYTES + 1)):
            status, output, error = self.cli(raw)
            self.assertEqual(status, 2)
            self.assertEqual(output, "")
            self.assertIn("invalid report", error)
        with redirect_stdout(io.StringIO()) as output, self.assertRaises(SystemExit) as exit_:
            checker.main(["--help"])
        self.assertEqual(exit_.exception.code, 0)
        self.assertIn("Schema version 1", output.getvalue())

    def test_cli_file_read_is_bounded_read_only_and_os_errors_fail_closed(self):
        source = io.BytesIO(json.dumps(report()).encode())
        with patch.object(Path, "open", return_value=source) as open_, redirect_stdout(io.StringIO()):
            self.assertEqual(checker.main(["synthetic.json"]), 0)
        open_.assert_called_once_with("rb")
        with patch.object(Path, "open", side_effect=OSError("unreadable")), redirect_stderr(io.StringIO()):
            self.assertEqual(checker.main(["missing.json"]), 2)

    def test_cli_huge_decimal_exponents_return_invalid_without_traceback(self):
        for token in ("1e999999999999999999999999999999",
                      "1e-999999999999999999999999999999"):
            raw = json.dumps(report()).replace('"mean_nll": 1', '"mean_nll": ' + token)
            status, output, error = self.cli(raw.encode())
            self.assertEqual(status, 2)
            self.assertEqual(output, "")
            self.assertIn("invalid report", error)

    def test_scored_tokens_are_bounded_individually_and_in_aggregate(self):
        value = report()
        for run in ("base", "candidate"):
            value[run]["held_out"][0]["token_count"] = checker.MAX_SCORED_TOKENS
        self.assertTrue(checker.validate_report(value)["accepted"])
        for run in ("base", "candidate"):
            value[run]["held_out"].append(
                {"id": "additional", "mean_nll": 1, "token_count": 1})
        self.invalid(value)
        status, output, _ = self.cli(json.dumps(value).encode())
        self.assertEqual((status, output), (2, ""))
        # Each input integer is JSON-decodable, but their sum would exceed
        # Python's integer-to-string limit without a bounded-token contract.
        for run in ("base", "candidate"):
            for record in value[run]["held_out"]:
                record["token_count"] = 9 * 10**4299
        status, output, error = self.cli(json.dumps(value).encode())
        self.assertEqual((status, output), (2, ""))
        self.assertIn("invalid report", error)


if __name__ == "__main__":
    unittest.main()
