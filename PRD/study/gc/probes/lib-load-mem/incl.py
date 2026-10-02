import re, sys, subprocess, collections
lines = open(sys.argv[1]).read().split("\n")
start = next(i for i,l in enumerate(lines) if l.startswith("Call graph:"))
end = next(i for i,l in enumerate(lines) if l.startswith("Total number in stack"))
nodes = []
for l in lines[start+1:end]:
    m = re.match(r"^([\s+!:|]*)(\d+)\s+(\S+)", l)
    if not m: continue
    nodes.append((len(m.group(1)), int(m.group(2)), m.group(3)))
names = sorted({n for _,_,n in nodes if n.startswith("_R")})
FILT = "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/llvm-cxxfilt"
out = subprocess.run([FILT], input="\n".join("_"+n for n in names), capture_output=True, text=True).stdout.split("\n")
dm = dict(zip(names, out))
pats = {
 "scope edit: map_syntax_identifiers (edit_scope_on_tagged)": "map_syntax_identifiers",
 "syntax provenance (Heap::source/record/inherit/children)": r"heap::source.*Heap>::(record_source|inherit_source|source|child_source|record_source_children)|Heap>::(record_source|inherit_source)",
 "stamp_expansion_source": "stamp_expansion_source",
 "macro expander (expand_macro)": r"Desugarer>::expand_macro\b",
 "desugar_with_imports total": "desugar_with_imports",
 "compile_with_qq_resolving (VM compiler)": "compile_with_qq_resolving",
 "VM execute (run_loop_until_outcome)": "run_loop_until_outcome",
 "library parse (SchemeLibraryLoader parse)": r"library_support",
 "GC (MarkSweepCollector::collect)": r"MarkSweepCollector|Heap>::sweep",
 "malloc+free (libsystem_malloc)": r"^_?x?zm_|^_malloc|^_free|^_xzm",
}
root_total = sum(c for d,c,n in nodes if d == min(dd for dd,_,_ in nodes))
res = collections.Counter()
for key, pat in pats.items():
    rx = re.compile(pat)
    stack = []
    tot = 0
    for d, c, n in nodes:
        while stack and stack[-1][0] >= d: stack.pop()
        name = dm.get(n, n)
        hit = bool(rx.search(name))
        anc = any(s[1] for s in stack)
        if hit and not anc: tot += c
        stack.append((d, hit or anc))
    res[key] = tot
print("root samples", root_total)
for k, v in res.items():
    print(f"{v:6d} {100*v/root_total:5.1f}%  {k}")
