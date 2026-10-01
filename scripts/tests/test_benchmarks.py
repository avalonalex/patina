"""Offline subprocess smoke tests; no compiler, interpreter or timing suite needed."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[2]


class BenchmarkRunners(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'scripts').mkdir()
        for name in ('benchmarks.py', 'bench_compare.sh', 'run_benchmarks.sh'):
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
        for script in ('bench_compare.sh', 'run_benchmarks.sh'):
            result = self.run_script(script, '--filter')
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


if __name__ == '__main__':
    unittest.main()
