"""Failure paths of check_gc_trace_names.py (#623), and the repository passing it."""

import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
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
        self.assertIn("f.rs:3: catch-all arm `_`", found[0])

    def test_a_field_bound_to_an_unused_name_without_a_reason_fails(self):
        found = problems("fn trace(k: &K) {\n    let K { body, resume: _resume, } = k;\n}\n")
        self.assertEqual(len(found), 1)
        self.assertIn("f.rs:2: `field: _` in fn trace without a reason", found[0])

    def test_an_ignored_payload_without_a_reason_fails(self):
        found = problems(
            "fn trace(d: &D) {\n"
            "    match d {\n"
            "        // A number.\n"
            "        D::Count(_) => {}\n"
            "        D::Leaf(_) | D::Other(_, _) => {}\n"
            "    }\n"
            "    for (_scope, env) in envs {}\n"
            "}\n"
        )
        self.assertEqual(len(found), 2)
        self.assertIn("f.rs:5: `_` payload in fn trace without a reason", found[0])
        self.assertIn("f.rs:7: `_` payload in fn trace without a reason", found[1])

    def test_an_ignored_payload_with_a_reason_passes(self):
        self.assertEqual(
            problems(
                "fn trace(d: &D, v: &mut V) {\n"
                "    match d {\n"
                "        D::Leaf(_) => {} // A name.\n"
                "        // A count.\n"
                "        D::Count(_) => {}\n"
                "    }\n"
                "    // Each entry is a scope and an environment.\n"
                "    for (_, env) in envs { v.visit_env(env); }\n"
                "}\n"
            ),
            [],
        )

    def test_a_binding_arm_fails(self):
        found = problems("fn trace(d: &D) {\n    match d {\n        other => {}\n    }\n}\n")
        self.assertEqual(len(found), 1)
        self.assertIn("f.rs:3: catch-all arm `other`", found[0])

    def test_a_guarded_catch_all_arm_fails(self):
        found = problems(
            "fn trace(d: &D) {\n"
            "    match d {\n"
            "        D::A(x) => {}\n"
            "        _ if true => {}\n"
            "        rest if rest.is_leaf() => {}\n"
            "    }\n"
            "}\n"
        )
        self.assertEqual(len(found), 2)
        self.assertIn("f.rs:4: catch-all arm `_`", found[0])
        self.assertIn("f.rs:5: catch-all arm `rest`", found[1])

    def test_variant_and_literal_arms_pass(self):
        self.assertEqual(
            problems(
                "fn trace(d: &D, b: bool) {\n"
                "    match d {\n"
                "        D::A(x) if x.is_live() => {}\n"
                "        D::B { body, flag } => {}\n"
                "        D::C | D::E => {}\n"
                "    }\n"
                "    match b { true => {}, false => {} }\n"
                "}\n"
            ),
            [],
        )

    def test_indexing_by_the_full_range_is_not_a_rest_pattern(self):
        self.assertEqual(
            problems(
                "fn trace(v: &mut V, xs: &[T], k: &K) {\n"
                "    v.visit_slice(&xs[..]);\n"
                "    v.visit_slice(&k.values()[..]);\n"
                "    v.visit_slice(&grid[0][..]);\n"
                "}\n"
            ),
            [],
        )

    def test_a_slice_rest_pattern_still_fails(self):
        found = problems(
            "fn trace(xs: &[T]) {\n    let [first, ..] = xs;\n    if let [..] = xs {}\n}\n"
        )
        self.assertEqual(len(found), 2)
        self.assertIn("f.rs:2:", found[0])
        self.assertIn("f.rs:3:", found[1])

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

    def test_a_root_provider_outside_the_list_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            src = root / "crates" / "a" / "src"
            (src / "gc").mkdir(parents=True)
            (src / "lib.rs").write_text(
                "mod gc;\n"
                "mod listed;\n"
                "#[cfg(test)]\n"
                "mod sentinel_tests;\n"
                "impl patina_core::GcRoots for Missed {\n"
                "    fn trace_roots(&self, v: &mut V) {}\n"
                "}\n"
                "#[cfg(test)]\n"
                "mod tests {\n"
                "    impl GcRoots for Inline {}\n"
                "}\n"
                "impl<'a> GcRoots for AfterTests<'a> {}\n"
            )
            (src / "listed.rs").write_text("impl GcRoots for Listed {}\n")
            (src / "sentinel_tests.rs").write_text("impl GcRoots for Only {}\n")
            (src / "gc.rs").write_text("#[cfg(test)]\n#[allow(dead_code)]\nmod roots;\n")
            (src / "gc" / "roots.rs").write_text("impl GcRoots for Nested {}\n")
            found = check.unchecked_root_providers(
                root, {"crates/a/src/listed.rs": ["trace_roots"]}
            )
        self.assertEqual(len(found), 2, found)
        self.assertIn("crates/a/src/lib.rs:5: a root provider the check does not read", found[0])
        self.assertIn("crates/a/src/lib.rs:12:", found[1])

    def test_the_repository_passes(self):
        with contextlib.redirect_stdout(io.StringIO()) as out:
            status = check.main(["check_gc_trace_names.py"])
        self.assertEqual(status, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
