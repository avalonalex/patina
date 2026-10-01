#!/usr/bin/env python3
"""Turn out/<bench>/<config>.{txt,csv} + out/timing.json into Markdown tables (stdout)."""
import os, re, json, csv, statistics as st, ast
SCRATCH = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
W = f"{SCRATCH}/instrumented/workloads/out"
BENCHES = "nboyer deriv destruc quicksort gcbench gcold mperm queue3 fibfp mbrot nucleic ctak fibc generator deeprec libload hashtable0 dynamic earley eqtable empty".split()
STRESS = ["s16K", "s64K", "s256K", "s1M", "s4M"]
VAR = ["BigInt","Rational","Real","Complex","Symbol","Bytevector","Exception","Procedure","Port","Macro","RecordType","Record","Identifier","Continuation","Parameter","Promise","Library","Values","EnvironmentSpecifier","PromptTag","LabelPlaceholder","MutableCell","Ephemeron","VmClosure","VmContinuationRef","VmDelimitedContinuationRef","CoreSyntax","Free"]
CLASSES = ["pair","vector","string","flonum","closure","cell","identifier","record","other"]
LEN = ["0","1","2","3","4","5-8","9-16","17-32","33-64","65-128","129-256","257-1024","1025-8192",">8192"]
SIZE = ["<=16","<=24","<=32","<=48","<=64","<=96","<=128","<=256","<=512","<=1K","<=2K","<=4K","<=8K","<=16K","<=32K",">32K"]
VIEWS = ["actual","16K","64K","256K","1M","4M"]
HEAP_SITES = ["set_car","set_cdr","vector_set","vm_vector_set","write_mutable_cell","set_vm_closure_free_var","promise_update","break_ephemeron","record_set","parameter_install","parameter_install_converted","promise_force_prim","promise_force_vm"]
ENV_SITES = ["env_define","env_set_slot_value","env_set_scoped","tw_cont_define"]

def parse_txt(p):
    d = {"sites": {}}
    if not os.path.exists(p): return None
    for line in open(p):
        line = line.rstrip("\n")
        if line.startswith("site "):
            m = re.match(r"site (\S+) total (\d+) imm (\d+) heapval (\d+) target_unknown (\d+) young_target (\[.*?\]) young_target_heapval (\[.*?\]) young_value (\[.*?\]) old_to_young (\[.*?\]) old_target_heapval (\[.*?\])", line)
            g = m.groups()
            d["sites"][g[0]] = dict(total=int(g[1]), imm=int(g[2]), heapval=int(g[3]), unknown=int(g[4]),
                young_target=ast.literal_eval(g[5]), young_target_heapval=ast.literal_eval(g[6]), young_value=ast.literal_eval(g[7]),
                o2y=ast.literal_eval(g[8]), old_target_heapval=ast.literal_eval(g[9]))
            continue
        if line.startswith("rs_sum"):
            m = re.match(r"rs_sum (\[.*?\]) rs_max (\[.*?\]) rs_nonempty (\[.*?\])", line)
            d["rs_sum"], d["rs_max"], d["rs_nonempty"] = [ast.literal_eval(x) for x in m.groups()]; continue
        if line.startswith("idhash"):
            nums = list(map(int, re.findall(r"\d+", line))); d["idhash"] = nums; continue
        if line.startswith("cap_full"):
            nums = list(map(int, re.findall(r"\d+", line))); d["cont"] = nums; continue
        parts = line.split(" ", 1)
        k, rest = parts[0], parts[1] if len(parts) > 1 else ""
        if rest.startswith("["): d[k] = ast.literal_eval(rest)
        else:
            toks = line.split()
            for i in range(0, len(toks) - 1, 2):
                try: d[toks[i]] = int(toks[i + 1])
                except ValueError: pass
    return d

def parse_csv(p):
    if not os.path.exists(p): return []
    with open(p) as f: return [{k: int(v) for k, v in r.items()} for r in csv.DictReader(f)]

def pct(a, b, nd=1):
    return "n/a" if not b else f"{100.0 * a / b:.{nd}f}"

def fmt(n):
    if n >= 1e9: return f"{n/1e9:.2f}G"
    if n >= 1e6: return f"{n/1e6:.2f}M"
    if n >= 1e4: return f"{n/1e3:.0f}K"
    return str(n)

R = {b: {c: parse_txt(f"{W}/{b}/{c}.txt") for c in ["default"] + STRESS} for b in BENCHES}
C = {b: {c: parse_csv(f"{W}/{b}/{c}.csv") for c in ["default"] + STRESS} for b in BENCHES}
P = {b: parse_txt(f"{W}/{b}/pause.txt") for b in BENCHES}
PC = {b: parse_csv(f"{W}/{b}/pause.csv") for b in BENCHES}
T = json.load(open(f"{W}/timing.json"))

out = []
def emit(s=""): out.append(s)

# ---------------- Baseline
emit("### E. Current-collector baseline (base binary; medians of 3 interleaved runs; pauses from instrumented `PATINA_DEMOG=pause` run)\n")
emit("| workload | GC on s | GC off s | GC cost % | RSS on MB | RSS off MB | collections | mean pause ms | max pause ms | total pause s | pause % of run | safepoint max ms |")
emit("|---|---|---|---|---|---|---|---|---|---|---|---|")
for b in BENCHES:
    t = T[b]; on = st.median(t["on_time"]); off = st.median(t["off_time"])
    p = P[b] or {}
    n = p.get("collections", 0)
    tot = p.get("pause_total_us", 0)
    emit(f"| {b} | {on:.2f} | {off:.2f} | {pct(on-off, off)} | {st.median(t['on_rss'])/2**20:.0f} | {st.median(t['off_rss'])/2**20:.0f} | {n} | {tot/n/1000 if n else 0:.2f} | {p.get('pause_max_us',0)/1000:.2f} | {tot/1e6:.3f} | {pct(tot/1e6, t['pause_run_time'])} | {p.get('sp_max_us',0)/1000:.2f} |")
emit()

# ---------------- Allocation census
emit("### B1. Allocation census (full run, default GC policy; whole process including bootstrap)\n")
emit("| workload | VM instrs | allocs | allocs / 1K instr | hyp. MB | mean B/obj | pair % | vector % | string % | flonum % | closure % | cell % | Values % | identifier % | record % | other % | flonum % of bytes |")
emit("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
agg = {"alloc": 0, "bytes": 0}
for b in BENCHES:
    d = R[b]["default"]; A = d["alloc_total"]; ov = d["obj_variant"]; ovb = d["obj_variant_bytes"]
    byts = sum(d["bytes_by_arena"])
    named = ov[2] + ov[23] + ov[21] + ov[17] + ov[12] + ov[11]
    other = d["alloc_by_arena"][3] - named
    emit(f"| {b} | {fmt(d['vm_instrs'])} | {fmt(A)} | {1000*A/max(1,d['vm_instrs']):.1f} | {byts/2**20:.1f} | {byts/max(1,A):.1f} | {pct(d['alloc_by_arena'][0],A)} | {pct(d['alloc_by_arena'][1],A)} | {pct(d['alloc_by_arena'][2],A)} | {pct(ov[2],A)} | {pct(ov[23],A)} | {pct(ov[21],A)} | {pct(ov[17],A)} | {pct(ov[12],A)} | {pct(ov[11],A)} | {pct(other,A)} | {pct(ovb[2], byts)} |")
emit()

# variants beyond the named ones
emit("Other object variants with ≥1% of a workload's allocations:\n")
for b in BENCHES:
    d = R[b]["default"]; A = d["alloc_total"]; ov = d["obj_variant"]
    extras = [f"{VAR[i]} {pct(ov[i],A)}%" for i in range(len(VAR)) if i not in (2,23,21,17,12,11) and ov[i] >= 0.01 * A]
    if extras: emit(f"- {b}: " + ", ".join(extras))
emit()

# vector/string/closure/record length histograms summed over non-empty workloads (excluding 'empty')
def hist_sum(key):
    s = [0] * len(LEN)
    for b in BENCHES:
        if b == "empty": continue
        for i, v in enumerate(R[b]["default"][key]): s[i] += v
    return s
emit("### B2. Length histograms (sum over the 19 workloads, default run)\n")
emit("| bucket | " + " | ".join(LEN) + " |")
emit("|---|" + "---|" * len(LEN))
for key, name in [("vec_len", "vectors"), ("str_len", "strings"), ("closure_fv", "closures by free vars"), ("record_fields", "records by fields"), ("values_len", "Values by length")]:
    emit(f"| {name} | " + " | ".join(fmt(x) for x in hist_sum(key)) + " |")
emit()
emit("Per-workload vector length distribution (share of that workload's vectors; only workloads with ≥1000 vectors):\n")
for b in BENCHES:
    v = R[b]["default"]["vec_len"]; n = sum(v)
    if n >= 1000:
        emit(f"- {b} ({fmt(n)} vectors): " + ", ".join(f"len {LEN[i]} {pct(x,n)}%" for i, x in enumerate(v) if x >= 0.01 * n))
emit()
emit("Per-workload closure free-variable count (share of closures; workloads with ≥1000 closures):\n")
for b in BENCHES:
    v = R[b]["default"]["closure_fv"]; n = sum(v)
    if n >= 1000:
        emit(f"- {b} ({fmt(n)}): " + ", ".join(f"{LEN[i]} fv {pct(x,n)}%" for i, x in enumerate(v) if x >= 0.01 * n))
emit()

# size histogram
emit("### B3. Object size histogram under the hypothetical layout (default run)\n")
emit("| workload | ≤16 B | ≤24 | ≤32 | ≤48 | ≤64 | ≤128 | ≤256 | >256 | >8K (count) | bytes in objects ≤32 B % | bytes in >256 B % | bytes in >8K % |")
emit("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
tot_h = [0]*16; tot_hb = [0]*16
for b in BENCHES:
    d = R[b]["default"]; h = d["size_hist"]; hb = d["size_hist_bytes"]; n = sum(h); nb = sum(hb)
    if b != "empty":
        for i in range(16): tot_h[i] += h[i]; tot_hb[i] += hb[i]
    emit(f"| {b} | {pct(h[0],n)} | {pct(h[1],n)} | {pct(h[2],n)} | {pct(h[3],n)} | {pct(h[4],n)} | {pct(h[5]+h[6],n)} | {pct(h[7],n)} | {pct(sum(h[8:]),n,2)} | {sum(h[13:])} | {pct(sum(hb[:3]),nb)} | {pct(sum(hb[8:]),nb)} | {pct(sum(hb[13:]),nb)} |")
n = sum(tot_h); nb = sum(tot_hb)
emit(f"| **all 19** | {pct(tot_h[0],n)} | {pct(tot_h[1],n)} | {pct(tot_h[2],n)} | {pct(tot_h[3],n)} | {pct(tot_h[4],n)} | {pct(tot_h[5]+tot_h[6],n)} | {pct(tot_h[7],n)} | {pct(sum(tot_h[8:]),n,2)} | {sum(tot_h[13:])} | {pct(sum(tot_hb[:3]),nb)} | {pct(sum(tot_hb[8:]),nb)} | {pct(sum(tot_hb[13:]),nb)} |")
emit()

# ---------------- Survival
def surv(b, c, k=1, bytes_=False):
    d = R[b][c]
    if d is None: return None
    a = d[f"young_alloc{'_bytes' if bytes_ else ''}_class{k}"]; s = d[f"young_surv{'_bytes' if bytes_ else ''}_class{k}"]
    return (sum(s), sum(a))
emit("### B4. Nursery survival: share of objects allocated in an interval that are live at its end (excludes collection #0, whose interval contains bootstrap). count % / bytes %\n")
emit("| workload | default | 16K | 64K | 256K | 1M | 4M | #GCs at 64K |")
emit("|---|---|---|---|---|---|---|---|")
for b in BENCHES:
    cells = []
    for c in ["default"] + STRESS:
        r = surv(b, c); rb = surv(b, c, bytes_=True)
        cells.append("n/a" if not r or not r[1] else f"{pct(*r)} / {pct(*rb)}")
    emit(f"| {b} | " + " | ".join(cells) + f" | {R[b]['s64K']['collections'] if R[b]['s64K'] else ''} |")
emit()
emit("Survival by class at 64K and 1M (count %, collections ≥1; '-' = <1000 allocated):\n")
emit("| workload | " + " | ".join(f"{c} 64K/1M" for c in CLASSES) + " |")
emit("|---|" + "---|" * len(CLASSES))
for b in BENCHES:
    cells = []
    for i, cl in enumerate(CLASSES):
        xs = []
        for c in ["s64K", "s1M"]:
            d = R[b][c]
            a = d["young_alloc_class1"][i]; s = d["young_surv_class1"][i]
            xs.append("-" if a < 1000 else pct(s, a))
        cells.append("/".join(xs))
    emit(f"| {b} | " + " | ".join(cells) + " |")
emit()

# age
emit("### B5. Survivor ages and tenured garbage (stress runs; summed over collections)\n")
emit("For each interval N: `2nd` = objects alive at their 2nd collection / objects alive at their 1st (P(survive another interval | survived one)); `3rd` = age2/age1; `dead-old` = objects found dead at a collection after surviving ≥1 earlier, as % of all first-time survivors (premature-promotion garbage a sticky-mark/promote-at-1 scheme would leave to the major GC).\n")
emit("| workload | 64K: 2nd / 3rd / dead-old | 256K: 2nd / 3rd / dead-old | 1M: 2nd / 3rd / dead-old |")
emit("|---|---|---|---|")
for b in BENCHES:
    cells = []
    for c in ["s64K", "s256K", "s1M"]:
        rows = C[b][c]
        if len(rows) < 5:
            cells.append("n/a"); continue
        # cohorts born after collection #0 only: numerators from rows >= 2 (age1) / >= 3 (age2),
        # denominators exclude the last row (no later collection to survive to).
        a0 = sum(r["age0"] for r in rows[1:-1]); a1 = sum(r["age1"] for r in rows[2:])
        a1d = sum(r["age1"] for r in rows[2:-1]); a2 = sum(r["age2"] for r in rows[3:])
        do = sum(r["dead_old"] for r in rows[2:])
        cells.append(f"{pct(a1,a0)} / {pct(a2,a1d)} / {pct(do, a0)}")
    emit(f"| {b} | " + " | ".join(cells) + " |")
emit()

# ---------------- live heap per GC, stack per GC
emit("### D1. Per-collection facts (default policy, full run): live objects by arena, stack shape at the safe point\n")
emit("| workload | GCs | mean alloc interval | live pairs mean/max | live vectors mean/max | live strings max | live objects mean/max | live hyp. MB max | frames mean/max | registers mean/max | arena slots at last GC (p/v/s/o) |")
emit("|---|---|---|---|---|---|---|---|---|---|---|")
for b in BENCHES:
    rows = C[b]["default"]; d = R[b]["default"]
    if not rows:
        emit(f"| {b} | 0 | | | | | | | | | |"); continue
    m = lambda k: (st.mean(r[k] for r in rows), max(r[k] for r in rows))
    lp, lv, lo, fr, rg = m("live_pairs"), m("live_vectors"), m("live_objects"), m("frames"), m("regs")
    last = rows[-1]
    emit(f"| {b} | {len(rows)} | {fmt(int(d['alloc_total']/len(rows)))} | {fmt(int(lp[0]))}/{fmt(lp[1])} | {fmt(int(lv[0]))}/{fmt(lv[1])} | {fmt(max(r['live_strings'] for r in rows))} | {fmt(int(lo[0]))}/{fmt(lo[1])} | {max(r['live_bytes'] for r in rows)/2**20:.1f} | {fr[0]:.0f}/{fr[1]} | {rg[0]:.0f}/{rg[1]} | {fmt(last['arena_pairs'])}/{fmt(last['arena_vectors'])}/{fmt(last['arena_strings'])}/{fmt(last['arena_objects'])} |")
emit()

# ---------------- Stores
emit("### C1. Store mix (default run). Heap stores = all in-heap channels; env = off-heap environment slots (define/set!/TW continuation define)\n")
emit("| workload | heap stores | per 1M instr | env stores | per 1M instr | imm % (heap) | young-target % actual/64K/1M | old→young % actual/64K/1M | top heap sites |")
emit("|---|---|---|---|---|---|---|---|---|")
for b in BENCHES:
    d = R[b]["default"]; S = d["sites"]; I = max(1, d["vm_instrs"])
    hs = [S[s] for s in HEAP_SITES if s in S]; es = [S[s] for s in ENV_SITES if s in S]
    H = sum(x["total"] for x in hs); E = sum(x["total"] for x in es)
    imm = sum(x["imm"] for x in hs)
    yt = [sum(x["young_target"][v] for x in hs) for v in range(6)]
    o2 = [sum(x["o2y"][v] for x in hs) for v in range(6)]
    top = sorted(((S[s]["total"], s) for s in HEAP_SITES if s in S), reverse=True)[:3]
    tops = ", ".join(f"{s} {pct(n,H,0)}%" for n, s in top if n)
    emit(f"| {b} | {fmt(H)} | {1e6*H/I:.0f} | {fmt(E)} | {1e6*E/I:.0f} | {pct(imm,H)} | {pct(yt[0],H)}/{pct(yt[2],H)}/{pct(yt[4],H)} | {pct(o2[0],H,2)}/{pct(o2[2],H,2)}/{pct(o2[4],H,2)} | {tops} |")
emit()

emit("### C2. Barrier filter funnel, heap stores summed over all workloads except libload/empty (default run)\n")
tot = {}
for b in BENCHES:
    if b in ("empty",): continue
    for s, x in R[b]["default"]["sites"].items():
        t = tot.setdefault(s, {"total":0,"imm":0,"heapval":0,"yt":[0]*6,"yth":[0]*6,"o2y":[0]*6,"oth":[0]*6})
        t["total"] += x["total"]; t["imm"] += x["imm"]; t["heapval"] += x["heapval"]
        for v in range(6):
            t["yt"][v] += x["young_target"][v]; t["yth"][v] += x["young_target_heapval"][v]; t["o2y"][v] += x["o2y"][v]; t["oth"][v] += x["old_target_heapval"][v]
emit("| site | stores | imm % | young target % (actual / 16K / 64K / 256K / 1M / 4M) | old→young % of stores (actual / 16K / 64K / 256K / 1M / 4M) |")
emit("|---|---|---|---|---|")
for s in HEAP_SITES + ENV_SITES:
    if s not in tot: continue
    t = tot[s]
    emit(f"| {s} | {fmt(t['total'])} | {pct(t['imm'],t['total'])} | " + " / ".join(pct(t['yt'][v], t['total']) for v in range(6)) + " | " + " / ".join(pct(t['o2y'][v], t['total'], 2) for v in range(6)) + " |")
emit()

emit("### C3. Remembered-set pressure: distinct old objects that received an old→young store per interval (heap targets only)\n")
emit("| workload | old→young edges (64K view) | distinct targets per 64K interval mean/max | edges (1M view) | distinct per 1M interval mean/max |")
emit("|---|---|---|---|---|")
for b in BENCHES:
    d = R[b]["default"]; S = d["sites"]
    e64 = sum(S[s]["o2y"][2] for s in HEAP_SITES if s in S); e1m = sum(S[s]["o2y"][4] for s in HEAP_SITES if s in S)
    n64 = max(1, d["alloc_total"] // 65536); n1m = max(1, d["alloc_total"] // 1048576)
    emit(f"| {b} | {fmt(e64)} | {d['rs_sum'][2]/n64:.1f}/{d['rs_max'][2]} | {fmt(e1m)} | {d['rs_sum'][4]/n1m:.1f}/{d['rs_max'][4]} |")
emit()

# ---------------- other rates
emit("### D2. Identity hashing, continuations (default run)\n")
emit("| workload | identity-hash: imm / by-index / by-Rc | equal-hash calls / index fallbacks | call/cc captures | mean frames / regs copied per capture | max regs | restores (mean regs) | delimited captures (mean frames/regs) |")
emit("|---|---|---|---|---|---|---|---|")
for b in BENCHES:
    d = R[b]["default"]; ih = d["idhash"]; c = d["cont"]
    capn, capf, capr, capmax, rn, rf, rr, dn, df, dr = c
    emit(f"| {b} | {fmt(ih[0])} / {fmt(ih[1])} / {fmt(ih[2])} | {fmt(ih[3])} / {fmt(ih[4])} | {fmt(capn)} | {capf/capn if capn else 0:.1f} / {capr/capn if capn else 0:.0f} | {capmax} | {fmt(rn)} ({rr/rn if rn else 0:.0f}) | {fmt(dn)} ({df/dn if dn else 0:.1f}/{dr/dn if dn else 0:.0f}) |")
emit()

# ---------------- Aggregates
emit("### B6. Aggregate allocation composition (sum over the 20 non-empty workloads, default run)\n")
TOT = [0]*9; TOTB = [0]*9; allA = 0; allB = 0
for b in BENCHES:
    if b == "empty": continue
    d = R[b]["default"]; ov = d["obj_variant"]; ovb = d["obj_variant_bytes"]; aa = d["alloc_by_arena"]; ba = d["bytes_by_arena"]
    cnt = [aa[0], aa[1], aa[2], ov[2], ov[23], ov[21], ov[12], ov[11], aa[3]-ov[2]-ov[23]-ov[21]-ov[12]-ov[11]]
    byt = [ba[0], ba[1], ba[2], ovb[2], ovb[23], ovb[21], ovb[12], ovb[11], ba[3]-ovb[2]-ovb[23]-ovb[21]-ovb[12]-ovb[11]]
    for i in range(9): TOT[i] += cnt[i]; TOTB[i] += byt[i]
    allA += d["alloc_total"]; allB += sum(ba)
emit("| class | " + " | ".join(CLASSES) + " | total |")
emit("|---|" + "---|" * 10)
emit("| allocations | " + " | ".join(f"{fmt(x)} ({pct(x,allA)}%)" for x in TOT) + f" | {fmt(allA)} |")
emit("| hyp. bytes | " + " | ".join(f"{x/2**20:.0f} MB ({pct(x,allB)}%)" for x in TOTB) + f" | {allB/2**20:.0f} MB |")
emit()
emit("### B7. Allocation rate at today's interpreter speed (GC-on wall time, base binary)\n")
emit("| workload | allocs/s | hyp. MB/s | 64K-alloc interval ≈ hyp. KB | 64K-alloc interval ≈ ms |")
emit("|---|---|---|---|---|")
for b in BENCHES:
    if b == "empty": continue
    d = R[b]["default"]; t = st.median(T[b]["on_time"]); A = d["alloc_total"]; B = sum(d["bytes_by_arena"])
    emit(f"| {b} | {fmt(int(A/t))} | {B/t/2**20:.0f} | {65536*B/A/1024:.0f} | {65536/(A/t)*1000:.1f} |")
emit()
emit("### B8. Trace-work estimate: live objects a full-heap collection marks vs young survivors a nursery collection copies, per collection at the same interval (stress runs, collections ≥1)\n")
emit("| workload | 64K: live / young-surv / ratio | 256K | 1M |")
emit("|---|---|---|---|")
for b in BENCHES:
    cells = []
    for c in ["s64K", "s256K", "s1M"]:
        rows = C[b][c][1:]
        if len(rows) < 2: cells.append("n/a"); continue
        L = st.mean(r["live_pairs"]+r["live_vectors"]+r["live_strings"]+r["live_objects"] for r in rows)
        Y = st.mean(r["young_surv"] for r in rows)
        cells.append(f"{fmt(int(L))} / {fmt(int(Y))} / {L/Y if Y else float('inf'):.1f}x")
    emit(f"| {b} | " + " | ".join(cells) + " |")
emit()
emit("### C4. Barrier funnel per workload at the 64K and 1M views (share of all heap stores reaching each stage)\n")
emit("| workload | heap stores | pass imm filter (heap value) % | ...and old target % 64K / 1M | ...and young value (old→young) % 64K / 1M | distinct old→young targets / edges 64K |")
emit("|---|---|---|---|---|---|")
for b in BENCHES:
    d = R[b]["default"]; S = d["sites"]
    hs = [S[x] for x in HEAP_SITES if x in S]; H = sum(x["total"] for x in hs)
    hv = sum(x["heapval"] for x in hs)
    ot = [sum(x["old_target_heapval"][v] for x in hs) for v in range(6)]
    o2 = [sum(x["o2y"][v] for x in hs) for v in range(6)]
    emit(f"| {b} | {fmt(H)} | {pct(hv,H)} | {pct(ot[2],H)} / {pct(ot[4],H)} | {pct(o2[2],H,2)} / {pct(o2[4],H,2)} | {d['rs_sum'][2]} / {o2[2]} |")
emit()
tw = [b for b in BENCHES if os.path.exists(f"{W}/{b}/tw.txt")]
if tw:
    emit("### B9. Tree-walker contrast (same workloads, `--tree-walker`, default policy)\n")
    emit("| workload | backend | allocs | pair % | closure-ish (Procedure) % | flonum % | cell % | env stores | env define (TW cont + define) |")
    emit("|---|---|---|---|---|---|---|---|---|")
    for b in tw:
        for lab, d in [("VM", R[b]["default"]), ("TW", parse_txt(f"{W}/{b}/tw.txt"))]:
            A = d["alloc_total"]; ov = d["obj_variant"]; S = d["sites"]
            E = sum(S[x]["total"] for x in ENV_SITES if x in S)
            ED = sum(S[x]["total"] for x in ("env_define","tw_cont_define") if x in S)
            emit(f"| {b} | {lab} | {fmt(A)} | {pct(d['alloc_by_arena'][0],A)} | {pct(ov[7]+ov[23],A)} | {pct(ov[2],A)} | {pct(ov[21],A)} | {fmt(E)} | {fmt(ED)} |")
    emit()

print("\n".join(out))
