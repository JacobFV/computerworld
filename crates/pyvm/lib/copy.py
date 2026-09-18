"""copy for the simulated interpreter."""


class Error(Exception):
    pass


error = Error

_atomic = (type(None), int, float, bool, complex, str, bytes, type, range,
           type(len), type(Ellipsis), type(NotImplemented), frozenset)


def copy(x):
    cls = type(x)
    if cls in _atomic or isinstance(x, type):
        return x
    if cls in (list, dict, set, bytearray):
        return cls(x)
    if cls is tuple:
        return x
    copier = getattr(cls, '__copy__', None)
    if copier is not None:
        return copier(x)
    reductor = getattr(cls, '__reduce_ex__', None)
    if isinstance(x, (list, dict, set, tuple)):
        y = cls.__new__(cls)
        if isinstance(x, list):
            y.extend(x)
        elif isinstance(x, dict):
            y.update(x)
        elif isinstance(x, set):
            y.update(x)
        else:
            y = cls(x) if not hasattr(cls, '_make') else cls._make(x)
        if hasattr(x, '__dict__'):
            y.__dict__.update(x.__dict__)
        return y
    y = cls.__new__(cls)
    if hasattr(x, '__dict__'):
        state = x.__dict__
        if hasattr(y, '__setstate__'):
            y.__setstate__(dict(state))
        else:
            y.__dict__.update(state)
    return y


def deepcopy(x, memo=None, _nil=[]):
    if memo is None:
        memo = {}
    d = id(x)
    y = memo.get(d, _nil)
    if y is not _nil:
        return y
    cls = type(x)
    if cls in _atomic or isinstance(x, type) or callable(x) and not hasattr(x, '__dict__') and cls not in (list, dict, set, tuple):
        return x
    copier = getattr(x, '__deepcopy__', None)
    if copier is not None and not isinstance(x, type):
        y = copier(memo)
    elif cls is list:
        y = []
        memo[d] = y
        for a in x:
            y.append(deepcopy(a, memo))
    elif cls is dict:
        y = {}
        memo[d] = y
        for key, value in x.items():
            y[deepcopy(key, memo)] = deepcopy(value, memo)
    elif cls is set:
        y = set()
        memo[d] = y
        for a in x:
            y.add(deepcopy(a, memo))
    elif cls is tuple:
        y = tuple(deepcopy(a, memo) for a in x)
    elif isinstance(x, (list, dict, set, tuple)):
        if isinstance(x, tuple):
            items = [deepcopy(a, memo) for a in x]
            y = cls._make(items) if hasattr(cls, '_make') else tuple.__new__(cls, items)
        else:
            y = cls.__new__(cls)
            memo[d] = y
            if isinstance(x, list):
                for a in x:
                    y.append(deepcopy(a, memo))
            elif isinstance(x, dict):
                for key, value in x.items():
                    dict.__setitem__(y, deepcopy(key, memo), deepcopy(value, memo))
            else:
                for a in x:
                    y.add(deepcopy(a, memo))
        if hasattr(x, '__dict__'):
            for k, v in x.__dict__.items():
                y.__dict__[k] = deepcopy(v, memo)
    else:
        y = cls.__new__(cls)
        memo[d] = y
        if hasattr(x, '__dict__'):
            state = deepcopy(x.__dict__, memo)
            if hasattr(y, '__setstate__'):
                y.__setstate__(state)
            else:
                y.__dict__.update(state)
    memo[d] = y
    return y


def replace(obj, /, **changes):
    func = getattr(type(obj), '__replace__', None)
    if func is None:
        raise TypeError(f"replace() does not support {type(obj).__name__} objects")
    return func(obj, **changes)
