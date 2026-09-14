"""Status and summary checks for larceny_report.py; no Larceny checkout required."""

import contextlib
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "larceny_report", Path(__file__).resolve().parents[1] / "larceny_report.py"
)
report = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(report)

ERROR = "Error: runtime error: Type error: car: expected pair, got ()"
FAILURE = "Expression:\n (car '(1))\nResult:\n 2\nExpected:\n 1\n\n"


class ParseLogTests(unittest.TestCase):
    def test_a_clean_tally_is_a_pass(self):
        self.assertEqual(
            report.parse_log("Running tests for (demo)\n12 tests passed\n"),
            ("pass", 12, 12, "", []),
        )

    def test_failures_without_an_error_are_a_fail(self):
        self.assertEqual(
            report.parse_log(FAILURE + "1 of 5 tests failed.\n"),
            ("fail", 4, 5, "", ["(car '(1))"]),
        )

    def test_a_tally_printed_after_a_top_level_error_is_truncated(self):
        # The shape `set` had: the run program's later forms still ran and
        # printed a tally, which the report used to call a clean pass.
        log = "Running tests for (demo)\n%s\n16 tests passed\n" % ERROR
        self.assertEqual(report.parse_log(log), ("truncated", 16, 16, ERROR, []))

    def test_failures_and_a_top_level_error_are_truncated(self):
        log = FAILURE + ERROR + "\n1 of 3 tests failed.\n"
        self.assertEqual(report.parse_log(log), ("truncated", 2, 3, ERROR, ["(car '(1))"]))

    def test_an_error_without_a_tally_is_still_a_load_error(self):
        self.assertEqual(report.parse_log(ERROR + "\n")[0], "load-error")


class SummaryTests(unittest.TestCase):
    def render(self, logs):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            log_dir = root / "logs"
            log_dir.mkdir()
            for name, text in logs.items():
                (log_dir / (name + ".txt")).write_text(text)
            suites = root / "Lib"
            (suites / "tests" / "scheme").mkdir(parents=True)
            out = root / "report.md"
            argv = ["larceny_report.py", "--logs", str(log_dir), "--suites", str(suites),
                    "--lane", "r7rs", "--commit", "0" * 40, "--backend", "VM",
                    "--out", str(out)]
            with patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(report.main(), 0)
            return out.read_text()

    def test_a_truncated_suite_is_not_counted_as_fully_passing(self):
        text = self.render({"clean": "7 tests passed\n", "cut": "%s\n16 tests passed\n" % ERROR})
        self.assertIn("| Suites fully passing | 1 of 2 |", text)
        self.assertIn("| Assertions passed | 23 of 23 (100.0%) |", text)
        self.assertIn("| Suites cut short by a top-level error | 1 |", text)
        self.assertIn("| Suites not reaching a tally | 0 |", text)
        self.assertIn("## Cut short by a top-level error (1)", text)
        self.assertIn("| cut | 16 of 16 passed | `%s` |" % ERROR, text)
        self.assertIn("| cut | truncated | 16 | 16 |", text)

    def test_a_clean_lane_has_no_truncation_section(self):
        text = self.render({"clean": "7 tests passed\n"})
        self.assertIn("| Suites fully passing | 1 of 1 |", text)
        self.assertIn("| Suites cut short by a top-level error | 0 |", text)
        self.assertNotIn("## Cut short", text)


if __name__ == "__main__":
    unittest.main()
