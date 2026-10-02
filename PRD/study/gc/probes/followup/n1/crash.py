import subprocess,sys,os,json,glob,time,re
W=os.path.dirname(os.path.abspath(__file__))
import os
NAME=os.environ.get('PN','ptn_a')
P=W+'/'+NAME
REPO=os.path.expanduser('~/Project/patina')
def short(s):
    import re
    i=s.find('patina_')
    t=s[i:] if i>=0 else s
    t=re.sub(r'(?<=[A-Za-z_])[0-9]+(?=[A-Za-z_])',':',t)
    return t[-80:]
shape,backend,n=sys.argv[1],sys.argv[2],int(sys.argv[3])
f=f'{W}/shapes/{shape}_{n}.scm'
if not os.path.exists(f):
    with open(f,'w') as o: subprocess.run(['python3',W+'/gen2.py',shape,str(n)],stdout=o,check=True)
cmd=[P]+(['--tree-walker'] if backend=='tw' else [])+[f]
p=subprocess.Popen(cmd,cwd=REPO,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
out,err=p.communicate(timeout=120)
print(f'{shape} {backend} n={n} rc={p.returncode} pid={p.pid}')
if p.returncode==0: sys.exit()
d=None
for _ in range(80):
    for g in sorted(glob.glob(os.path.expanduser('~/Library/Logs/DiagnosticReports/'+NAME+'-*.ips')),key=os.path.getmtime,reverse=True)[:10]:
        txt=open(g).read(); hdr,body=txt.split('\n',1); j=json.loads(body)
        if j.get('pid')==p.pid: d=j;break
    if d: break
    time.sleep(0.5)
if not d: print('no report'); sys.exit()
for t in d['threads']:
    if t.get('triggered'):
        print(' frames',t.get('originalLength'))
        for r in t.get('recursionInfoArray',[]):
            print('  recursion frames',r['hottestElided'],'..',r['coldestElided'],'cycle-repeats',r['depth'],'key',short(r['keyFrame']['symbol']))
        prev=None; seq=[]
        for x in t['frames'][7:]:
            s=short(x.get('symbol','?'))
            if s!=prev: seq.append(s)
            prev=s
        # compress
        print('  top:', ' <- '.join(s.split(':')[-1][:40] for s in seq[:10]))
        print('  bottom:', ' <- '.join(s.split(':')[-1][:40] for s in seq[-12:-5]))
