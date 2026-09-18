# Classes: inheritance, super(), dunder methods, properties, class/static methods.
from functools import total_ordering


class Shape:
    count = 0

    def __init__(self, name):
        self.name = name
        Shape.count += 1

    def area(self):
        raise NotImplementedError("subclass must implement area")

    def describe(self):
        return f"{self.name} with area {self.area():.2f}"

    def __repr__(self):
        return f"{type(self).__name__}({self.name!r})"


class Rect(Shape):
    def __init__(self, w, h):
        super().__init__("rect")
        self.w, self.h = w, h

    def area(self):
        return self.w * self.h


class Square(Rect):
    def __init__(self, side):
        super().__init__(side, side)
        self.name = "square"

    @property
    def side(self):
        return self.w

    @side.setter
    def side(self, value):
        if value <= 0:
            raise ValueError("side must be positive")
        self.w = self.h = value


class Circle(Shape):
    PI = 3.14159

    def __init__(self, r):
        super().__init__("circle")
        self.r = r

    def area(self):
        return self.PI * self.r ** 2

    @classmethod
    def unit(cls):
        return cls(1)

    @staticmethod
    def circumference(r):
        return 2 * Circle.PI * r


shapes = [Rect(3, 4), Square(5), Circle(2), Circle.unit()]
for s in shapes:
    print(s.describe(), repr(s))
print("count:", Shape.count, "mro:", [c.__name__ for c in Square.__mro__])
sq = shapes[1]
sq.side = 7
print("side:", sq.side, sq.area())
try:
    sq.side = -1
except ValueError as e:
    print("error:", e)
try:
    Shape("x").area()
except NotImplementedError as e:
    print("not implemented:", e)
print(isinstance(sq, Rect), isinstance(sq, Circle), issubclass(Square, Shape))
print(Circle.circumference(1))


@total_ordering
class Version:
    def __init__(self, s):
        self.parts = tuple(int(p) for p in s.split("."))

    def __eq__(self, other):
        return self.parts == other.parts

    def __lt__(self, other):
        return self.parts < other.parts

    def __hash__(self):
        return hash(self.parts)

    def __str__(self):
        return ".".join(map(str, self.parts))


versions = [Version(v) for v in ["1.10.0", "1.2.3", "1.2.10", "0.9"]]
print("sorted:", [str(v) for v in sorted(versions)])
print("max:", max(versions), Version("1.0") >= Version("0.9"), Version("2.0") <= Version("1.0"))
print("set:", len({Version("1.0"), Version("1.0"), Version("1.1")}))


class Vector:
    def __init__(self, *coords):
        self.coords = coords

    def __add__(self, other):
        return Vector(*(a + b for a, b in zip(self.coords, other.coords)))

    def __mul__(self, k):
        return Vector(*(a * k for a in self.coords))

    __rmul__ = __mul__

    def __neg__(self):
        return self * -1

    def __abs__(self):
        return sum(c * c for c in self.coords) ** 0.5

    def __len__(self):
        return len(self.coords)

    def __getitem__(self, i):
        return self.coords[i]

    def __iter__(self):
        return iter(self.coords)

    def __eq__(self, other):
        return isinstance(other, Vector) and self.coords == other.coords

    def __bool__(self):
        return any(self.coords)

    def __repr__(self):
        return f"Vector{self.coords}"

    def __contains__(self, x):
        return x in self.coords

    def __call__(self, scale):
        return self * scale


v = Vector(1, 2, 3)
w = Vector(4, 5, 6)
print(v + w, v * 2, 3 * v, -v, abs(Vector(3, 4)), len(v), v[1], list(v))
print(v == Vector(1, 2, 3), v != w, bool(Vector(0, 0)), 2 in v, v(10))


class Stack:
    def __init__(self):
        self._items = []

    def push(self, x):
        self._items.append(x)
        return self

    def pop(self):
        if not self._items:
            raise IndexError("pop from empty stack")
        return self._items.pop()

    def __len__(self):
        return len(self._items)


st = Stack().push(1).push(2).push(3)
print(len(st), st.pop(), st.pop(), len(st))


class Animal:
    def speak(self):
        return "..."

    def intro(self):
        return f"I say {self.speak()}"


class Dog(Animal):
    def speak(self):
        return "woof"


class Puppy(Dog):
    def speak(self):
        return super().speak() + "!"


class A:
    def who(self):
        return ["A"]


class B(A):
    def who(self):
        return ["B"] + super().who()


class C(A):
    def who(self):
        return ["C"] + super().who()


class D(B, C):
    def who(self):
        return ["D"] + super().who()


print(Puppy().intro(), D().who(), [k.__name__ for k in D.mro()])


class Counter:
    instances = 0

    def __new__(cls, *args):
        cls.instances += 1
        return super().__new__(cls)

    def __init__(self, start=0):
        self.value = start

    def __iadd__(self, n):
        self.value += n
        return self

    def __str__(self):
        return f"Counter({self.value})"


c = Counter(5)
c += 3
print(c, Counter.instances, getattr(c, "value"), hasattr(c, "nope"), vars(c))
setattr(c, "extra", 1)
del c.extra
print(sorted(vars(c)))


class Meta(type):
    def __new__(mcs, name, bases, ns):
        ns["registered"] = True
        return super().__new__(mcs, name, bases, ns)


class WithMeta(metaclass=Meta):
    pass


print(WithMeta.registered, type(WithMeta).__name__)


class Temperature:
    def __init__(self):
        self._c = 0

    def get_c(self):
        return self._c

    def set_c(self, v):
        self._c = v

    celsius = property(get_c, set_c)

    @property
    def fahrenheit(self):
        return self._c * 9 / 5 + 32


t = Temperature()
t.celsius = 100
print(t.celsius, t.fahrenheit)
try:
    t.fahrenheit = 5
except AttributeError as e:
    print("AttributeError:", e)
print(type(t).__name__, type(t).__qualname__, Temperature.__name__)
