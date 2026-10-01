#!/usr/bin/env python3
"""Perturbation table: paired per-rep ratios vs base, and ns added per VM instruction."""
import json, sys, statistics as st
r = json.load(open(sys.argv[1]))
instrs = json.load(open(sys.argv[2])) if len(sys.argv) > 2 else {}
V = ["lock", "arc", "a_all", "order", "allocfence"]
wls = []
for k in r:
    w = k.split("|")[0]
    if w not in wls: wls.append(w)
print("| workload | base s (median) | VM instr (M) | " + " | ".join(f"{v} %" for v in V) + " | " + " | ".join(f"{v} ns/instr" for v in V) + " |")
print("|---" * (3 + 2 * len(V)) + "|")
for w in wls:
    b = r[f"{w}|base"]
    bm = st.median(b)
    cells = []
    nsi = []
    n = instrs.get(w)
    for v in V:
        t = r[f"{w}|{v}"]
        ratios = sorted(t[i] / b[i] - 1 for i in range(len(b)))
        med = st.median(ratios)
        cells.append(f"{med*100:+.1f} ({ratios[0]*100:+.1f}…{ratios[-1]*100:+.1f})")
        if n:
            d = (st.median(t) - bm) / (n * 1e6) * 1e9
            nsi.append(f"{d:+.2f}")
        else:
            nsi.append("–")
    print(f"| {w} | {bm:.3f} | {n if n else '–'} | " + " | ".join(cells) + " | " + " | ".join(nsi) + " |")
