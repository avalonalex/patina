import importlib.util, io, contextlib
spec = importlib.util.spec_from_file_location("an", "analyze.py"); an = importlib.util.module_from_spec(spec)
with contextlib.redirect_stdout(io.StringIO()): spec.loader.exec_module(an)
R = an.R
# classes: 0 pair 1 vector 2 string 3 flonum 4 closure 5 cell 6 identifier 7 record 8 other
print("| workload | all classes 64K / 1M | excluding closures+cells 64K / 1M | excluding closures+cells+flonums 64K / 1M | closures+cells share of allocs |")
print("|---|---|---|---|---|")
for b in an.BENCHES:
    if b == "empty": continue
    cells = []
    for excl in [(), (4,5), (3,4,5)]:
        xs = []
        for c in ["s64K", "s1M"]:
            d = R[b][c]; a = d["young_alloc_class1"]; s = d["young_surv_class1"]
            A = sum(v for i, v in enumerate(a) if i not in excl); S = sum(v for i, v in enumerate(s) if i not in excl)
            xs.append("n/a" if A < 1000 else f"{100*S/A:.1f}")
        cells.append(" / ".join(xs))
    d = R[b]["default"]; ov = d["obj_variant"]
    cells.append(f"{100*(ov[23]+ov[21])/d['alloc_total']:.0f}%")
    print(f"| {b} | " + " | ".join(cells) + " |")
