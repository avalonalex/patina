import os,re,collections
root=os.path.expanduser('~/Project/patina/crates/patina-primitives/src')
src_all={}
for d,_,fs in os.walk(root):
    for f in fs:
        if f.endswith('.rs'):
            p=os.path.join(d,f); s=open(p).read(); k=s.find('#[cfg(test)]'); src_all[p]=s if k<0 else s[:k]
# registered handler names
reg=re.compile(r'PrimitiveFn::new_(heap|higher_order|resumable)\(\s*"[^"]*",\s*"([^"]*)",\s*[^,]+,\s*(?:"(?:[^"\\]|\\.)*"|concat!\([^)]*\))\s*,\s*([A-Za-z0-9_:]+)',re.S)
handlers={}
for p,s in src_all.items():
    for m in reg.finditer(s): handlers[m.group(3).split('::')[-1]]=m.group(1)
print('registered',len(handlers),collections.Counter(handlers.values()))
fnre=re.compile(r'fn\s+([a-zA-Z0-9_]+)\s*\(')
bodies={}
for p,s in src_all.items():
    for m in fnre.finditer(s):
        i=s.find('{',m.end()); depth=0;j=i
        while j<len(s):
            if s[j]=='{':depth+=1
            elif s[j]=='}':
                depth-=1
                if depth==0:break
            j+=1
        bodies.setdefault(m.group(1),s[i:j])
allocre=re.compile(r'\.alloc_|intern_symbol|list_from_iter|values_from|numeric_[a-z_]+\(|to_exact|to_inexact|make_rectangular|make_polar|list_reverse|list_append|Parser::')
callre=re.compile(r'\b([a-z_][a-z0-9_]*)\s*\(')
memo={}
def allocs(name,depth=0):
    if name in memo: return memo[name]
    if depth>6 or name not in bodies: return False
    memo[name]=False
    b=bodies[name]
    r=bool(allocre.search(b)) or any(allocs(c,depth+1) for c in set(callre.findall(b)) if c in bodies and c!=name)
    memo[name]=r; return r
c=collections.Counter()
for h,k in handlers.items():
    if k=='heap': c['heap_alloc' if allocs(h) else 'heap_noalloc']+=1
print(c)
