import importlib.util, io, contextlib
spec = importlib.util.spec_from_file_location("an", "analyze.py"); an = importlib.util.module_from_spec(spec)
with contextlib.redirect_stdout(io.StringIO()): spec.loader.exec_module(an)
R = an.R
mid = [0,1,2,3,4,6.5,12.5,24.5,48.5,96.5,192.5,640,4608,10000]  # bucket representative lengths
tot_today = tot_hyp = 0
for b in an.BENCHES:
    if b == "empty": continue
    d = R[b]["default"]; aa = d["alloc_by_arena"]; ov = d["obj_variant"]
    today = aa[0]*16
    # vectors: 24 B slot + malloc(8n) rounded to 16 B
    today += sum(n*(24 + (16*((8*L+15)//16) if L else 0)) for n, L in zip(d["vec_len"], mid))
    today += sum(n*(24 + (16*((4*L+15)//16) if L else 0)) for n, L in zip(d["str_len"], mid))
    today += aa[3]*72
    today += sum(n*(16*((8*L+15)//16) if L else 0) for n, L in zip(d["closure_fv"], mid))      # free_vars Vec buffer
    today += sum(n*(48 + 16*((8*L+15)//16)) for n, L in zip(d["record_fields"], mid))          # Rc<RefCell<Vec>> box + buffer
    hyp = sum(d["bytes_by_arena"])
    tot_today += today; tot_hyp += hyp
    print(f"{b:10s} today~{today/2**20:8.0f} MB  hyp {hyp/2**20:7.0f} MB  ratio {today/hyp:4.2f}")
print(f"ALL today~{tot_today/2**20:.0f} MB hyp {tot_hyp/2**20:.0f} MB ratio {tot_today/tot_hyp:.2f}")
