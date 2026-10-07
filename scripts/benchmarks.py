#!/usr/bin/env python3
"""Checked benchmark runners. Public entry points remain the .sh scripts:
run_benchmarks.sh (criterion), bench_compare.sh (compare) and
run_gc_benchmarks.sh (gc, scripts/gc_bench.py)."""
import argparse
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

import gc_bench

ROOT = Path(__file__).resolve().parent.parent
WORKLOADS = ROOT / 'crates/patina-tests/bench_programs/workloads.json'


class BenchmarkError(Exception):
    pass


def run(command, *, env=None, log=None, capture_log=None):
    """Stream Cargo diagnostics; capture interpreter protocol without hiding stderr."""
    if log is not None:
        with log.open('w') as output:
            process = subprocess.Popen(command, cwd=ROOT, env=env, text=True,
                                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            try:
                for line in process.stdout:
                    print(line, end='', flush=True)
                    output.write(line)
                status = process.wait()
            finally:
                process.stdout.close()
                if process.poll() is None:
                    process.kill()
                    process.wait()
        if status:
            raise BenchmarkError(f'{command[0]} exited {status}; diagnostics: {log}')
        return ''
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, capture_output=True)
    if capture_log is not None:
        capture_log.write_text(f'Command: {command!r}\nExit code: {result.returncode}\n'
                               f'{result.stdout}{result.stderr}')
    if result.stderr:
        print(result.stderr, end='', file=sys.stderr)
    if result.returncode:
        raise BenchmarkError(f'{command!r} exited {result.returncode}\n{result.stdout}')
    return result.stdout


def positive(value, context):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
        raise BenchmarkError(f'{context}: expected a finite positive measurement, got {value!r}')
    return value


def metadata(args):
    revision = run(['git', 'rev-parse', 'HEAD']).strip()
    dirty = bool(run(['git', 'status', '--porcelain']).strip())
    cpu = platform.processor() or platform.machine()
    if sys.platform == 'darwin':
        result = subprocess.run(['sysctl', '-n', 'machdep.cpu.brand_string'], text=True, capture_output=True)
        cpu = result.stdout.strip() if result.returncode == 0 else f'{cpu} (CPU model unavailable)'
    return dict(schema_version=1, started_utc=dt.datetime.now(dt.timezone.utc).isoformat(),
                revision=revision, dirty=dirty, toolchain=run(['rustc', '--version']).strip(),
                platform=platform.platform(), architecture=platform.machine(), cpu=cpu,
                logical_cpus=os.cpu_count(), runner_arguments=vars(args),
                build_environment={key: value for key, value in os.environ.items()
                                   if key in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_TARGET', 'CARGO_TARGET_DIR')
                                   or key.startswith(('CARGO_PROFILE_RELEASE_', 'CARGO_PROFILE_BENCH_'))},
                workload_sha256=hashlib.sha256(WORKLOADS.read_bytes()).hexdigest(),
                library_policy='isolated workspace libraries', correctness='passed')


def environment():
    env = os.environ.copy()
    env['PATINA_ISOLATED_LIBRARIES'] = '1'
    # GC/trace overrides would invalidate a normal-runtime baseline.
    env.pop('PATINA_GC', None)
    env.pop('PATINA_SCOPE_TRACE', None)
    env.pop('PATINA_AMBIGUITY_LOG', None)
    return env


def compare_source(case):
    setup = case['setup']
    if case['file']:
        setup = (WORKLOADS.parent / case['file']).read_text() + '\n' + setup
    return f'''(import (scheme base) (scheme time) (scheme write))
{setup}
(unless {case['check']} (error "benchmark precondition failed"))
(let* ((start (current-jiffy))
       (answer {case['expression']})
       (elapsed (- (current-jiffy) start)))
  (unless (equal? answer {case['expected']}) (error "incorrect benchmark answer" answer))
  (display "BENCH_OK ")
  (display (/ (* elapsed 1000.0) (jiffies-per-second)))
  (newline))
'''


def build_binary(directory, env):
    # Cargo reports the actual executable, including CARGO_TARGET_DIR overrides.
    output = run(['cargo', 'build', '--release', '--package', 'patina-repl', '--bin', 'patina',
                  '--message-format=json-render-diagnostics'], env=env, capture_log=directory / 'build.log')
    executables = []
    for line in output.splitlines():
        record = json.loads(line)
        if record.get('reason') == 'compiler-artifact' and record.get('target', {}).get('name') == 'patina' and record.get('executable'):
            executables.append(record['executable'])
    if len(executables) != 1:
        raise BenchmarkError('Cargo did not report exactly one patina executable')
    return executables[0]


def compare(args, directory):
    cases = [case for case in json.loads(WORKLOADS.read_text())
             if case['compare'] and (not args.quick or case['quick']) and args.filter in case['id']]
    if not cases:
        raise BenchmarkError('no comparison workloads match the requested filter')
    env = environment()
    binary = build_binary(directory, env)
    rows = []
    # Temporary sources are removed even on errors and KeyboardInterrupt.
    with tempfile.TemporaryDirectory(prefix='patina-bench-') as temporary:
        for index, case in enumerate(cases):
            source = Path(temporary) / f'{index}.scm'
            source.write_text(compare_source(case))
            row = dict(id=case['id'], expression=case['expression'], expected=case['expected'], extra_check=case['check'])
            for backend in ('tree-walker', 'vm'):
                print(f'Checking and timing {backend}: {case["id"]}', flush=True)
                command = [binary, '--isolated-libraries']
                if backend == 'tree-walker':
                    command.append('--tree-walker')
                output = run(command + [str(source)], env=env, capture_log=directory / f'{index}-{backend}.log')
                parts = output.split()
                if len(parts) != 2 or parts[0] != 'BENCH_OK':
                    raise BenchmarkError(f'{backend} {case["id"]}: missing checked timing result: {output!r}')
                try:
                    value = float(parts[1])
                except ValueError as error:
                    raise BenchmarkError(f'{backend} {case["id"]}: invalid timing {parts[1]!r}') from error
                row[backend + '_ms'] = positive(value, f'{backend} {case["id"]}')
            rows.append(row)
    return dict(kind='comparison', backends=['tree-walker', 'vm'],
                timing='one execution after setup; Scheme timer, excludes process startup and compilation; includes automatic GC',
                limitations='single samples are smoke measurements, not evidence of a speedup', measurements=rows)


def criterion_results(home, checks, backend):
    verified = {}
    if checks.exists():
        for line in checks.read_text().splitlines():
            record = json.loads(line)
            if record.get('correctness') != 'passed':
                raise BenchmarkError(f'failed correctness check: {record}')
            if record['id'] in verified:
                raise BenchmarkError(f'duplicate correctness check: {record["id"]}')
            verified[record['id']] = record
    rows = []
    for path in sorted(home.glob('**/new/estimates.json')):
        identity = json.loads(path.with_name('benchmark.json').read_text())['full_id']
        if not identity.startswith(backend + '/'):
            raise BenchmarkError(f'wrong backend in Criterion result: {identity}')
        estimates = json.loads(path.read_text())
        median = estimates['median']
        center = positive(median['point_estimate'], identity)
        interval = median['confidence_interval']
        low = positive(interval['lower_bound'], identity)
        high = positive(interval['upper_bound'], identity)
        if not low <= center <= high:
            raise BenchmarkError(f'invalid median confidence interval: {identity}')
        samples = json.loads(path.with_name('sample.json').read_text())
        times, iters = samples['times'], samples['iters']
        if not times or len(times) != len(iters):
            raise BenchmarkError(f'empty or inconsistent samples: {identity}')
        for value in times + iters:
            positive(value, identity)
        rows.append(dict(id=identity, median_ns=center, confidence_interval_ns=[low, high], samples=len(times)))
    ids = [row['id'] for row in rows]
    if not rows:
        raise BenchmarkError('no fresh Criterion measurements; the filter may match no benchmarks')
    if len(ids) != len(set(ids)) or set(ids) != set(verified):
        raise BenchmarkError(f'Criterion results and correctness checks disagree: results={ids}, checks={list(verified)}')
    for row in rows:
        row['parameters'] = verified[row['id']]['parameters']
    return rows


def criterion(args, directory):
    home = directory / 'criterion'
    checks = directory / 'checks.jsonl'
    env = environment()
    env.update(CRITERION_HOME=str(home), PATINA_BENCH_BACKEND=args.backend, PATINA_BENCH_CHECKS=str(checks))
    settings = dict(sample_size=10 if args.quick else 20,
                    measurement_seconds=1 if args.quick else 2,
                    warmup_seconds=0.5 if args.quick else 3)
    command = ['cargo', 'bench', '--package', 'patina-tests', '--bench', 'scheme_benchmarks', '--',
               '--sample-size', str(settings['sample_size']), '--measurement-time', str(settings['measurement_seconds']),
               '--warm-up-time', str(settings['warmup_seconds']), '--noplot', args.filter]
    run(command, env=env, log=directory / 'cargo.log')
    return dict(kind='criterion', backend=args.backend, settings=settings, command=command,
                timing_contract='docs/VM_TESTING.md: Checked benchmark lanes (at the recorded revision)',
                measurements=criterion_results(home, checks, args.backend),
                counter_limitations='Separate probe: 1000 calls of list_256 plus driver parsing/compilation and answer check. Arena/free slots and collection counts, not bytes, RSS, allocation throughput or pause times')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest='kind', required=True)
    for name in ('compare', 'criterion'):
        sub = subparsers.add_parser(name)
        sub.add_argument('--quick', action='store_true', help='shorter samples / smaller comparison set')
        sub.add_argument('--filter', default='', help='workload substring (compare), Criterion regex (criterion)')
        sub.add_argument('--output', help='also save JSON here; must not already exist')
        if name == 'criterion':
            sub.add_argument('--backend', choices=['vm', 'tree-walker'], default=os.environ.get('PATINA_BENCH_BACKEND', 'vm'))
    sub = subparsers.add_parser('gc', help='the GC benchmark set (#649); see docs/TEST_ORGANIZATION.md')
    sub.add_argument('--base', help='a revision or checkout to compare against, run base/branch/base')
    sub.add_argument('--keep-base', action='store_true', help='keep the base worktree and its target/ for reuse')
    sub.add_argument('--rounds', type=int, default=10, help='rounds (default 10)')
    sub.add_argument('--set', action='append', help='gbs, probes, barrier, io, load, tw, twins, large-live '
                     '(repeatable; default all but large-live)')
    sub.add_argument('--workload', action='append', default=[], help='one workload by name (repeatable)')
    sub.add_argument('--large-live', action='store_true', help='add the large-live set (0.5 and 1 GiB live)')
    sub.add_argument('--seed', type=int, default=653, help='seed of the per-round environment sizes')
    sub.add_argument('--timeout', type=float, default=300, help='seconds per run')
    sub.add_argument('--output', help='also save JSON here; must not already exist')
    args = parser.parse_args(argv)
    if args.kind == 'gc' and args.rounds < 1:
        parser.error('--rounds must be at least 1')
    if args.kind == 'criterion' and args.backend not in ('vm', 'tree-walker'):
        parser.error('PATINA_BENCH_BACKEND must be vm or tree-walker')
    output = Path(args.output).resolve() if args.output else None
    if output and output.exists():
        parser.error(f'output already exists: {output}; choose a new filename')
    directory = None
    try:
        info = metadata(args)
        base = ROOT / 'target/benchmark-runs'
        base.mkdir(parents=True, exist_ok=True)
        directory = Path(tempfile.mkdtemp(prefix=dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ-'), dir=base))
        print(f'Run artifacts: {directory}', flush=True)
        if args.kind == 'gc':
            fields, failed = gc_bench.gc_mode(args, directory, environment())
            report = dict(info, **fields)
            if failed:
                # The others' measurements are kept, never as a success report.
                report['correctness'] = f'failed: {sorted(fields["failed"])}'
                (directory / 'failed-report.json').write_text(json.dumps(report, indent=2, allow_nan=False) + '\n')
                raise BenchmarkError(f'{len(fields["failed"])} workloads failed; measurements of the rest: '
                                     f'{directory / "failed-report.json"}')
        else:
            report = dict(info, **(compare(args, directory) if args.kind == 'compare' else criterion(args, directory)))
        payload = json.dumps(report, indent=2, allow_nan=False) + '\n'
        if output:
            # Exclusive creation protects existing baselines, including races.
            with output.open('x') as file:
                file.write(payload)
        (directory / 'report.json').write_text(payload)
        print(f'Validated {len(report["measurements"])} measurements. Report: {output or directory / "report.json"}')
        for row in report['measurements']:
            if args.kind == 'compare':
                print(f'{row["id"]}: tree-walker {row["tree-walker_ms"]:.4f} ms; vm {row["vm_ms"]:.4f} ms')
            elif args.kind == 'criterion':
                print(f'{row["id"]}: median {row["median_ns"] / 1000:.3f} us')
        return 0
    except (BenchmarkError, OSError, ValueError, KeyError, TypeError, RuntimeError,
            subprocess.CalledProcessError, KeyboardInterrupt) as error:
        print(f'Benchmark failed: {error}', file=sys.stderr)
        if directory:
            print(f'Incomplete run diagnostics: {directory} (no success report)', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
