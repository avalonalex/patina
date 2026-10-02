import sys, re
path = sys.argv[1]
names = sys.argv[2:]
lines = open(path).read().split('\n')
out = {}
cur = None
for ln in lines:
    m = re.match(r'^_([A-Za-z0-9_]+):', ln)
    if m:
        cur = m.group(1) if m.group(1) in names else None
        if cur: out[cur] = []
        continue
    if cur is None: continue
    s = ln.strip()
    if s.startswith('.cfi_end_proc'):
        cur = None; continue
    if not s or s.startswith('.loc') or s.startswith('.cfi') or s.startswith('.file') or s.startswith(';') or s.startswith('.p2align') or s.startswith('Lfunc') or s.startswith('Ltmp'):
        continue
    out[cur].append('    ' + s if not s.endswith(':') else s)
for n in names:
    print(f'_{n}:')
    print('\n'.join(out.get(n, ['   <not found>'])))
    print()
