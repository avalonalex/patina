import os, re, sys, collections
root = os.path.expanduser('~/Project/patina/crates')
pat_bad = re.compile(r'\b(Rc<|RefCell<|(?<![A-Za-z])Cell<|Weak<|OnceCell<|SharedHeap\b|Rc<\[)')
res = collections.defaultdict(list)
for crate in sorted(os.listdir(root)):
    src = os.path.join(root, crate, 'src')
    if not os.path.isdir(src): continue
    for dp, dn, fn in os.walk(src):
        for f in fn:
            if not f.endswith('.rs'): continue
            p = os.path.join(dp, f)
            lines = open(p, encoding='utf-8', errors='replace').read().split('\n')
            in_test = False
            i = 0
            while i < len(lines):
                l = lines[i]
                if re.match(r'\s*#\[cfg\(test\)\]', l):
                    in_test = True
                m = re.match(r'\s*(pub(\([^)]*\))?\s+)?(struct|enum)\s+([A-Za-z0-9_]+)', l)
                if m and not in_test:
                    name = m.group(4)
                    # collect body
                    depth = 0; body=[]; j=i; started=False
                    while j < len(lines):
                        s = lines[j]
                        body.append(s)
                        depth += s.count('{') - s.count('}')
                        if '{' in s or '(' in s: started=True
                        if started and depth<=0 and ('}' in s or s.rstrip().endswith(';')):
                            break
                        j += 1
                    text = '\n'.join(x for x in body if not x.strip().startswith('//'))
                    hits = set(h[0] for h in pat_bad.findall(text))
                    if hits:
                        res[crate].append((name, os.path.relpath(p, root)+':'+str(i+1), sorted(hits)))
                    i = j
                i += 1
tot=0
for c,v in res.items():
    print(c, len(v)); tot+=len(v)
    for n,loc,h in v: print('   ', n, loc, ' '.join(h))
print('TOTAL', tot)
