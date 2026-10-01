import json,glob,os,re,sys
def short(s):
    i=s.find('patina_')
    t=s[i:] if i>=0 else s
    t=re.sub(r'(?<=[A-Za-z_])[0-9]+(?=[A-Za-z_])',':',t)
    t=re.sub(r'^patina_(\w+?):',r'\1:',t)
    return t
files=sorted(glob.glob(os.path.expanduser('~/Library/Logs/DiagnosticReports/patina-*.ips'))+glob.glob(os.path.expanduser('~/Library/Logs/DiagnosticReports/Retired/patina-*.ips')),key=os.path.getmtime)
for g in files:
    txt=open(g).read(); hdr,body=txt.split('\n',1); d=json.loads(body)
    if d.get('captureTime','')<'2026-10-01 08:3': continue
    for t in d['threads']:
        if not t.get('triggered'): continue
        fr=[short(x.get('symbol','?')) for x in t['frames']]
        if not any('stack_overflow' in x for x in fr):
            print(os.path.basename(g),'not a stack overflow'); continue
        rec=[(r['depth'],short(r['keyFrame']['symbol'])[-60:]) for r in t.get('recursionInfoArray',[])]
        # distinct patina functions, top 6 after sigtramp
        i=next(k for k,x in enumerate(fr) if '_sigtramp' in x)
        top=[]
        for x in fr[i+1:]:
            x2=x.split(':')
            nm=':'.join(x2[-2:])[:50]
            if not top or top[-1]!=nm: top.append(nm)
        be='tw' if any('tree_walker' in x for x in fr) else ('vm' if any('patina_vm' in x or 'vm:' in x for x in fr) else '?')
        print(d['captureTime'][11:19], d['pid'], be, 'len',t.get('originalLength'), 'rec',rec)
        print('    ', ' <- '.join(top[:8]))
