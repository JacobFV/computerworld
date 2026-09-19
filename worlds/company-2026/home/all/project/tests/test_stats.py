import unittest
from datetime import datetime

from telemetry.parse import Sample
from telemetry.stats import check_limits, summarize


def sample(channel, value, unit="V"):
    return Sample(datetime(2026, 9, 15, 12, 0, 0), channel, value, unit)


class Summarize(unittest.TestCase):
    def test_statistics_per_channel(self):
        s = summarize([sample("v33", 3.30), sample("v33", 3.32), sample("temp", 30.0, "degC")])
        self.assertEqual([x.channel for x in s], ["temp", "v33"])
        v33 = s[1]
        self.assertEqual(v33.count, 2)
        self.assertAlmostEqual(v33.mean, 3.31)
        self.assertAlmostEqual(v33.minimum, 3.30)
        self.assertAlmostEqual(v33.maximum, 3.32)
        self.assertAlmostEqual(v33.stdev, 0.014142, places=5)
        self.assertEqual(s[0].stdev, 0.0)

    def test_limits(self):
        s = summarize([sample("v33", 3.30), sample("v33", 3.40), sample("iload", 0.12, "A")])
        failures = check_limits(s)
        self.assertEqual(failures, [("v33", "max 3.400 above 3.366")])
        self.assertEqual(check_limits(s, {"v33": (3.0, 3.5)}), [])


if __name__ == "__main__":
    unittest.main()
