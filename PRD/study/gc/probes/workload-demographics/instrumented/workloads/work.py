import importlib.util, io, contextlib
spec = importlib.util.spec_from_file_location("an", "analyze.py"); an = importlib.util.module_from_spec(spec)
with contextlib.redirect_stdout(io.StringIO()): spec.loader.exec_module(an)
R, C = an.R, an.C
print("| workload | marked per alloc (today, default policy) | slots swept per alloc (today) | nursery survival s(N) count 64K / 256K / 1M | s(N) bytes 64K / 256K / 1M | verdict on trace work |")
print("|---|---|---|---|---|---|")
for b in an.BENCHES:
    if b == "empty": continue
    d = R[b]["default"]; rows = C[b]["default"]
    A = d["alloc_total"]
    marked = sum(r["live_pairs"]+r["live_vectors"]+r["live_strings"]+r["live_objects"] for r in rows)
    swept = sum(r["arena_pairs"]+r["arena_vectors"]+r["arena_strings"]+r["arena_objects"] for r in rows)
    s = []; sb = []
    for c in ["s64K","s256K","s1M"]:
        x = R[b][c]; a = sum(x["young_alloc_class1"]); v = sum(x["young_surv_class1"])
        ab = sum(x["young_alloc_bytes_class1"]); vb = sum(x["young_surv_bytes_class1"])
        s.append(v/a if a else None); sb.append(vb/ab if ab else None)
    m = marked/A
    f = lambda x: "n/a" if x is None else f"{x:.3f}"
    s64 = s[0] if s[0] is not None else 1
    verdict = "nursery copies less" if s64 < 0.7*m else ("about equal" if s64 < 1.4*m else "nursery copies MORE")
    print(f"| {b} | {m:.3f} | {swept/A:.2f} | {' / '.join(f(x) for x in s)} | {' / '.join(f(x) for x in sb)} | {verdict} |")
