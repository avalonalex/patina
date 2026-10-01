import json,sys,re
def dm(s):
    # crude v0 demangle: pull identifiers
    parts=re.findall(r'(\d+)([A-Za-z_][A-Za-z0-9_]*)',s)
    out=[];
    for n,rest in parts:
        n=int(n); out.append(rest[:n])
    return '::'.join(out[-3:]) if out else s
for f in sys.argv[1:]:
    txt=open(f).read()
    hdr,body=txt.split('\n',1)
    d=json.loads(body)
    print('==',f.split('/')[-1], d.get('captureTime'))
    for t in d['threads']:
        if t.get('triggered'):
            print(' originalLength',t.get('originalLength'))
            for r in t.get('recursionInfoArray',[]):
                print('  recursion: frames',r['hottestElided'],'..',r['coldestElided'],'depth',r['depth'],'key',dm(r['keyFrame']['symbol']))
            seen=[]
            for x in t['frames']:
                s=dm(x.get('symbol','?'))
                if not seen or seen[-1]!=s: seen.append(s)
            print('  ' + '\n  '.join(seen))
