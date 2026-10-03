"""Failure paths of check_gc_trace_names.py (#623), and the repository passing it."""

import contextlib
import importlib.util
import io
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location(
    "check_gc_trace_names", Path(__file__).resolve().parents[1] / "check_gc_trace_names.py"
)
check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check)


def problems(source, names=("trace",)):
    return check.check_source("f.rs", source, list(names))


class TraceNamesTests(unittest.TestCase):
    def test_a_full_destructure_with_reasons_passes(self):
        self.assertEqual(
            problems(
                "fn trace(k: &K, v: &mut V) {\n"
                "    let K {\n"
                "        body,\n"
                "        // A name.\n"
                "        param: _,\n"
                "        flag: _, // A flag.\n"
                "    } = k;\n"
                "    for x in &body[1..] { v.visit(*x); }\n"
                "    let _range = 0..n;\n"
                "}\n"
            ),
            [],
        )

    def test_a_rest_pattern_on_one_line_fails(self):
        found = problems("fn trace(k: &K) {\n    let K { body, .. } = k;\n}\n")
        self.assertEqual(len(found), 1)
        self.assertIn("f.rs:2: `..` in fn trace", found[0])

    def test_a_rest_pattern_rustfmt_split_across_lines_fails(self):
        found = problems(
            "fn trace(d: &D) {\n"
            "    match d {\n"
            "        D::Closure {\n"
            "            free_vars, globals, ..\n"
            "        } => {}\n"
            "    }\n"
            "}\n"
        )
        self.assertEqual(len(found), 1)
        self.assertIn("f.rs:4:", found[0])

    def test_a_tuple_rest_pattern_fails(self):
        self.assertEqual(len(problems("fn trace(d: &D) {\n    if let D::A(..) = d {}\n}\n")), 1)

    def test_a_rest_pattern_in_a_comment_or_string_is_not_code(self):
        self.assertEqual(
            problems('fn trace() {\n    // was `D { .. }`\n    let s = "{ .. }";\n}\n'),
            [],
        )

    def test_an_ignored_field_without_a_reason_fails(self):
        found = problems(
            "fn trace(k: &K) {\n"
            "    let K {\n"
            "        // A count.\n"
            "        count: _,\n"
            "        depth: _,\n"
            "    } = k;\n"
            "}\n"
        )
        self.assertEqual(len(found), 1)
        self.assertIn("f.rs:5: `field: _` in fn trace without a reason", found[0])

    def test_a_wildcard_arm_fails(self):
        found = problems("fn trace(d: &D) {\n    match d {\n        _ => {}\n    }\n}\n")
        self.assertEqual(len(found), 1)
        self.assertIn("wildcard arm", found[0])

    def test_a_listed_function_that_is_gone_fails(self):
        found = problems("fn other() {}\n", names=("trace",))
        self.assertEqual(found, ["f.rs: fn trace not found; update TRACE_FUNCTIONS"])

    def test_every_definition_of_a_name_is_checked(self):
        found = problems(
            "impl A for X {\n    fn trace(&self) {\n        let X { a } = self;\n    }\n}\n"
            "impl A for Y {\n    fn trace(&self) {\n        let Y { .. } = self;\n    }\n}\n"
        )
        self.assertEqual(len(found), 1)
        self.assertIn("f.rs:8:", found[0])

    def test_code_after_the_function_is_not_checked(self):
        self.assertEqual(
            problems("fn trace(k: &K) {\n    let K { a } = k;\n}\nfn other(k: &K) { let K { .. } = k; }\n"),
            [],
        )

    def test_the_repository_passes(self):
        with contextlib.redirect_stdout(io.StringIO()) as out:
            status = check.main(["check_gc_trace_names.py"])
        self.assertEqual(status, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
