#!/usr/bin/env python3
"""Run the instrumented binary over all workloads and configs (counts only).
usage: drive_full.py [jobs]  -- results in out/<bench>/<config>.{txt,csv,stdout}"""
import os, subprocess, sys, concurrent.futures as cf
SCRATCH = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
W = f"{SCRATCH}/instrumented/workloads"
BIN = f"{SCRATCH}/instrumented/bin/patina-demog"
BENCHES = os.environ.get("BENCHES", "eqtable nboyer deriv destruc quicksort gcbench gcold mperm queue3 fibfp mbrot nucleic ctak fibc generator deeprec libload hashtable0 dynamic earley empty").split()
CONFIGS = {"default": {}, "s16K": {"PATINA_GC_STRESS": "16384"}, "s64K": {"PATINA_GC_STRESS": "65536"},
           "s256K": {"PATINA_GC_STRESS": "262144"}, "s1M": {"PATINA_GC_STRESS": "1048576"},
           "s4M": {"PATINA_GC_STRESS": "4194304"}}
CONFIGS = {k: v for k, v in CONFIGS.items() if k in os.environ.get("CONFIGS", " ".join(CONFIGS)).split()}
def one(b, c):
    d = f"{W}/out/{b}"; os.makedirs(d, exist_ok=True)
    env = dict(os.environ); env.pop("PATINA_GC", None); env.pop("PATINA_GC_STRESS", None)
    env.update(CONFIGS[c]); env["PATINA_DEMOG"] = "full"
    env["PATINA_DEMOG_OUT"] = f"{d}/{c}.txt"; env["PATINA_DEMOG_GCLOG"] = f"{d}/{c}.csv"
    with open(f"{d}/{c}.stdout", "w") as o:
        try:
            r = subprocess.run([f"{W}/run1.sh", BIN, b], env=env, stdout=o, stderr=subprocess.STDOUT, timeout=1800)
            return (b, c, r.returncode)
        except subprocess.TimeoutExpired:
            return (b, c, "timeout")
jobs = int(sys.argv[1]) if len(sys.argv) > 1 else 4
with cf.ThreadPoolExecutor(jobs) as ex:
    for res in ex.map(lambda bc: one(*bc), [(b, c) for c in CONFIGS for b in BENCHES]):
        print(*res, flush=True)
