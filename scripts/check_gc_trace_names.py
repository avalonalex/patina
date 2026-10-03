#!/usr/bin/env python3
"""Check that the collector's trace code names every field (#623).

Every trace function and root provider takes its struct apart by name, so a
field added later is error E0027 until someone decides how it is traced. A
rest pattern would undo that silently, and so would a catch-all match arm for
a new enum variant. A field or payload that is deliberately not traced is
written `field: _` or `Variant(_)`, and its reason belongs beside it.

For each function in TRACE_FUNCTIONS this fails on:
- a rest pattern, `..` closing a struct, tuple or slice pattern
  (`{ .. }`, `{ a, .. }`, `Some(..)`, `[a, ..]`). A range has an operand on
  one side and does not match, nor does indexing by the full range
  (`xs[..]`). A call with the full range, `f(..)`, reads as a tuple pattern
  and does match: pass a named range or the slice itself;
- a catch-all arm: `_` or a lone binding before `=>`, with or without a guard
  (`_ =>`, `other =>`, `_ if done =>`);
- an ignored field (`field: _`, `field: _name`) or positional payload
  (`Variant(_)`, `(_, x)`, `(_name, x)`) with no `//` comment on its line or
  on the line above it;
- a listed function that is not found, so a rename cannot drop it from the
  check unnoticed.

A function named here is checked wherever it is defined in its file, every
`impl` included. A new trace function or root provider is added here, and its
struct gets a sentinel test (`docs/GC_DESIGN.md` §5.4, AGENTS.md "New heap
object type"). So that a root provider cannot be left out, the check also
fails on an `impl GcRoots for` under crates/*/src, outside `#[cfg(test)]`
code, whose file is not listed here with `trace_roots`.

    check_gc_trace_names.py [REPO_ROOT]

Run by the Clippy job in .github/workflows/ci.yml; tested offline by
scripts/tests/test_check_gc_trace_names.py.
"""

import re
import sys
from pathlib import Path

# File (relative to the repository root) -> the trace functions in it.
TRACE_FUNCTIONS = {
    "crates/patina-core/src/heap/gc.rs": [
        "visit_env",
        "visit_env_chain",
        "visit_promise",
        "visit_wind",
        "visit_wind_with",
        "visit_winds",
        "visit_winds_with",
        "visit_library",
        "drain",
        "trace_children",
        "trace_object_children",
        "trace_compiled_macro",
        "trace_continuation_children",
        "run_mark_phase",
        "trace_cont_env",
        "trace_cont_value",
        "trace_prompt_frame",
        "trace_exception_handler",
    ],
    "crates/patina-core/src/environment.rs": ["for_each_gc_edge"],
    "crates/patina-core/src/library.rs": ["for_each_gc_edge"],
    "crates/patina-vm/src/runtime/vm_state/gc_roots.rs": [
        "trace_roots",
        "trace_weak_ids",
        "trace_code",
        "trace_frames",
        "trace_winds",
        "trace_prompts",
        "trace_handlers",
        "trace_handler",
        "trace_continuation",
        "trace_delimited_continuation",
    ],
    "crates/patina-vm/src/runtime/execution_state.rs": ["trace_roots"],
    "crates/patina-vm/src/tracer.rs": ["trace_roots"],
    "crates/patina-runtime/src/library_registry.rs": ["trace_roots"],
    "crates/patina-tree-walker/src/eval/cps_eval/gc_roots.rs": [
        "trace_roots",
        "trace_step",
        "trace_stacks",
    ],
    "crates/patina-tree-walker/src/eval/cps_eval/types.rs": ["trace_pending_escape"],
}

# `..` closing a pattern: after `{`, `(`, `[` or `,`, and before `}`, `)`,
# `]` or `,`, across line breaks. A range (`a..b`, `xs[i..]`) has an operand
# on one side and does not match. Neither does `xs[..]`, set apart by what
# precedes its `[` (INDEXED).
REST_PATTERN = re.compile(r"[{(\[,]\s*\.\.\s*[})\],]")
INDEXED = re.compile(r"[\w)\]]")
# An arm whose pattern is `_` or a lone binding, which matches every variant,
# a new one included. `true` and `false` are literals, not bindings.
CATCH_ALL_ARM = re.compile(
    r"(?:^|[{,|])\s*(?:ref\s+)?(?:mut\s+)?([a-z_][A-Za-z0-9_]*)\s*(?:\bif\b.*?)?=>"
)
LITERALS = {"true", "false"}
# A field matched by `_` or by a binding it then ignores (`_name`).
IGNORED_FIELD = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\s*:\s*_\w*\s*[,})]")
# A positional payload matched by `_` or `_name`: `Variant(_)`, `(_, x)`.
IGNORED_PAYLOAD = re.compile(r"[(\[,]\s*_\w*\s*[)\],]")
STRING = re.compile(r'"(?:\\.|[^"\\])*"')


def code_of(line):
    """The line without its string literals and its `//` comment."""
    return STRING.sub('""', line).split("//", 1)[0]


def has_comment(line):
    return "//" in STRING.sub('""', line)


def block_end(lines, i):
    """The 0-based line on which the block that opens on or after line `i`
    closes, or None when a `;` ends the item first (a declaration without a
    body)."""
    depth = 0
    opened = False
    for j in range(i, len(lines)):
        code = code_of(lines[j])
        for ch in code:
            if ch == "{":
                depth += 1
                opened = True
            elif ch == "}":
                depth -= 1
        if opened and depth == 0:
            return j
        if not opened and code.rstrip().endswith(";"):
            return None
    return len(lines) - 1 if opened else None


def function_bodies(lines, name):
    """(first, last) 0-based line ranges of each `fn name` body in `lines`."""
    start = re.compile(r"\bfn\s+" + re.escape(name) + r"\b\s*[<(]")
    bodies = []
    i = 0
    while i < len(lines):
        if not start.search(code_of(lines[i])):
            i += 1
            continue
        last = block_end(lines, i)
        if last is None:
            # A trait's declaration: skip to its `;`.
            while i < len(lines) and not code_of(lines[i]).rstrip().endswith(";"):
                i += 1
            i += 1
            continue
        bodies.append((i, last))
        i = last + 1
    return bodies


def without_reason(lines, index):
    """Whether line `index` has no `//` comment and follows no comment line."""
    above = lines[index - 1].strip() if index > 0 else ""
    return not has_comment(lines[index]) and not above.startswith("//")


def check_source(path, text, names):
    """Every problem in the listed functions of one file, as messages."""
    lines = text.split("\n")
    problems = []
    for name in names:
        bodies = function_bodies(lines, name)
        if not bodies:
            problems.append(f"{path}: fn {name} not found; update TRACE_FUNCTIONS")
            continue
        for first, last in bodies:
            code = [code_of(line) for line in lines[first : last + 1]]
            joined = "\n".join(code)
            for match in REST_PATTERN.finditer(joined):
                at = match.start()
                if joined[at] == "[" and at > 0 and INDEXED.match(joined[at - 1]):
                    continue
                line = first + joined.count("\n", 0, at) + 1
                problems.append(
                    f"{path}:{line}: `..` in fn {name}: name every field, "
                    "and write a deliberate non-trace `field: _` with its reason"
                )
            for offset, line_code in enumerate(code):
                number = first + offset + 1
                for arm in CATCH_ALL_ARM.finditer(line_code):
                    if arm.group(1) not in LITERALS:
                        problems.append(
                            f"{path}:{number}: catch-all arm `{arm.group(1)}` in fn {name}: "
                            "match every variant"
                        )
                if IGNORED_FIELD.search(line_code) and without_reason(lines, first + offset):
                    problems.append(
                        f"{path}:{number}: `field: _` in fn {name} without a reason: "
                        "say in a comment on its line or the line above what it holds "
                        "instead of a value, or which test pins it"
                    )
                if IGNORED_PAYLOAD.search(line_code) and without_reason(lines, first + offset):
                    problems.append(
                        f"{path}:{number}: `_` payload in fn {name} without a reason: "
                        "say in a comment on its line or the line above what it holds "
                        "instead of a value"
                    )
    return problems


GC_ROOTS_IMPL = re.compile(r"\bimpl\b.*\bGcRoots\s+for\b")
CFG_TEST = re.compile(r"^\s*#\[cfg\(test\)\]\s*$")
MODULE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*([;{])")


def test_modules(lines):
    """The `#[cfg(test)]` modules of one file: the (first, last) line ranges
    of those written inline, and the names of those declared `mod name;`."""
    inline = []
    declared = []
    for i, line in enumerate(lines):
        if not CFG_TEST.match(line):
            continue
        j = i + 1
        while j < len(lines) and (
            not lines[j].strip() or lines[j].strip().startswith(("#[", "//"))
        ):
            j += 1
        module = MODULE.match(code_of(lines[j])) if j < len(lines) else None
        if module is None:
            continue
        if module.group(2) == ";":
            declared.append(module.group(1))
        else:
            last = block_end(lines, j)
            inline.append((j, last if last is not None else j))
    return inline, declared


def module_paths(path, name):
    """The files `mod name;` in `path` may name, and the directory of its
    submodules."""
    if path.name in ("mod.rs", "lib.rs", "main.rs"):
        base = path.parent
    else:
        base = path.parent / path.stem
    return {base / f"{name}.rs", base / name / "mod.rs"}, base / name


def unchecked_root_providers(root, trace_functions):
    """Every `impl GcRoots for` under crates/*/src, outside test code, whose
    file TRACE_FUNCTIONS does not list with `trace_roots`, as messages."""
    sources = sorted(root.glob("crates/*/src/**/*.rs"))
    texts = {path: path.read_text().split("\n") for path in sources}
    test_files = set()
    test_dirs = []
    for path, lines in texts.items():
        for name in test_modules(lines)[1]:
            files, directory = module_paths(path, name)
            test_files |= files
            test_dirs.append(directory)
    problems = []
    for path, lines in texts.items():
        if path in test_files or any(d in path.parents for d in test_dirs):
            continue
        relative = path.relative_to(root).as_posix()
        if "trace_roots" in trace_functions.get(relative, ()):
            continue
        inline = test_modules(lines)[0]
        for i, line in enumerate(lines):
            if GC_ROOTS_IMPL.search(code_of(line)) and not any(
                first <= i <= last for first, last in inline
            ):
                problems.append(
                    f"{relative}:{i + 1}: a root provider the check does not read: "
                    "add its file to TRACE_FUNCTIONS with trace_roots"
                )
    return problems


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(__file__).resolve().parents[1]
    problems = []
    for relative, names in TRACE_FUNCTIONS.items():
        path = root / relative
        if not path.is_file():
            problems.append(f"{relative}: file not found; update TRACE_FUNCTIONS")
            continue
        problems.extend(check_source(relative, path.read_text(), names))
    problems.extend(unchecked_root_providers(root, TRACE_FUNCTIONS))
    for problem in problems:
        print(problem)
    if problems:
        print(f"check_gc_trace_names: {len(problems)} problem(s) in the trace code (#623)")
        return 1
    count = sum(len(names) for names in TRACE_FUNCTIONS.values())
    print(f"check_gc_trace_names: {count} trace functions name every field")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
