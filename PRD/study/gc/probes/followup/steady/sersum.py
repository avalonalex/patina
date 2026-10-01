import json, sys
for f in sys.argv[1:]:
    for l in open(f):
        r=json.loads(l)
        s=r['series']
        if not s: print(r['impl'], r['probe'], 'no series', r['rc'], r['stderr'][-120:]); continue
        n=len(s)
        pts=[s[min(n-1,int(n*q))] for q in (0.05,0.25,0.5,0.75,0.999)]
        print(f"{r['impl']:9} {r['probe']:17} {' '.join(r['args']):12} rc={r['rc']} max={r['max_rss_mib']:.1f} iters={r['stdout'].split()[-1] if r['stdout'] else ''} " + ' '.join(f"t{t:.0f}s:{v:.0f}MiB" for t,v in pts))
