"""The steady-state lane's arithmetic (#652): parsing the driver's readings,
F(L), and each clause, on synthetic readings; no interpreter needed."""
import dataclasses
import importlib.util
import json
import os
from pathlib import Path
import re
import tempfile
import unittest
from unittest import mock
import warnings

SOURCE = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('steady', SOURCE / 'scripts/run_steady_state.py')
steady = importlib.util.module_from_spec(spec)
spec.loader.exec_module(steady)

MIB = 1 << 20


def run(n, live=(100_000, 100_000), counts=None, cpu=(1000, 2000, 3000, 4000),
        footprint=(5 * MIB, 5 * MIB), paced=(3, 3)):
    """Readings of one run: L, the footprint and each per-owner count at N
    and 4N (40 and 40 unless `counts` gives a key's pair); cpu-us at N (after
    the forced collections), 2N, 3N and 4N; paced collections in [N, 2N) and
    [3N, 4N). Built from the runner's list, so a key added there needs
    nothing here."""
    counts = {key: (40, 40) for key in steady.PER_OWNER_KEYS} | (counts or {})
    after = lambda i, l, f, c: {'live-bytes': l, 'committed-bytes': f, 'external-bytes': 0,
                                'cpu-us': c, 'pause-total-us': 0, 'collections': 10,
                                **{key: pair[i] for key, pair in counts.items()}}
    return {
        'after': {n: after(0, live[0], footprint[0], cpu[0]),
                  4 * n: after(1, live[1], footprint[1], cpu[3] + 5)},
        'pre': {n: {'cpu-us': cpu[0] - 5, 'pause-total-us': 0, 'collections': 8},
                2 * n: {'cpu-us': cpu[1], 'pause-total-us': 0, 'collections': 10 + paced[0]},
                3 * n: {'cpu-us': cpu[2], 'pause-total-us': 0, 'collections': 20},
                4 * n: {'cpu-us': cpu[3], 'pause-total-us': 0, 'collections': 20 + paced[1]}},
    }


class Parsing(unittest.TestCase):
    def test_readings_are_found_behind_a_prompt(self):
        out = ('patina> STEADY at=10 cpu-us=5 pause-total-us=1 collections=2\n'
               'STEADY-AFTER at=10 live-bytes=7 committed-bytes=9 external-bytes=none symbols=40'
               ' ports=3 namespaces=12 cells=900 binding-records=1500 resident-bytes=none\n'
               'something else\n')
        readings = steady.parse(out)
        self.assertEqual(readings['pre'][10], {'cpu-us': 5, 'pause-total-us': 1, 'collections': 2})
        self.assertEqual(readings['after'][10]['external-bytes'], None)
        self.assertEqual((readings['after'][10]['symbols'], readings['after'][10]['ports']), (40, 3))
        # A hyphenated key is read whole.
        self.assertEqual({key: readings['after'][10][key] for key in ('namespaces', 'cells', 'binding-records')},
                         {'namespaces': 12, 'cells': 900, 'binding-records': 1500})
        self.assertEqual(readings['after'][10]['resident-bytes'], None)


class Clauses(unittest.TestCase):
    row = steady.Row('test', 'churn', 'x.scm', {'vm': 1000, 'tree-walker': 1000})

    def test_a_flat_row_holds_every_clause(self):
        results = steady.evaluate(self.row, 'vm', [run(1000)] * 3, 1000)
        self.assertEqual({c: r[2] for c, r in results.items()}, {c: True for c in self.row.checks()})
        self.assertEqual(self.row.checks()[:3], ['SS1', 'SS2', 'SS4'])
        self.assertEqual(self.row.checks()[3:], [f'SS5:{key}' for key in steady.PER_OWNER_KEYS])

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

    def test_ss5_is_one_clause_per_per_owner_key(self):
        self.assertTrue({'symbols', 'ports', 'namespaces', 'cells', 'binding-records', 'code-units',
                         'scope-sets'}
                        <= set(steady.PER_OWNER_KEYS))
        results = steady.evaluate(self.row, 'vm', [run(10, counts={'ports': (40, 57)})], 10)
        self.assertEqual(results['SS5:symbols'], (0, 16, True))
        self.assertEqual(results['SS5:ports'], (17, 16, False))
        for key in ('namespaces', 'cells', 'binding-records', 'code-units', 'scope-sets'):
            self.assertEqual(results[f'SS5:{key}'], (0, 16, True), key)
        self.assertNotIn('SS5', results)

    def test_scope_sets_joins_last(self):
        # #788's key joins after `code-units`, the last before it, as each
        # key joins in the PR that adds it to (gc-stats); its clause fails
        # past max(16, 5%) like any other's.
        self.assertEqual(steady.PER_OWNER_KEYS[-2:], ('code-units', 'scope-sets'))
        self.assertEqual(self.row.checks()[-1], 'SS5:scope-sets')
        results = steady.evaluate(self.row, 'vm', [run(10, counts={'scope-sets': (300, 316)})], 10)
        self.assertEqual(results['SS5:scope-sets'], (16, 16, True))
        results = steady.evaluate(self.row, 'vm', [run(10, counts={'scope-sets': (300, 317)})], 10)
        self.assertEqual(results['SS5:scope-sets'], (17, 16, False))

    def test_ss5_allows_max_of_16_and_5_percent(self):
        for key in steady.PER_OWNER_KEYS:
            counts = lambda c1, c4: run(10, counts={key: (c1, c4)})
            clause = f'SS5:{key}'
            with self.subTest(key=key):
                self.assertTrue(steady.evaluate(self.row, 'vm', [counts(40, 56)], 10)[clause][2])
                self.assertFalse(steady.evaluate(self.row, 'vm', [counts(40, 57)], 10)[clause][2])
                self.assertTrue(steady.evaluate(self.row, 'vm', [counts(1000, 1050)], 10)[clause][2])
                self.assertFalse(steady.evaluate(self.row, 'vm', [counts(1000, 949)], 10)[clause][2])

    def test_class_d_and_peak_rows(self):
        row = steady.Row('d', 'churn', 'x.scm', {'vm': 1000}, clauses=('SS4',),
                         per_cycle={'vm': 100}, live_max=MIB)
        results = steady.evaluate(row, 'vm', [run(1000, live=(0, 3000 * 120))], 1000)
        self.assertEqual(results['D'][:2], (120.0, 120.0))
        self.assertTrue(results['D'][2])
        self.assertTrue(results['peak'][2])
        results = steady.evaluate(row, 'vm', [run(1000, live=(2 * MIB, 2 * MIB))], 1000)
        self.assertFalse(results['peak'][2])

    def test_a_pin_holds_a_class_a_row(self):
        n = 1000
        row = steady.Row('a', 'REPL', 'x.scm', {'vm': n, 'tree-walker': n}, clauses=steady.REPL,
                         red={'SS1': 'stage 5c'}, pin={'vm': 100, 'tree-walker': 200})
        self.assertEqual(steady.mark_errors(row), [])
        self.assertEqual(row.checks()[:2], ['SS1', 'pin'])
        at = lambda cost, backend='vm': steady.evaluate(row, backend, [run(n, live=(0, 3 * n * cost))], n)
        # Within 1.05x of the pin, on each backend's own pin, beside a red SS1.
        results = at(105)
        self.assertEqual(results['pin'], (105.0, 105.0, True))
        self.assertFalse(results['SS1'][2])
        self.assertEqual(row.red_reason('SS1', 'vm'), 'stage 5c')
        self.assertFalse(at(106)['pin'][2])
        self.assertTrue(at(210, 'tree-walker')['pin'][2])
        self.assertFalse(at(211, 'tree-walker')['pin'][2])
        self.assertIsNone(row.red_reason('pin', 'vm'))
        self.assertEqual(list(results), row.checks())
        # In the clauses' own order, however a row lists them.
        shuffled = dataclasses.replace(row, clauses=tuple(reversed(steady.REPL)))
        self.assertEqual(shuffled.checks(), row.checks())
        # At another N (`--scale`) the pin is not held: a capacity charge
        # steps with N.
        results = steady.evaluate(row, 'vm', [run(2 * n, live=(0, 6 * n * 500))], 2 * n)
        self.assertEqual(results['pin'], ('not held at N=2000', 105.0, True))
        # Only a row that declares SS1 has one, and it pins both backends.
        for bad in (steady.Row('b', 'churn', 'x.scm', {}, clauses=('SS4',),
                               pin={'vm': 1, 'tree-walker': 1}),
                    steady.Row('c', 'REPL', 'x.scm', {}, clauses=steady.REPL, pin={'vm': 1})):
            self.assertTrue(steady.mark_errors(bad), bad.name)

    def test_red_marks_name_a_clause_or_a_clause_on_one_backend(self):
        row = steady.Row('r', 'churn', 'x.scm', {}, red={'SS1': '#1', 'SS2/vm': '#2'})
        self.assertEqual(row.red_reason('SS1', 'tree-walker'), '#1')
        self.assertEqual(row.red_reason('SS2', 'vm'), '#2')
        self.assertIsNone(row.red_reason('SS2', 'tree-walker'))

    def test_new_per_owner_marks_are_accepted(self):
        # #834's keys and #836's `code-units` are marked as any other
        # per-owner key: by name, on one backend or both, and each silences
        # its own key only.
        row = steady.Row('r', 'REPL', 'x.scm', {}, clauses=steady.REPL,
                         red={'SS5:namespaces/vm': '#1', 'SS5:cells': '#2', 'SS5:binding-records': '#3',
                              'SS5:code-units/vm': '#4', 'SS5:scope-sets/tree-walker': '#5'})
        self.assertEqual(steady.mark_errors(row), [])
        self.assertEqual(row.red_reason('SS5:namespaces', 'vm'), '#1')
        self.assertIsNone(row.red_reason('SS5:namespaces', 'tree-walker'))
        self.assertEqual(row.red_reason('SS5:code-units', 'vm'), '#4')
        self.assertIsNone(row.red_reason('SS5:code-units', 'tree-walker'))
        self.assertEqual(row.red_reason('SS5:scope-sets', 'tree-walker'), '#5')
        self.assertIsNone(row.red_reason('SS5:scope-sets', 'vm'))
        for backend in steady.BACKENDS:
            self.assertEqual(row.red_reason('SS5:cells', backend), '#2')
            self.assertEqual(row.red_reason('SS5:binding-records', backend), '#3')
            self.assertIsNone(row.red_reason('SS5:symbols', backend))
        self.assertEqual(row.checks()[-5:],
                         ['SS5:namespaces', 'SS5:cells', 'SS5:binding-records', 'SS5:code-units',
                          'SS5:scope-sets'])

    def test_hidden_define_records_and_cells_are_stage_5c(self):
        # Each expansion's introduced definition is a record and a cell
        # (#834), so `cells` and `binding-records` grow one a cycle on both
        # backends until stage 5c reclaims introduced definitions (R1); the
        # other per-owner keys stay held, `code-units` among them (#836).
        rows = {row.name: row for row in steady.ROWS}
        hidden = rows['hidden-define']
        self.assertEqual(steady.mark_errors(hidden), [])
        held = [key for key in steady.PER_OWNER_KEYS if key not in ('cells', 'binding-records')]
        self.assertIn('code-units', held)
        for backend in steady.BACKENDS:
            for key in ('cells', 'binding-records'):
                self.assertEqual(hidden.red_reason(f'SS5:{key}', backend), 'stage 5c', (key, backend))
            for key in held:
                self.assertIsNone(hidden.red_reason(f'SS5:{key}', backend), (key, backend))
            # A reference to an unbound name makes a placeholder, dropped
            # once no code links it (#836).
            for key in ('namespaces', 'cells', 'binding-records'):
                self.assertIsNone(rows['unbound-ref'].red_reason(f'SS5:{key}', backend), (key, backend))
        # The owner's re-pin for the records (question 2 on #834).
        self.assertEqual(hidden.pin, {'vm': 257, 'tree-walker': 129})
        # `unbound-ref`'s readings at its N with the linking (#836; R9), the
        # VM's from CI's runner, where the placeholder store's table steps
        # once more inside the window than on an Apple M4 Pro (84.48 B).
        self.assertEqual(rows['unbound-ref'].pin, {'vm': 101.55, 'tree-walker': 84.35})
        self.assertIn('pin', rows['unbound-ref'].checks())

    def test_library_stream_is_promoted(self):
        # Its SS1 passes since #834 and #838 promoted it (start decision 12 on
        # #782): no mark, and no pin, since a green SS1 holds the row at its
        # bound, which the pin's 1.05x equalled (question 4 on #834).
        row = next(row for row in steady.ROWS if row.name == 'library-stream')
        self.assertEqual(steady.mark_errors(row), [])
        self.assertEqual(row.red, {})
        self.assertIsNone(row.pin)
        self.assertNotIn('pin', row.checks())
        self.assertEqual(row.clauses, steady.REPL)
        for backend in steady.BACKENDS:
            for check in row.checks():
                self.assertIsNone(row.red_reason(check, backend), (check, backend))
        # No row carries #614's mark any more.
        self.assertFalse([r.name for r in steady.ROWS if '#614' in r.red.values()])

    def test_the_pacer_s_row_runs_per_pr(self):
        # `env-lambda` is the row a pacer that follows dead namespaces fails
        # (#836): its collections come further apart until SS2 is vacuous.
        # It runs on every change, with SS2 declared and no mark on it.
        row = next(row for row in steady.ROWS if row.name == 'env-lambda')
        self.assertTrue(row.pr)
        self.assertIn('SS2', row.checks())
        for backend in steady.BACKENDS:
            self.assertIsNone(row.red_reason('SS2', backend))
            # Four paced collections a segment, where SS2 takes two: at 500
            # it held two, and a leaner namespace failed it for no defect.
            self.assertEqual(row.n[backend], 1000)

    def test_the_constants_row_is_red_on_the_vm_alone(self):
        # `const-lambda` is the lag the settled L does not reach: a dead
        # unit's constants are roots for one more collection, and L counts
        # what they hold. Red for #866 on the VM; the tree-walker,
        # which loads no code, holds SS2 unmarked.
        row = next(row for row in steady.ROWS if row.name == 'const-lambda')
        self.assertFalse(row.pr)
        self.assertEqual(row.red_reason('SS2', 'vm'), '#866')
        self.assertIsNone(row.red_reason('SS2', 'tree-walker'))
        self.assertEqual(steady.mark_errors(row), [])

    def test_ss5_marks_name_one_key(self):
        row = steady.Row('r', 'churn', 'x.scm', {}, red={'SS5:ports/vm': '#1'})
        self.assertEqual(row.red_reason('SS5:ports', 'vm'), '#1')
        self.assertIsNone(row.red_reason('SS5:ports', 'tree-walker'))
        self.assertIsNone(row.red_reason('SS5:symbols', 'vm'))
        self.assertEqual(steady.mark_errors(row), [])

    def test_every_row_s_program_exists(self):
        for row in steady.ROWS:
            if not row.program.startswith('stream:'):
                self.assertTrue((steady.STEADY / row.program).exists(), row.name)
            self.assertEqual(set(row.n), {'vm', 'tree-walker'}, row.name)
            self.assertEqual(steady.mark_errors(row), [], row.name)
        # A bare SS5 would silence every per-owner key; a key the list lacks
        # names no clause; and a mark must name a clause the row declares.
        for red in ({'SS5': '#1'}, {'SS5/vm': '#1'}, {'SS5:no-such-key': '#1'}, {'SS2': '#1'},
                    {'SS1/arm': '#1'}, {'pin': '#1'}, {'pin/vm': '#1'}):
            row = steady.Row('r', 'REPL', 'x.scm', {}, clauses=steady.REPL, red=red,
                             pin={'vm': 1, 'tree-walker': 1})
            self.assertTrue(steady.mark_errors(row), red)


class Readings(unittest.TestCase):
    """A run whose readings a clause cannot use fails the row by name, before
    any clause is evaluated, whatever the row's marks."""

    def test_a_per_owner_key_without_a_reading_fails(self):
        n = 10
        for red in ({}, {'SS5:ports': 'stage 5c'}):
            row = steady.Row('r', 'churn', 'x.scm', {}, red=red)
            readings = run(n)
            readings['after'][n]['ports'] = None
            problem = steady.reading_failure(row, readings, n)
            self.assertEqual(problem, f'no reading of `ports` at [{n}]')
            # It fails the row on its own backend, the mark notwithstanding.
            self.assertEqual(steady.problem_verdict(row, 'vm', problem, n), ('FAIL', []))
            # A key the driver did not print at all at 4N, as well.
            del readings['after'][4 * n]['ports']
            self.assertEqual(steady.reading_failure(row, readings, n),
                             f'no reading of `ports` at [{n}, {4 * n}]')
        self.assertIsNone(steady.reading_failure(steady.Row('r', 'churn', 'x.scm', {}), run(n), n))
        # A key no declared clause reads may be missing: SS5 is not declared.
        row = steady.Row('r', 'churn', 'x.scm', {}, clauses=('SS4',))
        readings = run(n)
        readings['after'][n]['ports'] = None
        self.assertIsNone(steady.reading_failure(row, readings, n))
        # cpu-us is read after the collections at N only, by SS4's first
        # segment; at 4N no clause reads it.
        readings['after'][4 * n]['cpu-us'] = None
        self.assertIsNone(steady.reading_failure(row, readings, n))
        # None in cpu-us, which SS4 reads, before the collections.
        readings['pre'][3 * n]['cpu-us'] = None
        self.assertEqual(steady.reading_failure(row, readings, n), f'no reading of `cpu-us` at [{3 * n}]')
        # A reading the driver never printed.
        readings = run(n)
        del readings['pre'][2 * n]
        self.assertEqual(steady.reading_failure(row, readings, n), f'no reading at [{2 * n}]')

    def test_only_the_resident_size_cap_is_red_on_a_marked_row(self):
        marked = steady.Row('r', 'REPL', 'x.scm', {}, clauses=steady.REPL, red={'SS5:symbols/vm': '#1'})
        unmarked = steady.Row('r', 'REPL', 'x.scm', {}, clauses=steady.REPL)
        cap = 'resident size passed the 4096 MiB cap'
        self.assertEqual(steady.problem_verdict(marked, 'vm', cap, 10), ('RED', ['#1']))
        self.assertEqual(steady.problem_verdict(marked, 'tree-walker', cap, 10), ('FAIL', []))
        self.assertEqual(steady.problem_verdict(unmarked, 'vm', cap, 10), ('FAIL', []))
        for problem in ('timed out after 600 s', 'exited 1: boom', 'no reading at [10]',
                        'no reading of `ports` at [10]'):
            self.assertEqual(steady.problem_verdict(marked, 'vm', problem, 10), ('FAIL', []), problem)
        # A mark on `peak` or `D` alone does not make a cap hit red.
        peak = steady.Row('p', 'server', 'x.scm', {}, live_max=1, red={'peak': '#2'})
        self.assertEqual(steady.problem_verdict(peak, 'vm', cap, 10), ('FAIL', []))
        # A pinned row's red SS1 does not make a cap hit red at the row's own
        # N, where the pin is held, on either backend; under `--scale` it is
        # red, as on any marked row.
        pinned = steady.Row('a', 'REPL', 'x.scm', {'vm': 10, 'tree-walker': 10}, clauses=steady.REPL,
                            red={'SS1': 'stage 5c'}, pin={'vm': 100, 'tree-walker': 100})
        for backend in steady.BACKENDS:
            self.assertEqual(steady.problem_verdict(pinned, backend, cap, 10), ('FAIL', []))
            self.assertEqual(steady.problem_verdict(pinned, backend, cap, 20), ('RED', ['stage 5c']))
        hidden = next(row for row in steady.ROWS if row.name == 'hidden-define')
        self.assertEqual(steady.problem_verdict(hidden, 'tree-walker', cap, hidden.n['tree-walker']),
                         ('FAIL', []))


class Bounds(unittest.TestCase):
    """A row that holds a key of (gc-stats) to a bound (#910): the reading
    its program makes with `steady-bound-reading`, in every run."""
    row = steady.Row('b', 'server', 'x.scm', {'vm': 10, 'tree-walker': 10},
                     bounds={'committed-stack-bytes': {'vm': (1, MIB), 'tree-walker': (0, 0)}})

    @staticmethod
    def bounded(n, value, key='committed-stack-bytes'):
        return run(n) | {'bound': {0: {key: value}}}

    def test_a_bound_reading_is_parsed_beside_the_others(self):
        out = ('STEADY-BOUND at=0 committed-stack-bytes=1048576 stack-limit=none\n'
               'STEADY at=10 cpu-us=5 pause-total-us=1 collections=2\n')
        readings = steady.parse(out)
        self.assertEqual(readings['bound'], {0: {'committed-stack-bytes': 1048576, 'stack-limit': None}})
        self.assertEqual(readings['pre'][10]['cpu-us'], 5)
        self.assertEqual(readings['after'], {})

    def test_a_bound_holds_each_end_and_every_run(self):
        n = 10
        name = 'bound:committed-stack-bytes'
        self.assertIn(name, self.row.checks())
        held = lambda backend, *values: steady.evaluate(
            self.row, backend, [self.bounded(n, v) for v in values], n)[name]
        self.assertEqual(held('vm', MIB), (f'{MIB} in [1, {MIB}]', MIB, True))
        self.assertTrue(held('vm', 1)[2])
        # One byte past either end fails, and so does one run of three.
        self.assertEqual(held('vm', MIB + 1), (f'{MIB + 1} in [1, {MIB}]', MIB, False))
        self.assertFalse(held('vm', 0)[2])
        self.assertEqual(held('vm', MIB, 2 * MIB, 4096), (f'{2 * MIB} in [1, {MIB}]', MIB, False))
        self.assertEqual(held('vm', MIB, 0, 4096)[0], f'0 in [1, {MIB}]')
        # Where every run is inside, the largest is shown.
        self.assertEqual(held('vm', 4096, MIB, 8192)[0], f'{MIB} in [1, {MIB}]')
        # Each backend has its own range.
        self.assertTrue(held('tree-walker', 0)[2])
        self.assertFalse(held('tree-walker', 1)[2])
        # A row with no bound has no such result.
        plain = steady.Row('p', 'server', 'x.scm', {})
        self.assertEqual([c for c in plain.checks() if c.startswith('bound:')], [])
        self.assertNotIn(name, steady.evaluate(plain, 'vm', [run(n)], n))

    def test_a_bound_without_its_reading_fails_the_row(self):
        n = 10
        self.assertIsNone(steady.reading_failure(self.row, self.bounded(n, 4096), n))
        # The program never called `steady-bound-reading`.
        problem = steady.reading_failure(self.row, run(n), n)
        self.assertEqual(problem, 'no bound reading')
        self.assertEqual(steady.problem_verdict(self.row, 'vm', problem, n), ('FAIL', []))
        # (gc-stats) lacks the key, or the reading names another.
        self.assertEqual(steady.reading_failure(self.row, self.bounded(n, None), n),
                         'no reading of `committed-stack-bytes` for its bound')
        self.assertEqual(steady.reading_failure(self.row, self.bounded(n, 1, key='stack-limit'), n),
                         'no reading of `committed-stack-bytes` for its bound')
        # A row with no bound needs no such reading.
        self.assertIsNone(steady.reading_failure(steady.Row('p', 'server', 'x.scm', {}), run(n), n))

    def test_no_mark_names_a_bound_and_a_bound_names_both_backends(self):
        marked = dataclasses.replace(self.row, red={'bound:committed-stack-bytes': '#1'})
        self.assertEqual(steady.mark_errors(marked),
                         ["b: 'bound:committed-stack-bytes' marks a bound, which is a gate"])
        on_one = dataclasses.replace(self.row, red={'bound:committed-stack-bytes/vm': '#1'})
        self.assertEqual(len(steady.mark_errors(on_one)), 1)
        self.assertEqual(steady.mark_errors(self.row), [])
        # A cap hit on a row whose only marks are elsewhere is as before,
        # and a bound gives it no reason of its own.
        cap = 'resident size passed the 4096 MiB cap'
        self.assertEqual(steady.problem_verdict(self.row, 'vm', cap, 10), ('FAIL', []))
        one = dataclasses.replace(self.row, bounds={'k': {'vm': (0, 1)}})
        self.assertEqual(len(steady.mark_errors(one)), 1)
        inverted = dataclasses.replace(self.row, bounds={'k': {'vm': (2, 1), 'tree-walker': (0, 0)}})
        self.assertEqual(len(steady.mark_errors(inverted)), 1)

    def test_arguments_may_differ_by_backend(self):
        same = steady.Row('s', 'server', 'x.scm', {}, ('7',))
        self.assertEqual((same.args_for('vm'), same.args_for('tree-walker')), (('7',), ('7',)))
        each = steady.Row('e', 'server', 'x.scm', {}, {'vm': ('10',), 'tree-walker': ('1',)})
        self.assertEqual((each.args_for('vm'), each.args_for('tree-walker')), (('10',), ('1',)))
        self.assertEqual(steady.mark_errors(each), [])
        self.assertEqual(len(steady.mark_errors(steady.Row('o', 'server', 'x.scm', {}, {'vm': ('10',)}))), 1)
        # What a run is given: N, then the backend's own arguments.
        with tempfile.TemporaryDirectory() as scratch:
            binary = Path(scratch) / 'patina'
            binary.write_text('#!/usr/bin/env python3\nimport json, sys\n'
                              'open(sys.argv[-1], "a").write(json.dumps(sys.argv[1:-1]) + "\\n")\n')
            binary.chmod(0o755)
            row = steady.Row('r', 'server', 'x.scm', {},
                             {'vm': ('10', '{scratch}/argv.txt'), 'tree-walker': ('1', '{scratch}/argv.txt')})
            with warnings.catch_warnings():
                warnings.simplefilter('ignore', ResourceWarning)
                for backend in steady.BACKENDS:
                    steady.run_once(binary, row, backend, 5, 60, 4096, scratch)
            seen = [json.loads(line) for line in (Path(scratch) / 'argv.txt').read_text().splitlines()]
        program = str(steady.STEADY / 'x.scm')
        self.assertEqual(seen, [[program, '5', '10'], ['--tree-walker', program, '5', '1']])

    def test_the_stack_returned_row_holds_4d_s_gate(self):
        # Row 4d's gate (PRD/GC_PRD.md §19): a 10 M-deep recursion on the VM,
        # its pages given back after two observations, read through the key;
        # 1 M frames on the tree-walker (§17.4), which keeps no such stack.
        row = next(row for row in steady.ROWS if row.name == 'stack-returned')
        self.assertEqual(row.args, {'vm': ('10000000',), 'tree-walker': ('1000000',)})
        self.assertFalse(row.pr)
        low, high = row.bounds['committed-stack-bytes']['vm']
        self.assertEqual(high, MIB)
        self.assertGreater(low, 0, 'a key that reads 0 is not a stack given back')
        self.assertEqual(row.bounds['committed-stack-bytes']['tree-walker'], (0, 0))
        low, high = row.bounds['stack-limit']['vm']
        self.assertGreater(low, 10_000_000 * 136)
        self.assertEqual(high, 8 << 30)
        self.assertEqual(row.bounds['stack-limit']['tree-walker'], (0, 0))
        self.assertEqual(row.red, {'SS2/vm': '#616'})
        self.assertEqual(steady.mark_errors(row), [])
        # Its program recurses, checks the answer and reads both keys after
        # two collections, before the steady loop.
        program = (steady.STEADY / row.program).read_text()
        reading = program.index("(steady-bound-reading '(committed-stack-bytes stack-limit))")
        self.assertLess(program.index('(deep depth)'), reading)
        self.assertLess(reading, program.index('(steady-run'))


class Driver(unittest.TestCase):
    driver = (steady.STEADY / 'driver.scm').read_text()

    def test_a_bound_is_read_after_two_collections(self):
        reading = self.driver[self.driver.index('(define (steady-bound-reading keys)'):]
        self.assertRegex(reading, r'^\(define \(steady-bound-reading keys\)\s*\(gc\)\s*\(gc\)\s*'
                                  r'\(steady-print "STEADY-BOUND" 0 keys \(gc-stats\)\)\)')

    def test_the_driver_prints_every_per_owner_key(self):
        keys = re.search(r"\(define steady-per-owner-keys '\(([^)]*)\)\)", self.driver)
        self.assertIsNotNone(keys)
        self.assertEqual(tuple(keys.group(1).split()), steady.PER_OWNER_KEYS)
        # Printed after the collections, as part of the after-collection keys.
        after = re.search(r'\(define steady-after-keys(.*?)\n\n', self.driver, re.S)
        self.assertIsNotNone(after)
        self.assertIn('steady-per-owner-keys', after.group(1))
        reading = self.driver[self.driver.index('(define (steady-reading'):]
        self.assertRegex(reading, r'\(gc\)\s*\(gc\)\s*\(let \(\(after \(gc-stats\)\)\)')
        self.assertIn('(steady-print "STEADY-AFTER" at steady-after-keys after)', reading)

    def test_every_key_is_read_through_one_lookup(self):
        # No `assq` outside `steady-value`, which prints `none` for a key
        # (gc-stats) lacks or one that answers #f.
        lookup = self.driver.index('(define (steady-value')
        rest = self.driver[:lookup] + self.driver[self.driver.index('(define (steady-print'):]
        self.assertNotIn('assq', rest)
        self.assertEqual(self.driver.count('(assq '), 1)
        self.assertIn("(if (and entry (cdr entry)) (cdr entry) 'none)", self.driver)


class Environment(unittest.TestCase):
    def test_a_run_gets_none_of_the_caller_s_patina_or_allocator_settings(self):
        # A stand-in for the interpreter, which writes the names of its
        # environment's variables to the file its last argument names.
        with tempfile.TemporaryDirectory() as scratch:
            binary = Path(scratch) / 'patina'
            binary.write_text('#!/usr/bin/env python3\nimport json, os, sys\n'
                              'open(sys.argv[-1], "w").write(json.dumps(sorted(os.environ)))\n')
            binary.chmod(0o755)
            row = steady.Row('r', 'churn', 'x.scm', {}, args=('{scratch}/environment.json',))
            caller = {'MallocNanoZone': '0', 'MallocXzoneThreadCaching': '0', 'PATINA_GC_STRESS': '16',
                      'STEADY_TEST_KEPT': '1'}
            # run_once leaves the child's pipes for the collector to close.
            with mock.patch.dict(os.environ, caller), warnings.catch_warnings():
                warnings.simplefilter('ignore', ResourceWarning)
                steady.run_once(binary, row, 'vm', 1, 60, 4096, scratch)
            names = json.loads((Path(scratch) / 'environment.json').read_text())
        self.assertIn('STEADY_TEST_KEPT', names)
        self.assertEqual([name for name in names if name.startswith(('PATINA_', 'Malloc'))], [])


if __name__ == '__main__':
    unittest.main()
