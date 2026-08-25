#!/usr/bin/env python3
"""Render a Larceny-suite run as a report organised by kind of problem.

Called by scripts/run_larceny_tests.sh after a lane has run; reads that lane's
per-suite logs and the suite sources in the reference checkout, and writes a
Markdown report. It quotes nothing from the suite — each failing assertion is
a GitHub permalink (pinned commit) to the test case plus the name of the
procedure under test — because the suite is LGPL and the report is tracked.

    larceny_report.py --logs DIR --suites DIR --lane r7rs|r6rs --commit SHA \
                      --backend NAME --out FILE
"""

import argparse
import os
import re
import sys
from collections import OrderedDict

REPO = "https://github.com/larcenists/larceny/blob/{commit}/test/R7RS/Lib/{path}#L{line}"

STATUS_ORDER = ["not-bundled", "crash", "timeout", "load-error", "fail", "pass"]
BINDING_FORMS = {"let", "let*", "letrec", "letrec*", "let-values", "let*-values",
                 "begin", "lambda", "do", "guard", "call-with-current-continuation",
                 "call/cc", "dynamic-wind", "parameterize", "if", "cond", "case",
                 "when", "unless", "and", "or", "quote", "test", "not"}


def read_log(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


def parse_log(text):
    """Return (status, passed, total, detail, failing_expressions)."""
    passed = re.findall(r"(?m)^(\d+) tests passed$", text)
    failed = re.findall(r"(?m)^(\d+) of (\d+) tests failed\.$", text)
    exprs = []
    # Blocks are "Expression:\n <expr...>\nResult:" — the expression may span lines.
    for m in re.finditer(r"(?ms)^Expression:\n(.*?)\nResult:", text):
        exprs.append(" ".join(m.group(1).split()))
    first_error = next((l for l in text.splitlines() if l.startswith("Error")), "")
    if "overflowed its stack" in text:
        return "crash", 0, 0, "stack overflow", exprs
    if passed:
        return "pass", int(passed[-1]), int(passed[-1]), "", exprs
    if failed:
        n, t = int(failed[-1][0]), int(failed[-1][1])
        return "fail", t - n, t, "", exprs
    m = re.search(r"Library \(([^)]*)\) not found", first_error)
    if m:
        return "not-bundled", 0, 0, "(" + m.group(1) + ")", exprs
    if "Abort trap" in text or "signal" in text:
        return "crash", 0, 0, "aborted", exprs
    if not text.strip() or "Running tests" in text and not first_error:
        # Output stopped without a tally and without an error: a hang the
        # runner's alarm cut short.
        return "timeout", 0, 0, "no result before the timeout", exprs
    return "load-error", 0, 0, first_error, exprs


class Source:
    """A suite's source files with whitespace stripped, for locating a
    written-out expression back to a file and line."""

    def __init__(self, files):
        self.parts = []  # (path, stripped_text, offset->line table)
        for path in files:
            try:
                with open(path, encoding="utf-8", errors="replace") as f:
                    text = f.read()
            except OSError:
                continue
            stripped = []
            lines = []
            line = 1
            for ch in text:
                if ch == "\n":
                    line += 1
                if not ch.isspace():
                    stripped.append(ch)
                    lines.append(line)
            self.parts.append((path, "".join(stripped), lines))
        self.cursor = {p: 0 for p, _, _ in self.parts}

    def locate(self, expr):
        """Best-effort (path, line) for an expression, searching forward from
        the previous hit so repeated shapes resolve in suite order."""
        key = "".join(expr.split())
        for n in (80, 50, 30, 18):
            needle = key[:n]
            if len(needle) < 8:
                break
            for path, text, lines in self.parts:
                start = self.cursor[path]
                i = text.find(needle, start)
                if i < 0 and start:
                    i = text.find(needle)
                if i >= 0:
                    self.cursor[path] = i + 1
                    return path, lines[i]
        # Fallback: the first two operator names in order, anywhere after the
        # cursor — for expressions whose written form differs from the source
        # spelling (a character literal written as a glyph, say).
        ops = [s for s in re.findall(r"\(([^\s()\"']+)", expr) if len(s) > 1][:2]
        if ops:
            pat = re.compile(r"\(" + re.escape(ops[0]) + (r".{0,120}?\(" + re.escape(ops[1]) if len(ops) > 1 else ""))
            for path, text, lines in self.parts:
                m = pat.search(text, self.cursor[path]) or pat.search(text)
                if m:
                    self.cursor[path] = m.start() + 1
                    return path, lines[m.start()]
        return None, None


def head_symbols(expr):
    """The procedure under test: the first one or two operator names that are
    not binding/control forms."""
    syms = re.findall(r"\(([^\s()\"']+)", expr)
    picked = []
    for s in syms:
        if s in BINDING_FORMS or len(s) < 2:
            continue
        if s not in picked:
            picked.append(s)
        if len(picked) == 2:
            break
    return " ".join("`%s`" % s for s in picked) if picked else "`" + expr[:24] + "`"


def suite_files(suites_dir, lane, suite):
    base = os.path.join(suites_dir, "tests", "scheme" if lane == "r7rs" else "r6rs")
    sld = os.path.join(base, suite + ".sld")
    files = [sld]
    d = os.path.dirname(sld)
    stem = os.path.basename(suite)
    try:
        for name in sorted(os.listdir(d)):
            if name != stem + ".sld" and name.startswith(stem) and name.endswith(".scm"):
                files.append(os.path.join(d, name))
    except OSError:
        pass
    return files


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--logs", required=True)
    ap.add_argument("--suites", required=True, help="the test/R7RS/Lib directory")
    ap.add_argument("--lane", choices=["r7rs", "r6rs"], required=True)
    ap.add_argument("--commit", required=True)
    ap.add_argument("--backend", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--generated", default="")
    args = ap.parse_args()

    rows = []
    for name in sorted(os.listdir(args.logs)):
        if not name.endswith(".txt"):
            continue
        suite = name[:-4].replace("_", "/", 1) if args.lane == "r6rs" and name.startswith(("arithmetic_", "io_")) else name[:-4]
        status, passed, total, detail, exprs = parse_log(read_log(os.path.join(args.logs, name)))
        # Patina names files by absolute path; keep the local checkout out of
        # a tracked file.
        detail = detail.replace(os.path.abspath(args.suites) + "/", "").replace(os.path.expanduser("~") + "/", "~/")
        links = []
        if status == "fail":
            src = Source(suite_files(args.suites, args.lane, suite))
            for e in exprs:
                path, line = src.locate(e)
                if path:
                    rel = os.path.relpath(path, args.suites)
                    url = REPO.format(commit=args.commit, path=rel, line=line)
                    links.append("[%s:%d](%s) — %s" % (os.path.basename(path), line, url, head_symbols(e)))
                else:
                    links.append("(not located) — %s" % head_symbols(e))
        rows.append((suite, status, passed, total, detail, links))

    by = OrderedDict((s, []) for s in STATUS_ORDER)
    for r in rows:
        by[r[1]].append(r)
    n_suites = len(rows)
    n_clean = len(by["pass"])
    tot_p = sum(r[2] for r in rows)
    tot_t = sum(r[3] for r in rows)
    pct = "%.1f%%" % (100.0 * tot_p / tot_t) if tot_t else "n/a"
    lane_desc = {"r7rs": "tests/scheme (R7RS-small + Red edition)",
                 "r6rs": "tests/r6rs ((r6rs …) emulation libraries)"}[args.lane]

    out = []
    w = out.append
    w("# Patina vs Larceny's R7RS test suite — by kind of problem\n")
    w("**Generated:** %s  " % args.generated)
    w("**Backend:** %s  " % args.backend)
    w("**Lane:** %s  " % lane_desc)
    w("**Suite:** larcenists/larceny @ `%s` — not vendored (LGPL); see `scripts/run_larceny_tests.sh`\n" % args.commit[:12])
    w("This report quotes nothing from the suite. Each failing assertion is a permalink to the test case at the pinned commit, with the procedure under test; the per-suite logs beside this file (untracked) have the full text.\n")
    w("| | |\n|---|---|")
    w("| Suites fully passing | %d of %d |" % (n_clean, n_suites))
    w("| Assertions passed | %d of %d (%s) |" % (tot_p, tot_t, pct))
    w("| Suites not reaching a tally | %d |\n" % (n_suites - n_clean - len(by["fail"])))
    w("A suite that cannot load reaches no tally, so the assertion total under-reports exactly as much as is broken; the suite line is the one to watch.\n")

    if by["not-bundled"]:
        w("## Library under test not bundled (%d)\n" % len(by["not-bundled"]))
        w("Bundling work, not defects — each is a Red-edition library Patina does not ship yet.\n")
        w("| Suite | Missing |\n|---|---|")
        for s, _, _, _, d, _ in by["not-bundled"]:
            w("| %s | `%s` |" % (s, d))
        w("")
    crashed = by["crash"] + by["timeout"]
    if crashed:
        w("## Crashed or hung (%d)\n" % len(crashed))
        w("No tally was reached: the process died, or the runner's timeout cut it off. A crash is a defect in Patina's runtime; a timeout may be one, or may be a suite that needs longer than the budget on this backend — the triage doc says which for each.\n")
        w("| Suite | What |\n|---|---|")
        for s, st, _, _, d, _ in crashed:
            w("| %s | %s |" % (s, d))
        w("")
    if by["load-error"]:
        w("## Failed to load (%d)\n" % len(by["load-error"]))
        w("The suite's library did not compile, so nothing in it ran. Patina's message:\n")
        w("| Suite | Message |\n|---|---|")
        for s, _, _, _, d, _ in by["load-error"]:
            w("| %s | `%s` |" % (s, d.replace("|", "\\|")[:160]))
        w("")
    if by["fail"]:
        n_fail = sum(r[3] - r[2] for r in by["fail"])
        w("## Assertion failures (%d in %d suites)\n" % (n_fail, len(by["fail"])))
        w("Each entry links to the test case; the name after it is the procedure the assertion exercises.\n")
        for s, _, p, t, _, links in by["fail"]:
            w("### %s — %d of %d failed\n" % (s, t - p, t))
            for l in links:
                w("- " + l)
            w("")
    w("## All suites\n")
    w("| Suite | Status | Passed | Total |\n|---|---|---|---|")
    for s, st, p, t, d, _ in rows:
        w("| %s | %s | %d | %d |" % (s, st, p, t))
    w("")
    with open(args.out, "w", encoding="utf-8") as f:
        f.write("\n".join(out))
    print("wrote %s" % args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
