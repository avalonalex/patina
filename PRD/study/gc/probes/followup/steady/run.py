#!/usr/bin/env python3
"""Run a probe under one implementation; report max RSS (MiB), wall time, and
optionally an RSS time series sampled with ps."""
import os, re, subprocess, sys, time, json

HERE = os.path.dirname(os.path.abspath(__file__))
PATINA = os.path.join(HERE, '..', 'target', 'release', 'patina')
LIB = os.path.expanduser('~/Project/patina/lib')
TIMEOUT = int(os.environ.get('PROBE_TIMEOUT', '180'))

def command(impl, probe, args):
    if impl == 'patina-stdin':
        return [PATINA] + [str(a) for a in args]
    if impl in ('patina', 'patina-tw'):
        path = probe if probe.startswith('/') else os.path.join(HERE, 'probes', probe + '.scm')
        if os.environ.get('PATINA_STATS'):
            src = open(path).read().replace('(import ', '(import (patina debug) ', 1)
            src += '\n(write (gc-stats)) (newline)\n'
            path = os.path.join(HERE, 'results', 'stats-' + probe + '.scm')
            open(path, 'w').write(src)
        cmd = [PATINA] + (['--tree-walker'] if impl == 'patina-tw' else []) + [path]
    elif impl == 'chibi':
        cmd = ['chibi-scheme', os.path.join(HERE, 'probes', probe + '.scm')]
    elif impl == 'gosh':
        cmd = ['gosh', '-r7', os.path.join(HERE, 'probes', probe + '.scm')]
    elif impl == 'chez':
        cmd = ['chez', '--script', os.path.join(HERE, 'chez', probe + '.ss')]
    else:
        raise SystemExit('unknown impl ' + impl)
    return cmd + [str(a) for a in args]

def run(impl, probe, args, series=False, interval=0.25):
    cmd = ['timeout', '-s', 'KILL', str(TIMEOUT), '/usr/bin/time', '-l'] + command(impl, probe, args)
    env = dict(os.environ, PATINA_LIBRARY_PATH=LIB)
    t0 = time.time()
    stdin = open(os.environ['STDIN_FILE']) if os.environ.get('STDIN_FILE') else subprocess.DEVNULL
    p = subprocess.Popen(cmd, stdin=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, text=True)
    samples = []
    if series:
        # find the grandchild (the interpreter) pid: timeout -> time -> interp
        target = None
        while p.poll() is None:
            if target is None:
                out = subprocess.run(['pgrep', '-P', str(p.pid)], capture_output=True, text=True).stdout.split()
                if out:
                    out2 = subprocess.run(['pgrep', '-P', out[0]], capture_output=True, text=True).stdout.split()
                    if out2:
                        target = out2[0]
            if target:
                r = subprocess.run(['ps', '-o', 'rss=', '-p', target], capture_output=True, text=True).stdout.strip()
                if r:
                    samples.append((round(time.time() - t0, 2), int(r) / 1024.0))
            time.sleep(interval)
    out, err = p.communicate()
    wall = time.time() - t0
    m = re.search(r'(\d+)\s+maximum resident set size', err)
    rss = int(m.group(1)) / (1024 * 1024) if m else None
    errlines = [l for l in err.splitlines() if 'resident' not in l and not re.match(r'^\s+\d+\s+[a-z]', l) and not re.match(r'^\s+[\d.]+ real', l)]
    return {'impl': impl, 'probe': probe, 'args': args, 'rc': p.returncode, 'max_rss_mib': rss,
            'wall_s': round(wall, 2), 'stdout': out.strip()[-400:], 'stderr': '\n'.join(errlines)[-400:],
            'series': samples}

if __name__ == '__main__':
    impl, probe = sys.argv[1], sys.argv[2]
    args = sys.argv[3:]
    series = os.environ.get('SERIES') == '1'
    r = run(impl, probe, args, series=series)
    print(json.dumps(r))
