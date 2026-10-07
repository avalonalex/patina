"""The steady-state lane's arithmetic (#652): parsing the driver's readings,
F(L), and each clause, on synthetic readings; no interpreter needed."""
import importlib.util
from pathlib import Path
import unittest

SOURCE = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('steady', SOURCE / 'scripts/run_steady_state.py')
steady = importlib.util.module_from_spec(spec)
spec.loader.exec_module(steady)

MIB = 1 << 20


def run(n, live=(100_000, 100_000), symbols=(40, 40), cpu=(1000, 2000, 3000, 4000),
        footprint=(5 * MIB, 5 * MIB), paced=(3, 3)):
    """Readings of one run: L, symbols and footprint at N and 4N; cpu-us at
    N (after the forced collections), 2N, 3N and 4N; paced collections in
    [N, 2N) and [3N, 4N)."""
    after = lambda l, s, f, c: {'live-bytes': l, 'committed-bytes': f, 'external-bytes': 0,
                                'symbols': s, 'cpu-us': c, 'pause-total-us': 0, 'collections': 10}
    return {
        'after': {n: after(live[0], symbols[0], footprint[0], cpu[0]),
                  4 * n: after(live[1], symbols[1], footprint[1], cpu[3] + 5)},
        'pre': {n: {'cpu-us': cpu[0] - 5, 'pause-total-us': 0, 'collections': 8},
                2 * n: {'cpu-us': cpu[1], 'pause-total-us': 0, 'collections': 10 + paced[0]},
                3 * n: {'cpu-us': cpu[2], 'pause-total-us': 0, 'collections': 20},
                4 * n: {'cpu-us': cpu[3], 'pause-total-us': 0, 'collections': 20 + paced[1]}},
    }


class Parsing(unittest.TestCase):
    def test_readings_are_found_behind_a_prompt(self):
        out = ('patina> STEADY at=10 cpu-us=5 pause-total-us=1 collections=2\n'
               'STEADY-AFTER at=10 live-bytes=7 committed-bytes=9 external-bytes=none\n'
               'something else\n')
        readings = steady.parse(out)
        self.assertEqual(readings['pre'][10], {'cpu-us': 5, 'pause-total-us': 1, 'collections': 2})
        self.assertEqual(readings['after'][10]['external-bytes'], None)


class Clauses(unittest.TestCase):
    row = steady.Row('test', 'churn', 'x.scm', {'vm': 1000, 'tree-walker': 1000})

    def test_a_flat_row_holds_every_clause(self):
        results = steady.evaluate(self.row, 'vm', [run(1000)] * 3, 1000)
        self.assertEqual({c: r[2] for c, r in results.items()},
                         {'SS1': True, 'SS2': True, 'SS4': True, 'SS5': True})

    def test_ss1_bound_is_3n_times_8_plus_64_kib(self):
        n = 1000
        bound = 3 * n * 8 + 64 * 1024
        holds = steady.evaluate(self.row, 'vm', [run(n, live=(0, bound))], n)['SS1']
        fails = steady.evaluate(self.row, 'vm', [run(n, live=(0, bound + 1))], n)['SS1']
        self.assertEqual((holds[2], fails[2]), (True, False))
        self.assertAlmostEqual(steady.leak_floor(n), bound / (3 * n))

    def test_ss2_is_f_of_l_and_vacuous_without_two_paced_collections(self):
        live = 1 * MIB
        bound = steady.footprint_bound(live)
        self.assertAlmostEqual(bound, 1.1 * (live + 8 * MIB) + 0.05 * (live + 8 * MIB))
        over = run(1000, live=(live, live), footprint=(int(bound) + 1, int(bound) + 1))
        self.assertFalse(steady.evaluate(self.row, 'vm', [over], 1000)['SS2'][2])
        thin = run(1000, live=(live, live), paced=(1, 3))
        value, _, holds = steady.evaluate(self.row, 'vm', [thin], 1000)['SS2']
        self.assertEqual((value, holds), ('vacuous (1 paced)', False))

    def test_ss4_takes_each_segment_s_minimum_over_the_runs(self):
        # Early segment: 1000 us in one run, 4000 in another: the minimum is
        # 1000; late: 1600 at best, a ratio of 1.6.
        fast = run(1000, cpu=(0, 1000, 5000, 6600))
        slow = run(1000, cpu=(0, 4000, 5000, 7000))
        value, bound, holds = steady.evaluate(self.row, 'vm', [fast, slow], 1000)['SS4']
        self.assertAlmostEqual(value, 1.6)
        self.assertFalse(holds)

    def test_ss5_allows_max_of_16_and_5_percent(self):
        self.assertTrue(steady.evaluate(self.row, 'vm', [run(10, symbols=(40, 56))], 10)['SS5'][2])
        self.assertFalse(steady.evaluate(self.row, 'vm', [run(10, symbols=(40, 57))], 10)['SS5'][2])
        self.assertTrue(steady.evaluate(self.row, 'vm', [run(10, symbols=(1000, 1050))], 10)['SS5'][2])

    def test_class_d_and_peak_rows(self):
        row = steady.Row('d', 'churn', 'x.scm', {'vm': 1000}, clauses=('SS4',),
                         per_cycle={'vm': 100}, live_max=MIB)
        results = steady.evaluate(row, 'vm', [run(1000, live=(0, 3000 * 120))], 1000)
        self.assertEqual(results['D'][:2], (120.0, 120.0))
        self.assertTrue(results['D'][2])
        self.assertTrue(results['peak'][2])
        results = steady.evaluate(row, 'vm', [run(1000, live=(2 * MIB, 2 * MIB))], 1000)
        self.assertFalse(results['peak'][2])

    def test_red_marks_name_a_clause_or_a_clause_on_one_backend(self):
        row = steady.Row('r', 'churn', 'x.scm', {}, red={'SS1': '#1', 'SS2/vm': '#2'})
        self.assertEqual(row.red_reason('SS1', 'tree-walker'), '#1')
        self.assertEqual(row.red_reason('SS2', 'vm'), '#2')
        self.assertIsNone(row.red_reason('SS2', 'tree-walker'))

    def test_every_row_s_program_exists(self):
        for row in steady.ROWS:
            if not row.program.startswith('stream:'):
                self.assertTrue((steady.STEADY / row.program).exists(), row.name)
            self.assertEqual(set(row.n), {'vm', 'tree-walker'}, row.name)
            for mark in row.red:
                clause, _, backend = mark.partition('/')
                self.assertIn(clause, row.clauses + ('peak', 'D'), row.name)
                self.assertIn(backend, ('', 'vm', 'tree-walker'), row.name)


if __name__ == '__main__':
    unittest.main()
