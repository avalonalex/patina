import json, glob, collections, re
rows = collections.defaultdict(list)
for f in ['results/scaled.jsonl','results/scaled16.jsonl','results/cmp.jsonl','results/macro.jsonl','results/macro-case.jsonl','results/rerun.jsonl','results/tw.jsonl']:
    try:
        for l in open(f):
            r = json.loads(l)
            if r['rc'] != 0 or r['max_rss_mib'] is None: continue
            args = r['args']
            n = int(args[0])
            key = (r['impl'], r['probe'], ' '.join(a for a in args[1:] if not a.startswith('/')))
            rows[key].append((n, r['max_rss_mib'], r['wall_s']))
    except FileNotFoundError:
        pass
for key in sorted(rows):
    pts = sorted(set(rows[key]))
    # dedupe by n: keep last
    d = {}
    for n, rss, t in pts: d[n] = (rss, t)
    ns = sorted(d)
    lo, hi = ns[0], ns[-1]
    if lo == hi:
        print(f"{key[0]:9} {key[1]:18} {key[2]:14} n={lo} rss={d[lo][0]:.1f}"); continue
    slope = (d[hi][0] - d[lo][0]) * 1048576 / (hi - lo)
    tr = d[hi][1] / max(d[lo][1], 0.01)
    series = ' '.join(f"{n}:{d[n][0]:.1f}MiB/{d[n][1]}s" for n in ns)
    print(f"{key[0]:9} {key[1]:18} {key[2]:14} slope={slope:8.0f} B/iter  time x{tr:5.1f} for n x{hi/lo:.0f}   {series}")
