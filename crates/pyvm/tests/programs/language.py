# Language features: unpacking, match, walrus, argument kinds, decorators,
# loops with else, slicing, numbers, sets and dicts.


def f(a, b=2, *args, c, d=4, **kw):
    return (a, b, args, c, d, sorted(kw.items()))


print(f(1, c=3), f(1, 5, 6, 7, c=8, z=9, y=0))


def pos_only(x, y, /, z=0, *, w=1):
    return x + y + z + w


print(pos_only(1, 2), pos_only(1, 2, z=3, w=4))
args, kwargs = (1, 2), {"z": 10}
print(pos_only(*args, **kwargs), max(*[3, 9, 2]), print(*"abc", sep="-"))

a, *middle, z = range(6)
print(a, middle, z)
(x, y), [p, q] = (1, 2), [3, 4]
print(x, y, p, q)
first, *_ = "hello"
print(first, [*range(3), *"ab"], {**{"a": 1}, "b": 2}, {*[1, 2], 3})
x, y = y, x
print(x, y)
nums = list(range(10))
nums[2:5] = ["a", "b"]
print(nums)
nums[::3] = [0, 0, 0]
print(nums, nums[-3:], nums[:-7], nums[::-2])
del nums[1::2]
print(nums)
s = "abcdefgh"
print(s[1:6:2], s[::-1][:3], s[-100:100], s[5:1:-1])


def describe(obj):
    match obj:
        case 0:
            return "zero"
        case int(n) if n < 0:
            return f"negative {n}"
        case int() | float():
            return f"number {obj}"
        case [x, y]:
            return f"pair {x},{y}"
        case [first, *rest]:
            return f"list starting {first} (+{len(rest)})"
        case {"type": "point", "x": px, **others}:
            return f"point x={px} others={others}"
        case str() as text if len(text) > 3:
            return f"long string {text!r}"
        case str():
            return "short string"
        case None:
            return "none"
        case _:
            return f"other {type(obj).__name__}"


for item in [0, -5, 3.5, [1, 2], [1, 2, 3], {"type": "point", "x": 1, "y": 2}, "hello", "hi", None, {1}]:
    print(describe(item))


class Point:
    __match_args__ = ("x", "y")

    def __init__(self, x, y):
        self.x, self.y = x, y


def where(pt):
    match pt:
        case Point(0, 0):
            return "origin"
        case Point(0, y):
            return f"on y axis at {y}"
        case Point(x=x, y=0):
            return f"on x axis at {x}"
        case Point():
            return "elsewhere"


print([where(Point(*c)) for c in [(0, 0), (0, 5), (3, 0), (1, 1)]])

data = [5, 3, 8, 1]
if (n := len(data)) > 3:
    print(f"list of {n} items")
while (item := data.pop()) != 3:
    print("popped", item)
print([y for x in range(6) if (y := x * x) % 2 == 0])

for i in range(3):
    if i == 5:
        break
else:
    print("for-else ran")
k = 0
while k < 3:
    k += 1
else:
    print("while-else ran", k)
for i in range(3):
    if i == 1:
        break
else:
    print("not printed")


def repeat(times):
    def deco(fn):
        def wrapper(*a, **kw):
            return [fn(*a, **kw) for _ in range(times)]
        return wrapper
    return deco


@repeat(3)
def hello(name):
    return f"hi {name}"


print(hello("bob"))


def register(cls):
    cls.registered = True
    return cls


@register
class Plugin:
    pass


print(Plugin.registered)
print(7 // 2, -7 // 2, 7 % 3, -7 % 3, 7 % -3, 2 ** 10, 2 ** -2, 10 / 4, divmod(17, 5), pow(3, 4, 5), pow(3, -1, 7))
print(0.1 + 0.2, 1 / 3, 2 / 3, 1e300 * 10, -1e300 * 10, 3.0 == 3, 0.1 * 3 == 0.3, abs(-7.5), round(0.5), round(1.5))
print(10 ** 30, (10 ** 30) // 7, -(10 ** 30) % 7, 2 ** 64 - 1, int("9" * 25) + 1, 123456789 * 987654321, (2 ** 100) >> 90)
print(0b1010 | 0b0101, 0xF0 & 0x3C, 6 ^ 3, ~5, 1 << 10, -16 >> 2, (1 << 70) | 1, bin(255), 255 .bit_length())
print(1 < 2 < 3, 1 < 3 < 2, 1 == 1.0 == True, "a" < "b" < "c", [1, 2] < [1, 3], (1, 2) > (1,), None is None)
print(int(True) + True, True + 0.5, bool(""), bool("0"), bool([]), bool([0]), not 0, 3 if 0 else 4, 0 or "default", 5 and 6)
print({3, 1, 2}, {10, 3, 7, 100, 1}, set("hello") == set("olleh"), {1, 2} | {2, 3}, {1, 2} & {2, 3}, {1, 2} - {2}, {1, 2} ^ {2, 3})
print(sorted({"banana": 3, "apple": 1}), {1: "a", 2: "b"}.get(3, "none"), dict(zip("abc", [1, 2, 3])), dict.fromkeys("ab", 0))
d = {"x": 1}
d.setdefault("y", []).append(5)
d.update(z=3)
print(d, d.pop("x"), d, list(d.items()), "y" in d, len(d))
print({k: v for k, v in sorted(d.items(), reverse=True)}, {x % 3: x for x in range(10)})
print(frozenset([1, 2]) | {3}, {1, 2}.issubset({1, 2, 3}), {1}.isdisjoint({2}), sorted({(1, 2), (0, 5)}))
counter = 0


def bump():
    global counter
    counter += 1
    return counter


bump()
bump()
print("counter", counter)
print(type(1).__name__, type(1.0).__name__, type("").__name__, type([]).__name__, type({}).__name__, type(None).__name__, type(len).__name__)
print(isinstance(True, int), isinstance(1, (str, float)), callable(len), callable(1), id(None) == id(None))
print(list(map(str.upper, ["a", "b"])), [len(w) for w in "the quick fox".split()], "".join(map(str, range(5))))
print(b"abc" + b"def", b"hello"[1:3], b"a" * 3, bytes([72, 105]), bytearray(b"xy") + b"z", b"test".upper(), "é".encode("utf-8"))
print(hash(1) == hash(1.0), hash("a") == hash("a"), len({1, 1.0, True}), sorted([3, 1, 2], reverse=True))
nested = [[1, [2, 3]], [4]]


def flatten(xs):
    for x in xs:
        if isinstance(x, list):
            yield from flatten(x)
        else:
            yield x


print(list(flatten(nested)))
print(sum(1 for _ in range(1000)), all([]), any([]), min([4, 2, 8], key=lambda v: -v), max("abc", "xy", key=len))
print(list(zip(*[(1, "a"), (2, "b")])), sorted(["b10", "a2", "b2"], key=lambda s: (s[0], int(s[1:]))))
assert True, "never"
print(__name__, __doc__, (lambda: 42)(), (lambda *a, **k: (a, k))(1, x=2))
