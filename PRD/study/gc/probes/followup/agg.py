import sys, statistics as st
files = sys.argv[1:]
data = {}
order = []
ghz = []
for f in files:
    for ln in open(f):
        if ln.startswith('# rounds'):
            ghz.append(float(ln.split('clock_est_GHz=')[1]))
            continue
        if ln.startswith('group') or not ln.strip(): continue
        g, c, med, mn, p10, p90, spread, cyc = ln.rstrip('\n').split('\t')
        k = (g, c)
        if k not in data: data[k] = []; order.append(k)
        data[k].append((float(med), float(mn), float(spread)))
G = st.median(ghz)
print(f"clock estimate GHz per run: {ghz}; median {G:.3f}")
print("| group | case | median ns | cycles @%.2f GHz | run medians (min–max) | within-run p10–p90 spread %% (max) |" % G)
print("|---|---|---|---|---|---|")
for k in order:
    v = data[k]
    meds = [x[0] for x in v]
    m = st.median(meds)
    print(f"| {k[0]} | {k[1]} | {m:.3f} | {m*G:.1f} | {min(meds):.3f}–{max(meds):.3f} | {max(x[2] for x in v):.1f} |")
