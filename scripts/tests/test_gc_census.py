"""The census script's payload shares (#931): what it reads of
SHARED_PAYLOADS, and the shares it computes from a summary's variant counts
and Drop flags."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gc_census  # noqa: E402


class SharedVariants(unittest.TestCase):
    def test_the_list_is_read_by_variant_with_comments_skipped(self):
        text = '''
const OTHER: &[&str] = &["Nope.0"];
const SHARED_PAYLOADS: &[&str] = &[
    "Continuation.k",
    // "Commented.out",
    "Macro.body",
    "Macro.relinks", // a trailing comment "Trailing.x"
    "Symbol.0",
];
'''
        self.assertEqual(gc_census.shared_variants(text), ['Continuation', 'Macro', 'Symbol'])

    def test_the_real_list_names_variants_the_census_knows(self):
        names = set(gc_census.shared_variants())
        self.assertIn('Procedure', names)
        self.assertNotIn('Identifier', names)


def summary(obj_variant, drops, vectors=0, strings=0, pairs=0):
    names = ['Real', 'Symbol', 'Procedure', 'Identifier']
    return {'variant_names': names, 'obj_variant': obj_variant, 'variant_drop': drops,
            'alloc_by_arena': [pairs, vectors, strings, sum(obj_variant)],
            'alloc_total': pairs + vectors + strings + sum(obj_variant)}


class Shares(unittest.TestCase):
    def test_the_shares_count_objects_and_slots(self):
        # 10 objects: 2 Real (no Drop), 3 Symbol (Drop, shared), 4
        # Procedure (Drop, shared), 1 Identifier (neither); 5 vector and 5
        # string slots, which own a buffer; 20 pairs.
        d = summary([2, 3, 4, 1], [0, 1, 1, 0], vectors=5, strings=5, pairs=20)
        shares = gc_census.payload_shares({'w': d})
        self.assertAlmostEqual(shares['drop_share_objects'], 7 / 40)
        self.assertAlmostEqual(shares['drop_share_slots'], 17 / 40)
        self.assertAlmostEqual(shares['shared_share_objects'], 7 / 40)
        self.assertEqual(shares['drop_variants'], ['Symbol', 'Procedure'])

    def test_a_flag_seen_in_any_workload_counts(self):
        a = summary([1, 0, 0, 0], [0, 0, 0, 0])
        b = summary([0, 1, 0, 0], [0, 1, 0, 0])
        shares = gc_census.payload_shares({'a': a, 'b': b})
        self.assertEqual(shares['drop_variants'], ['Symbol'])

    def test_a_run_without_flags_gives_none(self):
        d = summary([1, 1, 1, 1], [0, 0, 0, 0])
        del d['variant_drop']
        self.assertIsNone(gc_census.payload_shares({'w': d})['drop_share_objects'])


if __name__ == '__main__':
    unittest.main()
