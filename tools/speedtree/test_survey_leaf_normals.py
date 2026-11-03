import unittest
from survey_leaf_normals import check_trace

HEADER = 'lod\tleaves\tchanged_components\ttested_components\tmax_abs_difference\n'
ROW = '0\t1\t0\t192\t0\n'


class LeafNormalSurveyTests(unittest.TestCase):
    def test_valid(self):
        self.assertEqual(check_trace(HEADER + ROW, 1), (1, 0, 192, 0.0))
        self.assertEqual(check_trace(HEADER + '0\t1\t2\t192\t0.25\n', 1), (1, 2, 192, 0.25))
        self.assertEqual(check_trace(HEADER, 0), (0, 0, 0, 0.0))

    def test_invalid(self):
        for text, count in [(HEADER, 1), (HEADER + ROW * 2, 1),
                            (HEADER + ROW, 0), (HEADER + ROW.replace('192', '191'), 1),
                            (HEADER + ROW.replace('\t0\t192', '\t1\t192'), 1),
                            (HEADER + ROW.replace('\t0\n', '\tnan\n'), 1),
                            ('bad\n', 1)]:
            with self.subTest(text=text), self.assertRaises(ValueError):
                check_trace(text, count)


if __name__ == '__main__':
    unittest.main()
