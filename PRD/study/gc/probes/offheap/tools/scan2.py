#!/usr/bin/env python3
"""Second, tighter handle-hazard scan (read-only).

Allocation sites = direct calls of Heap allocation methods, plus a curated
list of helpers verified (by reading) to allocate. No name-based transitive
closure, so the numbers are a floor, not a ceiling.

Per function, classify:
  A  fresh-unrooted: `let x = <...alloc...>` and a later alloc site, with x
     used after that later site  -> unsafe if GC may run inside allocation,
     even with a NON-moving collector (x is reachable only from Rust).
  B  moving-hazard: an alloc site followed by a use of a TaggedValue that was
     obtained before it (function args slice `args`, a TaggedValue-typed
     parameter, or a local initialised from a heap read)  -> unsafe if objects
     can MOVE during the allocation.
  C  collection-held: a Vec/SmallVec/HashMap of TaggedValues or raw-bits keys
     live in the function together with an allocation in a loop/iterator.
  D  callback: allocates AND re-enters evaluation (apply_proc/eval/desugar/
     import/load) in the same function.
"""
import os, re, sys, collections

ROOT = sys.argv[1]
SKIP = {"patina-tests", "patina-compat"}
HELPERS = ["vec_to_list_tagged", "make_list_tagged", "make_list", "build_list",
           "alloc_exception", "circular_list", "list_from_vec", "slice_to_list",
           "values_from", "list_reverse", "list_append", "map_syntax_identifiers",
           "map_syntax_identifiers_memo", "intern_symbol", "core_syntax",
           "alloc_source_identifier", "string_to_symbol_tv", "make_exception", "list_from_iter", "list_from_iter_with_tail","numeric_add","numeric_sub","numeric_mul","numeric_div","numeric_neg","numeric_quotient","numeric_remainder","numeric_modulo","numeric_floor_quotient","numeric_floor_remainder","real_part","imag_part","magnitude","angle","make_rectangular","make_polar","numerator","denominator","to_exact","to_inexact","gcd_many","lcm_many","numeric_floor","numeric_ceiling","numeric_truncate","numeric_round","numeric_abs","numeric_max","numeric_min","numeric_sqrt","numeric_square","numeric_expt","numeric_sin","numeric_cos","numeric_tan","numeric_asin","numeric_acos","numeric_atan","numeric_atan2","numeric_exp","numeric_log","numeric_log_base"]
ALLOC = re.compile(r"\.(alloc_[a-z_0-9]+|" + "|".join(HELPERS) + r")\s*\(|(?<![\w.])(" + "|".join(HELPERS) + r")\s*\(")
HEAPREAD = re.compile(r"\b(car|cdr|get_pair|vector_ref|vector_slice|record_field|get_field|args\s*\[|args\.iter|args\.get|first|second)\b")
CALLBACK = re.compile(r"\b(apply_proc|eval_expr|call_any_sync|run_thunk|execute_nested|run_loop_until|eval_in_env\w*|apply_from_direct\w*|run_trampoline|desugar_with_imports|load_scheme_library|load_library\w*|evaluate_parsed\w*|process_import\w*)\s*\(")
COLL = re.compile(r"(Vec<TaggedValue>|SmallVec<\[TaggedValue|HashMap<u64|FxHashMap<u64|HashSet<u64>|FxHashSet<u64>|\.raw_bits\(\)|\.to_vec\(\)|collect::<Vec<TaggedValue>>|: Vec<_> =)")
LOOP = re.compile(r"\b(for\s+[^{;]*\bin\b[^{;]*\{|while\b[^{;]*\{|loop\s*\{)|\.(map|for_each|fold|filter_map|flat_map|try_for_each|try_fold|scan)\s*\(")

def strip(src):
    m = re.search(r"#\[cfg\(test\)\]\s*(pub\s+)?mod\s+\w+", src)
    if m: src = src[:m.start()]
    src = re.sub(r"//[^\n]*", lambda m: " " * len(m.group(0)), src)
    src = re.sub(r"/\*.*?\*/", lambda m: re.sub(r"[^\n]", " ", m.group(0)), src, flags=re.S)
    src = re.sub(r'"(?:\\.|[^"\\])*"', lambda m: '"' + " " * (len(m.group(0)) - 2) + '"', src)
    return src

FN = re.compile(r"\bfn\s+([A-Za-z_]\w*)\s*(<[^>{]*>)?\s*\(")

def fns(src):
    for m in FN.finditer(src):
        i, d = m.end(), 1
        while i < len(src) and d:
            d += {'(': 1, ')': -1}.get(src[i], 0); i += 1
        params = src[m.end():i-1]
        j = i
        while j < len(src) and src[j] not in '{;': j += 1
        if j >= len(src) or src[j] == ';': continue
        k, d = j + 1, 1
        while k < len(src) and d:
            d += {'{': 1, '}': -1}.get(src[k], 0); k += 1
        yield m.group(1), m.start(), params, src[j:k]

def extent(body, start, opener):
    close = '}' if opener == '{' else ')'
    k, d = start, 1
    while k < len(body) and d:
        if body[k] == opener: d += 1
        elif body[k] == close: d -= 1
        k += 1
    return k

def main():
    per = collections.defaultdict(collections.Counter)
    ex = collections.defaultdict(lambda: collections.defaultdict(list))
    for crate in sorted(os.listdir(os.path.join(ROOT, "crates"))):
        if crate in SKIP: continue
        for dp, _, files in os.walk(os.path.join(ROOT, "crates", crate, "src")):
            if os.path.basename(dp) == "tests": continue
            for f in files:
                if not f.endswith(".rs") or f.endswith("_tests.rs") or f == "tests.rs": continue
                path = os.path.join(dp, f)
                src = strip(open(path).read())
                rel = os.path.relpath(path, ROOT)
                # nested fns are counted inside their parents too; dedupe by
                # attributing a site to the innermost fn only
                spans = list(fns(src))
                for name, start, params, body in spans:
                    line = src.count("\n", 0, start) + 1
                    sites = [m.start() for m in ALLOC.finditer(body)]
                    c = per[crate]
                    c["fns"] += 1
                    if not sites: continue
                    c["alloc_fns"] += 1
                    c["sites"] += len(sites)
                    # A: fresh-unrooted
                    A = False
                    for lm in re.finditer(r"\blet\s+(mut\s+)?([a-z_]\w*)\s*(:[^=]+)?=", body):
                        semi = body.find(";", lm.end())
                        if semi < 0: continue
                        init = body[lm.end():semi]
                        if not ALLOC.search(init): continue
                        var = lm.group(2)
                        later = [s for s in sites if s > semi]
                        if not later: continue
                        if re.search(r"\b" + re.escape(var) + r"\b", body[later[0]+1:]):
                            # exclude the trivial "passed as argument to that very alloc" case
                            A = True; break
                    # B: moving hazard
                    tv_params = [p.split(":")[0].strip().lstrip("mut ").strip()
                                 for p in params.split(",") if "TaggedValue" in p]
                    names = set(tv_params)
                    for lm in re.finditer(r"\blet\s+(mut\s+)?([a-z_]\w*)\s*(:[^=]+)?=", body):
                        semi = body.find(";", lm.end())
                        init = body[lm.end():semi] if semi > 0 else ""
                        if HEAPREAD.search(init) or (lm.group(3) and "TaggedValue" in lm.group(3)):
                            if not ALLOC.search(init):
                                names.add(lm.group(2))
                    B = False
                    first = sites[0]
                    # strict: a use strictly after the END of the statement
                    # holding the first allocation (so the allocation's own
                    # arguments do not count; an allocator can root those).
                    B = False
                    for s0 in sites:
                        end = body.find(";", s0)
                        if end < 0: continue
                        tail = body[end+1:]
                        if any(n and re.search(r"\b" + re.escape(n) + r"\b", tail) for n in names) or re.search(r"\bargs\s*[\[.]", tail):
                            # and another allocation happens after that point too
                            if any(s > end for s in sites) or True:
                                B = True; break
                    # C: collection held with alloc in loop
                    in_loop = False
                    for lm in LOOP.finditer(body):
                        opener = body[lm.end()-1]
                        k = extent(body, lm.end(), opener)
                        if any(lm.end() <= s < k for s in sites):
                            in_loop = True; break
                    C = in_loop and bool(COLL.search(body))
                    D = bool(CALLBACK.search(body))
                    for key, val in (("A_fresh_unrooted", A), ("B_moving", B), ("alloc_in_loop", in_loop),
                                     ("C_collection_held", C), ("D_callback", D)):
                        if val:
                            c[key] += 1
                            ex[crate][key].append(f"{rel}:{line} {name}")
    tot = collections.Counter()
    hdr = ["fns", "alloc_fns", "sites", "A_fresh_unrooted", "B_moving", "alloc_in_loop", "C_collection_held", "D_callback"]
    print("| crate | " + " | ".join(hdr) + " |")
    print("|" + "---|" * (len(hdr) + 1))
    for crate, c in sorted(per.items()):
        tot.update(c)
        print(f"| {crate} | " + " | ".join(str(c[h]) for h in hdr) + " |")
    print("| TOTAL | " + " | ".join(str(tot[h]) for h in hdr) + " |")
    for crate in sorted(ex):
        for key in ("A_fresh_unrooted", "B_moving", "C_collection_held", "D_callback"):
            lst = ex[crate][key]
            if lst:
                print(f"\n{crate} {key} ({len(lst)}):")
                for e in lst[:40]: print("   ", e)

main()
