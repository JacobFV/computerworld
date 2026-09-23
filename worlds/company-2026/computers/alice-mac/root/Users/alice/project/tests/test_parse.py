import unittest
from datetime import datetime

from telemetry.parse import LogError, Sample, parse_rows


class ParseRows(unittest.TestCase):
    def test_reads_samples(self):
        rows = [
            ["timestamp", "channel", "value", "unit"],
            ["2026-09-15T14:02:10", "V33", "3.301", "V"],
            [],
            ["2026-09-15T14:02:11", "temp", "31.5", "degC"],
        ]
        samples = parse_rows(rows)
        self.assertEqual(len(samples), 2)
        self.assertEqual(samples[0], Sample(datetime(2026, 9, 15, 14, 2, 10), "v33", 3.301, "V"))
        self.assertEqual(samples[1].channel, "temp")

    def test_rejects_bad_header(self):
        with self.assertRaises(LogError):
            parse_rows([["time", "chan", "val", "unit"]])

    def test_reports_line_numbers(self):
        rows = [["timestamp", "channel", "value", "unit"], ["yesterday", "v33", "3.3", "V"]]
        with self.assertRaises(LogError) as cm:
            parse_rows(rows, source="log.csv")
        self.assertIn("log.csv:2", str(cm.exception))

    def test_empty_log(self):
        self.assertEqual(parse_rows([]), [])


if __name__ == "__main__":
    unittest.main()
