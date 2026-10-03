#!/usr/bin/env python3
"""Check that the collector's trace code names every field (#623).

Every trace function and root provider takes its struct apart by name, so a
field added later is error E0027 until someone decides how it is traced. A
rest pattern would undo that silently, and so would a wildcard match arm for a
new enum variant. A field that is deliberately not traced is written
`field: _`, and its reason belongs beside it.

For each function in TRACE_FUNCTIONS this fails on:
- a rest pattern, `..` closing a struct, tuple or slice pattern
  (`{ .. }`, `{ a, .. }`, `Some(..)`, `[a, ..]`);
- a wildcard arm, `_ =>`;
- a `field: _` with no `//` comment on its line or on the line above it;
- a listed function that is not found, so a rename cannot drop it from the
  check unnoticed.

A function named here is checked wherever it is defined in its file, every
`impl` included. A new trace function or root provider is added here, and its
struct gets a sentinel test (`docs/GC_DESIGN.md` §5, AGENTS.md "New heap
object type").

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
        "visit_env_edges",
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
# on one side and does not match.
REST_PATTERN = re.compile(r"[{(\[,]\s*\.\.\s*[})\],]")
WILDCARD_ARM = re.compile(r"(?:^|[\s|])_\s*=>")
IGNORED_FIELD = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\s*:\s*_\s*[,})]")
STRING = re.compile(r'"(?:\\.|[^"\\])*"')


def code_of(line):
    """The line without its string literals and its `//` comment."""
    return STRING.sub('""', line).split("//", 1)[0]


def has_comment(line):
    return "//" in STRING.sub('""', line)


def function_bodies(lines, name):
    """(first, last) 0-based line ranges of each `fn name` body in `lines`."""
    start = re.compile(r"\bfn\s+" + re.escape(name) + r"\b\s*[<(]")
    bodies = []
    i = 0
    while i < len(lines):
        if not start.search(code_of(lines[i])):
            i += 1
            continue
        depth = 0
        opened = False
        j = i
        while j < len(lines):
            for ch in code_of(lines[j]):
                if ch == "{":
                    depth += 1
                    opened = True
                elif ch == "}":
                    depth -= 1
            if opened and depth == 0:
                break
            # A declaration without a body (a trait's) ends at its `;`.
            if not opened and code_of(lines[j]).rstrip().endswith(";"):
                break
            j += 1
        if opened:
            bodies.append((i, j))
        i = j + 1
    return bodies


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
                line = first + joined.count("\n", 0, match.start()) + 1
                problems.append(
                    f"{path}:{line}: `..` in fn {name}: name every field, "
                    "and write a deliberate non-trace `field: _` with its reason"
                )
            for offset, line_code in enumerate(code):
                number = first + offset + 1
                if WILDCARD_ARM.search(line_code):
                    problems.append(
                        f"{path}:{number}: wildcard arm in fn {name}: match every variant"
                    )
                if IGNORED_FIELD.search(line_code):
                    here = lines[first + offset]
                    above = lines[first + offset - 1].strip() if offset > 0 else ""
                    if not has_comment(here) and not above.startswith("//"):
                        problems.append(
                            f"{path}:{number}: `field: _` in fn {name} without a reason: "
                            "say in a comment on its line or the line above what it holds "
                            "instead of a value, or which test pins it"
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
