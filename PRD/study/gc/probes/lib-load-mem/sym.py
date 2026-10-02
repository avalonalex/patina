#!/usr/bin/env python3
"""Symbolize lib-load-mem profiles with atos and aggregate by call site."""
import re, subprocess, sys, collections, json, os

EXE = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'lib-load-mem')
prof = sys.argv[1]
cache_path = prof + ".sym.json"

snaps = collections.OrderedDict()
stacks = {}
with open(prof) as f:
    mode = None
    for line in f:
        p = line.split()
        if p[0] == "SNAP":
            cur = []
            snaps[p[1]] = (int(p[2]), cur)
            mode = "snap"
            continue
        if p[0] == "STACKS":
            mode = "stacks"
            continue
        if mode == "snap":
            cur.append((int(p[0]), int(p[1])))
        else:
            stacks[int(p[0])] = [int(x, 16) for x in p[1:]]

TOK = re.compile(r"(\d+)_?([A-Za-z_][A-Za-z0-9_]*)")


def demangle(sym):
    if not sym.startswith("_R"):
        return sym
    out = []
    i = 0
    s = sym
    while i < len(s):
        m = TOK.match(s, i)
        if m:
            n = int(m.group(1))
            ident_start = m.start(2)
            ident = s[ident_start:ident_start + n]
            if len(ident) == n and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", ident):
                out.append(ident)
                i = ident_start + n
                continue
        i += 1
    return "::".join(out)


if os.path.exists(cache_path):
    sym = {int(k): v for k, v in json.load(open(cache_path)).items()}
else:
    addrs = sorted({a for st in stacks.values() for a in st[1:]})
    sym = {}
    B = 1500
    for i in range(0, len(addrs), B):
        batch = addrs[i:i + B]
        out = subprocess.run(["atos", "-o", EXE, "-l", "0x100000000", "-i"] + [hex(a - 1) for a in batch],
                             capture_output=True, text=True).stdout
        groups = out.split("\n\n")
        groups = [g for g in groups if g.strip() != ""]
        if len(groups) != len(batch):
            # fall back: one call per address
            for a in batch:
                o = subprocess.run(["atos", "-o", EXE, "-l", "0x100000000", "-i", hex(a - 1)], capture_output=True, text=True).stdout
                sym[a] = [l for l in o.strip().split("\n") if l]
            continue
        for a, g in zip(batch, groups):
            sym[a] = [l for l in g.strip().split("\n") if l]
    json.dump(sym, open(cache_path, "w"))


FILT = "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/llvm-cxxfilt"
_dm = {}
def build_demangle():
    names = set()
    for lines in sym.values():
        for line in lines:
            names.add(line.split(" ")[0])
    names = sorted(names)
    out = subprocess.run([FILT], input="\n".join("_" + n for n in names), capture_output=True, text=True).stdout.split("\n")
    for n, d in zip(names, out):
        _dm[n] = d if not d.startswith("__R") else n
build_demangle()
def demangle(n):
    return _dm.get(n, n)

def frames_of(sid):
    res = []
    for a in stacks[sid][1:]:
        for line in sym.get(a, [hex(a)]):
            m = re.match(r"(\S+) \(in [^)]*\) \(([^)]*)\)", line)
            if m:
                res.append((demangle(m.group(1)), m.group(2)))
            else:
                res.append((demangle(line.split(" ")[0]), ""))
    return res

SKIP = ("lib_load_mem", "__rust", "alloc::", "core::", "std::", "hashbrown", "smallvec", "rc::", "raw_vec", "boxed")


def own(fn):
    return fn.lstrip("<").startswith("patina_")
def short(fn):
    fn = re.sub(r"::h[0-9a-f]{16}$", "", fn)
    return fn[:150]
def leaf(frs):
    for fn, loc in frs:
        if own(fn):
            return short(fn) + " @" + loc
    return "?"
def leaf2(frs):
    out = []
    for fn, loc in frs:
        if own(fn):
            out.append(short(fn).split("::")[-1] + "@" + loc)
            if len(out) == 3:
                break
    return " <- ".join(out)


def phase(frs):
    names = " ".join(fn for fn, _ in frs)
    tags = []
    for key, tag in [("Parser", "parse"), ("library_support", "sld"), ("desugar", "desugar"),
                     ("macro_expander", "expand"), ("compiler", "compile"), ("compile_with_qq", "compile"),
                     ("run_loop_until", "run"), ("cps_eval", "cps-run"), ("lower", "lower"),
                     ("load_bootstrap", "boot"), ("build_library", "build_lib")]:
        if key in names and tag not in tags:
            tags.append(tag)
    return ",".join(tags)

def report(name, top=45, key=leaf):
    live, items = snaps[name]
    agg = collections.Counter()
    for sid, w in items:
        agg[key(frames_of(sid))] += w
    tot = sum(agg.values())
    print(f"\n=== {name}: sampled {tot/1048576:.1f} MB (live counter {live/1048576:.1f} MB) ===")
    for k, w in agg.most_common(top):
        print(f"{w/1048576:8.1f} MB {100*w/tot:5.1f}%  {k}")

if __name__ == "__main__":
    which = sys.argv[2:] or list(snaps.keys())
    k = leaf2 if os.environ.get("LEAF2") else leaf
    for n in which:
        report(n, key=k, top=int(os.environ.get("TOP", "45")))

def inner_alloc(frs):
    """innermost non-profiler frame: shows whether it is a hashbrown table, an Rc, a Vec, ..."""
    for fn, loc in frs:
        if fn.startswith("lib_load_mem") or "__rust" in fn or "GlobalAlloc" in fn or fn.startswith("<lib_load_mem") or "std::thread::local" in fn or "core::cell" in fn or "core::mem::replace" in fn:
            continue
        return short(fn)[:90] + "@" + loc
    return "?"
