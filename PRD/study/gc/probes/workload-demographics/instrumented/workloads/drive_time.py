#!/usr/bin/env python3
"""Baseline timing: base binary GC on vs PATINA_GC=0 (interleaved, N reps, /usr/bin/time -l),
then one instrumented run in PATINA_DEMOG=pause mode for collection count and pauses.
Writes out/timing.json."""
import os, re, json, subprocess, statistics
SCRATCH = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
W = f"{SCRATCH}/instrumented/workloads"
BASE = f"{SCRATCH}/instrumented/bin/patina-base"; DEMOG = f"{SCRATCH}/instrumented/bin/patina-demog"
BENCHES = os.environ.get("BENCHES", "eqtable nboyer deriv destruc quicksort gcbench gcold mperm queue3 fibfp mbrot nucleic ctak fibc generator deeprec libload hashtable0 dynamic earley empty").split()
REPS = int(os.environ.get("REPS", "3"))
def timed(binary, b, extra_env):
    env = dict(os.environ); env.pop("PATINA_GC", None); env.pop("PATINA_GC_STRESS", None); env.pop("PATINA_DEMOG", None)
    env.update(extra_env)
    r = subprocess.run(["/usr/bin/time", "-l", f"{W}/run1.sh", binary, b], env=env, capture_output=True, text=True)
    real = float(re.search(r"([\d.]+) real", r.stderr).group(1))
    rss = int(re.search(r"(\d+)\s+maximum resident set size", r.stderr).group(1))
    return real, rss, r.stdout + r.stderr
res = json.load(open(f"{W}/out/timing.json")) if os.path.exists(f"{W}/out/timing.json") else {}
for b in BENCHES:
    on, off = [], []
    for i in range(REPS):
        on.append(timed(BASE, b, {})[:2]); off.append(timed(BASE, b, {"PATINA_GC": "0"})[:2])
    d = f"{W}/out/{b}"; os.makedirs(d, exist_ok=True)
    env = {"PATINA_DEMOG": "pause", "PATINA_DEMOG_OUT": f"{d}/pause.txt", "PATINA_DEMOG_GCLOG": f"{d}/pause.csv"}
    t = timed(DEMOG, b, env)
    res[b] = dict(on_time=[x[0] for x in on], on_rss=[x[1] for x in on], off_time=[x[0] for x in off], off_rss=[x[1] for x in off], pause_run_time=t[0])
    print(b, "on %.2f s %.0f MB | off %.2f s %.0f MB" % (statistics.median(res[b]["on_time"]), statistics.median(res[b]["on_rss"])/2**20,
          statistics.median(res[b]["off_time"]), statistics.median(res[b]["off_rss"])/2**20), flush=True)
json.dump(res, open(f"{W}/out/timing.json", "w"), indent=1)
