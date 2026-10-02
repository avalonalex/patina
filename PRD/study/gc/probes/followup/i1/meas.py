import subprocess, re, sys, os, time
W = os.path.dirname(os.path.abspath(__file__))
P = os.path.join(W, '..', 'target', 'release', 'patina')
env = dict(os.environ, PATINA_LIBRARY_PATH=os.path.expanduser('~/Project/patina/lib') + ':' + W + '/lib')
def gen(path, n, form, imports):
    with open(path, 'w') as f:
        f.write('(import (scheme base) %s)\n' % imports)
        for _ in range(n):
            f.write(form + '\n')
def run(cmd, stdin=None):
    t0 = time.time()
    p = subprocess.run(['/usr/bin/time', '-l', 'timeout', '-s', 'KILL', '300'] + cmd, capture_output=True, text=True, env=env, stdin=open(stdin) if stdin else subprocess.DEVNULL)
    m = re.search(r'(\d+)\s+maximum resident set size', p.stderr)
    return p.returncode, int(m.group(1)) / 1048576, time.time() - t0, p.stderr[-300:] if p.returncode else ''
impls = {'vm': [P], 'tw': [P, '--tree-walker'], 'chibi': ['chibi-scheme', '-I', W + '/lib'], 'gosh': ['gosh', '-r7', '-I', W + '/lib']}
forms = sys.argv[1].split('|')
which = sys.argv[2].split(',')
ns = [int(x) for x in sys.argv[3].split(',')]
mode = sys.argv[4] if len(sys.argv) > 4 else 'file'
for form in forms:
    imports = '(t m)' if form.startswith('(via') else ('(t g)' if form.startswith('(g') and not form.startswith('(guard') else '')
    for impl in which:
        res = []
        for n in ns:
            path = os.path.join(W, 'm_%d.scm' % n)
            gen(path, n, form, imports)
            if mode == 'file':
                rc, rss, wall, err = run(impls[impl] + [path])
            else:
                rc, rss, wall, err = run(impls[impl] + (['-i'] if mode == 'repl' else []), stdin=path)
            res.append((n, rc, round(rss, 1), round(wall, 1), err))
        slope = (res[-1][2] - res[0][2]) * 1048576 / (ns[-1] - ns[0])
        print('%-34s %-6s %-5s %s  slope=%.0f B/form' % (form, impl, mode, [(r[0], r[1], r[2], r[3]) for r in res], slope), flush=True)
        for r in res:
            if r[4]: print('   ERR', r[4].replace('\n', ' | '))
