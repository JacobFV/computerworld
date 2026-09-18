# Standard library tour: collections, itertools, functools, dataclasses, enum,
# datetime, math, statistics, fractions, textwrap, string, copy, heapq, bisect.
import bisect
import copy
import heapq
import itertools
import math
import operator
import statistics
import string
import textwrap
from collections import ChainMap, Counter, OrderedDict, defaultdict, deque, namedtuple
from dataclasses import asdict, astuple, dataclass, field, replace
from datetime import date, datetime, time, timedelta, timezone
from enum import Enum, IntEnum, Flag, auto
from fractions import Fraction
from functools import cache, cmp_to_key, partial, reduce, total_ordering, wraps

words = "the quick brown fox jumps over the lazy dog the end".split()
counts = Counter(words)
print(counts.most_common(3), counts["the"], counts["missing"], sum(counts.values()))
print(Counter("aab") + Counter("abc"), Counter("aaab") - Counter("ab"), Counter(a=2, b=1) & Counter(a=1, b=3))
groups = defaultdict(list)
for w in words:
    groups[len(w)].append(w)
print(sorted(groups.items()))
od = OrderedDict([("b", 2), ("a", 1)])
od.move_to_end("b")
print(od, list(od.keys()), od.popitem(last=False))
dq = deque(range(5))
dq.rotate(2)
print(dq, dq[0], dq[-1])
dq.extendleft([9, 8])
print(list(dq), dq.count(1), 3 in dq)
Point = namedtuple("Point", ["x", "y"], defaults=[0])
p = Point(3)
print(p, p._fields, Point._make([1, 2]), p == (3, 0), tuple(p), hash(p) == hash((3, 0)))
cm = ChainMap({"a": 1}, {"a": 2, "b": 3})
print(cm["a"], cm["b"], len(cm), sorted(cm))

print(list(itertools.combinations_with_replacement("ab", 2)), list(itertools.zip_longest("ab", "xyz", fillvalue="-")))
print(list(itertools.starmap(pow, [(2, 3), (3, 2)])), list(itertools.takewhile(lambda x: x < 3, [1, 2, 3, 1])))
print(list(itertools.dropwhile(lambda x: x < 3, [1, 2, 3, 1])), list(itertools.compress("abcd", [1, 0, 1, 0])))
print(list(itertools.pairwise([1, 2, 3, 4])), list(itertools.batched("abcdefg", 3)), list(itertools.repeat("x", 3)))
a, b = itertools.tee([1, 2, 3])
print(list(a), list(b), list(itertools.filterfalse(lambda x: x % 2, range(6))))
print(list(itertools.accumulate([1, 2, 3, 4], operator.mul)), list(itertools.islice("abcdefg", 1, 6, 2)))
cyc = itertools.cycle("ab")
print([next(cyc) for _ in range(5)])

print(reduce(operator.add, range(10)), reduce(lambda acc, x: acc + [x * 2], [1, 2], []))
double = partial(operator.mul, 2)
print(double(21), sorted(["b", "A", "c"], key=str.lower), sorted([3, 1, 2], key=cmp_to_key(lambda x, y: y - x)))


@cache
def ways(n):
    return 1 if n <= 1 else ways(n - 1) + ways(n - 2)


print(ways(60))


def logged(fn):
    @wraps(fn)
    def wrapper(*args, **kwargs):
        return fn(*args, **kwargs)
    return wrapper


@logged
def greet(name):
    """Say hello."""
    return f"hi {name}"


print(greet("x"), greet.__name__, greet.__doc__)


@dataclass
class Item:
    name: str
    price: float
    tags: list = field(default_factory=list)
    qty: int = 1

    @property
    def total(self):
        return self.price * self.qty


@dataclass(frozen=True, order=True)
class Coord:
    lat: float
    lon: float


it = Item("pen", 1.5, qty=4)
print(it, it.total, asdict(it), astuple(it), replace(it, qty=1))
c = Coord(1.0, 2.0)
print(c, c < Coord(1.0, 3.0), {c: "here"}[Coord(1.0, 2.0)])
try:
    c.lat = 5
except Exception as e:
    print(type(e).__name__, e)


class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3


class Level(IntEnum):
    LOW = 1
    HIGH = 2


class Perm(Flag):
    R = auto()
    W = auto()
    X = auto()


print(Color.RED, Color.RED.name, Color.RED.value, Color(3), Color["GREEN"], list(Color)[1], len(Color))
print(Level.HIGH > Level.LOW, Level.HIGH + 1, Level(1), isinstance(Level.LOW, int), Color.RED is Color(1))
rw = Perm.R | Perm.W
print(Perm.R in rw, Perm.X in rw, rw.value, repr(Perm.R))

d = date(2024, 2, 28)
print(d, d + timedelta(days=2), d.weekday(), d.isoformat(), d.strftime("%A %d %B %Y"), d.replace(day=1))
dt = datetime(2024, 3, 15, 14, 30, 5)
print(dt, repr(dt), dt.strftime("%Y-%m-%d %H:%M:%S %j %a %b"), dt.isoformat(), dt.date(), dt.time())
print(dt - datetime(2024, 1, 1), (dt - datetime(2024, 1, 1)).days, timedelta(hours=25, minutes=3), timedelta(seconds=-1))
print(datetime.fromisoformat("2023-06-01T12:00:00"), datetime.strptime("2021-07-04 09:15", "%Y-%m-%d %H:%M"))
print(date(2000, 1, 1) < date(2000, 1, 2), max(date(2020, 5, 1), date(2019, 1, 1)), time(9, 5), timedelta(days=1, seconds=90).total_seconds())
print(datetime(2024, 1, 1, tzinfo=timezone.utc).isoformat(), date.fromordinal(738000), date(2024, 12, 30).isocalendar())

print(math.comb(10, 3), math.perm(5, 2), math.isqrt(99), math.log(100, 10), math.log2(1024), math.hypot(3, 4))
print(math.ceil(2.1), math.floor(-2.1), math.trunc(-2.9), math.fsum([0.1] * 10), math.prod([1, 2, 3, 4]), math.dist((0, 0), (3, 4)))
print(math.isclose(0.1 + 0.2, 0.3), math.degrees(math.pi), round(math.sin(math.pi / 6), 10), math.copysign(3, -0.0), math.lcm(4, 6))
data = [2, 4, 4, 4, 5, 5, 7, 9]
print(statistics.mean(data), statistics.median(data), statistics.mode(data), statistics.pstdev(data), statistics.variance(data))
print(statistics.median([1, 3, 2, 4]), statistics.stdev([1.5, 2.5, 2.5, 2.75, 3.25, 4.75]), statistics.fmean([1, 2, 3]))
f = Fraction(3, 4) + Fraction(1, 6)
print(f, Fraction(0.5), Fraction("2/6"), f * 2, float(f), Fraction(7, 3).limit_denominator(2), f.numerator, f.denominator)
text = "The quick brown fox jumps over the lazy dog and keeps running far away"
print(textwrap.wrap(text, 20))
print(textwrap.fill(text, 30, initial_indent="> ", subsequent_indent="  "))
print(textwrap.dedent("    a\n      b\n    c"), textwrap.indent("x\ny", "# "), textwrap.shorten(text, 25))
print(string.capwords("hello   world"), string.digits, string.Template("${who} likes $what").safe_substitute(who="Tim"))
orig = {"a": [1, 2], "b": {"c": 3}}
shallow, deep = copy.copy(orig), copy.deepcopy(orig)
orig["a"].append(9)
print(shallow["a"], deep["a"], deep == {"a": [1, 2], "b": {"c": 3}})
h = [5, 1, 8, 3]
heapq.heapify(h)
heapq.heappush(h, 0)
print([heapq.heappop(h) for _ in range(3)], heapq.nlargest(2, [4, 9, 1, 7]), heapq.nsmallest(2, [4, 9, 1, 7]))
xs = [1, 3, 3, 5, 8]
print(bisect.bisect_left(xs, 3), bisect.bisect_right(xs, 3), bisect.bisect(xs, 6))
bisect.insort(xs, 4)
print(xs, operator.itemgetter(1, 0)(["a", "b"]), operator.attrgetter("real")(5), sorted([(1, "b"), (1, "a"), (0, "z")]))


@total_ordering
class Money:
    def __init__(self, cents):
        self.cents = cents

    def __eq__(self, o):
        return self.cents == o.cents

    def __lt__(self, o):
        return self.cents < o.cents


print(Money(5) > Money(3), Money(2) >= Money(2), Money(1) <= Money(0))
