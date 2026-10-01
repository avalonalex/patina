# Column-0 top-level non-procedure bindings in bundled Scheme, and the mutators applied to them.
import os,re,collections,sys
root=os.path.expanduser('~/Project/patina/lib')
defre=re.compile(r'^\(define\s+([^\s()]+)\s+(?!\(lambda|\(case-lambda|\(make-parameter)', re.M)
out=[]
for dp,dn,fn in os.walk(root):
    for f in fn:
        if not f.endswith(('.scm','.sld')): continue
        p=os.path.join(dp,f); s=open(p,errors='replace').read()
        for m in defre.finditer(s):
            n=m.group(1); q=re.escape(n)
            sets=[s.count('\n',0,x.start())+1 for x in re.finditer(r'\(set!\s+'+q+r'[\s)]', s)]
            cont=[(x.group(1), s.count('\n',0,x.start())+1) for x in re.finditer(r'\(([^\s()]+!)\s+'+q+r'[\s)]', s) if x.group(1)!='set!']
            if sets or cont:
                out.append((os.path.relpath(p,root), n, s.count('\n',0,m.start())+1, sets, cont))
for r in sorted(out): print(r)
print(len(out),'bindings in',len({r[0] for r in out}),'files')
