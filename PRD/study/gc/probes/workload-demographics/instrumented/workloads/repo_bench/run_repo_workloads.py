#!/usr/bin/env python3
"""Run every case of the repo's workloads.json through the same wrapper
scripts/benchmarks.py's compare_source() builds, on the base release binary (VM),
without touching the repo (benchmarks.py itself needs a git checkout and writes
to the repo's target/)."""
import json, subprocess, sys, os, time
sys.path.insert(0, os.path.expanduser('~/Project/patina/scripts'))
import benchmarks  # compare_source only; nothing is built or written
SCRATCH = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..'))
BIN = f"{SCRATCH}/instrumented/bin/patina-base"
D = f"{SCRATCH}/instrumented/workloads/repo_bench"
env = dict(os.environ, PATINA_LIBRARY_PATH=os.path.expanduser('~/Project/patina/lib'))
ok = 0
for i, case in enumerate(json.load(open(benchmarks.WORKLOADS))):
    src = f"{D}/{i}.scm"; open(src, "w").write(benchmarks.compare_source(case))
    t = time.time(); r = subprocess.run([BIN, src], capture_output=True, text=True, env=env); wall = time.time() - t
    good = r.returncode == 0 and r.stdout.startswith("BENCH_OK")
    ok += good
    print(f"{case['id']:40s} {'OK ' if good else 'BAD'} {r.stdout.split()[-1] if good else r.stdout[-80:]+r.stderr[-200:]} ms (wall {wall:.2f}s)")
print("ok", ok)
