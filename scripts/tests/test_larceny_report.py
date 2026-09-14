"""Status and summary checks for larceny_report.py; no Larceny checkout required."""

import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "larceny_report", Path(__file__).resolve().parents[1] / "larceny_report.py"
)
report = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(report)

ERROR = "Error: runtime error: Type error: car: expected pair, got ()"
FAILURE = "Expression:\n (car '(1))\nResult:\n 2\nExpected:\n 1\n\n"


def trailer(rc):
    """The line scripts/run_larceny_tests.sh appends once the suite exits."""
    return "--- run_larceny_tests.sh: exit status %d ---\n" % rc


class ParseLogTests(unittest.TestCase):
    def test_a_clean_tally_is_a_pass(self):
        self.assertEqual(
            report.parse_log("Running tests for (demo)\n12 tests passed\n" + trailer(0)),
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

    def test_an_error_without_a_tally_is_a_load_error(self):
        self.assertEqual(report.parse_log(ERROR + "\n" + trailer(1))[0], "load-error")
        self.assertEqual(report.parse_log(ERROR + "\n")[0], "load-error")

    def test_an_error_inside_a_result_is_not_a_top_level_error(self):
        # A written symbol can carry a raw carriage return. Only a line that
        # starts with "Error" at a real line break is a top-level error.
        log = "Expression:\n (f)\nResult:\n |x\rError|\nExpected:\n y\n\n1 of 3 tests failed.\n"
        self.assertEqual(report.parse_log(log + trailer(0))[:3], ("fail", 2, 3))

    def test_the_exit_status_decides_a_timeout(self):
        # The alarm fired after a top-level error: that is a timeout, as the
        # runner always reported it, not a load error.
        log = "Running tests for (demo)\n%s\n%s" % (ERROR, trailer(142))
        self.assertEqual(report.parse_log(log)[:4], ("timeout", 0, 0, "no result before the timeout"))

    def test_the_exit_status_decides_a_crash(self):
        self.assertEqual(
            report.parse_log("Running tests for (demo)\n" + trailer(139))[:4],
            ("crash", 0, 0, "signal 11"),
        )
        panic = "Running tests for (demo)\nthread 'main' panicked at src/x.rs:1:1:\nboom\n"
        self.assertEqual(report.parse_log(panic + trailer(101))[:4], ("crash", 0, 0, "panic (exit 101)"))

    def test_a_silent_stop_is_a_timeout_only_without_an_exit_status(self):
        self.assertEqual(report.parse_log("Running tests for (demo)\n")[0], "timeout")
        self.assertEqual(report.parse_log("Running tests for (demo)\n" + trailer(0))[0], "load-error")


class ClassifyTests(unittest.TestCase):
    def test_classify_prints_one_tab_separated_line(self):
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "set.txt"
            log.write_text("Running tests for (demo)\n%s\n16 tests passed\n%s" % (ERROR, trailer(0)),
                           encoding="utf-8")
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(report.main(["--classify", str(log)]), 0)
        self.assertEqual(out.getvalue(), "truncated\t16\t16\t%s\n" % ERROR)


class CodeCellTests(unittest.TestCase):
    def test_a_plain_message_gets_single_backticks(self):
        self.assertEqual(report.code_cell("Error: boom"), "`Error: boom`")

    def test_a_backticked_message_gets_a_longer_fence(self):
        # The VM's unbound-variable message names the variable between backticks.
        self.assertEqual(report.code_cell("Error: unbound variable: `x`"),
                         "`` Error: unbound variable: `x` ``")

    def test_pipes_are_escaped_and_empty_stays_empty(self):
        self.assertEqual(report.code_cell("a | b"), "`a \\| b`")
        self.assertEqual(report.code_cell(""), "")


class SummaryTests(unittest.TestCase):
    def render(self, logs):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            log_dir = root / "logs"
            log_dir.mkdir()
            for name, text in logs.items():
                (log_dir / (name + ".txt")).write_text(text, encoding="utf-8")
            out = root / "report.md"
            argv = ["--logs", str(log_dir), "--suites", str(root / "Lib"), "--lane", "r7rs",
                    "--commit", "0" * 40, "--backend", "VM", "--out", str(out)]
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(report.main(argv), 0)
            return out.read_text(encoding="utf-8")

    def test_a_truncated_suite_is_not_counted_as_fully_passing(self):
        text = self.render({"clean": "7 tests passed\n", "cut": "%s\n16 tests passed\n" % ERROR})
        self.assertIn("| Suites fully passing | 1 of 2 |", text)
        self.assertIn("| Assertions passed | 23 of 23 (100.0%) |", text)
        self.assertIn("| Suites cut short by a top-level error | 1 |", text)
        self.assertIn("| Suites not reaching a tally | 0 |", text)
        self.assertIn("## Cut short by a top-level error (1)", text)
        self.assertIn("| cut | 16 of 16 passed | `%s` |" % ERROR, text)
        self.assertIn("| cut | truncated | 16 | 16 |", text)
        self.assertNotIn("## Assertion failures", text)

    def test_a_truncated_suites_failures_count_under_assertion_failures(self):
        text = self.render({
            "broken": FAILURE + FAILURE + "2 of 10 tests failed.\n",
            "clean": "7 tests passed\n",
            "cut": FAILURE + ERROR + "\n1 of 3 tests failed.\n",
        })
        self.assertIn("| Assertions passed | 17 of 20 (85.0%) |", text)
        self.assertIn("## Assertion failures (3 in 2 suites)", text)
        self.assertIn("### broken — 2 of 10 failed\n", text)
        self.assertIn("### cut — 1 of 3 failed (cut short)\n", text)
        self.assertEqual(text.count("- (not located) — `car`"), 3)
        self.assertIn("| cut | 2 of 3 passed | `%s` |" % ERROR, text)
        self.assertIn("| cut | truncated | 2 | 3 |", text)

    def test_a_clean_lane_has_no_truncation_section(self):
        text = self.render({"clean": "7 tests passed\n"})
        self.assertIn("| Suites fully passing | 1 of 1 |", text)
        self.assertIn("| Suites cut short by a top-level error | 0 |", text)
        self.assertNotIn("## Cut short", text)


if __name__ == "__main__":
    unittest.main()
