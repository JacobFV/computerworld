"""unittest for the simulated interpreter (TestCase, assertions, main)."""
import sys
import traceback

__all__ = ['TestCase', 'main', 'skip', 'skipIf', 'skipUnless', 'expectedFailure',
           'TestSuite', 'TextTestRunner', 'TestLoader', 'SkipTest', 'mock']


class SkipTest(Exception):
    pass


class _AssertRaisesContext:
    def __init__(self, expected, test_case, expected_regex=None):
        self.expected = expected
        self.test_case = test_case
        self.expected_regex = expected_regex
        self.exception = None

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, tb):
        if exc_type is None:
            name = getattr(self.expected, '__name__', str(self.expected))
            raise self.test_case.failureException(f"{name} not raised")
        if not issubclass(exc_type, self.expected):
            return False
        self.exception = exc_value
        if self.expected_regex is not None:
            import re
            if not re.search(self.expected_regex, str(exc_value)):
                raise self.test_case.failureException(
                    f'"{self.expected_regex}" does not match "{exc_value}"')
        return True


class TestCase:
    failureException = AssertionError
    longMessage = True
    maxDiff = 80 * 8

    def __init__(self, methodName='runTest'):
        self._testMethodName = methodName

    def setUp(self):
        pass

    def tearDown(self):
        pass

    @classmethod
    def setUpClass(cls):
        pass

    @classmethod
    def tearDownClass(cls):
        pass

    def id(self):
        return f"{type(self).__module__}.{type(self).__qualname__}.{self._testMethodName}"

    def __str__(self):
        return f"{self._testMethodName} ({type(self).__module__}.{type(self).__qualname__}.{self._testMethodName})"

    def __repr__(self):
        return f"<{type(self).__module__}.{type(self).__qualname__} testMethod={self._testMethodName}>"

    def shortDescription(self):
        doc = getattr(self, self._testMethodName).__doc__
        return doc.strip().split('\n')[0].strip() if doc else None

    def skipTest(self, reason):
        raise SkipTest(reason)

    def fail(self, msg=None):
        raise self.failureException(msg)

    def _msg(self, msg, standard):
        if msg is None:
            return standard
        if not self.longMessage:
            return msg
        return f'{standard} : {msg}'

    def assertEqual(self, first, second, msg=None):
        if not first == second:
            standard = f'{first!r} != {second!r}'
            if isinstance(first, str) and isinstance(second, str) and ('\n' in first or '\n' in second):
                standard = f"{first!r} != {second!r}"
            raise self.failureException(self._msg(msg, standard))

    assertEquals = assertEqual

    def assertNotEqual(self, first, second, msg=None):
        if not first != second:
            raise self.failureException(self._msg(msg, f'{first!r} == {second!r}'))

    def assertTrue(self, expr, msg=None):
        if not expr:
            raise self.failureException(self._msg(msg, f'{expr!r} is not true'))

    def assertFalse(self, expr, msg=None):
        if expr:
            raise self.failureException(self._msg(msg, f'{expr!r} is not false'))

    def assertIs(self, a, b, msg=None):
        if a is not b:
            raise self.failureException(self._msg(msg, f'{a!r} is not {b!r}'))

    def assertIsNot(self, a, b, msg=None):
        if a is b:
            raise self.failureException(self._msg(msg, f'unexpectedly identical: {a!r}'))

    def assertIsNone(self, obj, msg=None):
        if obj is not None:
            raise self.failureException(self._msg(msg, f'{obj!r} is not None'))

    def assertIsNotNone(self, obj, msg=None):
        if obj is None:
            raise self.failureException(self._msg(msg, 'unexpectedly None'))

    def assertIn(self, member, container, msg=None):
        if member not in container:
            raise self.failureException(self._msg(msg, f'{member!r} not found in {container!r}'))

    def assertNotIn(self, member, container, msg=None):
        if member in container:
            raise self.failureException(self._msg(msg, f'{member!r} unexpectedly found in {container!r}'))

    def assertIsInstance(self, obj, cls, msg=None):
        if not isinstance(obj, cls):
            raise self.failureException(self._msg(msg, f'{obj!r} is not an instance of {cls!r}'))

    def assertNotIsInstance(self, obj, cls, msg=None):
        if isinstance(obj, cls):
            raise self.failureException(self._msg(msg, f'{obj!r} is an instance of {cls!r}'))

    def assertGreater(self, a, b, msg=None):
        if not a > b:
            raise self.failureException(self._msg(msg, f'{a!r} not greater than {b!r}'))

    def assertGreaterEqual(self, a, b, msg=None):
        if not a >= b:
            raise self.failureException(self._msg(msg, f'{a!r} not greater than or equal to {b!r}'))

    def assertLess(self, a, b, msg=None):
        if not a < b:
            raise self.failureException(self._msg(msg, f'{a!r} not less than {b!r}'))

    def assertLessEqual(self, a, b, msg=None):
        if not a <= b:
            raise self.failureException(self._msg(msg, f'{a!r} not less than or equal to {b!r}'))

    def assertAlmostEqual(self, first, second, places=None, msg=None, delta=None):
        if first == second:
            return
        diff = abs(first - second)
        if delta is not None:
            if diff <= delta:
                return
            standard = f'{first!r} != {second!r} within {delta!r} delta ({diff!r} difference)'
        else:
            if places is None:
                places = 7
            if round(diff, places) == 0:
                return
            standard = f'{first!r} != {second!r} within {places!r} places ({diff!r} difference)'
        raise self.failureException(self._msg(msg, standard))

    def assertNotAlmostEqual(self, first, second, places=None, msg=None, delta=None):
        diff = abs(first - second)
        if delta is not None:
            if not first == second and diff > delta:
                return
        else:
            if places is None:
                places = 7
            if not first == second and round(diff, places) != 0:
                return
        raise self.failureException(self._msg(msg, f'{first!r} == {second!r} within {places!r} places'))

    def assertCountEqual(self, first, second, msg=None):
        from collections import Counter
        if Counter(first) != Counter(second):
            raise self.failureException(self._msg(msg, 'Element counts were not equal'))

    def assertListEqual(self, a, b, msg=None):
        self.assertEqual(a, b, msg)

    assertTupleEqual = assertListEqual
    assertDictEqual = assertListEqual
    assertSetEqual = assertListEqual
    assertSequenceEqual = assertListEqual
    assertMultiLineEqual = assertListEqual

    def assertRegex(self, text, expected_regex, msg=None):
        import re
        if not re.search(expected_regex, text):
            raise self.failureException(self._msg(msg, f"Regex didn't match: {expected_regex!r} not found in {text!r}"))

    def assertNotRegex(self, text, unexpected_regex, msg=None):
        import re
        m = re.search(unexpected_regex, text)
        if m:
            raise self.failureException(self._msg(msg, f"Regex matched: {m.group()!r} matches {unexpected_regex!r} in {text!r}"))

    def assertRaises(self, expected_exception, *args, **kwargs):
        context = _AssertRaisesContext(expected_exception, self)
        if not args:
            return context
        callable_obj, *args = args
        with context:
            callable_obj(*args, **kwargs)
        return context

    def assertRaisesRegex(self, expected_exception, expected_regex, *args, **kwargs):
        context = _AssertRaisesContext(expected_exception, self, expected_regex)
        if not args:
            return context
        callable_obj, *args = args
        with context:
            callable_obj(*args, **kwargs)
        return context

    def assertWarns(self, expected_warning, *args, **kwargs):
        class _Ctx:
            def __enter__(s):
                return s

            def __exit__(s, *a):
                return False
        return _Ctx()

    def subTest(self, msg=None, **params):
        class _Sub:
            def __enter__(s):
                return s

            def __exit__(s, *a):
                return False
        return _Sub()

    def run(self, result=None):
        result = result if result is not None else TestResult()
        method = getattr(self, self._testMethodName)
        result.testsRun += 1
        try:
            self.setUp()
        except SkipTest as e:
            result.skipped.append((self, str(e)))
            return result
        except BaseException as e:
            result.errors.append((self, _format(e)))
            result._mark('E')
            return result
        try:
            method()
        except SkipTest as e:
            result.skipped.append((self, str(e)))
            result._mark('s')
        except self.failureException as e:
            if getattr(method, '__unittest_expecting_failure__', False):
                result.expectedFailures.append((self, _format(e)))
                result._mark('x')
            else:
                result.failures.append((self, _format(e)))
                result._mark('F')
        except BaseException as e:
            if isinstance(e, (KeyboardInterrupt, SystemExit)):
                raise
            result.errors.append((self, _format(e)))
            result._mark('E')
        else:
            if getattr(method, '__unittest_expecting_failure__', False):
                result.unexpectedSuccesses.append(self)
                result._mark('u')
            else:
                result._mark('.')
        finally:
            try:
                self.tearDown()
            except BaseException as e:
                result.errors.append((self, _format(e)))
        return result

    def __call__(self, result=None):
        return self.run(result)


def _format(e):
    lines = traceback.format_exception(e)
    kept = [l for l in lines if '<frozen unittest>' not in l]
    return ''.join(kept)


class TestResult:
    def __init__(self, stream=None, descriptions=None, verbosity=1):
        self.failures = []
        self.errors = []
        self.skipped = []
        self.expectedFailures = []
        self.unexpectedSuccesses = []
        self.testsRun = 0
        self._stream = stream
        self._verbosity = verbosity

    def _mark(self, ch):
        if self._stream is not None and self._verbosity == 1:
            self._stream.write(ch)

    def wasSuccessful(self):
        return not self.failures and not self.errors and not self.unexpectedSuccesses


class TestSuite:
    def __init__(self, tests=()):
        self._tests = list(tests)

    def addTest(self, test):
        self._tests.append(test)

    def addTests(self, tests):
        for t in tests:
            self.addTest(t)

    def __iter__(self):
        return iter(self._tests)

    def countTestCases(self):
        return sum(t.countTestCases() if isinstance(t, TestSuite) else 1 for t in self._tests)

    def run(self, result):
        for t in self._tests:
            t.run(result)
        return result


class TestLoader:
    testMethodPrefix = 'test'

    def getTestCaseNames(self, testCaseClass):
        names = [n for n in dir(testCaseClass) if n.startswith(self.testMethodPrefix)
                 and callable(getattr(testCaseClass, n))]
        return sorted(names)

    def loadTestsFromTestCase(self, testCaseClass):
        return TestSuite([testCaseClass(n) for n in self.getTestCaseNames(testCaseClass)])

    def loadTestsFromModule(self, module):
        tests = []
        for name in dir(module):
            obj = getattr(module, name)
            if isinstance(obj, type) and issubclass(obj, TestCase) and obj is not TestCase:
                tests.append(self.loadTestsFromTestCase(obj))
        return TestSuite(tests)


defaultTestLoader = TestLoader()


class TextTestRunner:
    def __init__(self, stream=None, descriptions=True, verbosity=1, failfast=False, buffer=False, resultclass=None, warnings=None, *, tb_locals=False):
        self.stream = stream or sys.stderr
        self.verbosity = verbosity

    def run(self, test):
        result = TestResult(self.stream, verbosity=self.verbosity)
        classes = []
        for t in _flatten(test):
            cls = type(t)
            if cls not in classes:
                classes.append(cls)
                cls.setUpClass()
            if self.verbosity > 1:
                self.stream.write(f"{t._testMethodName} ({cls.__module__}.{cls.__qualname__}.{t._testMethodName}) ... ")
                before = (len(result.failures), len(result.errors), len(result.skipped))
                t.run(result)
                after = (len(result.failures), len(result.errors), len(result.skipped))
                if after[0] > before[0]:
                    self.stream.write("FAIL\n")
                elif after[1] > before[1]:
                    self.stream.write("ERROR\n")
                elif after[2] > before[2]:
                    self.stream.write(f"skipped {result.skipped[-1][1]!r}\n")
                else:
                    self.stream.write("ok\n")
            else:
                t.run(result)
        for cls in classes:
            cls.tearDownClass()
        if self.verbosity == 1:
            self.stream.write("\n")
        for kind, items in (("ERROR", result.errors), ("FAIL", result.failures)):
            for test, text in items:
                self.stream.write("=" * 70 + "\n")
                self.stream.write(f"{kind}: {test}\n")
                self.stream.write("-" * 70 + "\n")
                self.stream.write(text + "\n")
        self.stream.write("-" * 70 + "\n")
        run = result.testsRun
        self.stream.write(f"Ran {run} test{'s' if run != 1 else ''} in 0.000s\n\n")
        infos = []
        if result.failures:
            infos.append(f"failures={len(result.failures)}")
        if result.errors:
            infos.append(f"errors={len(result.errors)}")
        if result.skipped:
            infos.append(f"skipped={len(result.skipped)}")
        if result.expectedFailures:
            infos.append(f"expected failures={len(result.expectedFailures)}")
        if result.unexpectedSuccesses:
            infos.append(f"unexpected successes={len(result.unexpectedSuccesses)}")
        if not result.wasSuccessful():
            self.stream.write("FAILED" + (f" ({', '.join(infos)})" if infos else "") + "\n")
        elif run == 0:
            self.stream.write("NO TESTS RAN\n")
        else:
            self.stream.write("OK" + (f" ({', '.join(infos)})" if infos else "") + "\n")
        return result


def _flatten(test):
    if isinstance(test, TestSuite):
        for t in test:
            yield from _flatten(t)
    else:
        yield test


class main:
    def __init__(self, module='__main__', defaultTest=None, argv=None, testRunner=None,
                 testLoader=defaultTestLoader, exit=True, verbosity=1, failfast=None,
                 catchbreak=None, buffer=None, warnings=None):
        if isinstance(module, str):
            module = sys.modules[module]
        argv = sys.argv if argv is None else argv
        if '-v' in argv[1:] or '--verbose' in argv[1:]:
            verbosity = 2
        suite = testLoader.loadTestsFromModule(module)
        runner = testRunner or TextTestRunner(verbosity=verbosity)
        if isinstance(runner, type):
            runner = runner(verbosity=verbosity)
        self.result = runner.run(suite)
        if exit:
            sys.exit(not self.result.wasSuccessful())


def skip(reason):
    def decorator(test_item):
        def skip_wrapper(*args, **kwargs):
            raise SkipTest(reason)
        skip_wrapper.__name__ = getattr(test_item, '__name__', 'skip_wrapper')
        return skip_wrapper
    return decorator


def skipIf(condition, reason):
    if condition:
        return skip(reason)
    return lambda f: f


def skipUnless(condition, reason):
    if not condition:
        return skip(reason)
    return lambda f: f


def expectedFailure(test_item):
    test_item.__unittest_expecting_failure__ = True
    return test_item
