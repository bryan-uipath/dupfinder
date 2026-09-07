import unittest
from run import matches, parse_pairs


class EvidenceTests(unittest.TestCase):
    def test_names_preserve_locations_and_scores(self):
        output = '0.75  [name 1.00 / types 0.00]\n      fn `alpha`  src/a file.ts:2\n      fn `beta`  src/b.ts:9\n'
        pair, = parse_pairs('names', output)
        self.assertEqual(pair['a'], {'name': 'alpha', 'file': 'src/a file.ts', 'start': 2, 'end': 2})
        self.assertEqual(pair['score'], 0.75)
        self.assertEqual(pair['b']['start'], 9)

    def test_clone_ranges_match_either_orientation_but_not_unrelated_regions(self):
        pair, = parse_pairs('clones', 'src/a.ts:10-20 <-> src/b.ts:30-40  (10 lines)\n')
        self.assertTrue(matches(pair, {'a': pair['b'], 'b': pair['a']}))
        self.assertFalse(matches(pair, {'a': {**pair['a'], 'start': 21, 'end': 30}, 'b': pair['b']}))
        self.assertFalse(matches(pair, {'a': {**pair['a'], 'file': 'other/a.ts'}, 'b': pair['b']}))


if __name__ == '__main__':
    unittest.main()
