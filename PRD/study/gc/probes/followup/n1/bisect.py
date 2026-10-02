import subprocess,sys,os
W=os.path.dirname(os.path.abspath(__file__))
P=W+'/../target/release/patina'
REPO=os.path.expanduser('~/Project/patina')
def run(backend,shape,n,stack=None):
    f=f'{W}/shapes/{shape}_{n}.scm'
    if not os.path.exists(f):
        with open(f,'w') as o: subprocess.run(['python3',W+'/gen2.py',shape,str(n)],stdout=o,check=True)
    cmd=[P]+(['--tree-walker'] if backend=='tw' else [])+[f]
    if stack:
        cmd=['/bin/bash','-c',f'ulimit -s {stack}; exec "$@"','x']+cmd
    try:
        r=subprocess.run(['timeout','-s','KILL','120']+cmd,cwd=REPO,capture_output=True,text=True)
    except Exception as e: return ('err',str(e))
    out=(r.stdout+r.stderr).strip().splitlines()
    return (r.returncode, out[-1][:120] if out else '')
def ok(res): return res[0]==0
shape=sys.argv[1]; backend=sys.argv[2]; stack=sys.argv[3] if len(sys.argv)>3 and sys.argv[3]!='-' else None
hi_lim=int(sys.argv[4]) if len(sys.argv)>4 else 1000000
# exponential search
lo=1; n=64; fail=None
while n<=hi_lim:
    r=run(backend,shape,n,stack)
    if ok(r): lo=n; n*=2
    else: fail=(n,r); break
if fail is None:
    print(f'{shape:8} {backend} stack={stack} passes up to {lo} (limit)'); sys.exit()
hi=fail[0]
while hi-lo>max(1,lo//200):
    m=(lo+hi)//2
    r=run(backend,shape,m,stack)
    if ok(r): lo=m
    else: hi=m; fail=(m,r)
print(f'{shape:8} {backend} stack={stack} max_pass~{lo} first_fail={hi} rc={fail[1][0]} :: {fail[1][1]}')
