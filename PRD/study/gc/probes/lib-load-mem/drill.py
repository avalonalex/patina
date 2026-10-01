import sys, os, collections
prof, snap, cat = sys.argv[1], sys.argv[2], sys.argv[3]
exec(open("classify.py").read().split("for name in")[0].replace("prof = sys.argv[1]", "prof = %r" % prof))
live, items = snaps[snap]
agg = collections.Counter()
for sid, w in items:
    frs = frames_of(sid)
    if classify(frs).startswith(cat):
        agg[(inner_alloc(frs)[:70], leaf2(frs)[:170])] += w
for k, w in agg.most_common(int(os.environ.get("TOP","14"))):
    print(f"{w/1048576:8.1f} MB  {k[1]}\n             inner: {k[0]}")
