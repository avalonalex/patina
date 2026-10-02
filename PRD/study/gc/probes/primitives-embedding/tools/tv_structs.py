import os,re,sys
root=os.path.expanduser('~/Project/patina/crates')
sre=re.compile(r'^\s*(pub(\([a-z]+\))?\s+)?(struct|enum)\s+([A-Za-z0-9_]+)[^{;]*\{',re.M)
for c in sys.argv[1:]:
    for d,_,fs in os.walk(os.path.join(root,c)):
        for f in fs:
            if not f.endswith('.rs'):continue
            p=os.path.join(d,f); src=open(p).read()
            k=src.find('#[cfg(test)]'); src=src if k<0 else src[:k]
            for m in sre.finditer(src):
                i=m.end()-1;depth=0;j=i
                while j<len(src):
                    if src[j]=='{':depth+=1
                    elif src[j]=='}':
                        depth-=1
                        if depth==0:break
                    j+=1
                body=src[i:j]
                body_nc='\n'.join(l for l in body.split('\n') if not l.strip().startswith('//'))
                if re.search(r'\bTaggedValue\b|MatchValue|ScopedBinding|HeapIndex',body_nc):
                    line=src.count('\n',0,m.start())+1
                    hits=[l.strip() for l in body_nc.split('\n') if re.search(r'TaggedValue|HeapIndex',l)][:3]
                    print(f"{os.path.relpath(p,root)}:{line} {m.group(3)} {m.group(4)} :: {' | '.join(hits)[:160]}")
