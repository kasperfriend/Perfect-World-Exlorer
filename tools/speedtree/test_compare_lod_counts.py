import csv
from pathlib import Path
import tempfile
import unittest

from compare_lod_counts import compare


class CountComparisonTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.emulated = Path(self.temp.name) / 'emulated.tsv'
        self.execution = Path(self.temp.name) / 'execution.tsv'
        self.write(self.emulated, ['type_id', 'emulated_compute_status', 'branch_count', 'frond_count', 'leaf_count'],
                   [[i, 'ok', 2, 2, 2] for i in range(59)])
        self.write(self.execution, ['type', 'branch', 'frond', 'leaf'],
                   [[i, 2, 2, 2] for i in range(59)])

    @staticmethod
    def write(path, header, rows):
        with path.open('w', newline='') as file:
            writer = csv.writer(file, delimiter='\t')
            writer.writerow(header)
            writer.writerows(rows)

    def test_equal(self):
        self.assertEqual(compare(self.emulated, self.execution), 59)

    def test_mismatch_and_incomplete_or_duplicate_types_are_rejected(self):
        for rows, message in [
            ([[i, 2, 2, 3 if i == 7 else 2] for i in range(59)], 'mismatches'),
            ([[i, 2, 2, 2] for i in range(58)], 'expected exactly'),
            ([[i, 2, 2, 2] for i in range(59)] + [[0, 2, 2, 2]], 'duplicate'),
        ]:
            self.write(self.execution, ['type', 'branch', 'frond', 'leaf'], rows)
            with self.assertRaisesRegex(ValueError, message):
                compare(self.emulated, self.execution)


if __name__ == '__main__':
    unittest.main()
