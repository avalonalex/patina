"""Failure paths of the per-PR GC stress lane (#626), against a fake cargo.

run_gc_stress_tests.sh passes only when every target passed, ran under its
interval, and collected at least its pinned minimum. These run it against a
fake `cargo` that writes the collection record PATINA_GC_COUNT_DIR asks for,
so each way the lane can fail is shown failing it, among them the positive
control for a run that reports no collections (#5). No compiler or
interpreter is needed.
"""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[2]

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
        f.write('pid=%d exe=%s-0123abcd env=%s collections=%020d\n' % (os.getpid(), target, env, count))
if mode == 'fail':
    print('test some_test ... FAILED')
    print('test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out')
    sys.exit(101)
print('test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out')
'''


class Lanes(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'scripts').mkdir()
        shutil.copy2(SOURCE / 'scripts/run_gc_stress_tests.sh', self.root / 'scripts')
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.command('cargo', FAKE_CARGO)
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'])
        for name in ('PATINA_GC', 'PATINA_GC_STRESS', 'PATINA_GC_ZEAL', 'PATINA_GC_COUNT_DIR'):
            self.env.pop(name, None)

    def command(self, name, body):
        path = self.bin / name
        path.write_text('#!/usr/bin/env python3\n' + body)
        path.chmod(0o755)

    def run_script(self, script, *args, mode='ok', **env):
        return subprocess.run(['bash', str(self.root / 'scripts' / script), *args],
                              cwd=self.temporary.name, env=dict(self.env, FAKE_MODE=mode, **env),
                              text=True, capture_output=True)

    def assert_lane_fails(self, result, diagnostic):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(diagnostic, result.stdout)

    def test_per_pr_lane_passes_when_every_target_collected(self):
        result = self.run_script('run_gc_stress_tests.sh')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('OK   gc_vm at 16: ok. 3 passed', result.stdout)
        self.assertNotIn('FAIL', result.stdout)

    def test_per_pr_lane_fails_a_run_that_did_not_collect(self):
        self.assert_lane_fails(self.run_script('run_gc_stress_tests.sh', mode='zero'),
                               'FAIL gc_vm at 16: ok. 3 passed')
        self.assert_lane_fails(self.run_script('run_gc_stress_tests.sh', mode='zero'),
                               '0 collections, pinned minimum 8001')

    def test_per_pr_lane_fails_a_run_the_variables_did_not_reach(self):
        self.assert_lane_fails(self.run_script('run_gc_stress_tests.sh', mode='no-record'),
                               'no collection record from the test binary')
        self.assert_lane_fails(self.run_script('run_gc_stress_tests.sh', mode='other-mode'),
                               'a process ran under another mode')

    def test_per_pr_lane_fails_a_failing_target(self):
        result = self.run_script('run_gc_stress_tests.sh', mode='fail')
        self.assert_lane_fails(result, 'cargo test exited 101')
        self.assertIn('test some_test ... FAILED', result.stdout)


if __name__ == '__main__':
    unittest.main()
