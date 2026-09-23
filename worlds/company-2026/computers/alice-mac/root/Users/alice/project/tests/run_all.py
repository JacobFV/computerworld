"""Run every test module: ``python3 -m tests.run_all`` (also what ``make test`` does)."""

import sys
import unittest

from tests import test_parse, test_stats

MODULES = [test_parse, test_stats]


def suite():
    loader = unittest.TestLoader()
    s = unittest.TestSuite()
    for module in MODULES:
        s.addTests(loader.loadTestsFromModule(module))
    return s


if __name__ == "__main__":
    verbosity = 2 if "-v" in sys.argv[1:] else 1
    result = unittest.TextTestRunner(verbosity=verbosity).run(suite())
    sys.exit(0 if result.wasSuccessful() else 1)
