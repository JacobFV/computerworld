"""collections for the simulated interpreter."""
from _collections import deque
import sys as _sys

__all__ = ['ChainMap', 'Counter', 'OrderedDict', 'UserDict', 'UserList',
           'UserString', 'defaultdict', 'deque', 'namedtuple']


class defaultdict(dict):
    def __init__(self, default_factory=None, *args, **kwargs):
        if default_factory is not None and not callable(default_factory):
            raise TypeError('first argument must be callable or None')
        super().__init__(*args, **kwargs)
        self.default_factory = default_factory

    def __missing__(self, key):
        if self.default_factory is None:
            raise KeyError(key)
        value = self.default_factory()
        self[key] = value
        return value

    def __repr__(self):
        f = self.default_factory
        fr = 'None' if f is None else repr(f)
        return f'{type(self).__name__}({fr}, {dict.__repr__(self)})'

    def copy(self):
        return type(self)(self.default_factory, self)

    __copy__ = copy

    def __reduce__(self):
        return (type(self), (self.default_factory,), None, None, iter(self.items()))


class OrderedDict(dict):
    def move_to_end(self, key, last=True):
        value = dict.pop(self, key)
        if last:
            dict.__setitem__(self, key, value)
        else:
            items = list(dict.items(self))
            dict.clear(self)
            dict.__setitem__(self, key, value)
            for k, v in items:
                dict.__setitem__(self, k, v)

    def popitem(self, last=True):
        if not self:
            raise KeyError('dictionary is empty')
        if last:
            return dict.popitem(self)
        key = next(iter(self))
        value = dict.pop(self, key)
        return key, value

    def __repr__(self):
        if not self:
            return f'{type(self).__name__}()'
        return f'{type(self).__name__}({dict.__repr__(self)})'

    def __eq__(self, other):
        if isinstance(other, OrderedDict):
            return dict.__eq__(self, other) and list(self) == list(other)
        return dict.__eq__(self, other)

    def __ne__(self, other):
        return not self == other

    def __reversed__(self):
        return reversed(list(self))

    def copy(self):
        return type(self)(self)

    @classmethod
    def fromkeys(cls, iterable, value=None):
        self = cls()
        for key in iterable:
            self[key] = value
        return self


class Counter(dict):
    def __init__(self, iterable=None, /, **kwds):
        super().__init__()
        self.update(iterable, **kwds)

    def __missing__(self, key):
        return 0

    def total(self):
        return sum(self.values())

    def most_common(self, n=None):
        items = sorted(self.items(), key=lambda kv: kv[1], reverse=True)
        if n is None:
            return items
        return items[:max(n, 0)]

    def elements(self):
        for elem, count in self.items():
            for _ in range(count):
                yield elem

    @classmethod
    def fromkeys(cls, iterable, v=None):
        raise NotImplementedError(
            'Counter.fromkeys() is undefined.  Use Counter(iterable) instead.')

    def update(self, iterable=None, /, **kwds):
        if iterable is not None:
            if hasattr(iterable, 'keys'):
                if self:
                    for elem, count in iterable.items():
                        self[elem] = count + self.get(elem, 0)
                else:
                    dict.update(self, iterable)
            else:
                for elem in iterable:
                    self[elem] = self.get(elem, 0) + 1
        if kwds:
            self.update(kwds)

    def subtract(self, iterable=None, /, **kwds):
        if iterable is not None:
            if hasattr(iterable, 'keys'):
                for elem, count in iterable.items():
                    self[elem] = self.get(elem, 0) - count
            else:
                for elem in iterable:
                    self[elem] = self.get(elem, 0) - 1
        if kwds:
            self.subtract(kwds)

    def copy(self):
        return self.__class__(self)

    def __reduce__(self):
        return self.__class__, (dict(self),)

    def __delitem__(self, elem):
        if elem in self:
            super().__delitem__(elem)

    def __repr__(self):
        if not self:
            return f'{self.__class__.__name__}()'
        try:
            d = dict(self.most_common())
        except TypeError:
            d = dict(self)
        return f'{self.__class__.__name__}({d!r})'

    def __eq__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        return all(self[e] == other[e] for c in (self, other) for e in c)

    def __ne__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        return not self == other

    def __le__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        return all(self[e] <= other[e] for c in (self, other) for e in c)

    def __lt__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        return self <= other and self != other

    def __ge__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        return all(self[e] >= other[e] for c in (self, other) for e in c)

    def __gt__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        return self >= other and self != other

    def __add__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            newcount = count + other[elem]
            if newcount > 0:
                result[elem] = newcount
        for elem, count in other.items():
            if elem not in self and count > 0:
                result[elem] = count
        return result

    def __sub__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            newcount = count - other[elem]
            if newcount > 0:
                result[elem] = newcount
        for elem, count in other.items():
            if elem not in self and count < 0:
                result[elem] = 0 - count
        return result

    def __or__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            other_count = other[elem]
            newcount = other_count if count < other_count else count
            if newcount > 0:
                result[elem] = newcount
        for elem, count in other.items():
            if elem not in self and count > 0:
                result[elem] = count
        return result

    def __and__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            other_count = other[elem]
            newcount = count if count < other_count else other_count
            if newcount > 0:
                result[elem] = newcount
        return result

    def __pos__(self):
        result = Counter()
        for elem, count in self.items():
            if count > 0:
                result[elem] = count
        return result

    def __neg__(self):
        result = Counter()
        for elem, count in self.items():
            if count < 0:
                result[elem] = 0 - count
        return result

    def _keep_positive(self):
        nonpositive = [elem for elem, count in self.items() if not count > 0]
        for elem in nonpositive:
            del self[elem]
        return self

    def __iadd__(self, other):
        for elem, count in other.items():
            self[elem] += count
        return self._keep_positive()

    def __isub__(self, other):
        for elem, count in other.items():
            self[elem] -= count
        return self._keep_positive()

    def __ior__(self, other):
        for elem, other_count in other.items():
            count = self[elem]
            if other_count > count:
                self[elem] = other_count
        return self._keep_positive()

    def __iand__(self, other):
        for elem, count in self.items():
            other_count = other[elem]
            if other_count < count:
                self[elem] = other_count
        return self._keep_positive()


def namedtuple(typename, field_names, *, rename=False, defaults=None, module=None):
    if isinstance(field_names, str):
        field_names = field_names.replace(',', ' ').split()
    field_names = list(map(str, field_names))
    typename = str(typename)
    if rename:
        seen = set()
        for index, name in enumerate(field_names):
            if (not name.isidentifier() or name.startswith('_') or name in seen):
                field_names[index] = f'_{index}'
            seen.add(name)
    for name in [typename] + field_names:
        if type(name) is not str:
            raise TypeError('Type names and field names must be strings')
        if not name.isidentifier():
            raise ValueError('Type names and field names must be valid '
                             f'identifiers: {name!r}')
    seen = set()
    for name in field_names:
        if name.startswith('_') and not rename:
            raise ValueError('Field names cannot start with an underscore: '
                             f'{name!r}')
        if name in seen:
            raise ValueError(f'Encountered duplicate field name: {name!r}')
        seen.add(name)
    field_defaults = {}
    if defaults is not None:
        defaults = tuple(defaults)
        if len(defaults) > len(field_names):
            raise TypeError('Got more default values than field names')
        field_defaults = dict(reversed(list(zip(reversed(field_names),
                                                reversed(defaults)))))
    field_names = tuple(field_names)
    num_fields = len(field_names)

    def __new__(_cls, *args, **kwargs):
        if len(args) > num_fields:
            raise TypeError(f'{typename}.__new__() takes {num_fields + 1} positional arguments but {len(args) + 1} were given')
        values = list(args)
        for name in field_names[len(args):]:
            if name in kwargs:
                values.append(kwargs.pop(name))
            elif name in field_defaults:
                values.append(field_defaults[name])
            else:
                missing = [n for n in field_names[len(values):] if n not in kwargs and n not in field_defaults]
                if len(missing) == 1:
                    raise TypeError(f"{typename}.__new__() missing 1 required positional argument: '{missing[0]}'")
                quoted = ' and '.join(repr(m) for m in missing) if len(missing) == 2 else ', '.join(repr(m) for m in missing[:-1]) + ', and ' + repr(missing[-1])
                raise TypeError(f"{typename}.__new__() missing {len(missing)} required positional arguments: {quoted}")
        if kwargs:
            name = next(iter(kwargs))
            if name in field_names:
                raise TypeError(f"{typename}.__new__() got multiple values for argument '{name}'")
            raise TypeError(f"{typename}.__new__() got an unexpected keyword argument '{name}'")
        return tuple.__new__(_cls, values)

    @classmethod
    def _make(cls, iterable):
        result = tuple.__new__(cls, iterable)
        if len(result) != num_fields:
            raise TypeError(f'Expected {num_fields} arguments, got {len(result)}')
        return result

    def _replace(self, /, **kwds):
        result = self._make([kwds.pop(name) if name in kwds else getattr(self, name) for name in field_names])
        if kwds:
            raise ValueError(f'Got unexpected field names: {list(kwds)!r}')
        return result

    def __repr__(self):
        return self.__class__.__name__ + '(' + ', '.join(
            f'{name}={value!r}' for name, value in zip(field_names, self)) + ')'

    def _asdict(self):
        return dict(zip(self._fields, self))

    def __getnewargs__(self):
        return tuple(self)

    class_namespace = {
        '__doc__': f'{typename}({", ".join(field_names)})',
        '__slots__': (),
        '_fields': field_names,
        '_field_defaults': field_defaults,
        '__new__': __new__,
        '_make': _make,
        '_replace': _replace,
        '__repr__': __repr__,
        '_asdict': _asdict,
        '__getnewargs__': __getnewargs__,
        '__match_args__': field_names,
    }
    for index, name in enumerate(field_names):
        class_namespace[name] = property(lambda self, _i=index: self[_i])
    result = type(typename, (tuple,), class_namespace)
    if module is None:
        module = '__main__'
    result.__module__ = module
    return result


class ChainMap(dict):
    def __init__(self, *maps):
        self.maps = list(maps) or [{}]

    def __missing__(self, key):
        raise KeyError(key)

    def __getitem__(self, key):
        for mapping in self.maps:
            if key in mapping:
                return mapping[key]
        return self.__missing__(key)

    def get(self, key, default=None):
        return self[key] if key in self else default

    def __len__(self):
        return len(set().union(*self.maps))

    def __iter__(self):
        d = {}
        for mapping in reversed(self.maps):
            d.update(dict.fromkeys(mapping))
        return iter(d)

    def __contains__(self, key):
        return any(key in m for m in self.maps)

    def __bool__(self):
        return any(self.maps)

    def __repr__(self):
        return f'{self.__class__.__name__}({", ".join(map(repr, self.maps))})'

    def keys(self):
        return list(iter(self))

    def items(self):
        return [(k, self[k]) for k in self]

    def values(self):
        return [self[k] for k in self]

    def copy(self):
        return self.__class__(self.maps[0].copy(), *self.maps[1:])

    def new_child(self, m=None, **kwargs):
        if m is None:
            m = kwargs
        elif kwargs:
            m.update(kwargs)
        return self.__class__(m, *self.maps)

    @property
    def parents(self):
        return self.__class__(*self.maps[1:])

    def __setitem__(self, key, value):
        self.maps[0][key] = value

    def __delitem__(self, key):
        try:
            del self.maps[0][key]
        except KeyError:
            raise KeyError(f'Key not found in the first mapping: {key!r}')

    def pop(self, key, *args):
        try:
            return self.maps[0].pop(key, *args)
        except KeyError:
            raise KeyError(f'Key not found in the first mapping: {key!r}')

    def clear(self):
        self.maps[0].clear()


class UserDict(dict):
    def __init__(self, dict=None, /, **kwargs):
        super().__init__()
        self.data = self
        if dict is not None:
            self.update(dict)
        if kwargs:
            self.update(kwargs)


class UserList(list):
    def __init__(self, initlist=None):
        super().__init__(initlist or [])
        self.data = self


class UserString(str):
    def __new__(cls, seq=''):
        return str.__new__(cls, seq)

    @property
    def data(self):
        return str(self)
