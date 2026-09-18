"""functools for the simulated interpreter."""

WRAPPER_ASSIGNMENTS = ('__module__', '__name__', '__qualname__', '__doc__')
WRAPPER_UPDATES = ('__dict__',)

_initial_missing = object()


def update_wrapper(wrapper, wrapped, assigned=WRAPPER_ASSIGNMENTS, updated=WRAPPER_UPDATES):
    for attr in assigned:
        try:
            value = getattr(wrapped, attr)
        except AttributeError:
            pass
        else:
            try:
                setattr(wrapper, attr, value)
            except (AttributeError, TypeError):
                pass
    for attr in updated:
        try:
            getattr(wrapper, attr).update(getattr(wrapped, attr, {}))
        except AttributeError:
            pass
    try:
        wrapper.__wrapped__ = wrapped
    except (AttributeError, TypeError):
        pass
    return wrapper


def wraps(wrapped, assigned=WRAPPER_ASSIGNMENTS, updated=WRAPPER_UPDATES):
    return partial(update_wrapper, wrapped=wrapped, assigned=assigned, updated=updated)


def reduce(function, sequence, initial=_initial_missing):
    it = iter(sequence)
    if initial is _initial_missing:
        try:
            value = next(it)
        except StopIteration:
            raise TypeError("reduce() of empty iterable with no initial value") from None
    else:
        value = initial
    for element in it:
        value = function(value, element)
    return value


class partial:
    def __new__(cls, func, /, *args, **keywords):
        if not callable(func):
            raise TypeError("the first argument must be callable")
        if isinstance(func, partial):
            args = func.args + args
            keywords = {**func.keywords, **keywords}
            func = func.func
        self = super().__new__(cls)
        self.func = func
        self.args = args
        self.keywords = keywords
        return self

    def __call__(self, /, *args, **keywords):
        keywords = {**self.keywords, **keywords}
        return self.func(*self.args, *args, **keywords)

    def __repr__(self):
        qualname = type(self).__qualname__
        args = [repr(self.func)]
        args.extend(repr(x) for x in self.args)
        args.extend(f"{k}={v!r}" for (k, v) in self.keywords.items())
        if type(self).__module__ == "functools":
            return f"functools.{qualname}({', '.join(args)})"
        return f"{qualname}({', '.join(args)})"

    def __get__(self, obj, objtype=None):
        return self


class partialmethod:
    def __init__(self, func, /, *args, **keywords):
        self.func = func
        self.args = args
        self.keywords = keywords

    def __get__(self, obj, cls=None):
        if obj is None:
            return self
        return partial(self.func, obj, *self.args, **self.keywords)


def cmp_to_key(mycmp):
    class K(object):
        __slots__ = ['obj']

        def __init__(self, obj):
            self.obj = obj

        def __lt__(self, other):
            return mycmp(self.obj, other.obj) < 0

        def __gt__(self, other):
            return mycmp(self.obj, other.obj) > 0

        def __eq__(self, other):
            return mycmp(self.obj, other.obj) == 0

        def __le__(self, other):
            return mycmp(self.obj, other.obj) <= 0

        def __ge__(self, other):
            return mycmp(self.obj, other.obj) >= 0

        __hash__ = None
    return K


def total_ordering(cls):
    roots = {op for op in ('__lt__', '__le__', '__gt__', '__ge__')
             if getattr(cls, op, None) is not getattr(object, op, None)}
    if not roots:
        raise ValueError('must define at least one ordering operation: < > <= >=')
    root = max(roots)

    def _gt_from_lt(self, other):
        r = type(self).__lt__(self, other)
        if r is NotImplemented:
            return r
        return not r and self != other

    def _le_from_lt(self, other):
        r = type(self).__lt__(self, other)
        if r is NotImplemented:
            return r
        return r or self == other

    def _ge_from_lt(self, other):
        r = type(self).__lt__(self, other)
        if r is NotImplemented:
            return r
        return not r

    def _ge_from_le(self, other):
        r = type(self).__le__(self, other)
        if r is NotImplemented:
            return r
        return not r or self == other

    def _lt_from_le(self, other):
        r = type(self).__le__(self, other)
        if r is NotImplemented:
            return r
        return r and self != other

    def _gt_from_le(self, other):
        r = type(self).__le__(self, other)
        if r is NotImplemented:
            return r
        return not r

    def _lt_from_gt(self, other):
        r = type(self).__gt__(self, other)
        if r is NotImplemented:
            return r
        return not r and self != other

    def _ge_from_gt(self, other):
        r = type(self).__gt__(self, other)
        if r is NotImplemented:
            return r
        return r or self == other

    def _le_from_gt(self, other):
        r = type(self).__gt__(self, other)
        if r is NotImplemented:
            return r
        return not r

    def _le_from_ge(self, other):
        r = type(self).__ge__(self, other)
        if r is NotImplemented:
            return r
        return not r or self == other

    def _gt_from_ge(self, other):
        r = type(self).__ge__(self, other)
        if r is NotImplemented:
            return r
        return r and self != other

    def _lt_from_ge(self, other):
        r = type(self).__ge__(self, other)
        if r is NotImplemented:
            return r
        return not r

    convert = {
        '__lt__': [('__gt__', _gt_from_lt), ('__le__', _le_from_lt), ('__ge__', _ge_from_lt)],
        '__le__': [('__ge__', _ge_from_le), ('__lt__', _lt_from_le), ('__gt__', _gt_from_le)],
        '__gt__': [('__lt__', _lt_from_gt), ('__ge__', _ge_from_gt), ('__le__', _le_from_gt)],
        '__ge__': [('__le__', _le_from_ge), ('__gt__', _gt_from_ge), ('__lt__', _lt_from_ge)],
    }
    for opname, opfunc in convert[root]:
        if opname not in roots:
            opfunc.__name__ = opname
            setattr(cls, opname, opfunc)
    return cls


class _CacheInfo(tuple):
    def __new__(cls, hits, misses, maxsize, currsize):
        return tuple.__new__(cls, (hits, misses, maxsize, currsize))

    hits = property(lambda self: self[0])
    misses = property(lambda self: self[1])
    maxsize = property(lambda self: self[2])
    currsize = property(lambda self: self[3])

    def __repr__(self):
        return f"CacheInfo(hits={self[0]}, misses={self[1]}, maxsize={self[2]}, currsize={self[3]})"


def _make_key(args, kwds, typed):
    key = args
    if kwds:
        key += (_kwd_mark,)
        for item in kwds.items():
            key += item
    if typed:
        key += tuple(type(v) for v in args)
        if kwds:
            key += tuple(type(v) for v in kwds.values())
    elif len(key) == 1 and type(key[0]) in (int, str):
        return key[0]
    return key


_kwd_mark = (object(),)


def lru_cache(maxsize=128, typed=False):
    if isinstance(maxsize, int):
        if maxsize < 0:
            maxsize = 0
    elif callable(maxsize) and isinstance(typed, bool):
        user_function, maxsize = maxsize, 128
        return _lru_cache_wrapper(user_function, maxsize, typed)
    elif maxsize is not None:
        raise TypeError('Expected first argument to be an integer, a callable, or None')

    def decorating_function(user_function):
        return _lru_cache_wrapper(user_function, maxsize, typed)
    return decorating_function


def _lru_cache_wrapper(user_function, maxsize, typed):
    cache = {}
    stats = [0, 0]

    def wrapper(*args, **kwds):
        key = _make_key(args, kwds, typed)
        if key in cache:
            stats[0] += 1
            if maxsize is not None:
                value = cache.pop(key)
                cache[key] = value
                return value
            return cache[key]
        stats[1] += 1
        result = user_function(*args, **kwds)
        if maxsize is None or maxsize > 0:
            if key not in cache:
                cache[key] = result
                if maxsize is not None and len(cache) > maxsize:
                    del cache[next(iter(cache))]
        return result

    def cache_info():
        return _CacheInfo(stats[0], stats[1], maxsize, len(cache))

    def cache_clear():
        cache.clear()
        stats[0] = stats[1] = 0

    wrapper.cache_info = cache_info
    wrapper.cache_clear = cache_clear
    wrapper.cache_parameters = lambda: {'maxsize': maxsize, 'typed': typed}
    update_wrapper(wrapper, user_function)
    return wrapper


def cache(user_function, /):
    return lru_cache(maxsize=None)(user_function)


class cached_property:
    def __init__(self, func):
        self.func = func
        self.attrname = None
        self.__doc__ = func.__doc__

    def __set_name__(self, owner, name):
        if self.attrname is None:
            self.attrname = name
        elif name != self.attrname:
            raise TypeError(
                "Cannot assign the same cached_property to two different names "
                f"({self.attrname!r} and {name!r})."
            )

    def __get__(self, instance, owner=None):
        if instance is None:
            return self
        if self.attrname is None:
            raise TypeError("Cannot use cached_property instance without calling __set_name__ on it.")
        cache = instance.__dict__
        if self.attrname in cache:
            return cache[self.attrname]
        val = self.func(instance)
        cache[self.attrname] = val
        return val


def singledispatch(func):
    registry = {}

    def dispatch(cls):
        for c in cls.__mro__:
            if c in registry:
                return registry[c]
        return func

    def register(cls, method=None):
        if method is None:
            if isinstance(cls, type):
                return lambda f: register(cls, f)
            ann = getattr(cls, '__annotations__', {})
            if not ann:
                raise TypeError(f"Invalid first argument to `register()`: {cls!r}. Use either `@register(some_class)` or plain `@register` on an annotated function.")
            f = cls
            argname, cls = next(iter(ann.items()))
            registry[cls] = f
            return f
        registry[cls] = method
        return method

    def wrapper(*args, **kw):
        if not args:
            raise TypeError(f'{funcname} requires at least 1 positional argument')
        return dispatch(args[0].__class__)(*args, **kw)

    funcname = getattr(func, '__name__', 'singledispatch function')
    registry[object] = func
    wrapper.register = register
    wrapper.dispatch = dispatch
    wrapper.registry = registry
    update_wrapper(wrapper, func)
    return wrapper
