# Exceptions: hierarchy, handlers, finally, chaining, custom exceptions, messages.


class AppError(Exception):
    def __init__(self, message, code=1):
        super().__init__(message)
        self.code = code


class NotFound(AppError):
    pass


def lookup(d, key):
    try:
        return d[key]
    except KeyError:
        raise NotFound(f"missing {key}", code=404) from None


try:
    lookup({}, "x")
except AppError as e:
    print(type(e).__name__, e, e.code, e.args, e.__cause__, e.__suppress_context__)


def risky(n):
    try:
        if n == 0:
            raise ValueError("zero")
        elif n == 1:
            return 1 / 0
        elif n == 2:
            return [][1]
        elif n == 3:
            return {}["k"]
        elif n == 4:
            return int("abc")
        elif n == 5:
            return undefined_name
        elif n == 6:
            return "a" + 1
        elif n == 7:
            return None.attr
        return "ok"
    except (ValueError, ZeroDivisionError) as e:
        return f"caught {type(e).__name__}: {e}"
    except LookupError as e:
        return f"lookup {type(e).__name__}: {e}"
    except Exception as e:
        return f"other {type(e).__name__}: {e}"
    finally:
        print(f"  finally for {n}")


for i in range(9):
    print(i, risky(i))


def finally_order():
    log = []
    try:
        try:
            log.append("try")
            raise RuntimeError("inner")
        except RuntimeError:
            log.append("except")
            raise
        finally:
            log.append("finally")
    except RuntimeError as e:
        log.append(f"outer {e}")
    else:
        log.append("else")
    return log


print(finally_order())


def try_else():
    try:
        x = 1
    except Exception:
        return "except"
    else:
        return f"else {x}"
    finally:
        print("  cleanup")


print(try_else())


def return_in_finally():
    try:
        return "try"
    finally:
        print("  finally runs before return")


print(return_in_finally())


def loop_with_finally():
    out = []
    for i in range(5):
        try:
            if i == 1:
                continue
            if i == 3:
                break
            out.append(i)
        finally:
            out.append(f"f{i}")
    return out


print(loop_with_finally())

try:
    try:
        raise ValueError("first")
    except ValueError as e:
        raise TypeError("second") from e
except TypeError as e:
    print(repr(e), repr(e.__cause__), e.__context__ is e.__cause__)

try:
    try:
        1 / 0
    except ZeroDivisionError:
        raise KeyError("during")
except KeyError as e:
    print(repr(e), type(e.__context__).__name__, e.__cause__)

try:
    raise ExceptionGroup if False else OSError(2, "No such file or directory", "x.txt")
except OSError as e:
    print(e, e.errno, e.strerror, e.filename)

try:
    open("/definitely/missing.txt")
except FileNotFoundError as e:
    print(type(e).__name__, e)

try:
    assert 1 + 1 == 3, "math is broken"
except AssertionError as e:
    print("assert:", e)

try:
    [1, 2, 3].index(9)
except ValueError as e:
    print(e)

try:
    raise StopIteration(5)
except StopIteration as e:
    print("stop value", e.value)


class Resource:
    def __init__(self, name):
        self.name = name

    def __enter__(self):
        print(f"  enter {self.name}")
        return self

    def __exit__(self, et, ev, tb):
        print(f"  exit {self.name} {et.__name__ if et else None}")
        return et is KeyError


with Resource("a") as r, Resource("b"):
    print("  body", r.name)
with Resource("c"):
    raise KeyError("suppressed")
print("after suppressed")
try:
    with Resource("d"):
        raise ValueError("propagates")
except ValueError as e:
    print("got", e)

errors = []
for exc in (IndexError, KeyError, ZeroDivisionError, TypeError):
    try:
        raise exc("msg")
    except (IndexError, KeyError) as e:
        errors.append(("lookup", type(e).__name__, str(e)))
    except ArithmeticError as e:
        errors.append(("arith", type(e).__name__))
    except Exception as e:
        errors.append(("other", type(e).__name__))
print(errors)
print(issubclass(FileNotFoundError, OSError), issubclass(KeyError, LookupError), issubclass(bool, int))
e = ValueError("a", "b")
print(e.args, str(e), repr(e), repr(ValueError()), str(KeyError("k")))


def gen_cleanup():
    try:
        yield 1
        yield 2
    finally:
        print("  generator finalized")


g = gen_cleanup()
print(next(g))
g.close()
print("closed")
