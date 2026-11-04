import importlib.util
from pathlib import Path
import struct
import unittest

spec = importlib.util.spec_from_file_location('asset_survey', Path(__file__).with_name('asset_survey.py'))
survey = importlib.util.module_from_spec(spec)
spec.loader.exec_module(survey)


def world(records):
    header = bytearray(116)
    struct.pack_into('<II', header, 0, 0x45435744, 12)
    table = bytearray()
    for pair in records:
        for text in pair:
            encoded = text.encode('gbk')
            table += struct.pack('<I', len(encoded)) + encoded
    struct.pack_into('<III', header, 36, len(header), len(header) + len(table), len(records))
    return bytes(header + table)


class TreeTableTest(unittest.TestCase):
    def test_paths_and_order(self):
        data = world([('Trees\\树.spt', 'Trees\\叶.dds'), ('b.spt', 'b.dds')])
        self.assertEqual(survey.tree_types(data), [('Trees/树.spt', 'Trees/叶.dds'), ('b.spt', 'b.dds')])

    def test_bad_header_and_bounds(self):
        with self.assertRaises(ValueError):
            survey.tree_types(b'bad')
        data = bytearray(world([('a', 'b')]))
        struct.pack_into('<I', data, 40, len(data) + 1)
        with self.assertRaises(ValueError):
            survey.tree_types(data)

    def test_truncated_or_extra_record_data(self):
        data = bytearray(world([('a', 'b')]))
        struct.pack_into('<I', data, 116, 5000)
        with self.assertRaises(ValueError):
            survey.tree_types(data)
        data = bytearray(world([('a', 'b')]))
        struct.pack_into('<I', data, 44, 0)
        with self.assertRaises(ValueError):
            survey.tree_types(data)

    def test_invalid_gbk_is_not_silently_replaced(self):
        data = bytearray(world([('a', 'b')]))
        data[120] = 0xff
        with self.assertRaises(UnicodeError):
            survey.tree_types(data)


if __name__ == '__main__':
    unittest.main()
