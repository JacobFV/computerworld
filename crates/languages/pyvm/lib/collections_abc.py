"""collections.abc for the simulated interpreter: structural checks via
__subclasshook__, and mixin methods for user subclasses."""
from abc import ABCMeta, abstractmethod


def _check_methods(C, *methods):
    mro = C.__mro__
    for method in methods:
        for B in mro:
            if method in B.__dict__:
                if B.__dict__[method] is None:
                    return NotImplemented
                break
        else:
            return NotImplemented
    return True


class Hashable(metaclass=ABCMeta):
    @classmethod
    def __subclasshook__(cls, C):
        if cls is Hashable:
            return _check_methods(C, "__hash__")
        return NotImplemented


class Iterable(metaclass=ABCMeta):
    @abstractmethod
    def __iter__(self):
        while False:
            yield None

    @classmethod
    def __subclasshook__(cls, C):
        if cls is Iterable:
            return _check_methods(C, "__iter__")
        return NotImplemented


class Iterator(Iterable):
    @abstractmethod
    def __next__(self):
        raise StopIteration

    def __iter__(self):
        return self

    @classmethod
    def __subclasshook__(cls, C):
        if cls is Iterator:
            return _check_methods(C, '__iter__', '__next__')
        return NotImplemented


class Reversible(Iterable):
    @classmethod
    def __subclasshook__(cls, C):
        if cls is Reversible:
            return _check_methods(C, "__reversed__", "__iter__")
        return NotImplemented


class Generator(Iterator):
    @classmethod
    def __subclasshook__(cls, C):
        if cls is Generator:
            return _check_methods(C, '__iter__', '__next__', 'send', 'throw', 'close')
        return NotImplemented


class Sized(metaclass=ABCMeta):
    @abstractmethod
    def __len__(self):
        return 0

    @classmethod
    def __subclasshook__(cls, C):
        if cls is Sized:
            return _check_methods(C, "__len__")
        return NotImplemented


class Container(metaclass=ABCMeta):
    @abstractmethod
    def __contains__(self, x):
        return False

    @classmethod
    def __subclasshook__(cls, C):
        if cls is Container:
            return _check_methods(C, "__contains__")
        return NotImplemented


class Collection(Sized, Iterable, Container):
    @classmethod
    def __subclasshook__(cls, C):
        if cls is Collection:
            return _check_methods(C, "__len__", "__iter__", "__contains__")
        return NotImplemented


class Callable(metaclass=ABCMeta):
    @abstractmethod
    def __call__(self, *args, **kwds):
        return False

    @classmethod
    def __subclasshook__(cls, C):
        if cls is Callable:
            return _check_methods(C, "__call__")
        return NotImplemented


class Sequence(Reversible, Collection):
    @abstractmethod
    def __getitem__(self, index):
        raise IndexError

    def __iter__(self):
        i = 0
        try:
            while True:
                v = self[i]
                yield v
                i += 1
        except IndexError:
            return

    def __contains__(self, value):
        for v in self:
            if v is value or v == value:
                return True
        return False

    def __reversed__(self):
        for i in reversed(range(len(self))):
            yield self[i]

    def index(self, value, start=0, stop=None):
        i = start
        while stop is None or i < stop:
            try:
                v = self[i]
            except IndexError:
                break
            if v is value or v == value:
                return i
            i += 1
        raise ValueError

    def count(self, value):
        return sum(1 for v in self if v is value or v == value)


class MutableSequence(Sequence):
    def append(self, value):
        self.insert(len(self), value)

    def extend(self, values):
        for v in values:
            self.append(v)

    def pop(self, index=-1):
        v = self[index]
        del self[index]
        return v

    def remove(self, value):
        del self[self.index(value)]

    def reverse(self):
        n = len(self)
        for i in range(n // 2):
            self[i], self[n - i - 1] = self[n - i - 1], self[i]

    def clear(self):
        try:
            while True:
                self.pop()
        except IndexError:
            pass

    def __iadd__(self, values):
        self.extend(values)
        return self


class Set(Collection):
    def __le__(self, other):
        if len(self) > len(other):
            return False
        for elem in self:
            if elem not in other:
                return False
        return True

    def __ge__(self, other):
        if len(self) < len(other):
            return False
        for elem in other:
            if elem not in self:
                return False
        return True

    def __eq__(self, other):
        return len(self) == len(other) and self.__le__(other)

    def isdisjoint(self, other):
        for value in other:
            if value in self:
                return False
        return True


class MutableSet(Set):
    def remove(self, value):
        if value not in self:
            raise KeyError(value)
        self.discard(value)


class Mapping(Collection):
    @abstractmethod
    def __getitem__(self, key):
        raise KeyError

    def get(self, key, default=None):
        try:
            return self[key]
        except KeyError:
            return default

    def __contains__(self, key):
        try:
            self[key]
        except KeyError:
            return False
        else:
            return True

    def keys(self):
        return list(self)

    def items(self):
        return [(key, self[key]) for key in self]

    def values(self):
        return [self[key] for key in self]

    def __eq__(self, other):
        if not isinstance(other, Mapping):
            return NotImplemented
        return dict(self.items()) == dict(other.items())


class MutableMapping(Mapping):
    def pop(self, key, *default):
        try:
            value = self[key]
        except KeyError:
            if default:
                return default[0]
            raise
        else:
            del self[key]
            return value

    def popitem(self):
        try:
            key = next(iter(self))
        except StopIteration:
            raise KeyError from None
        value = self[key]
        del self[key]
        return key, value

    def clear(self):
        try:
            while True:
                self.popitem()
        except KeyError:
            pass

    def update(self, other=(), /, **kwds):
        if isinstance(other, Mapping) or hasattr(other, 'keys'):
            for key in other.keys():
                self[key] = other[key]
        else:
            for key, value in other:
                self[key] = value
        for key, value in kwds.items():
            self[key] = value

    def setdefault(self, key, default=None):
        try:
            return self[key]
        except KeyError:
            self[key] = default
        return default


class MappingView(Sized):
    pass


class KeysView(MappingView, Set):
    pass


class ItemsView(MappingView, Set):
    pass


class ValuesView(MappingView, Collection):
    pass


class Awaitable(metaclass=ABCMeta):
    pass


class Coroutine(Awaitable):
    pass


class AsyncIterable(metaclass=ABCMeta):
    pass


class AsyncIterator(AsyncIterable):
    pass


class AsyncGenerator(AsyncIterator):
    pass


class ByteString(Sequence):
    pass


for _t in (list, tuple, str, range, bytes):
    Sequence.register(_t)
MutableSequence.register(list)
for _t in (set, frozenset):
    Set.register(_t)
MutableSet.register(set)
Mapping.register(dict)
MutableMapping.register(dict)
for _t in (list, tuple, str, dict, set, frozenset, range, bytes):
    Iterable.register(_t)
    Collection.register(_t)
    Sized.register(_t)
    Container.register(_t)
del _t
