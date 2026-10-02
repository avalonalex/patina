import json, sys
for line in sys.stdin:
    line = line.strip()
    if not line.startswith('{'):
        print(line); continue
    r = json.loads(line)
    rss = r["max_rss_mib"] or -1
    print(f"{r["impl"]:9} {r['probe']:18} {' '.join(a[-12:] for a in r['args']):22} rc={r['rc']} rss={rss:.1f}MiB t={r['wall_s']}s out={r['stdout'][-260:]!r} err={r['stderr'][-160:]!r}")
