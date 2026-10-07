#!/usr/bin/env python3
"""The steady-state lane (PRD/GC_PRD.md §17.1 and §17.4, #652).

Each row is a program that repeats one cycle 4N times through
crates/patina-tests/bench_programs/gc/steady/driver.scm, which prints the
heap's readings at N, 2N, 3N and 4N. This runner runs every row three times
on each backend, under a timeout and a resident-size watchdog, and holds it to
the clauses it declares:

  SS1  L(4N) - L(N) <= 3N * 8 B + 64 KiB          (L: live-bytes after (gc))
  SS2  footprint <= F(L) at N and at 4N, F(L) = 1.1 * (L + max(8 MiB, 2L))
       plus the free reserve, max(8 blocks of 32 KiB, 5% of L + max(8 MiB, 2L))
       (footprint: committed-bytes plus external-bytes)
  SS4  mutator CPU per cycle over [3N, 4N) <= 1.5 x that over [N, 2N),
       each segment the minimum of the runs (cpu-us less pause-total-us)
  SS5  symbols(4N) within max(16, 5%) of symbols(N)

A clause a row fails today is marked red with the issue or stage that fixes
it; it is reported, and a red clause that passes is called out for promotion,
but neither fails the lane. Every other failure does: a clause, a run that
exits non-zero, a missing reading, a timeout or the watchdog's cap.

usage: run_steady_state.py [--quick] [--backend vm|tree-walker|both]
                           [--row NAME ...] [--runs 3] [--binary PATH]
"""
import argparse
import dataclasses
import os
import re
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STEADY = ROOT / 'crates/patina-tests/bench_programs/gc/steady'
MIB = 1 << 20
KIB = 1 << 10
BACKENDS = ('vm', 'tree-walker')
CLAUSES = ('SS1', 'SS2', 'SS4', 'SS5')


@dataclasses.dataclass(frozen=True)
class Row:
    """One row: `program` is a file under STEADY, or `stream:<form>` for a
    stream of top-level forms the runner writes, fed as a file, on standard
    input or to the REPL (`mode`). `n` is N per backend; `red` maps a clause,
    or `clause/backend`, to the issue or stage that fixes it."""
    name: str
    group: str
    program: str
    n: dict
    args: tuple = ()
    clauses: tuple = CLAUSES
    red: dict = dataclasses.field(default_factory=dict)
    pr: bool = False
    mode: str = 'file'
    # Class D: the program keeps a growing name set, outside the contract.
    # The lane holds its cost per cycle to 1.2x the pinned bytes.
    per_cycle: dict = None
    # A row that drops a peak before its window: L at N and at 4N must be
    # below this, or the peak is still live ('peak').
    live_max: int = None

    def red_reason(self, clause, backend):
        return self.red.get(f'{clause}/{backend}') or self.red.get(clause)


STREAM_FORMS = {
    'define': '(define (f x) (let ((y (* x 2))) (if (> y 10) (- y 1) (+ y 1)))) (f 3)',
    'guard': "(guard (e (#t 0)) (raise 'x))",
    'library': '(define-library (tmp steady) (export f g) (import (scheme base))'
               ' (begin (define (f x) (+ x 1)) (define g (list 1 2 3))))'
               ' (import (tmp steady)) (f 1)',
}

# The arenas keep their high-water, the deferred library loads' among them,
# so a row whose footprint passes F(L) at both readings, without growing,
# is red for #616 (PRD/GC_PRD.md stage 5e).
HIGH_WATER = '#616'

# What each row repeats is in its program's header comment.
# REPL rows hold SS1, SS4 and SS5: what they leak shows in L and in the
# per-owner counts. SS2 is held where a row allocates enough per segment for
# two paced collections, so the footprint is read at its plateau; the REPL
# rows' footprints are the high-water of the deferred library loads (#616),
# which steady-alloc and the peak rows already hold the line on.
REPL = ('SS1', 'SS4', 'SS5')

ROWS = [
    # REPL-style.
    Row('eval-redefine', 'REPL', 'eval-redefine.scm', {'vm': 4000, 'tree-walker': 4000}, clauses=REPL, pr=True),
    Row('eval-lambda', 'REPL', 'eval-lambda.scm', {'vm': 4000, 'tree-walker': 4000}, clauses=REPL, pr=True),
    Row('macro-eval-case', 'REPL', 'macro-eval.scm', {'vm': 4000, 'tree-walker': 4000}, ('case',),
        clauses=REPL, pr=True),
    Row('macro-eval-guard', 'REPL', 'macro-eval.scm', {'vm': 2000, 'tree-walker': 2000}, ('guard',),
        clauses=REPL, red={'SS1': '#611', 'SS5': '#611'}),
    Row('macro-eval-let-values', 'REPL', 'macro-eval.scm', {'vm': 4000, 'tree-walker': 4000}, ('let-values',),
        clauses=REPL),
    Row('macro-eval-parameterize', 'REPL', 'macro-eval.scm', {'vm': 4000, 'tree-walker': 4000},
        ('parameterize',), clauses=REPL),
    Row('macro-eval-case-lambda', 'REPL', 'macro-eval.scm', {'vm': 4000, 'tree-walker': 4000},
        ('case-lambda',), clauses=REPL),
    Row('macro-eval-delay', 'REPL', 'macro-eval.scm', {'vm': 4000, 'tree-walker': 4000}, ('delay',),
        clauses=REPL),
    Row('macro-eval-do', 'REPL', 'macro-eval.scm', {'vm': 4000, 'tree-walker': 4000}, ('do',), clauses=REPL),
    Row('macro-eval-record', 'REPL', 'macro-eval.scm', {'vm': 2000, 'tree-walker': 2000}, ('record',),
        clauses=REPL),
    Row('macro-eval-let', 'REPL', 'macro-eval.scm', {'vm': 4000, 'tree-walker': 4000}, ('let',), clauses=REPL),
    # N = 2,000: #613's time per expansion grows with the expansions before
    # it, about 2x over the window here, clear of SS4's 1.5.
    Row('hidden-define', 'REPL', 'hidden-define.scm', {'vm': 2000, 'tree-walker': 2000}, clauses=REPL,
        red={'SS1/vm': '#613', 'SS4': '#613'}),
    Row('unbound-ref', 'REPL', 'unbound-ref.scm', {'vm': 4000, 'tree-walker': 4000}, clauses=REPL,
        red={'SS1': 'stage 5c', 'SS5': 'stage 5c'}),
    Row('load-repeat', 'REPL', 'load-repeat.scm', {'vm': 1000, 'tree-walker': 1000}, clauses=REPL),
    Row('reimport', 'REPL', 'reimport.scm', {'vm': 1000, 'tree-walker': 1000}, clauses=REPL),
    Row('record-redefine', 'REPL', 'record-redefine.scm', {'vm': 2000, 'tree-walker': 2000}, clauses=REPL),
    Row('define-stream-file', 'REPL', 'stream:define', {'vm': 2000, 'tree-walker': 2000}, clauses=REPL),
    Row('define-stream-stdin', 'REPL', 'stream:define', {'vm': 2000, 'tree-walker': 2000}, clauses=REPL,
        mode='stdin'),
    Row('define-stream-repl', 'REPL', 'stream:define', {'vm': 2000, 'tree-walker': 2000}, clauses=REPL,
        mode='repl'),
    Row('guard-stream-file', 'REPL', 'stream:guard', {'vm': 1000, 'tree-walker': 1000}, clauses=REPL,
        red={'SS1': '#611', 'SS5': '#611'}),
    Row('guard-stream-stdin', 'REPL', 'stream:guard', {'vm': 1000, 'tree-walker': 1000}, clauses=REPL,
        red={'SS1': '#611', 'SS5': '#611'}, mode='stdin'),
    Row('guard-stream-repl', 'REPL', 'stream:guard', {'vm': 1000, 'tree-walker': 1000}, clauses=REPL,
        red={'SS1': '#611', 'SS5': '#611'}, mode='repl'),
    Row('library-stream', 'REPL', 'stream:library', {'vm': 250, 'tree-walker': 250}, clauses=REPL,
        red={'SS1': '#614'}),
    # Server-style.
    Row('steady-alloc', 'server', 'steady-alloc.scm', {'vm': 100000, 'tree-walker': 50000}, pr=True,
        red={'SS2/vm': '#616'}),
    Row('peak-then-drop', 'server', 'peak-then-drop.scm', {'vm': 200000, 'tree-walker': 100000},
        ('2000000',), live_max=4 * MIB,
        red={'SS2/vm': '#616', 'SS2/tree-walker': '#655', 'peak/tree-walker': '#655'}),
    Row('peak-then-sparse', 'server', 'peak-then-sparse.scm', {'vm': 200000, 'tree-walker': 100000},
        ('2000000',), live_max=4 * MIB,
        red={'SS2/vm': '#616', 'SS2/tree-walker': '#655', 'peak/tree-walker': '#655'}),
    Row('deep-then-steady', 'server', 'deep-then-steady.scm', {'vm': 200000, 'tree-walker': 100000},
        ('200000',), red={'SS2/vm': '#616'}),
    Row('port-churn', 'server', 'port-churn.scm', {'vm': 60000, 'tree-walker': 10000}, pr=True),
    Row('file-port-churn', 'server', 'file-port-churn.scm', {'vm': 5000, 'tree-walker': 5000},
        ('{scratch}/file-port-churn.txt',)),
    # Churn.
    Row('sym-churn', 'churn', 'sym-churn.scm', {'vm': 20000, 'tree-walker': 20000}, clauses=REPL,
        red={'SS1': 'stage 5c', 'SS5': 'stage 5c'}),
    Row('eval-fresh-names', 'churn', 'eval-fresh-names.scm', {'vm': 4000, 'tree-walker': 4000},
        clauses=('SS4',), per_cycle={'vm': 180, 'tree-walker': 180}),
    Row('env-churn', 'churn', 'env-churn.scm', {'vm': 500, 'tree-walker': 500}),
    Row('env-lambda', 'churn', 'env-lambda.scm', {'vm': 500, 'tree-walker': 500}),
    Row('form-eval-fresh', 'churn', 'form-eval-fresh.scm', {'vm': 4000, 'tree-walker': 4000}, clauses=REPL,
        pr=True),
    Row('cont-churn-10', 'churn', 'cont-churn.scm', {'vm': 20000, 'tree-walker': 10000}, ('10',), pr=True),
    Row('cont-churn-100', 'churn', 'cont-churn.scm', {'vm': 5000, 'tree-walker': 2000}, ('100',),
        red={'SS2/tree-walker': '#656'}),
    Row('cont-churn-1000', 'churn', 'cont-churn.scm', {'vm': 1000, 'tree-walker': 500}, ('1000',),
        red={'SS2/tree-walker': '#656'}),
]

READING = re.compile(r'(STEADY(?:-AFTER)?) at=(\d+)((?: [a-z-]+=\S+)*)')


def parse(stdout):
    """{'pre': {at: {key: value}}, 'after': {at: {key: value}}} from a run's
    output. A REPL may print a prompt before a reading, so a reading is
    found anywhere in its line."""
    readings = {'pre': {}, 'after': {}}
    for line in stdout.splitlines():
        m = READING.search(line)
        if not m:
            continue
        kind = 'after' if m.group(1) == 'STEADY-AFTER' else 'pre'
        values = {}
        for pair in m.group(3).split():
            key, _, value = pair.partition('=')
            values[key] = None if value == 'none' else int(value)
        readings[kind][int(m.group(2))] = values
    return readings


def footprint_bound(live):
    """F(L), with the free reserve (§17.1)."""
    target = live + max(8 * MIB, 2 * live)
    return 1.1 * target + max(8 * 32 * KIB, 0.05 * target)


def leak_floor(n):
    """The smallest leak per cycle SS1 can see at N."""
    return (3 * n * 8 + 64 * KIB) / (3 * n)


def segment(run, start, end, n, start_after):
    """Mutator CPU per cycle from `start` to `end`: cpu-us less pause-us."""
    a = run['after' if start_after else 'pre'][start]
    b = run['pre'][end]
    return ((b['cpu-us'] - a['cpu-us']) - (b['pause-total-us'] - a['pause-total-us'])) / n


def evaluate(row, backend, runs, n):
    """Each declared clause's value, bound and whether it holds."""
    median = lambda f: statistics.median(f(r) for r in runs)
    first = lambda r: r['after'][n]
    last = lambda r: r['after'][4 * n]
    live1, live4 = median(lambda r: first(r)['live-bytes']), median(lambda r: last(r)['live-bytes'])
    results = {}
    if 'SS1' in row.clauses:
        bound = 3 * n * 8 + 64 * KIB
        results['SS1'] = (live4 - live1, bound, live4 - live1 <= bound)
    if 'SS2' in row.clauses:
        # Paced collections in the window's two segments: at least two in
        # each, or the footprint was never read at a plateau (§17.1).
        paced = min(min(r['pre'][2 * n]['collections'] - r['after'][n]['collections'],
                        r['pre'][4 * n]['collections'] - r['pre'][3 * n]['collections'])
                    for r in runs)
        worst = None
        for point in (first, last):
            live = median(lambda r: point(r)['live-bytes'])
            fp = median(lambda r: point(r)['committed-bytes'] + point(r)['external-bytes'])
            ratio = fp / footprint_bound(live)
            worst = ratio if worst is None else max(worst, ratio)
        results['SS2'] = (worst, 1.0, worst <= 1.0) if paced >= 2 else (f'vacuous ({paced} paced)', 1.0, False)
    if 'SS4' in row.clauses:
        early = min(segment(r, n, 2 * n, n, True) for r in runs)
        late = min(segment(r, 3 * n, 4 * n, n, False) for r in runs)
        ratio = late / early if early > 0 else 1.0
        results['SS4'] = (ratio, 1.5, ratio <= 1.5)
    if 'SS5' in row.clauses:
        s1, s4 = median(lambda r: first(r)['symbols']), median(lambda r: last(r)['symbols'])
        bound = max(16, 0.05 * s1)
        results['SS5'] = (s4 - s1, bound, abs(s4 - s1) <= bound)
    if row.per_cycle:
        pinned = row.per_cycle[backend]
        cost = (live4 - live1) / (3 * n)
        results['D'] = (cost, 1.2 * pinned, cost <= 1.2 * pinned)
    if row.live_max:
        peak = max(live1, live4)
        results['peak'] = (peak, row.live_max, peak <= row.live_max)
    return results


def describe(runs, n):
    """L and the footprint at N and 4N, in KiB, from the first run."""
    def at(point):
        r = runs[0]['after'][point]
        return r['live-bytes'] // KIB, (r['committed-bytes'] + r['external-bytes']) // KIB
    (l1, f1), (l4, f4) = at(n), at(4 * n)
    return f'L {l1}->{l4} KiB, footprint {f1}->{f4} KiB'


def resident_kib(pid):
    out = subprocess.run(['ps', '-o', 'rss=', '-p', str(pid)], capture_output=True, text=True).stdout
    return int(out.strip()) if out.strip() else 0


def program_text(row, n):
    """A stream row's program: the driver, then 4N top-level forms with a
    reading after each N."""
    form = STREAM_FORMS[row.program.split(':', 1)[1]]
    driver = (STEADY / 'driver.scm').read_text()
    parts = ['(import (scheme base) (scheme write) (scheme process-context) (patina debug))', driver]
    for k in range(1, 5):
        parts.extend([form] * n)
        parts.append(f'(steady-reading {k * n} {"#t" if k in (1, 4) else "#f"})')
    return '\n'.join(parts) + '\n'


def run_once(binary, row, backend, n, timeout, cap_mib, scratch):
    """One run: (readings, failure or None, seconds, max RSS in MiB)."""
    cmd = [str(binary)] + (['--tree-walker'] if backend == 'tree-walker' else [])
    stdin = subprocess.DEVNULL
    if row.program.startswith('stream:'):
        path = Path(scratch) / f'{row.name}.scm'
        path.write_text(program_text(row, n))
        if row.mode == 'file':
            cmd.append(str(path))
        else:
            if row.mode == 'repl':
                cmd.append('-i')
            stdin = path.open()
    else:
        args = [a.replace('{scratch}', scratch) for a in row.args]
        cmd += [str(STEADY / row.program), str(n)] + args
    env = {k: v for k, v in os.environ.items() if not k.startswith('PATINA_')}
    start = time.monotonic()
    child = subprocess.Popen(cmd, cwd=STEADY, env=env, stdin=stdin, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True)
    failure = None
    with tempfile.TemporaryFile('w+') as out, tempfile.TemporaryFile('w+') as err:
        # Drain the pipes on threads so the watchdog can poll.
        def pump(src, dst):
            for chunk in iter(lambda: src.read(65536), ''):
                dst.write(chunk)
        threads = [threading.Thread(target=pump, args=(child.stdout, out)),
                   threading.Thread(target=pump, args=(child.stderr, err))]
        for t in threads:
            t.start()
        # Reaped by wait4 alone, for the child's own resource usage.
        while True:
            pid, status, usage = os.wait4(child.pid, os.WNOHANG)
            if pid == child.pid:
                break
            if failure is None and time.monotonic() - start > timeout:
                child.kill()
                failure = f'timed out after {timeout} s'
            elif failure is None and resident_kib(child.pid) > cap_mib * 1024:
                child.kill()
                failure = f'resident size passed the {cap_mib} MiB cap'
            time.sleep(0.1)
        child.returncode = os.waitstatus_to_exitcode(status)
        for t in threads:
            t.join()
        out.seek(0)
        err.seek(0)
        stdout, stderr = out.read(), err.read()
    if stdin is not subprocess.DEVNULL:
        stdin.close()
    seconds = time.monotonic() - start
    # ru_maxrss is in bytes on macOS and in KiB on Linux.
    maxrss = usage.ru_maxrss / (MIB if sys.platform == 'darwin' else KIB)
    if failure is None and child.returncode != 0:
        failure = f'exited {child.returncode}: {stderr.strip()[-300:]}'
    readings = parse(stdout)
    if failure is None:
        missing = [at for at in (n, 4 * n) if at not in readings['after']]
        missing += [at for at in (2 * n, 3 * n, 4 * n) if at not in readings['pre']]
        if missing:
            failure = f'no reading at {missing}'
    return readings, failure, seconds, maxrss


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--quick', action='store_true', help='only the per-PR rows')
    parser.add_argument('--backend', choices=['vm', 'tree-walker', 'both'], default='both')
    parser.add_argument('--row', action='append', default=[], help='run only this row (repeatable)')
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--binary', default=str(ROOT / 'target/release/patina'))
    parser.add_argument('--timeout', type=int, default=600, help='seconds per run')
    parser.add_argument('--cap', type=int, default=4096, help='resident-size cap per run, MiB')
    parser.add_argument('--scale', type=float, default=1.0, help='multiply every N')
    args = parser.parse_args(argv)
    backends = BACKENDS if args.backend == 'both' else (args.backend,)
    rows = [r for r in ROWS if (not args.quick or r.pr) and (not args.row or r.name in args.row)]
    if args.row and len(rows) != len(args.row):
        parser.error(f'unknown rows: {sorted(set(args.row) - {r.name for r in ROWS})}')
    failed = []
    promote = []
    lane_start = time.monotonic()
    with tempfile.TemporaryDirectory() as scratch:
        for row in rows:
            for backend in backends:
                n = max(1, int(row.n[backend] * args.scale))
                runs, problem, seconds, maxrss = [], None, 0.0, 0.0
                for _ in range(args.runs):
                    readings, failure, secs, rss = run_once(args.binary, row, backend, n,
                                                            args.timeout, args.cap, scratch)
                    seconds, maxrss = seconds + secs, max(maxrss, rss)
                    if failure:
                        problem = failure
                        break
                    runs.append(readings)
                label = f'{row.name} {backend} N={n}'
                if problem:
                    reasons = {row.red_reason(c, backend) for c in row.clauses} - {None}
                    if reasons and 'resident size' in problem:
                        print(f'RED  {label}: {problem} ({", ".join(sorted(reasons))})')
                    else:
                        print(f'FAIL {label}: {problem}')
                        failed.append(label)
                    continue
                results = evaluate(row, backend, runs, n)
                parts, verdict = [], 'OK  '
                for clause, (value, bound, holds) in results.items():
                    red = row.red_reason(clause, backend)
                    shown = (f'{clause} {value:.2f}/{bound:.2f}' if isinstance(value, float)
                             else f'{clause} {value}' if isinstance(value, str)
                             else f'{clause} {value}/{int(bound)}')
                    if holds:
                        parts.append(shown + (f' passes, red for {red}: promote' if red else ''))
                        if red:
                            promote.append(f'{label} {clause} ({red})')
                    elif red:
                        parts.append(f'{shown} red ({red})')
                        verdict = 'RED ' if verdict == 'OK  ' else verdict
                    else:
                        parts.append(f'{shown} FAILS')
                        verdict = 'FAIL'
                if verdict == 'FAIL':
                    failed.append(label)
                print(f'{verdict} {label}: {"; ".join(parts)}; {describe(runs, n)}; '
                      f'leak floor {leak_floor(n):.1f} B/cycle; {seconds:.1f} s, max RSS {maxrss:.0f} MiB',
                      flush=True)
    print(f'\nsteady-state lane: {time.monotonic() - lane_start:.0f} s')
    if promote:
        print('Red clauses that now pass (promote them):')
        for p in promote:
            print(f'  {p}')
    if failed:
        print(f'{len(failed)} row(s) failed:')
        for f in failed:
            print(f'  {f}')
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
