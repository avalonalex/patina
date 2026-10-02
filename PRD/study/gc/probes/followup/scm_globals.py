import os,re,collections
root=os.path.expanduser('~/Project/patina/lib')
tot_files=0; hits=collections.Counter(); examples=[]
for dp,dn,fn in os.walk(root):
    for f in fn:
        if not f.endswith(('.scm','.sld')): continue
        p=os.path.join(dp,f); s=open(p,errors='replace').read()
        # top-level (define name <non-lambda>) at column 0 or 2 within define-library begin
        names=set(m.group(1) for m in re.finditer(r'^\s{0,4}\(define\s+([^\s()]+)\s+(?!\(lambda|\(case-lambda|\(make-parameter)', s, re.M))
        if not names: continue
        mut=[n for n in names if re.search(r'\(set!\s+'+re.escape(n)+r'[\s)]', s)]
        if mut:
            tot_files+=1; hits[os.path.relpath(p,root)]=len(mut); examples.append((os.path.relpath(p,root), mut[:4]))
print('files with top-level non-procedure defines that are set!:', tot_files, 'variables:', sum(hits.values()))
for p,m in sorted(examples)[:60]: print(' ',p,m)
