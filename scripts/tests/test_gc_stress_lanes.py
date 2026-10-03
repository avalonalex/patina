"""Failure paths of the GC stress lanes (#626), against fake binaries.

run_gc_stress_tests.sh (per PR) and run_larceny_gc_stress.sh (nightly) each
pass only when every run passed, ran under its interval, and collected at
least its pinned minimum; the per-PR one also requires every test of a
target to have run, and the nightly one holds each suite's tally to its
pinned row, its exit status to its tally, and its process to the lane's
backend. These run both scripts against a fake `cargo` and a fake `patina`
that write the collection record PATINA_GC_COUNT_DIR asks for, so each way
a lane can fail is shown failing it: the positive controls for a run that
reports no collections (#5) and a changed tally (#201). No compiler,
interpreter or Larceny checkout is needed.
"""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[2]
PINNED = "fef550c7d3923deb7a5a1ccd5a628e54cf231c75"

# A test binary: one record for the process, then libtest's summary line.
FAKE_CARGO = r'''
import os, sys
mode = os.environ.get('FAKE_MODE', 'ok')
target = sys.argv[sys.argv.index('--test') + 1]
directory = os.environ.get('PATINA_GC_COUNT_DIR')
stress = os.environ.get('PATINA_GC_STRESS')
env = 'adaptive' if mode == 'other-mode' or not stress else 'PATINA_GC_STRESS=' + stress
count = 0 if mode == 'zero' else 100000
if directory and mode != 'no-record':
    with open(os.path.join(directory, 'gc-count.%d' % os.getpid()), 'w') as f:
        f.write('pid=%d exe=%s-0123abcd env=%s backends=tree-walker,vm collections=%020d\n'
                % (os.getpid(), target, env, count))
if mode == 'fail':
    print('test some_test ... FAILED')
    print('test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out')
    sys.exit(101)
if mode == 'filtered':
    print('test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out')
else:
    print('test result: ok. 99 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out')
'''

# A suite run: the record, then what Larceny's runner prints. A failing
# tally names one failing assertion, which each fake suite's source has.
FAKE_PATINA = r'''
import os, sys, time
if sys.argv[1:] == ['--version']:
    print(os.environ.get('FAKE_VERSION', 'patina 0.1.0 (gc-check)'))
    sys.exit(0)
mode = os.environ.get('FAKE_MODE', 'ok')
suite = os.path.basename(sys.argv[-1])[:-len('.sps')]
directory = os.environ.get('PATINA_GC_COUNT_DIR')
stress = os.environ.get('PATINA_GC_STRESS')
env = 'adaptive' if mode == 'other-mode' or not stress else 'PATINA_GC_STRESS=' + stress
count = {'zero': 0, 'one': 1}.get(mode, 1000)
backend = 'tree-walker' if '--tree-walker' in sys.argv else 'vm'
if mode == 'other-backend':
    backend = {'vm': 'tree-walker', 'tree-walker': 'vm'}[backend]
if directory and mode != 'no-record':
    with open(os.path.join(directory, 'gc-count.%d' % os.getpid()), 'w') as f:
        f.write('pid=%d exe=patina env=%s backends=%s collections=%020d\n'
                % (os.getpid(), env, backend, count))
print('Running tests for (scheme %s)' % suite)
sys.stdout.flush()
if mode == 'hang':
    time.sleep(30)
if mode == 'panic':
    print("thread 'main' panicked at crates/patina-core/src/heap/check.rs:1:1:", file=sys.stderr)
    print('read of a freed pair slot', file=sys.stderr)
    sys.exit(101)
if mode == 'tally' or mode == 'stress-tally' and stress:
    print('Expression:\n (%s-probe 1)\nResult:\n 2\n' % suite)
    print('1 of 10 tests failed.')
else:
    print('10 tests passed')
sys.stdout.flush()
if mode == 'crash-after-tally':
    os._exit(139)
'''

# `git -C <dir> rev-parse HEAD`, answering for the Larceny checkout.
FAKE_GIT = r'''
import os, sys
print(os.environ.get('FAKE_COMMIT', '%s'))
'''


class Lanes(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'scripts').mkdir()
        for name in ('run_gc_stress_tests.sh', 'run_larceny_gc_stress.sh',
                     'run_larceny_tests.sh', 'larceny_report.py'):
            shutil.copy2(SOURCE / 'scripts' / name, self.root / 'scripts' / name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.command('cargo', FAKE_CARGO)
        self.command('patina', FAKE_PATINA)
        self.command('git', FAKE_GIT % PINNED)
        self.lib = self.root / 'larceny/test/R7RS/Lib'
        runs = self.lib / 'tests/scheme/run'
        runs.mkdir(parents=True)
        for suite in ('alpha', 'beta', 'stream', 'ephemeron'):
            (runs / (suite + '.sps')).write_text('(display "fake")\n')
            (runs.parent / (suite + '.sld')).write_text(
                '(define-library (tests scheme %s)\n  (begin\n    (test (%s-probe 1) 1)))\n'
                % (suite, suite))
        self.baseline = self.root / 'scheme_tests/reports/larceny_gc_stress.tsv'
        self.baseline.parent.mkdir(parents=True)
        self.pin(['alpha', 'beta', 'stream'])
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
                        PATINA_BIN=str(self.bin / 'patina'), LARCENY_TESTS_DIR=str(self.lib))
        for name in ('PATINA_GC', 'PATINA_GC_STRESS', 'PATINA_GC_ZEAL', 'PATINA_GC_COUNT_DIR',
                     'LARCENY_GC_STRESS_OUT', 'LARCENY_TEST_TIMEOUT'):
            self.env.pop(name, None)

    def command(self, name, body):
        path = self.bin / name
        path.write_text('#!/usr/bin/env python3\n' + body)
        path.chmod(0o755)

    def pin(self, suites, minimum=500, lane='r7rs-vm', status='pass', passed='10'):
        rows = ''.join('%s\t%s\t%s\t%s\t10\t%d\n' % (lane, suite, status, passed, minimum)
                       for suite in suites)
        self.baseline.write_text('# a comment\n' + rows)

    def run_script(self, script, *args, mode='ok', **env):
        return subprocess.run(['bash', str(self.root / 'scripts' / script), *args],
                              cwd=self.temporary.name, env=dict(self.env, FAKE_MODE=mode, **env),
                              text=True, capture_output=True)

    def assert_lane_fails(self, result, diagnostic):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(diagnostic, result.stdout)

    # The per-PR lane.

    def test_per_pr_lane_passes_when_every_target_collected(self):
        result = self.run_script('run_gc_stress_tests.sh')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('OK   gc_vm at 16: ok. 99 passed', result.stdout)
        self.assertNotIn('FAIL', result.stdout)

    def test_per_pr_lane_fails_a_run_that_did_not_collect(self):
        self.assert_lane_fails(self.run_script('run_gc_stress_tests.sh', mode='zero'),
                               'FAIL gc_vm at 16: ok. 99 passed')
        self.assert_lane_fails(self.run_script('run_gc_stress_tests.sh', mode='zero'),
                               '0 collections, pinned minimum 8001')

    def test_per_pr_lane_fails_a_run_the_variables_did_not_reach(self):
        self.assert_lane_fails(self.run_script('run_gc_stress_tests.sh', mode='no-record'),
                               'no collection record from the test binary')
        self.assert_lane_fails(self.run_script('run_gc_stress_tests.sh', mode='other-mode'),
                               'a process ran under another mode')

    def test_per_pr_lane_fails_a_run_that_did_not_run_every_test(self):
        result = self.run_script('run_gc_stress_tests.sh', mode='filtered')
        self.assert_lane_fails(result, '2 test(s) filtered out')
        self.assertIn('1 test(s) passed, pinned 23', result.stdout)

    def test_per_pr_lane_fails_a_failing_target(self):
        result = self.run_script('run_gc_stress_tests.sh', mode='fail')
        self.assert_lane_fails(result, 'cargo test exited 101')
        self.assertIn('test some_test ... FAILED', result.stdout)

    # The nightly Larceny lane.

    def test_nightly_lane_passes_on_the_pinned_tallies(self):
        result = self.run_script('run_larceny_gc_stress.sh')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('OK   alpha at 16: pass 10/10, 1000 collections', result.stdout)
        self.assertIn('OK   stream at 4096: pass 10/10', result.stdout)
        self.assertIn('SKIP ephemeron', result.stdout)

    def test_nightly_lane_fails_a_changed_tally(self):
        result = self.run_script('run_larceny_gc_stress.sh', mode='tally')
        self.assert_lane_fails(result, 'tally fail 9/10, pinned pass 10/10')
        # What failed, as a permalink rather than the suite's text.
        self.assertIn('[alpha.sld:3](https://github.com/larcenists/larceny/blob/%s/'
                      'test/R7RS/Lib/tests/scheme/alpha.sld#L3)' % PINNED, result.stdout)
        self.assertNotIn('Expression', result.stdout)

    def test_nightly_lane_says_whether_stress_or_the_baseline_moved_a_tally(self):
        self.assert_lane_fails(self.run_script('run_larceny_gc_stress.sh', mode='tally'),
                               'without stress the same fail 9/10: not the collector')
        self.assert_lane_fails(self.run_script('run_larceny_gc_stress.sh', mode='stress-tally'),
                               'without stress pass 10/10: stress changed the tally')

    def test_nightly_lane_holds_only_the_total_of_a_row_with_dashes(self):
        self.pin(['alpha', 'beta', 'stream'], status='-', passed='-')
        for mode in ('ok', 'tally'):
            result = self.run_script('run_larceny_gc_stress.sh', mode=mode)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        # A run that reached no tally is still not a pass or a fail.
        self.assert_lane_fails(self.run_script('run_larceny_gc_stress.sh', mode='panic'),
                               'pinned - -/10')

    def test_nightly_lane_fails_an_exit_status_its_tally_does_not_explain(self):
        self.assert_lane_fails(
            self.run_script('run_larceny_gc_stress.sh', mode='crash-after-tally'),
            'exit status 139 after a pass tally, which exits 0')

    def test_nightly_lane_fails_a_run_on_the_other_backend(self):
        self.pin(['alpha', 'beta', 'stream'], lane='r7rs-tree-walker')
        result = self.run_script('run_larceny_gc_stress.sh', '--tree-walker')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assert_lane_fails(
            self.run_script('run_larceny_gc_stress.sh', '--tree-walker', mode='other-backend'),
            'the process ran other than the tree-walker backend')
        self.pin(['alpha', 'beta', 'stream'])
        self.assert_lane_fails(
            self.run_script('run_larceny_gc_stress.sh', mode='other-backend'),
            'the process ran other than the vm backend')

    def test_nightly_lane_fails_a_check_panic(self):
        self.assert_lane_fails(self.run_script('run_larceny_gc_stress.sh', mode='panic'),
                               'a panic: thread \'main\' panicked at')

    def test_nightly_lane_fails_a_timeout(self):
        result = self.run_script('run_larceny_gc_stress.sh', 'alpha', mode='hang',
                                 LARCENY_TEST_TIMEOUT='1')
        self.assert_lane_fails(result, 'no result within 1s')

    def test_nightly_lane_fails_a_run_that_did_not_collect(self):
        self.assert_lane_fails(self.run_script('run_larceny_gc_stress.sh', mode='zero'),
                               '0 collections, pinned minimum 500')
        self.assert_lane_fails(self.run_script('run_larceny_gc_stress.sh', mode='no-record'),
                               'no collection record')
        self.assert_lane_fails(self.run_script('run_larceny_gc_stress.sh', mode='other-mode'),
                               'the process ran under another mode')

    def test_nightly_lane_fails_a_suite_with_no_row_and_a_row_with_no_suite(self):
        self.pin(['alpha', 'stream', 'gamma'])
        result = self.run_script('run_larceny_gc_stress.sh')
        self.assert_lane_fails(result, 'no pinned row for r7rs-vm beta')
        self.assertIn('FAIL gamma: pinned in', result.stdout)

    def test_nightly_lane_refuses_a_plain_build_and_another_commit(self):
        self.assert_lane_fails(
            self.run_script('run_larceny_gc_stress.sh', FAKE_VERSION='patina 0.1.0'),
            'is not a check build')
        self.assert_lane_fails(
            self.run_script('run_larceny_gc_stress.sh', FAKE_COMMIT='0' * 40),
            'the Larceny checkout is at ' + '0' * 40)

    def test_nightly_lane_update_pins_this_run(self):
        self.pin([])
        result = self.run_script('run_larceny_gc_stress.sh', '--update-baseline', mode='tally')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        rows = self.baseline.read_text().splitlines()
        self.assertEqual(rows[0], '# a comment')
        self.assertIn('r7rs-vm\talpha\tfail\t9\t10\t500', rows)
        self.assertEqual(len(rows), 4)

    def test_nightly_lane_update_does_not_pin_a_failed_run(self):
        self.pin([])
        before = self.baseline.read_text()
        for mode, diagnostic in (('panic', 'a panic'),
                                 ('zero', 'no collections: a run that did not collect'),
                                 ('stress-tally', 'stress changed the tally'),
                                 ('crash-after-tally', 'exit status 139')):
            result = self.run_script('run_larceny_gc_stress.sh', '--update-baseline', mode=mode)
            self.assert_lane_fails(result, 'Not rewriting')
            self.assertIn(diagnostic, result.stdout)
            self.assertEqual(self.baseline.read_text(), before)

    def test_nightly_lane_update_pins_a_minimum_of_at_least_one(self):
        self.pin(['alpha'])
        result = self.run_script('run_larceny_gc_stress.sh', '--update-baseline', 'alpha',
                                 mode='one')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('r7rs-vm\talpha\tpass\t10\t10\t1', self.baseline.read_text().splitlines())

    def test_nightly_lane_update_keeps_a_rows_dashes(self):
        self.pin(['alpha'], status='-', passed='-')
        result = self.run_script('run_larceny_gc_stress.sh', '--update-baseline', 'alpha',
                                 mode='tally')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('r7rs-vm\talpha\t-\t-\t10\t500', self.baseline.read_text().splitlines())

    # The plain runner, which warns when a tally leaves its stress row.

    def test_runner_warns_when_a_tally_differs_from_its_stress_row(self):
        result = subprocess.run(['bash', str(self.root / 'scripts/run_larceny_tests.sh'), 'alpha', 'beta'],
                                cwd=self.temporary.name, env=dict(self.env, FAKE_MODE='ok'),
                                text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn('Warning: tallies', result.stdout)
        self.pin(['alpha', 'beta'], status='fail', passed='9')
        result = subprocess.run(['bash', str(self.root / 'scripts/run_larceny_tests.sh'), 'alpha', 'beta'],
                                cwd=self.temporary.name, env=dict(self.env, FAKE_MODE='ok'),
                                text=True, capture_output=True)
        # A warning, not a failure: the run itself was clean.
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('Warning: tallies that differ from their rows', result.stdout)
        self.assertIn('  alpha: pass 10/10, pinned fail 9/10', result.stdout)


if __name__ == '__main__':
    unittest.main()
