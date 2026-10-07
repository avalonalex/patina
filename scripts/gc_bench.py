"""The `gc` mode of scripts/benchmarks.py (#649): the GC benchmark set.

Runs the workloads of crates/patina-tests/bench_programs/gc/gbs.json, either
one build's absolute measurements or base/branch/base interleaved, and reports
per workload and backend:

- wall and user time, instructions retired and cycles, peak RSS, peak
  footprint and page reclaims (/usr/bin/time -l on macOS; /usr/bin/time -v and
  perf stat on Linux);
- from PATINA_GC_LOG (#648): collections, the maximum and total pause, and the
  minimum mutator utilisation at 1-100 ms windows;
- after a final pair of (gc) calls: live bytes, the footprint and the
  resident size;
- for a comparison, each metric's ratio, branch over the mean of the round's
  two base runs, as a geomean over rounds with a bootstrap 95% interval, and
  the geomean over workloads. Differences under 2% are judged in
  instructions or cycles, never wall time.

The environment's size is drawn at random per round and shared by the
round's runs (#653): the VM's speed on tight loops depends on where the stack
lands, and a fixed size measures each build at one layout.

The Larceny-derived workloads are LGPL and are not vendored: they run from
the Larceny checkout (LARCENY_DIR, by default ~/Project/reference/larceny),
as scripts/run_larceny_tests.sh does, and are reported as skipped without it.
"""
import hashlib
import json
import math
import os
from pathlib import Path
import random
import re
import resource
import shutil
import signal
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
GC = ROOT / 'crates/patina-tests/bench_programs/gc'
MMU_WINDOWS_MS = (1, 2, 5, 10, 20, 50, 100)
PAD_MAX = 4096
SIDES_ABA = ('base', 'branch', 'base')

EPILOGUE = """
(import (patina debug))
(gc)
(gc)
(let ((s (gc-stats)))
  (display "GC_FINAL")
  (for-each (lambda (k)
              (let ((p (assq k s)))
                (display " ") (display k) (display "=") (display (if p (cdr p) "none"))))
            '(live-bytes committed-bytes external-bytes resident-bytes
              wait-max-bytes wait-site deferral-max-bytes deferral-site))
  (newline))
"""


class Skip(Exception):
    """A workload that cannot run here, with the reason."""


def larceny_dir():
    return Path(os.environ.get('LARCENY_DIR', Path.home() / 'Project/reference/larceny'))


def load_workloads(sets, names=()):
    """The workloads in any of `sets` (or named in `names`), in table order."""
    table = json.loads((GC / 'gbs.json').read_text())['workloads']
    chosen = [w for w in table if (names and w['name'] in names)
              or (not names and set(w['sets']) & set(sets))]
    if names and len(chosen) != len(set(names)):
        known = {w['name'] for w in table}
        raise ValueError(f'unknown workloads: {sorted(set(names) - known)}')
    return chosen


def apply_input(spec, checkout_text):
    """A Larceny workload's input: Patina-authored text, or the checkout's
    input with its leading count replaced."""
    if 'text' in spec:
        return spec['text']
    lines = checkout_text.split('\n')
    for i, line in enumerate(lines):
        stripped = line.strip()
        if stripped and not stripped.startswith(';'):
            comment = line[line.index(';'):] if ';' in line else ''
            lines[i] = f'{spec["count"]}{"   " + comment if comment else ""}'
            return '\n'.join(lines)
    raise ValueError('an input with no count')


def macroprog(n=2500):
    """A program of 2,500 top-level definitions through the standard macros,
    each called once (the loading workload of #643)."""
    lines = ['(import (scheme base) (scheme write) (scheme case-lambda))']
    for i in range(n):
        k = i % 5
        if k == 0:
            lines.append(f'(define (f{i} x) (let loop ((j 0) (acc 0)) (cond ((= j x) acc) (else (loop (+ j 1) '
                         f'(case (modulo j 3) ((0) (+ acc 1)) ((1) (+ acc 2)) (else acc)))))))')
        elif k == 1:
            lines.append(f'(define (f{i} x) (let* ((a (* x 2)) (b (+ a 1))) (when (> b 0) (unless (< b 0) '
                         f'(and (> a -1) (or #f (+ a b)))))))')
        elif k == 2:
            lines.append(f'(define (f{i} x) (do ((j 0 (+ j 1)) (s 0 (+ s j))) ((= j x) s)))')
        elif k == 3:
            lines.append(f'(define f{i} (case-lambda ((x) (f{i} x 1)) ((x y) (let-values (((q r) (floor/ (+ x y) 3))) '
                         f'(+ q r)))))')
        else:
            lines.append(f'(define-record-type r{i} (make-r{i} a b) r{i}? (a r{i}-a) (b r{i}-b set-r{i}-b!))\n'
                         f'(define (f{i} x) (let ((r (make-r{i} x 2))) (set-r{i}-b! r 3) (+ (r{i}-a r) (r{i}-b r))))')
    lines.append('(define total 0)')
    lines += [f'(set! total (+ total (f{i} 7)))' for i in range(n)]
    lines.append('(write total) (newline)')
    return '\n'.join(lines) + '\n'


def prepare(workload, scratch):
    """Write the workload's program into `scratch`: (program path or None,
    arguments, stdin path or None, working directory, check). Raises Skip."""
    name, kind = workload['name'], workload['kind']
    run_dir = scratch / 'run'
    (run_dir / 'outputs').mkdir(parents=True, exist_ok=True)
    args = [a.replace('{scratch}', str(scratch)) for a in workload.get('args', [])]
    if kind in ('larceny-r7rs', 'larceny-gc'):
        root = larceny_dir() / 'test/Benchmarking'
        r7rs = root / 'R7RS'
        if not r7rs.is_dir():
            raise Skip(f'no Larceny checkout at {larceny_dir()} (set LARCENY_DIR)')
        link = run_dir / 'inputs'
        if not link.exists():
            link.symlink_to(r7rs / 'inputs')
        for derived, how in workload.get('derive', {}).items():
            target = run_dir / derived
            target.parent.mkdir(parents=True, exist_ok=True)
            with (run_dir / how['from']).open(errors='replace') as source:
                target.write_text(''.join(line for _, line in zip(range(how['lines']), source)))
        if kind == 'larceny-r7rs':
            text = (r7rs / 'src' / f'{workload["source"]}.scm').read_text() + (r7rs / 'src/common.scm').read_text()
            stdin = scratch / f'{name}.input'
            checkout = (r7rs / 'inputs' / f'{workload["source"]}.input')
            stdin.write_text(apply_input(workload['input'], checkout.read_text() if checkout.exists() else ''))
        else:
            text = ((GC / 'shims/larceny-gc-prefix.scm').read_text()
                    + (root / 'GC' / f'{workload["source"]}.sch').read_text() + '\n' + workload['entry'] + '\n')
            stdin = None
        program = scratch / f'{name}.scm'
        program.write_text(text + EPILOGUE)
        return program, args, stdin, run_dir, 'larceny'
    if kind == 'patina':
        program = scratch / f'{name}.scm'
        program.write_text((GC / workload['source']).read_text() + EPILOGUE)
        return program, args, None, run_dir, 'output'
    if kind == 'generated':
        program = scratch / f'{name}.scm'
        program.write_text(macroprog() + EPILOGUE)
        return program, args, None, run_dir, 'output'
    if kind == 'example':
        return None, args, None, run_dir, 'output'
    raise ValueError(f'{name}: unknown kind {kind}')


# ---------------------------------------------------------------- metrics

TIME_L = {
    'wall_s': (r'([\d.]+) real', float),
    'user_s': (r'([\d.]+) user', float),
    'max_rss': (r'(\d+)\s+maximum resident set size', int),
    'peak_footprint': (r'(\d+)\s+peak memory footprint', int),
    'instructions': (r'(\d+)\s+instructions retired', int),
    'cycles': (r'(\d+)\s+cycles elapsed', int),
    'page_reclaims': (r'(\d+)\s+page reclaims', int),
}
TIME_V = {
    'wall_s': (r'Elapsed \(wall clock\) time \(h:mm:ss or m:ss\): (\S+)', None),
    'user_s': (r'User time \(seconds\): ([\d.]+)', float),
    'max_rss': (r'Maximum resident set size \(kbytes\): (\d+)', lambda v: int(v) * 1024),
    'page_reclaims': (r'Minor \(reclaiming a frame\) page faults: (\d+)', int),
}


def parse_time(stderr):
    """Metrics from /usr/bin/time -l (macOS) or -v (Linux), and perf stat's
    CSV lines (Linux) when present; a metric the platform lacks is None."""
    metrics = {}
    table = TIME_L if 'maximum resident set size' in stderr else TIME_V
    for key, (pattern, convert) in table.items():
        m = re.search(pattern, stderr)
        if not m:
            metrics[key] = None
        elif key == 'wall_s' and table is TIME_V:
            parts = [float(p) for p in m.group(1).split(':')]
            metrics[key] = sum(p * 60 ** i for i, p in enumerate(reversed(parts)))
        else:
            metrics[key] = convert(m.group(1))
    for event in ('instructions', 'cycles'):
        m = re.search(rf'^(\d+),[^,]*,{event}', stderr, re.M)
        if m:
            metrics[event] = int(m.group(1))
        metrics.setdefault(event, None)
    metrics.setdefault('peak_footprint', None)
    return metrics


def parse_final(stdout):
    m = re.search(r'GC_FINAL((?: [a-z-]+=\S+)*)', stdout)
    if not m:
        return {}
    out = {}
    for pair in m.group(1).split():
        key, _, value = pair.partition('=')
        # A site is a path and line; the rest are counts.
        out[key.replace('-', '_')] = None if value in ('#f', 'none') else int(value) if value.isdigit() else value
    return out


def mmu(intervals, windows_ms=MMU_WINDOWS_MS):
    """The minimum mutator utilisation at each window over (start, end)
    intervals in microseconds: the worst window ends at an interval's end or
    starts at an interval's start (the same rule as heap/telemetry.rs).
    A window that would start before time zero, or end after the last
    interval's end, is not counted."""
    result = {}
    if not intervals:
        return {w: 1.0 for w in windows_ms}
    horizon = max(end for _, end in intervals)
    for w in windows_ms:
        width = w * 1000
        worst = 1.0
        candidates = [end - width for _, end in intervals] + [start for start, _ in intervals]
        for lo in candidates:
            hi = lo + width
            if lo < 0 or hi > horizon:
                continue
            busy = sum(max(0, min(end, hi) - max(start, lo)) for start, end in intervals)
            worst = min(worst, 1.0 - busy / width)
        result[w] = max(0.0, worst)
    return result


def parse_log(path):
    """Collections, paced collections, pause and mark figures and the MMU
    from a PATINA_GC_LOG file. The epilogue's two forced collections are
    not counted; the first, a (gc) right after the program, is reported
    alone. Without a log (a build before #648), all of it is None."""
    if not path.exists():
        return dict.fromkeys(('collections', 'paced', 'pause_max_us', 'pause_total_us', 'mark_max_us',
                              'mark_mean_us', 'forced_pause_us', 'mmu'))
    rows = []
    lines = path.read_text().splitlines()
    col = {h: i for i, h in enumerate(lines[0].split(','))}
    for line in lines[1:]:
        f = line.split(',')
        rows.append((int(f[col['start_us']]), int(f[col['pause_us']]), f[col['reason']], int(f[col['mark_us']])))
    forced = None
    if len(rows) >= 2 and all(r[2] == 'call' for r in rows[-2:]):
        forced, rows = rows[-2][1], rows[:-2]
    pauses = [r[1] for r in rows]
    marks = [r[3] for r in rows]
    return {
        'collections': len(rows),
        'paced': sum(1 for r in rows if r[2] not in ('call', 'posted')),
        'pause_max_us': max(pauses, default=0),
        'pause_total_us': sum(pauses),
        'mark_max_us': max(marks, default=0),
        'mark_mean_us': statistics.mean(marks) if marks else 0,
        'forced_pause_us': forced,
        'mmu': mmu([(r[0], r[0] + r[1]) for r in rows]),
    }


# ---------------------------------------------------------------- running

def timing_command():
    """time(1) with its resource report, and perf stat's counters on Linux."""
    time = shutil.which('time') or '/usr/bin/time'
    if sys.platform == 'darwin':
        return [time, '-l']
    command = []
    if shutil.which('perf'):
        command = ['perf', 'stat', '-x', ',', '-e', 'instructions,cycles', '--']
    return command + [time, '-v']


def run_one(side, workload, backend, scratch, pad, timeout):
    """One run of `workload` on `side` ({'binary', 'root', 'example'}):
    its metrics, or an exception describing the failure."""
    program, args, stdin, cwd, check = prepare(workload, scratch)
    log = scratch / f'gc-{workload["name"]}-{backend}-{side["label"]}.csv'
    if log.exists():
        log.unlink()
    env = {k: v for k, v in os.environ.items() if not k.startswith('PATINA_')}
    # Isolated, each binary loads its own checkout's lib/.
    env.update(PATINA_GC_LOG=str(log), PATINA_ISOLATED_LIBRARIES='1', PATINA_BENCH_PAD='x' * pad)
    if workload['kind'] == 'example':
        command = [side['example'](workload['source'])] + args
    else:
        command = [side['binary']] + (['--tree-walker'] if backend == 'tree-walker' else []) + [str(program)] + args
    nofile = workload.get('nofile')

    def limits():
        if nofile:
            resource.setrlimit(resource.RLIMIT_NOFILE, (nofile, resource.getrlimit(resource.RLIMIT_NOFILE)[1]))

    # Its own session, so that a timeout kills the interpreter under time(1).
    with open(stdin or os.devnull) as input_file:
        process = subprocess.Popen(timing_command() + command, cwd=cwd, env=env, stdin=input_file,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                   start_new_session=True, preexec_fn=limits)
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.communicate()
            raise RuntimeError(f'timed out after {timeout} s') from None
    if process.returncode != 0:
        raise RuntimeError(f'exited {process.returncode}: {stderr.strip()[-400:]}')
    if check == 'larceny' and ('ERROR' in stdout or 'Elapsed time' not in stdout):
        raise RuntimeError(f'wrong result: {stdout.strip()[-300:]}')
    metrics = parse_time(stderr)
    metrics.update(parse_log(log))
    metrics.update(parse_final(stdout))
    metrics['output'] = re.sub(r'GC_FINAL.*', '', stdout).strip() if check == 'output' else None
    return metrics


def build_side(label, root, env):
    """{'label', 'root', 'binary', 'example'} for a checkout at `root`,
    building its release binary (and an example when asked)."""
    def cargo(*args):
        subprocess.run(['cargo', 'build', '--release', *args], cwd=root, env=env, check=True,
                       stdout=subprocess.DEVNULL)
    cargo('-p', 'patina-repl', '--bin', 'patina')

    built = {}

    def example(name):
        if name not in built:
            try:
                cargo('-p', 'patina-interpreter', '--features', 'vm', '--example', name)
            except subprocess.CalledProcessError as error:
                raise RuntimeError(f'{label} has no example {name}') from error
            built[name] = str(Path(root) / 'target/release/examples' / name)
        return built[name]
    return {'label': label, 'root': str(root), 'binary': str(Path(root) / 'target/release/patina'),
            'example': example}


def base_checkout(revision, directory):
    """A detached worktree of `revision` under `directory`, reused when it
    is already at that revision."""
    sha = subprocess.run(['git', 'rev-parse', revision], cwd=ROOT, capture_output=True, text=True,
                         check=True).stdout.strip()
    path = directory / f'base-{sha[:12]}'
    if not path.exists():
        subprocess.run(['git', 'worktree', 'add', '--detach', str(path), sha], cwd=ROOT, check=True,
                       stdout=subprocess.DEVNULL)
    return path, sha


# ---------------------------------------------------------------- analysis

def geomean(xs):
    return math.exp(sum(math.log(x) for x in xs) / len(xs))


def bootstrap(xs, draws=2000, seed=649):
    """The 95% interval of the geomean of `xs` by resampling."""
    rng = random.Random(seed)
    samples = sorted(geomean([rng.choice(xs) for _ in xs]) for _ in range(draws))
    return samples[int(0.025 * draws)], samples[int(0.975 * draws) - 1]


INTERVALS = ('wall_s', 'max_rss', 'peak_footprint', 'pause_max_us', 'mark_mean_us', 'forced_pause_us')


def median_interval(xs, draws=2000, seed=649):
    """The 95% interval of the median of `xs` by resampling."""
    rng = random.Random(seed)
    samples = sorted(statistics.median([rng.choice(xs) for _ in xs]) for _ in range(draws))
    return samples[int(0.025 * draws)], samples[int(0.975 * draws) - 1]


def summarize(rows, comparing):
    """Per (workload, backend): medians per side; for a comparison, ratios
    with intervals for instructions, cycles and wall time."""
    by = {}
    for row in rows:
        by.setdefault((row['workload'], row['backend']), []).append(row)
    summary = []
    for (name, backend), group in by.items():
        entry = {'workload': name, 'backend': backend}
        sides = ('base', 'branch') if comparing else ('branch',)
        for side in sides:
            runs = [r['metrics'] for r in group if r['side'] == side]
            for key in ('wall_s', 'user_s', 'instructions', 'cycles', 'max_rss', 'peak_footprint',
                        'page_reclaims', 'collections', 'paced', 'pause_max_us', 'pause_total_us',
                        'mark_max_us', 'mark_mean_us', 'forced_pause_us', 'live_bytes', 'committed_bytes',
                        'resident_bytes', 'wait_max_bytes', 'deferral_max_bytes'):
                values = [r[key] for r in runs if r.get(key) is not None]
                entry[f'{side}_{key}'] = statistics.median(values) if values else None
                if key in INTERVALS and len(values) > 1:
                    entry[f'{side}_{key}_ci'] = median_interval(values)
            for key in ('wait_site', 'deferral_site'):
                sites = sorted({r[key] for r in runs if r.get(key)})
                entry[f'{side}_{key}'] = sites or None
            for w in MMU_WINDOWS_MS:
                values = [r['mmu'][w] for r in runs if r.get('mmu') and w in r['mmu']]
                entry[f'{side}_mmu_{w}ms'] = statistics.median(values) if values else None
        if comparing:
            rounds = {}
            for r in group:
                rounds.setdefault(r['round'], {}).setdefault(r['side'], []).append(r['metrics'])
            for key in ('instructions', 'cycles', 'wall_s'):
                ratios = []
                for sides_of in rounds.values():
                    base = [m[key] for m in sides_of.get('base', []) if m.get(key)]
                    branch = [m[key] for m in sides_of.get('branch', []) if m.get(key)]
                    if base and branch:
                        ratios.append(branch[0] / statistics.mean(base))
                if ratios:
                    low, high = bootstrap(ratios)
                    entry[f'{key}_ratio'] = (geomean(ratios), low, high)
        summary.append(entry)
    return summary


def overall(summary, backend, key):
    """The geomean of the workloads' ratios, with its interval by
    resampling workloads."""
    centers = [s[f'{key}_ratio'][0] for s in summary if s['backend'] == backend and s.get(f'{key}_ratio')]
    if not centers:
        return None
    return (geomean(centers), *bootstrap(centers))


def print_table(summary, comparing):
    def pct(r):
        return f'{100 * (r[0] - 1):+.2f}% [{100 * (r[1] - 1):+.2f}, {100 * (r[2] - 1):+.2f}]' if r else '-'

    def mib(v):
        return f'{v / 2**20:.0f}' if v else '-'
    for s in summary:
        side = 'branch'
        line = (f'{s["workload"]:26} {s["backend"]:11} {s[f"{side}_wall_s"] or 0:6.2f}s '
                f'rss {mib(s[f"{side}_max_rss"]):>5} MiB, footprint {mib(s[f"{side}_peak_footprint"]):>5} MiB, '
                f'{s[f"{side}_collections"] or 0:>5g} GCs, max pause {(s[f"{side}_pause_max_us"] or 0) / 1000:7.2f} ms, '
                f'MMU(10 ms) {s[f"{side}_mmu_10ms"] if s[f"{side}_mmu_10ms"] is not None else 1:.2f}')
        if comparing:
            line += f' | instructions {pct(s.get("instructions_ratio"))}, cycles {pct(s.get("cycles_ratio"))}'
        print(line)
    if comparing:
        for backend in ('vm', 'tree-walker'):
            for key in ('instructions', 'cycles', 'wall_s'):
                g = overall(summary, backend, key)
                if g:
                    print(f'{backend} geomean {key}: {pct(g)}')


def measure(args, workloads, sides, scratch):
    """The runs: per round, each workload and backend on each side in turn,
    under one environment size drawn for the round."""
    order = SIDES_ABA if 'base' in sides else ('branch',)
    rng = random.Random(args.seed)
    rows, skipped, failed = [], {}, {}
    for round_number in range(args.rounds):
        for workload in workloads:
            name = workload['name']
            for backend in workload.get('backends', ['vm']):
                if name in skipped or name in failed:
                    continue
                pad = rng.randrange(PAD_MAX)
                outputs = []
                for position, side in enumerate(order):
                    try:
                        metrics = run_one(sides[side], workload, backend, scratch, pad, args.timeout)
                    except Skip as reason:
                        skipped[name] = str(reason)
                        break
                    except RuntimeError as error:
                        failed[name] = f'{side} {backend}: {error}'
                        break
                    outputs.append(metrics.pop('output'))
                    rows.append({'workload': name, 'backend': backend, 'side': side, 'round': round_number,
                                 'position': position, 'pad': pad, 'metrics': metrics})
                if len({o for o in outputs if o is not None}) > 1 and not workload.get('timing_output'):
                    failed[name] = f'{backend}: the sides printed different answers: {outputs}'
        print(f'round {round_number + 1} of {args.rounds} done', flush=True)
    return rows, skipped, failed


def gc_mode(args, directory, env):
    """benchmarks.py gc: build each side, measure, print the table; the
    report's fields and whether any workload failed."""
    sets = args.set or ['gbs', 'probes', 'barrier', 'io', 'load', 'tw', 'twins']
    if args.large_live:
        sets = list(sets) + ['large-live']
    workloads = load_workloads(sets, args.workload)
    scratch = directory / 'scratch'
    scratch.mkdir(parents=True, exist_ok=True)
    worktree, base_sha = None, None
    try:
        sides = {'branch': build_side('branch', ROOT, env)}
        if args.base:
            if Path(args.base).is_dir():
                root = Path(args.base).resolve()
            else:
                root, base_sha = base_checkout(args.base, ROOT / 'target/benchmark-runs/worktrees')
                worktree = None if args.keep_base else root
            sides['base'] = build_side('base', root, env)
        rows, skipped, failed = measure(args, workloads, sides, scratch)
    finally:
        # A base worktree builds its own target/, a gigabyte or more.
        if worktree:
            subprocess.run(['git', 'worktree', 'remove', '--force', str(worktree)], cwd=ROOT, check=False)
    for name, reason in skipped.items():
        print(f'SKIPPED {name}: {reason}')
    for name, reason in failed.items():
        print(f'FAILED {name}: {reason}')
    comparing = bool(args.base)
    summary = summarize(rows, comparing)
    print_table(summary, comparing)
    overall_rows = {f'{backend} {key}': overall(summary, backend, key)
                    for backend in ('vm', 'tree-walker') for key in ('instructions', 'cycles', 'wall_s')
                    if comparing and overall(summary, backend, key)}
    report = dict(kind='gc', base=base_sha or args.base, sets=sets, rounds=args.rounds, seed=args.seed,
                  gbs_sha256=hashlib.sha256((GC / 'gbs.json').read_bytes()).hexdigest(),
                  larceny=str(larceny_dir()), skipped=skipped, failed=failed, geomeans=overall_rows,
                  measurements=summary, runs=rows)
    return report, bool(failed)
