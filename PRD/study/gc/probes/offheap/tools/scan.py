#!/usr/bin/env python3
"""Rough handle-hazard scanner for patina (read-only).

For every non-test Rust function under crates/*/src, find:
  - allocation call sites (direct Heap::alloc_* etc., plus helpers discovered
    transitively: functions whose body contains an allocation call),
  - whether the function has >= 2 allocation sites, or an allocation inside a
    loop / iterator closure (a proxy for "holds a TaggedValue across an
    allocation"),
  - whether it holds a Vec<TaggedValue> / raw_bits-keyed map in a local,
  - whether it calls back into evaluation (apply/eval/run_loop/call_procedure)
    while also allocating.
Prints per-crate tables and a sample of hits.
"""
import os, re, sys, collections, json

ROOT = sys.argv[1]
SKIP_CRATES = {"patina-tests", "patina-compat"}

DIRECT = r"(alloc_[a-z_]+|intern_symbol|list_reverse|list_append|values_from|core_syntax|alloc_source_identifier|map_syntax_identifiers(?:_memo)?)"

def strip_tests(src):
    # cut at the first `#[cfg(test)]` that introduces a module
    m = re.search(r"#\[cfg\(test\)\]\s*(pub\s+)?mod\s+\w+", src)
    return src[:m.start()] if m else src

def strip_comments(src):
    src = re.sub(r"//[^\n]*", lambda m: " " * len(m.group(0)), src)
    src = re.sub(r"/\*.*?\*/", lambda m: re.sub(r"[^\n]", " ", m.group(0)), src, flags=re.S)
    # blank string literals (rough)
    src = re.sub(r'"(?:\\.|[^"\\])*"', lambda m: '"' + " " * (len(m.group(0)) - 2) + '"', src)
    return src

FN_RE = re.compile(r"\bfn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*(<[^>{]*>)?\s*\(")

def functions(src):
    """Yield (name, start, body_start, body_end) for each fn with a body."""
    for m in FN_RE.finditer(src):
        # find the opening brace of the body (skip signature)
        i = m.end()
        depth = 1
        while i < len(src) and depth:
            if src[i] == '(':
                depth += 1
            elif src[i] == ')':
                depth -= 1
            i += 1
        # signature continues until '{' or ';'
        j = i
        while j < len(src) and src[j] not in '{;':
            j += 1
        if j >= len(src) or src[j] == ';':
            continue
        # match braces
        k = j + 1
        depth = 1
        while k < len(src) and depth:
            c = src[k]
            if c == '{':
                depth += 1
            elif c == '}':
                depth -= 1
            k += 1
        yield m.group(1), m.start(), j, k, src[m.start():j]

def line_of(src, pos):
    return src.count("\n", 0, pos) + 1

LOOP_RE = re.compile(r"\b(for\s+[^{]*\bin\b[^{]*\{|while\b[^{]*\{|loop\s*\{|\.(map|for_each|fold|filter_map|flat_map|try_for_each|try_fold|rev\(\)\.map)\s*\()")
CALLBACK_RE = re.compile(r"\b(apply_procedure|call_procedure|call_any_sync|run_thunk|execute_nested|run_loop_until|eval_in_env|apply_from_direct\w*|run_trampoline|\.eval\(|evaluate\w*\(|import\(|load_library\w*\(|desugar\w*\()")
VEC_TV_RE = re.compile(r"(Vec<TaggedValue>|SmallVec<\[TaggedValue|HashMap<u64,\s*TaggedValue>|HashSet<u64>|FxHashMap<u64|\.raw_bits\(\)|\.raw\(\))")

def main():
    files = []
    for crate in sorted(os.listdir(os.path.join(ROOT, "crates"))):
        if crate in SKIP_CRATES:
            continue
        srcdir = os.path.join(ROOT, "crates", crate, "src")
        for dp, dn, fn in os.walk(srcdir):
            if "/tests" in dp or dp.endswith("tests"):
                continue
            for f in fn:
                if f.endswith(".rs") and not f.endswith("_tests.rs") and f != "tests.rs":
                    files.append((crate, os.path.join(dp, f)))
    fndata = []
    for crate, path in files:
        raw = open(path).read()
        src = strip_comments(strip_tests(raw))
        for name, start, bstart, bend, sig in functions(src):
            body = src[bstart:bend]
            fndata.append(dict(crate=crate, path=os.path.relpath(path, ROOT), name=name,
                               line=line_of(src, start), body=body, bstart=bstart, sig=sig))
    # helper discovery: functions (by name) whose body directly allocates and
    # that take a heap (&mut Heap / SharedHeap / heap param) -> treat as allocating
    alloc_names = set()
    direct = re.compile(r"\." + DIRECT + r"\(")
    for f in fndata:
        if direct.search(f["body"]):
            alloc_names.add(f["name"])
    # iterate transitive closure a few rounds (name-based; over-approximate)
    common = {"new", "from", "default", "clone", "get", "set", "call", "apply", "run", "eval",
              "fmt", "drop", "len", "push", "insert", "map", "into", "to_string", "next", "with"}
    for _ in range(4):
        names_re = re.compile(r"\b(" + "|".join(sorted(re.escape(n) for n in alloc_names - common)) + r")\s*\(")
        added = 0
        for f in fndata:
            if f["name"] in alloc_names:
                continue
            if names_re.search(f["body"]):
                alloc_names.add(f["name"]); added += 1
        if not added:
            break
    names_re = re.compile(r"(?<![a-zA-Z0-9_])(?:\.)?(" + "|".join(sorted(re.escape(n) for n in alloc_names - common)) + r")\s*\(")
    stats = collections.defaultdict(lambda: collections.Counter())
    samples = collections.defaultdict(list)
    for f in fndata:
        body = f["body"]
        dsites = [m.start() for m in direct.finditer(body)]
        isites = [m.start() for m in names_re.finditer(body) if m.start() not in dsites]
        sites = sorted(set(dsites) | set(isites))
        c = stats[f["crate"]]
        c["fns"] += 1
        if not sites:
            continue
        c["alloc_fns"] += 1
        c["direct_sites"] += len(dsites)
        c["all_sites"] += len(sites)
        in_loop = False
        for lm in LOOP_RE.finditer(body):
            # crude: alloc after a loop opener within ~ the loop's extent
            # find extent of block/closure starting at lm.end()
            k = lm.end(); depth = 1
            opener = body[lm.end()-1]
            close = '}' if opener == '{' else ')'
            while k < len(body) and depth:
                if body[k] == opener: depth += 1
                elif body[k] == close: depth -= 1
                k += 1
            if any(lm.end() <= s < k for s in sites):
                in_loop = True; break
        multi = len(sites) >= 2
        holds = bool(VEC_TV_RE.search(body))
        cb = bool(CALLBACK_RE.search(body))
        if multi: c["multi_alloc_fns"] += 1
        if in_loop: c["loop_alloc_fns"] += 1
        if multi or in_loop: c["hazard_fns"] += 1
        if (multi or in_loop) and holds: c["hazard_with_vec_or_rawbits"] += 1
        if cb: c["alloc_and_callback_fns"] += 1
        if multi or in_loop:
            samples[f["crate"]].append((f["path"], f["line"], f["name"], len(sites), in_loop, holds, cb))
    print(json.dumps({k: dict(v) for k, v in sorted(stats.items())}, indent=1))
    tot = collections.Counter()
    for v in stats.values(): tot.update(v)
    print("TOTAL", dict(tot))
    print("allocating fn names discovered:", len(alloc_names))
    for crate, lst in sorted(samples.items()):
        print(f"\n== {crate}: {len(lst)} hazard-shaped fns; top by sites ==")
        for row in sorted(lst, key=lambda r: -r[3])[:25]:
            print("  %s:%d %s sites=%d loop=%s vec/rawbits=%s callback=%s" % row)

main()
