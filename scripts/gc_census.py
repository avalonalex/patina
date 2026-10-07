"""The `census` mode of scripts/benchmarks.py (#651): the GC census over
the GC benchmark set, and the rows of #647's measurement table it answers.

It builds the release binary with `patina-core`'s `gc-census` feature into
its own target directory (target/gc-census, so the main build is left
alone) and runs each workload of crates/patina-tests/bench_programs/gc/
gbs.json once under the default trigger and once under each nursery size
of PATINA_GC_STRESS (16 K to 4 M allocations), with the census on
(crates/patina-core/src/census.rs) and #648's PATINA_GC_LOG beside it.
From the summaries and logs it computes:

- the bytes of today's layout against the headered layout of
  PRD/GC_PRD.md §6, by the byte account's charge and by the study's
  estimator (which adds malloc's rounding and the Rc boxes);
- the object sizes and the allocation mix;
- survival per nursery size, the share of what an interval allocated that
  its collection marks, without the first collection (bootstrap);
- the store mix: the immediate filter, young holders, old-to-young edges;
- the live heap's peak in the headered layout, the VM's deepest stack at a
  collection, and the collector's rates (sweep time per slot, mark time per
  live object), from the census log joined to the GC log.

The census changes timing (each allocation and store takes a lock), so it
answers counts and shapes; the gc mode answers times.
"""
import concurrent.futures
import csv
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

import gc_bench

ROOT = gc_bench.ROOT
TARGET = ROOT / 'target/gc-census'
CONFIGS = {'default': None, 's16K': 16_384, 's64K': 65_536, 's256K': 262_144, 's1M': 1_048_576,
           's4M': 4_194_304}
CLASSES = ['pair', 'vector', 'string', 'flonum', 'closure', 'cell', 'identifier', 'record', 'other']
HEAP_SITES = ['set_car', 'set_cdr', 'vector_set', 'vm_vector_set', 'write_mutable_cell',
              'set_vm_closure_free_var', 'promise_update', 'break_ephemeron', 'record_set',
              'parameter_install', 'parameter_install_converted', 'promise_force_prim', 'promise_force_vm']
V_REAL, V_CELL, V_CLOSURE = 2, 21, 23
# A representative length per length bucket, for the study's estimator.
BUCKET_MID = [0, 1, 2, 3, 4, 6.5, 12.5, 24.5, 48.5, 96.5, 192.5, 640, 4608, 10000]
VIEW_64K = 2


def build(env):
    """The census binary's path, built into target/gc-census."""
    subprocess.run(['cargo', 'build', '--release', '-p', 'patina-repl', '--bin', 'patina',
                    '--features', 'patina-core/gc-census'], cwd=ROOT, check=True, stdout=subprocess.DEVNULL,
                   env=dict(env, CARGO_TARGET_DIR=str(TARGET)))
    return TARGET / 'release/patina'


# ---------------------------------------------------------------- parsing

def parse_summary(text):
    """A census summary: `key [list]` lines, `key value ...` pairs, and the
    per-site lines."""
    d = {'sites': {}}
    for line in text.splitlines():
        if line.startswith('site '):
            f = line.split(' ', 2)
            name, rest = f[1], f[2]
            entry = {}
            for key, value in re.findall(r'(\w+) (\[[^\]]*\]|\d+)', rest):
                entry[key] = json.loads(value)
            d['sites'][name] = entry
            continue
        key, _, rest = line.partition(' ')
        if key in ('rs_sum', 'idhash', 'cap_full'):
            # Lines of several figures; the hashing and continuation lines
            # reuse names (`regs`, `max_regs`), so they keep their own.
            fields = {k: json.loads(v) for k, v in re.findall(r'(\w+) (\[[^\]]*\]|\d+)', line)}
            if key == 'rs_sum':
                d.update(fields)
            else:
                d['hashing' if key == 'idhash' else 'continuations'] = fields
        elif rest.startswith('['):
            d[key] = json.loads(rest)
        elif key == 'variant_names':
            d[key] = rest.split(',')
        else:
            tokens = line.split()
            for k, v in zip(tokens[::2], tokens[1::2]):
                if v.lstrip('-').isdigit():
                    d[k] = int(v)
    return d


def parse_csv(path):
    if not path.exists():
        return []
    with path.open() as f:
        return [{k: (int(v) if v.lstrip('-').isdigit() else v) for k, v in row.items()} for row in csv.DictReader(f)]


# ---------------------------------------------------------------- the rows

def today_estimate(d):
    """The study's estimate of today's bytes (its today.py): 16 B a pair, a
    24 B slot plus malloc's 16 B-rounded buffer for vectors and strings,
    72 B an object slot, plus closures' free-variable buffers and records'
    Rc box and buffer."""
    def buffers(hist, unit, slot):
        return sum(n * (slot + (16 * ((unit * L + 15) // 16) if L else 0)) for n, L in zip(hist, BUCKET_MID))
    aa = d['alloc_by_arena']
    today = aa[0] * 16 + buffers(d['vec_len'], 8, 24) + buffers(d['str_len'], 4, 24) + aa[3] * 72
    today += buffers(d['closure_fv'], 8, 0)
    today += buffers(d['record_fields'], 8, 48)
    return today


def survival(d, k=1, by_bytes=False, exclude=()):
    """(survived, allocated) over the run's collections, the first left out
    when k is 1, without the classes in `exclude`."""
    key = '_bytes' if by_bytes else ''
    a = d[f'young_alloc{key}_class{k}']
    s = d[f'young_surv{key}_class{k}']
    return (sum(v for i, v in enumerate(s) if CLASSES[i] not in exclude),
            sum(v for i, v in enumerate(a) if CLASSES[i] not in exclude))


def heap_stores(d):
    return [d['sites'][s] for s in HEAP_SITES if s in d['sites']]


LARGE = 100_000


def slots_of(r):
    return r['arena_pairs'] + r['arena_vectors'] + r['arena_strings'] + r['arena_objects']


def live_objects(r):
    return r['live_pairs'] + r['live_vectors'] + r['live_strings'] + r['live_objects']


def rows(results):
    """#647's census rows from {workload: {config: {'summary', 'log', 'gclog'}}}."""
    names = [n for n in results if results[n].get('default')]
    default = {n: results[n]['default']['summary'] for n in names}
    out = {}
    # Bytes, by the account and by the study's estimator.
    headered = sum(sum(d['bytes_by_arena']) for d in default.values())
    out['bytes_ratio_account'] = sum(sum(d['today_bytes_by_arena']) for d in default.values()) / headered
    out['bytes_ratio_estimate'] = sum(today_estimate(d) for d in default.values()) / headered
    # Sizes, gcold excluded as the study did (its large vectors dominate).
    hist = [0] * 16
    for n, d in default.items():
        if n != 'gcold':
            hist = [a + b for a, b in zip(hist, d['size_hist'])]
    total = sum(hist)
    out['size_16'] = hist[0] / total
    out['size_128'] = sum(hist[:7]) / total
    out['size_over_8k'] = sum(hist[13:])
    # The mix, by count.
    allocations = sum(d['alloc_total'] for d in default.values())
    out['allocations'] = allocations
    out['mix_closures'] = sum(d['obj_variant'][V_CLOSURE] for d in default.values()) / allocations
    out['mix_pairs'] = sum(d['alloc_by_arena'][0] for d in default.values()) / allocations
    out['mix_flonums'] = sum(d['obj_variant'][V_REAL] for d in default.values()) / allocations
    out['mix_vectors'] = sum(d['alloc_by_arena'][1] for d in default.values()) / allocations
    out['mix_cells'] = sum(d['obj_variant'][V_CELL] for d in default.values()) / allocations
    # Survival per nursery size, by count.
    out['survival'] = {}
    for n in names:
        out['survival'][n] = {}
        for config in CONFIGS:
            run = results[n].get(config)
            if run:
                s, a = survival(run['summary'])
                out['survival'][n][config] = s / a if a else None
    at64 = [v['s64K'] for v in out['survival'].values() if v.get('s64K') is not None]
    out['survival_64k_at_most_1_5'] = sum(1 for v in at64 if v <= 0.015)
    out['survival_64k_measured'] = len(at64)
    # Stores: the immediate filter over all heap stores, and the median
    # workload's young-holder share at the 64 K view.
    stores = [x for d in default.values() for x in heap_stores(d)]
    total_stores = sum(x['total'] for x in stores)
    out['stores'] = total_stores
    out['store_immediate'] = sum(x['imm'] for x in stores) / total_stores if total_stores else None
    out['store_old_to_young_64k'] = (sum(x['old_to_young'][VIEW_64K] for x in stores) / total_stores
                                     if total_stores else None)
    young = []
    for d in default.values():
        hs = heap_stores(d)
        t = sum(x['total'] for x in hs)
        if t:
            young.append(sum(x['young_target'][VIEW_64K] for x in hs) / t)
    out['store_young_holder_median_64k'] = statistics.median(young) if young else None
    # Live heaps, stacks and rates, from the default run's logs.
    out['live_peak'] = {}
    out['stack'] = {}
    out['rates'] = {}
    for n in names:
        log = results[n]['default']['log']
        gclog = results[n]['default']['gclog']
        if not log:
            continue
        out['live_peak'][n] = max(r['live_bytes'] for r in log)
        out['stack'][n] = (max(r['frames'] for r in log), max(r['regs'] for r in log))
        if len(gclog) == len(log):
            # Per slot and per live object over the large collections only,
            # where a collection's fixed costs (the roots, the bootstrap's
            # tables) no longer dominate: 100 K slots swept, 100 K marked.
            big = [(r, g) for r, g in zip(log, gclog) if live_objects(r) >= LARGE]
            wide = [(r, g) for r, g in zip(log, gclog) if slots_of(r) >= LARGE]
            slots = sum(slots_of(r) for r, _ in wide)
            live = sum(live_objects(r) for r, _ in big)
            sweep = sum(g['sweep_us'] for _, g in wide)
            mark = sum(g['mark_us'] for _, g in big)
            # The study's mark phase: roots through pruning, as #648's
            # log splits it.
            deep = [g['roots_us'] + g['mark_us'] + g['weak_us'] + g['prune_us']
                    for r, g in zip(log, gclog) if r['frames'] >= 100_000]
            out['rates'][n] = {'sweep_ns_per_slot': 1000 * sweep / slots if slots else None,
                               'mark_ns_per_live': 1000 * mark / live if live else None,
                               'deep_mark_mean_us': statistics.mean(deep) if deep else None}
    return out


def report_lines(r):
    """The rows as #647's table states them."""
    def pct(x, d=1):
        return '-' if x is None else f'{100 * x:.{d}f}%'
    surv = r['survival']
    pick = ', '.join(f'{n} {pct(surv[n].get("s64K"), 0)}' for n in ('nboyer', 'mperm', 'queue3', 'deeprec')
                     if n in surv)
    peaks = sorted(r['live_peak'].items(), key=lambda kv: -kv[1])[:4]
    deep = r['stack'].get('deeprec')
    lines = [
        f'bytes against the headered layout: {r["bytes_ratio_estimate"]:.2f}x by the study\'s estimator, '
        f'{r["bytes_ratio_account"]:.2f}x by the byte account\'s charge',
        f'object sizes (gcold excluded): {pct(r["size_16"], 0)} at 16 B; {pct(r["size_128"])} <= 128 B; '
        f'{r["size_over_8k"]} objects > 8 KiB',
        f'allocation mix ({r["allocations"] / 1e6:.0f} M objects): closures {pct(r["mix_closures"])}, '
        f'pairs {pct(r["mix_pairs"])}, flonums {pct(r["mix_flonums"])}, vectors {pct(r["mix_vectors"])}, '
        f'cells {pct(r["mix_cells"])}',
        f'survival at 64 K: <= 1.5% on {r["survival_64k_at_most_1_5"]} of {r["survival_64k_measured"]}; {pick}',
        f'store mix: the immediate filter removes {pct(r["store_immediate"], 0)} of heap stores; the median '
        f'workload sends {pct(r["store_young_holder_median_64k"])} to young holders; old-to-young '
        f'{pct(r["store_old_to_young_64k"])} at 64 K',
        'live heaps (headered): ' + ', '.join(f'{n} {v / 2**20:.0f} MB' for n, v in peaks),
    ]
    if deep:
        rate = r['rates'].get('deeprec', {})
        mean = rate.get('deep_mark_mean_us')
        lines.append(f'deep stacks: deeprec {deep[0]:,} frames and {deep[1]:,} registers at most'
                     + (f'; mean mark phase (roots to pruning) {mean / 1000:.1f} ms over its deep collections'
                        if mean else ''))
    sweeps = [v['sweep_ns_per_slot'] for v in r['rates'].values() if v.get('sweep_ns_per_slot')]
    marks = [v['mark_ns_per_live'] for v in r['rates'].values() if v.get('mark_ns_per_live')]
    if sweeps:
        lines.append(f'collector rates, collections of >= 100 K: sweep {statistics.median(sweeps):.1f} ns per slot '
                     f'(median of {len(sweeps)}; {min(sweeps):.1f}-{max(sweeps):.1f}); mark '
                     + (f'{statistics.median(marks):.1f} ns per live object (median of {len(marks)}; '
                        f'{min(marks):.1f}-{max(marks):.1f})' if marks else '-'))
    return lines


# ---------------------------------------------------------------- running

def run_one(binary, workload, config, prepared, out, timeout):
    """One census run of a workload `prepared` by `gc_bench.prepare`: its
    summary, census log and GC log, or an error."""
    program, args, stdin, cwd, check = prepared
    stem = out / f'{workload["name"]}-{config}'
    env = {k: v for k, v in os.environ.items() if not k.startswith('PATINA_')}
    env.update(PATINA_ISOLATED_LIBRARIES='1', PATINA_GC_CENSUS='1', PATINA_GC_CENSUS_OUT=f'{stem}.txt',
               PATINA_GC_CENSUS_LOG=f'{stem}.csv', PATINA_GC_LOG=f'{stem}.gc.csv')
    if CONFIGS[config]:
        env['PATINA_GC_STRESS'] = str(CONFIGS[config])
    with open(stdin or os.devnull) as input_file:
        result = subprocess.run([str(binary), str(program), *args], cwd=cwd, env=env, stdin=input_file,
                                capture_output=True, text=True, timeout=timeout)
    Path(f'{stem}.stdout').write_text(result.stdout + result.stderr)
    if result.returncode != 0:
        raise RuntimeError(f'exited {result.returncode}: {result.stderr.strip()[-300:]}')
    if check == 'larceny' and ('ERROR' in result.stdout or 'Elapsed time' not in result.stdout):
        raise RuntimeError(f'wrong result: {result.stdout.strip()[-300:]}')
    return {'summary': parse_summary(Path(f'{stem}.txt').read_text()),
            'log': parse_csv(Path(f'{stem}.csv')), 'gclog': parse_csv(Path(f'{stem}.gc.csv'))}


def census_mode(args, directory, env):
    """benchmarks.py census: build, run every workload under every
    configuration, print the rows; the report's fields and whether any run
    failed."""
    workloads = gc_bench.load_workloads(args.set or ['gbs'], args.workload)
    configs = args.config or list(CONFIGS)
    binary = build(env)
    out = directory / 'census'
    out.mkdir(parents=True, exist_ok=True)
    results, skipped, failed = {}, {}, {}
    jobs = []
    for workload in workloads:
        # Written once: the configurations run it side by side.
        try:
            prepared = gc_bench.prepare(workload, directory / 'scratch' / workload['name'], epilogue='')
        except gc_bench.Skip as reason:
            skipped[workload['name']] = str(reason)
            continue
        jobs += [(workload, config, prepared) for config in configs]
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        futures = {pool.submit(run_one, binary, w, c, s, out, args.timeout): (w['name'], c) for w, c, s in jobs}
        for future in concurrent.futures.as_completed(futures):
            name, config = futures[future]
            try:
                results.setdefault(name, {})[config] = future.result()
                print(f'{name} {config}: done', flush=True)
            except (RuntimeError, subprocess.TimeoutExpired, OSError) as error:
                failed[f'{name} {config}'] = str(error)
                print(f'{name} {config}: FAILED {error}', flush=True)
    for name, reason in skipped.items():
        print(f'SKIPPED {name}: {reason}')
    for name, reason in failed.items():
        print(f'FAILED {name}: {reason}')
    measured = rows(results) if any(r.get('default') for r in results.values()) else None
    if measured:
        for line in report_lines(measured):
            print(line)
    summaries = {n: {c: r['summary'] for c, r in v.items()} for n, v in results.items()}
    return dict(kind='census', configs=configs, binary_features=['patina-core/gc-census'], skipped=skipped,
                failed=failed, measurements=measured or {}, summaries=summaries), bool(failed)


def load(out):
    """The results of a finished run from its census directory
    (`target/benchmark-runs/<stamp>/census`), to analyze again."""
    results = {}
    for summary in sorted(out.glob('*.txt')):
        name, _, config = summary.stem.rpartition('-')
        if config not in CONFIGS:
            continue
        stem = summary.with_suffix('')
        results.setdefault(name, {})[config] = {
            'summary': parse_summary(summary.read_text()),
            'log': parse_csv(Path(f'{stem}.csv')), 'gclog': parse_csv(Path(f'{stem}.gc.csv'))}
    return results


if __name__ == '__main__':
    import sys
    for line in report_lines(rows(load(Path(sys.argv[1])))):
        print(line)
