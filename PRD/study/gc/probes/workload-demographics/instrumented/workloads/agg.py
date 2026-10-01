import importlib.util, sys
spec = importlib.util.spec_from_file_location("an", "analyze.py"); an = importlib.util.module_from_spec(spec)
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()): spec.loader.exec_module(an)
R = an.R; B = [b for b in an.BENCHES if b != "empty"]
H = an.HEAP_SITES
tot=0; imm=0; yt=[0]*6; ot=[0]*6; o2=[0]*6; hv=0
for b in B:
    S = R[b]["default"]["sites"]
    for s in H:
        if s not in S: continue
        x=S[s]; tot+=x["total"]; imm+=x["imm"]; hv+=x["heapval"]
        for v in range(6): yt[v]+=x["young_target"][v]; ot[v]+=x["old_target_heapval"][v]; o2[v]+=x["o2y"][v]
print("AGG heap stores", tot, "imm%", 100*imm/tot)
for v in range(6): print(f"  view {an.VIEWS[v]:6s} young-target% {100*yt[v]/tot:5.1f}  heapval&old-target% {100*ot[v]/tot:5.2f}  old->young% {100*o2[v]/tot:5.2f}")
# median per-workload
import statistics as st
def per(b, v):
    S=R[b]["default"]["sites"]; t=sum(S[s]["total"] for s in H if s in S)
    return (100*sum(S[s]["imm"] for s in H if s in S)/t, 100*sum(S[s]["young_target"][v] for s in H if s in S)/t, 100*sum(S[s]["old_target_heapval"][v] for s in H if s in S)/t, 100*sum(S[s]["o2y"][v] for s in H if s in S)/t)
for v in (2,4):
    rows=[per(b,v) for b in B]
    print("median over workloads view", an.VIEWS[v], [round(st.median(c),2) for c in zip(*rows)])
# stores per 1M instr
for b in B:
    d=R[b]["default"]; S=d["sites"]; t=sum(S[s]["total"] for s in H if s in S); hvv=sum(S[s]["heapval"] for s in H if s in S)
    print(f"{b:10s} heap stores/1K instr {1000*t/d['vm_instrs']:6.2f} heap-valued/1K {1000*hvv/d['vm_instrs']:6.2f} allocs/1K {1000*d['alloc_total']/d['vm_instrs']:6.1f}")
# size distribution excluding gcold
h=[0]*16; hb=[0]*16
for b in B:
    if b=="gcold": continue
    d=R[b]["default"]
    for i in range(16): h[i]+=d["size_hist"][i]; hb[i]+=d["size_hist_bytes"][i]
n=sum(h); nb=sum(hb); c=0; cb=0
print("size excl gcold: bound count% cum% bytes% cumbytes%")
for i in range(16):
    c+=h[i]; cb+=hb[i]
    print(f"  {an.SIZE[i]:6s} {100*h[i]/n:5.1f} {100*c/n:5.1f} {100*hb[i]/nb:5.1f} {100*cb/nb:5.1f}")
print("mean bytes excl gcold", nb/n)
# flonum
fa=sum(R[b]["default"]["obj_variant"][2] for b in B); A=sum(R[b]["default"]["alloc_total"] for b in B)
print("flonum allocs", fa, "of", A, 100*fa/A)
