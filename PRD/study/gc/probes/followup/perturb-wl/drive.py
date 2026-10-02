#!/usr/bin/env python3
"""Interleaved timing of the perturbation variants. usage: drive.py REPS [workload...]"""
import os, subprocess, sys, time, json, statistics as st
HERE = os.path.dirname(os.path.abspath(__file__))
T = os.path.dirname(HERE)
SCR = os.path.dirname(T)
W = f"{SCR}/workload-demographics/instrumented/workloads"
VARIANTS = ["base", "lock", "arc", "a_all", "order", "allocfence"]
ALL = ["fib32", "namedlet", "consloop", "closure", "nboyer", "deriv", "destruc", "quicksort",
       "fibfp", "generator", "hashtable0", "mperm"]
reps = int(sys.argv[1]); wls = sys.argv[2:] or ALL
env = dict(os.environ, PATINA_LIBRARY_PATH=f"{T}/patina-perturb/lib")
def src(b):
    if os.path.exists(f"{HERE}/{b}.scm"): return f"{HERE}/{b}.scm", None
    if os.path.exists(f"{W}/extra/{b}.scm"): s = f"{W}/extra/{b}.scm"
    else: s = f"{W}/src/{b}.scm"
    i = f"{W}/inputs/{b}.input"
    return s, (i if os.path.exists(i) else None)
res = {}
outs = {}
for r in range(reps):
    for wi, w in enumerate(wls):
        order = VARIANTS[r % len(VARIANTS):] + VARIANTS[:r % len(VARIANTS)]
        for v in order:
            s, i = src(w)
            stdin = open(i) if i else subprocess.DEVNULL
            t0 = time.perf_counter()
            p = subprocess.run([f"{T}/target-pb-{v}/release/patina", s], stdin=stdin, cwd=f"{W}/inputs",
                               env=env, capture_output=True, timeout=300)
            dt = time.perf_counter() - t0
            if p.returncode != 0:
                print(f"FAIL {v} {w} rc={p.returncode} {p.stderr[:200]!r}", file=sys.stderr)
            key = (w, v)
            res.setdefault(key, []).append(dt)
            import re
            o = re.sub(rb"[0-9.]+", b"#", p.stdout[-300:])
            if b"wrong" in p.stdout.lower() or b"error" in p.stdout.lower(): print(f"BAD OUTPUT {v} {w}: {p.stdout[-200:]!r}", file=sys.stderr)
            if w in outs and outs[w] != o: print(f"OUTPUT DIFFERS {v} {w}", file=sys.stderr)
            outs.setdefault(w, o)
    print(f"rep {r+1}/{reps} done", file=sys.stderr, flush=True)
json.dump({f"{k[0]}|{k[1]}": v for k, v in res.items()}, open(f"{HERE}/results.json", "w"))
print("workload\t" + "\t".join(VARIANTS[1:]) + "\tbase_s")
for w in wls:
    b = st.median(res[(w, "base")])
    row = [w]
    for v in VARIANTS[1:]:
        m = st.median(res[(w, v)])
        row.append(f"{(m/b-1)*100:+.1f}%")
    row.append(f"{b:.3f}")
    print("\t".join(row))
