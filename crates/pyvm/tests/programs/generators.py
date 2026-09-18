# Generators, iterators, yield from, send/throw, generator expressions, closures.


def countdown(n):
    while n > 0:
        yield n
        n -= 1
    return "liftoff"


print(list(countdown(5)))
g = countdown(2)
print(next(g), next(g))
try:
    next(g)
except StopIteration as e:
    print("returned:", e.value)


def fib():
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b


def take(n, it):
    out = []
    for x in it:
        if len(out) == n:
            break
        out.append(x)
    return out


print(take(12, fib()))


def chain(*its):
    for it in its:
        result = yield from it
        if result is not None:
            print("  sub returned", result)


print(list(chain([1, 2], countdown(3), "ab")))


def accumulator():
    total = 0
    while True:
        value = yield total
        if value is None:
            break
        total += value
    return total


acc = accumulator()
print(next(acc), acc.send(10), acc.send(5), acc.send(-3))
try:
    acc.send(None)
except StopIteration as e:
    print("final total", e.value)


def resilient():
    while True:
        try:
            x = yield
            print("  got", x)
        except ValueError as e:
            print("  handled", e)


r = resilient()
next(r)
r.send(1)
r.throw(ValueError("bad"))
r.send(2)
r.close()

squares = (x * x for x in range(10))
print(sum(squares), sum(x for x in range(10) if x % 3 == 0))
print(list(zip(range(3), "abc", [True, False, None])))
print(list(enumerate("xyz", start=1)), dict(enumerate("ab")))
print(list(map(lambda a, b: a * b, [1, 2, 3], [4, 5, 6])), list(filter(None, [0, 1, "", "a", None, []])))
print(list(reversed([1, 2, 3])), list(reversed(range(5))), ''.join(reversed("hello")))
it = iter([1, 2, 3])
print(next(it), list(it), next(it, "default"))
print(sorted({"b": 2, "a": 1, "c": 3}.items(), key=lambda kv: -kv[1]))


class Countdown:
    def __init__(self, start):
        self.current = start

    def __iter__(self):
        return self

    def __next__(self):
        if self.current <= 0:
            raise StopIteration
        self.current -= 1
        return self.current + 1


print(list(Countdown(4)), [x for x in Countdown(3)], max(Countdown(5)))


def make_counter():
    count = 0

    def inc(step=1):
        nonlocal count
        count += step
        return count
    return inc


c1, c2 = make_counter(), make_counter()
print(c1(), c1(), c1(10), c2())

adders = [lambda x, i=i: x + i for i in range(3)]
print([f(10) for f in adders])
late = [lambda: i for i in range(3)]
print([f() for f in late])

total = 0


def add_global(n):
    global total
    total += n


for k in range(5):
    add_global(k)
print("total", total)


def outer():
    x = "outer"

    def middle():
        def inner():
            return x
        return inner()
    return middle()


print(outer())


def gen_with_state():
    seen = set()
    for word in "the quick the lazy the end".split():
        if word not in seen:
            seen.add(word)
            yield word


print(list(gen_with_state()))
nested = [[1, 2], [3], [], [4, 5, 6]]
print([x for sub in nested for x in sub], {x % 3 for x in range(10)}, {k: v for k, v in zip("abc", range(3))})
matrix = [[1, 2, 3], [4, 5, 6]]
print([list(row) for row in zip(*matrix)], [[r * c for c in range(3)] for r in range(3)])
print(any(x > 5 for x in range(10)), all(x < 5 for x in range(10)), min((3, "c"), (1, "a")), max("hello"))
gen = (i for i in range(3))
print(list(gen), list(gen))
print(sum(range(101)), sum([0.1] * 10), sum([[1], [2]], []))
