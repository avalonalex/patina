#!/usr/bin/env python3
"""Does each Larceny R7RS benchmark run on Patina? Input = the suite's own input file with
the first datum (the iteration count) replaced by 1; 30 s timeout; base release binary (VM)."""
import os, re, glob, subprocess, concurrent.futures as cf, time, shutil
SCRATCH = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
L = os.path.expanduser("~/Project/reference/larceny/test/Benchmarking/R7RS")
W = f"{SCRATCH}/instrumented/workloads"; S = f"{W}/larceny_scan"
BIN = f"{SCRATCH}/instrumented/bin/patina-base"
os.makedirs(f"{S}/inputs/inputs", exist_ok=True); os.makedirs(f"{S}/inputs/outputs", exist_ok=True)
for f in glob.glob(f"{L}/inputs/*"):
    if os.path.isfile(f): shutil.copy(f, f"{S}/inputs/inputs/")
if os.path.isdir(f"{L}/inputs/bib"): pass
env = dict(os.environ, PATINA_LIBRARY_PATH=os.path.expanduser('~/Project/patina/lib'))
def one(name):
    inp = open(f"{L}/inputs/{name}.input").read()
    # replace the first datum that is not inside a comment
    lines = inp.split("\n"); done = False
    for i, ln in enumerate(lines):
        code = ln.split(";")[0]
        m = re.search(r"\S+", code)
        if m and not done:
            lines[i] = code[:m.start()] + "1" + code[m.end():]; done = True; break
    open(f"{S}/inputs/{name}.input", "w").write("\n".join(lines))
    t = time.time()
    try:
        r = subprocess.run([BIN, f"{W}/src/{name}.scm"], stdin=open(f"{S}/inputs/{name}.input"), capture_output=True, text=True, timeout=30, cwd=f"{S}/inputs", env=env)
        out = r.stdout + r.stderr
        if "ERROR: returned incorrect result" in out: st = "wrong-result"
        elif "Elapsed time" in out and r.returncode == 0: st = "ok"
        else: st = "error"
        msg = "" if st == "ok" else out.strip().splitlines()[-1][:120] if out.strip() else f"rc={r.returncode}"
    except subprocess.TimeoutExpired:
        st, msg = "timeout", ""
    return name, st, time.time() - t, msg
names = sorted(os.path.basename(p)[:-6] for p in glob.glob(f"{L}/inputs/*.input"))
names = [n for n in names if os.path.exists(f"{W}/src/{n}.scm")]
with cf.ThreadPoolExecutor(6) as ex:
    res = list(ex.map(one, names))
for n, st, t, msg in res: print(f"{n:14s} {st:13s} {t:6.2f}s {msg}")
from collections import Counter; print(Counter(r[1] for r in res), len(res))
