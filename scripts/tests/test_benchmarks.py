"""Offline subprocess smoke tests; no compiler, interpreter or timing suite needed."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SOURCE = Path(__file__).resolve().parents[2]


class BenchmarkRunners(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'scripts').mkdir()
        for name in ('benchmarks.py', 'gc_bench.py', 'gc_census.py', 'bench_compare.sh', 'run_benchmarks.sh',
                     'run_gc_benchmarks.sh', 'run_gc_census.sh'):
            shutil.copy2(SOURCE / 'scripts' / name, self.root / 'scripts' / name)
        shutil.copytree(SOURCE / 'crates/patina-tests/bench_programs', self.root / 'crates/patina-tests/bench_programs')
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
                        FAKE_ROOT=str(self.root), MODE='ok')
        self.env.pop('PATINA_BENCH_BACKEND', None)
        self.command('git', 'import sys\nprint("abc123" if "rev-parse" in sys.argv else "", end="")')
        self.command('rustc', 'print("rustc test")')
        self.command('sysctl', 'print("test CPU")')
        self.command('cargo', '''
import json, os, pathlib, sys
root = pathlib.Path(os.environ['FAKE_ROOT'])
(root / 'cargo-args.json').write_text(json.dumps(sys.argv[1:]))
mode = os.environ['MODE']
if mode == 'cargo-fail':
    print('injected Cargo failure', file=sys.stderr)
    sys.exit(17)
if 'build' in sys.argv:
    print(json.dumps({'reason': 'compiler-artifact', 'target': {'name': 'patina'}, 'executable': str(root / 'bin/patina')}))
    sys.exit(0)
home = pathlib.Path(os.environ['CRITERION_HOME'])
(root / 'criterion-home').write_text(str(home))
if mode == 'empty':
    print('plausible time: [1 us 2 us 3 us]')
    sys.exit(0)
backend = os.environ['PATINA_BENCH_BACKEND']
identity = ('wrong' if mode == 'wrong-backend' else backend) + '/phases/execution/sum_100'
folder = home / 'sample/new'
folder.mkdir(parents=True)
(folder / 'benchmark.json').write_text(json.dumps({'full_id': identity}))
median = float('nan') if mode == 'nan' else 2
(folder / 'estimates.json').write_text(json.dumps({'median': {'point_estimate': median, 'confidence_interval': {'lower_bound': 1, 'upper_bound': 3}}}))
(folder / 'sample.json').write_text(json.dumps({'iters': [1, 2], 'times': [] if mode == 'empty-samples' else [2, 4]}))
if mode != 'missing-check':
    pathlib.Path(os.environ['PATINA_BENCH_CHECKS']).write_text(json.dumps({'id': identity, 'correctness': 'passed', 'parameters': {}}) + '\\n')
if mode == 'fail-after-results':
    print('injected late Cargo failure', file=sys.stderr)
    sys.exit(18)
''')
        self.command('patina', '''
import json, os, pathlib, sys
root = pathlib.Path(os.environ['FAKE_ROOT'])
source = pathlib.Path(sys.argv[-1])
with (root / 'interpreter-calls.jsonl').open('a') as log:
    log.write(json.dumps(sys.argv[1:]) + '\\n')
assert '--isolated-libraries' in sys.argv[:-1]
text = source.read_text()
assert '(scheme write)' in text and '(equal? answer ' in text
mode = os.environ['MODE']
if mode in ('interpreter-fail', 'wrong-answer') or (mode == 'second-backend-fail' and '--tree-walker' not in sys.argv):
    print('incorrect benchmark answer' if mode == 'wrong-answer' else 'injected interpreter failure', file=sys.stderr)
    sys.exit(9)
print({'empty': '', 'nan': 'BENCH_OK nan', 'zero': 'BENCH_OK 0', 'negative': 'BENCH_OK -1', 'garbage': 'BENCH_OK bogus', 'unchecked': '1.25'}.get(mode, 'BENCH_OK 1.25'))
''')

    def command(self, name, body):
        path = self.bin / name
        path.write_text('#!/usr/bin/env python3\n' + body)
        path.chmod(0o755)

    def run_script(self, script='run_benchmarks.sh', *args, mode='ok', env=None):
        return subprocess.run(['bash', str(self.root / 'scripts' / script), *args],
                              cwd=self.temporary.name, env=dict(self.env, MODE=mode, **(env or {})),
                              text=True, capture_output=True)

    def reports(self):
        return list((self.root / 'target/benchmark-runs').glob('*/report.json'))

    def assert_failure(self, result, diagnostic):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(diagnostic, result.stderr)
        self.assertNotIn('Validated ', result.stdout)
        self.assertEqual(self.reports(), [])

    def test_criterion_success_forwards_args_and_uses_fresh_home(self):
        stale = self.root / 'old-criterion'
        stale.mkdir()
        result = self.run_script('run_benchmarks.sh', '--quick', '--backend', 'tree-walker', '--filter', 'sum 100', env={'CRITERION_HOME': str(stale)})
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads((self.root / 'cargo-args.json').read_text())
        self.assertEqual(args[:6], ['bench', '--package', 'patina-tests', '--bench', 'scheme_benchmarks', '--'])
        self.assertEqual(args[-1], 'sum 100')
        self.assertIn('--warm-up-time', args)
        self.assertNotEqual((self.root / 'criterion-home').read_text(), str(stale))
        report = json.loads(self.reports()[0].read_text())
        self.assertEqual(report['backend'], 'tree-walker')
        self.assertEqual(report['revision'], 'abc123')
        self.assertEqual(report['correctness'], 'passed')
        self.assertEqual(report['measurements'][0]['median_ns'], 2)

    def test_cargo_failure_and_late_failure_do_not_publish(self):
        for mode in ('cargo-fail', 'fail-after-results'):
            with self.subTest(mode=mode):
                self.assert_failure(self.run_script(mode=mode), 'exited')

    def test_criterion_rejects_invalid_or_unchecked_measurements(self):
        for mode, message in [('empty', 'no fresh'), ('nan', 'finite positive'), ('empty-samples', 'empty or inconsistent'), ('wrong-backend', 'wrong backend'), ('missing-check', 'checks disagree')]:
            with self.subTest(mode=mode):
                self.assert_failure(self.run_script(mode=mode), message)

    def test_comparison_checks_both_backends_and_cleans_sources(self):
        result = self.run_script('bench_compare.sh', '--quick', '--filter', 'r7rs/fib/20')
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = [json.loads(line) for line in (self.root / 'interpreter-calls.jsonl').read_text().splitlines()]
        self.assertEqual(len(calls), 2)
        self.assertIn('--tree-walker', calls[0][:-1])
        self.assertNotIn('--tree-walker', calls[1])
        self.assertTrue(all(not Path(call[-1]).exists() for call in calls))
        self.assertEqual(json.loads(self.reports()[0].read_text())['measurements'][0]['vm_ms'], 1.25)

    def test_comparison_rejects_failed_empty_unchecked_and_invalid_results(self):
        for mode in ('cargo-fail', 'interpreter-fail', 'second-backend-fail', 'wrong-answer', 'empty', 'nan', 'zero', 'negative', 'garbage', 'unchecked'):
            with self.subTest(mode=mode):
                self.assert_failure(self.run_script('bench_compare.sh', '--quick', '--filter', 'r7rs/fib/20', mode=mode), 'Benchmark failed:')
                calls = self.root / 'interpreter-calls.jsonl'
                if calls.exists():
                    self.assertTrue(all(not Path(json.loads(line)[-1]).exists() for line in calls.read_text().splitlines()))

    def test_invalid_arguments_fail_before_cargo(self):
        for script in ('bench_compare.sh', 'run_benchmarks.sh', 'run_gc_benchmarks.sh'):
            result = self.run_script(script, '--rounds' if script == 'run_gc_benchmarks.sh' else '--filter')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('expected one argument', result.stderr)
        self.assert_failure(self.run_script('bench_compare.sh', '--filter', 'no-such-case'), 'no comparison workloads')
        self.assertFalse((self.root / 'cargo-args.json').exists())
        result = self.run_script(env={'PATINA_BENCH_BACKEND': 'typo'})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('must be vm or tree-walker', result.stderr)

    def test_existing_output_is_never_mistaken_for_new_success(self):
        output = self.root / 'baseline.json'
        output.write_text('old baseline')
        result = self.run_script('run_benchmarks.sh', '--output', str(output))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output.read_text(), 'old baseline')
        self.assertFalse((self.root / 'cargo-args.json').exists())

    # ------------------------------------------------------------ gc mode

    def fake_checkout(self, root, answer='42'):
        """A release binary at root/target/release/patina that writes a GC log
        (two paced collections, then the epilogue's two forced ones) and the
        epilogue's final readings."""
        binary = root / 'target/release/patina'
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_text(f'''#!/usr/bin/env python3
import json, os, pathlib, sys
with pathlib.Path(os.environ['FAKE_ROOT'], 'gc-calls.jsonl').open('a') as log:
    log.write(json.dumps({{'root': {str(root)!r}, 'argv': sys.argv[1:], 'pad': len(os.environ['PATINA_BENCH_PAD']),
                          'isolated': os.environ.get('PATINA_ISOLATED_LIBRARIES')}}) + '\\n')
assert '(gc-stats)' in pathlib.Path(sys.argv[-1]).read_text()
pathlib.Path(os.environ['PATINA_GC_LOG']).write_text(
    'heap,number,start_us,reason,pause_us,mark_us\\n1,1,1000,bytes,300,200\\n1,2,50000,bytes,700,600\\n'
    '1,3,90000,call,5000,4000\\n1,4,96000,call,4500,4000\\n')
print({answer!r})
print('GC_FINAL live-bytes=1000 committed-bytes=4096 external-bytes=0 resident-bytes=#f '
      'wait-max-bytes=0 wait-site=#f deferral-max-bytes=854658 deferral-site=crates/x.rs:212:25')
''')
        binary.chmod(0o755)

    def gc_setup(self):
        # A perf that cannot count, as on GitHub's Ubuntu runners: the mode
        # falls back to time(1) alone wherever perf would be used.
        self.command('perf', 'import sys\nprint("perf_event_paranoid setting is 4", file=sys.stderr)\nsys.exit(255)')
        self.command('time', '''
import subprocess, sys
flag, command = sys.argv[1], sys.argv[2:]
status = subprocess.run(command).returncode
if flag == '-l':
    sys.stderr.write('        1.50 real         1.20 user         0.10 sys\\n'
                     '            23412736  maximum resident set size\\n'
                     '                1903  page reclaims\\n'
                     '         11805187762  instructions retired\\n'
                     '          1949912135  cycles elapsed\\n'
                     '            20513176  peak memory footprint\\n')
else:
    sys.stderr.write('\\tUser time (seconds): 1.20\\n'
                     '\\tElapsed (wall clock) time (h:mm:ss or m:ss): 0:01.50\\n'
                     '\\tMaximum resident set size (kbytes): 22864\\n'
                     '\\tMinor (reclaiming a frame) page faults: 1903\\n')
sys.exit(status)
''')
        self.fake_checkout(self.root)
        self.base = self.root / 'base-checkout'
        self.fake_checkout(self.base)

    def gc_calls(self):
        return [json.loads(line) for line in (self.root / 'gc-calls.jsonl').read_text().splitlines()]

    def test_gc_runs_base_branch_base_under_one_pad_per_round(self):
        self.gc_setup()
        result = self.run_script('run_gc_benchmarks.sh', '--workload', 'escape1000', '--workload', 'deriv',
                                 '--rounds', '3', '--base', str(self.base),
                                 env={'LARCENY_DIR': str(self.root / 'no-larceny')})
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('SKIPPED deriv: no Larceny checkout', result.stdout)
        calls = self.gc_calls()
        self.assertEqual([c['root'] for c in calls], [str(self.base), str(self.root), str(self.base)] * 3)
        self.assertTrue(all(c['isolated'] == '1' for c in calls))
        pads = [c['pad'] for c in calls]
        self.assertTrue(all(len(set(pads[i:i + 3])) == 1 for i in range(0, 9, 3)), pads)
        report = json.loads(self.reports()[0].read_text())
        self.assertEqual(list(report['skipped']), ['deriv'])
        [row] = report['measurements']
        # The epilogue's forced pair is not counted.
        self.assertEqual((row['branch_collections'], row['branch_pause_max_us'], row['branch_mark_mean_us'],
                          row['branch_forced_pause_us']), (2, 700, 400, 5000))
        # time -l counts instructions on macOS; on Linux the fake perf
        # cannot count, so the mode records none.
        self.assertEqual(row['branch_instructions'], 11805187762 if sys.platform == 'darwin' else None)
        self.assertEqual(row['branch_live_bytes'], 1000)
        self.assertIsNone(row['branch_resident_bytes'])
        self.assertEqual((row['branch_deferral_max_bytes'], row['branch_deferral_site'], row['branch_wait_site']),
                         (854658, ['crates/x.rs:212:25'], None))
        self.assertEqual(row['wall_s_ratio'], [1.0, 1.0, 1.0])
        self.assertEqual('instructions_ratio' in row, sys.platform == 'darwin')

    def test_gc_fails_without_a_success_report_when_the_sides_disagree(self):
        self.gc_setup()
        self.fake_checkout(self.base, answer='41')
        result = self.run_script('run_gc_benchmarks.sh', '--workload', 'samedepth1000', '--rounds', '1',
                                 '--base', str(self.base))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('FAILED samedepth1000', result.stdout)
        self.assertIn('the sides printed different answers', result.stdout)
        self.assertEqual(self.reports(), [])
        failed = list((self.root / 'target/benchmark-runs').glob('*/failed-report.json'))
        self.assertEqual(len(failed), 1)

    def test_census_runs_each_configuration_with_the_census_on(self):
        binary = self.root / 'target/gc-census/release/patina'
        binary.parent.mkdir(parents=True)
        binary.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
with pathlib.Path(os.environ['FAKE_ROOT'], 'census-calls.jsonl').open('a') as log:
    log.write(json.dumps({'stress': os.environ.get('PATINA_GC_STRESS'), 'census': os.environ['PATINA_GC_CENSUS'],
                          'program': sys.argv[-1]}) + '\\n')
z = [0] * 14
pathlib.Path(os.environ['PATINA_GC_CENSUS_OUT']).write_text('\\n'.join([
    'collections 1', 'alloc_total 10', 'alloc_by_arena [10, 0, 0, 0]', 'bytes_by_arena [160, 0, 0, 0]',
    'today_bytes_by_arena [160, 0, 0, 0]', f'vec_len {z}', f'str_len {z}', f'obj_variant {[0] * 28}',
    f'closure_fv {z}', f'record_fields {z}', f'size_hist {[10] + [0] * 15}',
    f'young_alloc_class1 {[10] + [0] * 8}', f'young_surv_class1 {[1] + [0] * 8}']) + '\\n')
pathlib.Path(os.environ['PATINA_GC_CENSUS_LOG']).write_text(
    'live_bytes,frames,regs,live_pairs,live_vectors,live_strings,live_objects,arena_pairs,arena_vectors,'
    'arena_strings,arena_objects\\n160,3,30,1,0,0,0,10,0,0,0\\n')
pathlib.Path(os.environ['PATINA_GC_LOG']).write_text(
    'roots_us,mark_us,weak_us,prune_us,sweep_us\\n1,1,0,0,1\\n')
print('300000')
''')
        binary.chmod(0o755)
        result = self.run_script('run_gc_census.sh', '--workload', 'deeprec', '--config', 'default',
                                 '--config', 's64K', '--jobs', '1')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = [json.loads(line) for line in (self.root / 'census-calls.jsonl').read_text().splitlines()]
        self.assertEqual(sorted(str(c['stress']) for c in calls), ['65536', 'None'])
        self.assertTrue(all(c['census'] == '1' for c in calls))
        # The census runs the program alone, without the gc mode's epilogue.
        program = Path(calls[0]['program']).read_text()
        self.assertNotIn('gc-stats', program)
        report = json.loads(self.reports()[0].read_text())
        self.assertEqual(report['kind'], 'census')
        self.assertEqual(report['measurements']['survival']['deeprec']['s64K'], 0.1)
        self.assertIn('live heaps (headered): deeprec 0 MB', result.stdout)


class GcArithmetic(unittest.TestCase):
    """scripts/gc_bench.py's parsing and statistics, on synthetic input."""

    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location('gc_bench', SOURCE / 'scripts/gc_bench.py')
        cls.gc = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.gc)

    def test_time_reports_of_both_platforms(self):
        mac = self.gc.parse_time('  2.47 real  2.11 user  0.14 sys\n  1069  maximum resident set size\n'
                                 '  7 page reclaims\n  99 instructions retired\n  50 cycles elapsed\n'
                                 '  1000  peak memory footprint\n')
        self.assertEqual((mac['wall_s'], mac['max_rss'], mac['instructions'], mac['peak_footprint']),
                         (2.47, 1069, 99, 1000))
        linux = self.gc.parse_time('99,,instructions:u,100,100.00,,\n50,,cycles:u,100,100.00,,\n'
                                   '\tUser time (seconds): 1.5\n\tElapsed (wall clock) time (h:mm:ss or m:ss): 1:02.50\n'
                                   '\tMaximum resident set size (kbytes): 2\n')
        self.assertEqual((linux['wall_s'], linux['max_rss'], linux['instructions'], linux['cycles']),
                         (62.5, 2048, 99, 50))
        self.assertIsNone(linux['peak_footprint'])

    def test_perf_is_used_only_where_it_can_count(self):
        with tempfile.TemporaryDirectory() as directory:
            perf = Path(directory) / 'perf'
            for status, usable in ((255, False), (0, True)):
                perf.write_text(f'#!/bin/sh\nexit {status}\n')
                perf.chmod(0o755)
                self.gc.perf_usable.cache_clear()
                with mock.patch.dict(os.environ, PATH=directory), contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(self.gc.perf_usable(), str(perf) if usable else None)
        self.gc.perf_usable.cache_clear()

    def test_mmu_takes_the_worst_window_inside_the_run(self):
        # A 5 ms pause from 10 ms to 15 ms in a run that ends at 100 ms.
        intervals = [(10_000, 15_000), (99_000, 100_000)]
        result = self.gc.mmu(intervals, (1, 5, 10, 50, 95))
        self.assertEqual(result[1], 0.0)
        self.assertEqual(result[5], 0.0)
        self.assertAlmostEqual(result[10], 0.5)
        self.assertAlmostEqual(result[50], 0.9)
        # Only a window this wide holds both pauses.
        self.assertAlmostEqual(result[95], 1 - 6 / 95)
        self.assertEqual(self.gc.mmu([], (10,)), {10: 1.0})

    def test_counted_input_keeps_the_rest_of_the_checkout_s_input(self):
        self.assertEqual(self.gc.apply_input({'count': 7}, '; note\n100   ; runs\n(x y)\n'), '; note\n7   ; runs\n(x y)\n')
        self.assertEqual(self.gc.apply_input({'text': '1\n2\n'}, 'ignored'), '1\n2\n')

    def test_ratios_are_branch_over_the_round_s_base_mean(self):
        runs = []
        for round_number, (a, b, c) in enumerate([(100, 110, 100), (90, 99, 110)]):
            for position, (side, value) in enumerate(zip(('base', 'branch', 'base'), (a, b, c))):
                runs.append({'workload': 'w', 'backend': 'vm', 'side': side, 'round': round_number,
                             'position': position, 'metrics': {'instructions': value, 'mmu': {10: 1.0}}})
        [row] = self.gc.summarize(runs, True)
        center, low, high = row['instructions_ratio']
        self.assertAlmostEqual(center, (1.1 * 0.99) ** 0.5)
        self.assertLessEqual(low, center)
        self.assertGreaterEqual(high, center)
        self.assertEqual(self.gc.bootstrap([1.25] * 10), (1.25, 1.25))

    def test_the_table_s_workloads(self):
        table = json.loads((self.gc.GC / 'gbs.json').read_text())['workloads']
        names = [w['name'] for w in table]
        self.assertEqual(len(names), len(set(names)))
        gbs = [w['name'] for w in table if 'gbs' in w['sets']]
        self.assertEqual(gbs, ['nboyer', 'deriv', 'gcbench', 'destruc', 'quicksort', 'gcold', 'mperm', 'queue3',
                               'fibfp', 'mbrot', 'nucleic', 'ctak', 'fibc', 'generator', 'deeprec', 'libload',
                               'hashtable0', 'eqtable', 'dynamic', 'earley'])
        for w in table:
            if w['kind'] == 'patina':
                self.assertTrue((self.gc.GC / w['source']).exists(), w['name'])
            if w['kind'] == 'larceny-r7rs':
                self.assertTrue('count' in w['input'] or 'text' in w['input'], w['name'])
            if w['kind'] == 'larceny-gc':
                self.assertIn('entry', w)
        with self.assertRaises(ValueError):
            self.gc.load_workloads([], ['no-such-workload'])


class GcCensusArithmetic(unittest.TestCase):
    """scripts/gc_census.py's parsing and #647's rows, on synthetic summaries."""

    @classmethod
    def setUpClass(cls):
        sys.path.insert(0, str(SOURCE / 'scripts'))
        spec = importlib.util.spec_from_file_location('gc_census', SOURCE / 'scripts/gc_census.py')
        cls.census = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.census)

    @staticmethod
    def summary(pairs=600, closures=300, flonums=100, survive=(0, 0), stores=((100, 60, 95),)):
        """A census summary: `pairs` pairs (16 B), `closures` closures of two
        free variables (32 B headered), `flonums` flonums (16 B); class-1
        survival (pairs survived, pairs allocated); and vector stores as
        (total, immediates, to young holders at 64 K)."""
        variants = [0] * 28
        variants[23], variants[2] = closures, flonums
        total = pairs + closures + flonums
        fv = [0] * 14
        fv[2] = closures
        size = [0] * 16
        size[0], size[2] = pairs + flonums, closures
        lines = [
            'heaps_total 1 heaps_max_live 1', 'collections 3', 'max_frames 7 max_regs 70',
            f'alloc_total {total}', 'vm_instrs 1000',
            f'alloc_by_arena {[pairs, 0, 0, closures + flonums]}',
            f'bytes_by_arena {[16 * pairs, 0, 0, 32 * closures + 16 * flonums]}',
            f'today_bytes_by_arena {[16 * pairs, 0, 0, 88 * closures + 72 * flonums]}',
            f'vec_len {[0] * 14}', f'str_len {[0] * 14}', f'obj_variant {variants}',
            f'closure_fv {fv}', f'record_fields {[0] * 14}', f'size_hist {size}',
            f'young_alloc_class1 {[survive[1]] + [0] * 8}', f'young_surv_class1 {[survive[0]] + [0] * 8}',
            'rs_sum [1, 2, 3, 4, 5, 6] rs_max [1, 1, 1, 1, 1, 1] rs_nonempty [1, 2, 3, 4, 5, 6]',
            'cap_full 2 frames 10 regs 40 max_regs 30 restore 1 rframes 5 rregs 20 cap_delim 0 dframes 0 dregs 0',
            'variant_names BigInt,Rational,Real',
        ]
        for t, imm, young in stores:
            lines.append(f'site vector_set total {t} imm {imm} heapval {t - imm} target_unknown 0 '
                         f'young_target {[young] * 6} young_target_heapval {[0] * 6} young_value {[0] * 6} '
                         f'old_to_young {[0, 0, 5, 0, 0, 0]} old_target_heapval {[0] * 6}')
        return '\n'.join(lines) + '\n'

    def test_summaries_parse_into_numbers_lists_and_sites(self):
        d = self.census.parse_summary(self.summary())
        self.assertEqual((d['alloc_total'], d['max_frames'], d['max_regs']), (1000, 7, 70))
        self.assertEqual(d['alloc_by_arena'], [600, 0, 0, 400])
        self.assertEqual(d['rs_sum'], [1, 2, 3, 4, 5, 6])
        self.assertEqual(d['continuations'], {'cap_full': 2, 'frames': 10, 'regs': 40, 'max_regs': 30,
                                              'restore': 1, 'rframes': 5, 'rregs': 20, 'cap_delim': 0,
                                              'dframes': 0, 'dregs': 0})
        self.assertEqual(d['variant_names'], ['BigInt', 'Rational', 'Real'])
        site = d['sites']['vector_set']
        self.assertEqual((site['total'], site['imm'], site['old_to_young'][2]), (100, 60, 5))

    def test_the_rows(self):
        parse = self.census.parse_summary
        # One large collection, and one too small to count in the rates.
        log = [{'live_bytes': 5 << 20, 'frames': 200_000, 'regs': 2_000_000, 'live_pairs': 100_000,
                'live_vectors': 0, 'live_strings': 0, 'live_objects': 0, 'arena_pairs': 200_000,
                'arena_vectors': 0, 'arena_strings': 0, 'arena_objects': 0},
               {'live_bytes': 1, 'frames': 1, 'regs': 1, 'live_pairs': 10, 'live_vectors': 0, 'live_strings': 0,
                'live_objects': 0, 'arena_pairs': 20, 'arena_vectors': 0, 'arena_strings': 0, 'arena_objects': 0}]
        gclog = [{'roots_us': 1, 'mark_us': 300, 'weak_us': 0, 'prune_us': 1, 'sweep_us': 400},
                 {'roots_us': 50, 'mark_us': 50, 'weak_us': 0, 'prune_us': 0, 'sweep_us': 50}]
        results = {
            'a': {'default': {'summary': parse(self.summary(stores=((100, 60, 95),))), 'log': log, 'gclog': gclog},
                  's64K': {'summary': parse(self.summary(survive=(1, 100))), 'log': [], 'gclog': []}},
            'deeprec': {'default': {'summary': parse(self.summary(stores=((300, 0, 30),))), 'log': log,
                                    'gclog': gclog},
                        's64K': {'summary': parse(self.summary(survive=(100, 100))), 'log': [], 'gclog': []}},
        }
        r = self.census.rows(results)
        # Headered: 600*16 + 300*32 + 100*16 = 20,800 per workload; the
        # account charges 600*16 + 300*88 + 100*72 = 43,200.
        self.assertAlmostEqual(r['bytes_ratio_account'], 43_200 / 20_800)
        self.assertAlmostEqual(r['size_16'], 0.7)
        self.assertEqual(r['size_over_8k'], 0)
        self.assertAlmostEqual(r['mix_closures'], 0.3)
        self.assertAlmostEqual(r['mix_pairs'], 0.6)
        self.assertEqual((r['survival']['a']['s64K'], r['survival']['deeprec']['s64K']), (0.01, 1.0))
        self.assertEqual(r['survival_64k_at_most_1_5'], 1)
        # 60 immediates of 400 stores; young holders 95% and 10%.
        self.assertAlmostEqual(r['store_immediate'], 60 / 400)
        self.assertAlmostEqual(r['store_young_holder_median_64k'], (0.95 + 0.1) / 2)
        self.assertEqual(r['live_peak']['a'], 5 << 20)
        self.assertEqual(r['stack']['deeprec'], (200_000, 2_000_000))
        rates = r['rates']['deeprec']
        self.assertEqual((rates['sweep_ns_per_slot'], rates['mark_ns_per_live'], rates['deep_mark_mean_us']),
                         (2.0, 3.0, 302))
        self.assertTrue(any('deeprec 200,000 frames' in line for line in self.census.report_lines(r)))

    def test_the_study_s_estimate_of_today_s_bytes(self):
        d = self.census.parse_summary(self.summary(pairs=10, closures=1, flonums=0))
        # 10 pairs at 16 B, one object slot of 72 B and its two free
        # variables' 16 B buffer.
        self.assertEqual(self.census.today_estimate(d), 160 + 72 + 16)


if __name__ == '__main__':
    unittest.main()
