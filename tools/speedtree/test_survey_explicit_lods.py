import unittest
from pathlib import Path
from survey_explicit_lods import FIELDS, check_rows, check_combinations

HEADER = '\t'.join(FIELDS) + '\n'
ROW = '0\t0\t0\t0\t3\t3\t0123456789abcdef\t0123456789abcdef\n'


class ExplicitSurveyTests(unittest.TestCase):
    def test_complete(self):
        self.assertEqual(check_rows(HEADER + ROW, (1, 0, 0)), 1)
        self.assertEqual(check_combinations('12\t0\n', (2, 2, 3)), 12)

    def test_missing_duplicate_and_bad_geometry(self):
        for text, counts in [(HEADER, (1, 0, 0)), (HEADER + ROW * 2, (1, 0, 0)),
                             (HEADER + ROW.replace('3\t3', '3\t2'), (1, 0, 0)),
                             (HEADER + ROW.replace('abcdef\n', 'abcdee\n'), (1, 0, 0)),
                             (HEADER + ROW, (0, 0, 0))]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                check_rows(text, counts)

    def test_compatibility_trace_fixtures(self):
        fixtures = Path(__file__).parent / 'fixtures' / 'lod-explicit'
        for name, counts in [('000', (2, 2, 2)), ('012', (6, 4, 3)),
                             ('039', (5, 5, 6))]:
            with self.subTest(name=name):
                self.assertEqual(check_rows((fixtures / (name + '.tsv')).read_text(), counts),
                                 sum(counts))

    def test_bad_combinations(self):
        for text in ['11\t0', '12\t1', '12', 'garbage\t0']:
            with self.subTest(text=text), self.assertRaises(ValueError):
                check_combinations(text, (2, 2, 3))


if __name__ == '__main__':
    unittest.main()
