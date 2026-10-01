import os,re,sys,collections
root=sys.argv[1]
fnre=re.compile(r'^\s*(pub(\([a-z]+\))?\s+)?fn\s+([a-zA-Z0-9_]+)',re.M)
def functions(src):
    # crude: find fn starts, then match braces from first '{' after signature
    out=[]
    for m in fnre.finditer(src):
        i=src.find('{',m.end())
        semi=src.find(';',m.end())
        if i<0 or (semi>=0 and semi<i): continue
        depth=0;j=i
        while j<len(src):
            c=src[j]
            if c=='{':depth+=1
            elif c=='}':
                depth-=1
                if depth==0:break
            j+=1
        out.append((m.group(3),src[m.start():j+1],src.count('\n',0,m.start())+1))
    return out
stats=collections.Counter(); ex=collections.defaultdict(list)
for d,_,fs in os.walk(root):
    for f in fs:
        if not f.endswith('.rs'):continue
        p=os.path.join(d,f); src=open(p).read()
        k=src.find('#[cfg(test)]'); src=src if k<0 else src[:k]
        for name,body,line in functions(src):
            r=len(re.findall(r'\.borrow\(\)',body)); w=len(re.findall(r'\.borrow_mut\(\)',body))
            usesheap=('heap' in body) and (r or w)
            alloc=bool(re.search(r'\.alloc_|intern_symbol|list_from_iter|values_from|core_syntax\(',body))
            ctx=bool(re.search(r'apply_proc|eval_expr|load_scheme_library',body))
            if not usesheap and not alloc: continue
            stats['fns_touching_heap']+=1
            if r and w: stats['read_then_write_borrows']+=1
            if alloc and r: stats['read_and_alloc']+=1; ex['read_and_alloc'].append(f"{os.path.relpath(p,root)}:{line} {name}")
            if r+w>=4: stats['>=4_borrows']+=1
            if ctx: stats['calls_back_into_machine']+=1; ex['ctx'].append(f"{os.path.relpath(p,root)}:{line} {name}")
            if re.search(r'let\s+(mut\s+)?[a-z_]+\s*=\s*[a-z_.()]*heap[a-z_.()]*\.borrow(_mut)?\(\)\s*;',body): stats['named_guard']+=1
for k,v in sorted(stats.items()): print(k,v)
if '-v' in sys.argv:
    for k in ex: print(k,len(ex[k])); print('  '+'\n  '.join(ex[k][:80]))
