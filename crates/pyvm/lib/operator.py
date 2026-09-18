"""operator for the simulated interpreter."""


def lt(a, b): return a < b
def le(a, b): return a <= b
def eq(a, b): return a == b
def ne(a, b): return a != b
def ge(a, b): return a >= b
def gt(a, b): return a > b
def not_(a): return not a
def truth(a): return True if a else False
def is_(a, b): return a is b
def is_not(a, b): return a is not b
import builtins as _builtins
def abs(a): return _builtins.abs(a)
def add(a, b): return a + b
def and_(a, b): return a & b
def floordiv(a, b): return a // b
def index(a): return a.__index__()
def inv(a): return ~a
invert = inv
def lshift(a, b): return a << b
def mod(a, b): return a % b
def mul(a, b): return a * b
def matmul(a, b): return a @ b
def neg(a): return -a
def or_(a, b): return a | b
def pos(a): return +a
def pow(a, b): return a ** b
def rshift(a, b): return a >> b
def sub(a, b): return a - b
def truediv(a, b): return a / b
def xor(a, b): return a ^ b
def concat(a, b): return a + b
def contains(a, b): return b in a
def countOf(a, b): return sum(1 for x in a if x is b or x == b)
def delitem(a, b): del a[b]
def getitem(a, b): return a[b]
def indexOf(a, b):
    for i, j in enumerate(a):
        if j is b or j == b:
            return i
    raise ValueError('sequence.index(x): x not in sequence')
def setitem(a, b, c): a[b] = c
def length_hint(obj, default=0):
    try:
        return len(obj)
    except TypeError:
        return default
def iadd(a, b):
    a += b
    return a
def isub(a, b):
    a -= b
    return a
def imul(a, b):
    a *= b
    return a


class attrgetter:
    def __init__(self, attr, *attrs):
        if not attrs:
            names = attr.split('.')

            def func(obj):
                for name in names:
                    obj = getattr(obj, name)
                return obj
            self._call = func
        else:
            getters = tuple(map(attrgetter, (attr,) + attrs))

            def func(obj):
                return tuple(getter(obj) for getter in getters)
            self._call = func
        self._attrs = (attr,) + attrs

    def __call__(self, obj):
        return self._call(obj)

    def __repr__(self):
        return 'operator.attrgetter(%s)' % ', '.join(map(repr, self._attrs))


class itemgetter:
    def __init__(self, item, *items):
        if not items:
            self._items = (item,)

            def func(obj):
                return obj[item]
            self._call = func
        else:
            self._items = items = (item,) + items

            def func(obj):
                return tuple(obj[i] for i in items)
            self._call = func

    def __call__(self, obj):
        return self._call(obj)

    def __repr__(self):
        return 'operator.itemgetter(%s)' % ', '.join(map(repr, self._items))


class methodcaller:
    def __init__(self, name, /, *args, **kwargs):
        self._name = name
        self._args = args
        self._kwargs = kwargs

    def __call__(self, obj):
        return getattr(obj, self._name)(*self._args, **self._kwargs)


__lt__ = lt
__le__ = le
__eq__ = eq
__ne__ = ne
__ge__ = ge
__gt__ = gt
__add__ = add
__sub__ = sub
__mul__ = mul
__getitem__ = getitem
