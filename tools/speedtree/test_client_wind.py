import unittest

from client_wind_reference import WindNoise


class ClientWindTests(unittest.TestCase):
    def test_emulated_client_samples(self):
        wind = WindNoise()
        self.assertEqual(wind.offsets, [215, 370, 417, 434, 952, 739, 9, 779])
        for time, expected in [(0, .386193573), (.125, .385221481), (1, .378416747),
                               (16, .356702805), (256, .286081135),
                               (1024, .194191948), (65536, .386193573)]:
            self.assertAlmostEqual(wind.sample(time), expected, places=6)

    def test_invalid_time(self):
        wind = WindNoise()
        for time in [-1., float('nan'), float('inf')]:
            with self.assertRaises(ValueError):
                wind.sample(time)


if __name__ == '__main__':
    unittest.main()
