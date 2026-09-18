"""weakref for the simulated interpreter. There is no garbage collector to
observe, so references stay alive for the life of the program."""


class ref:
    def __init__(self, obj, callback=None):
        self._obj = obj
        self._callback = callback

    def __call__(self):
        return self._obj

    def __eq__(self, other):
        return isinstance(other, ref) and self._obj is other._obj

    def __hash__(self):
        return hash(id(self._obj))

    def __repr__(self):
        return f"<weakref at 0x7f0000002000; to '{type(self._obj).__name__}' at {hex(id(self._obj))}>"


ReferenceType = ref


def proxy(obj, callback=None):
    return obj


def getweakrefcount(obj):
    return 0


def getweakrefs(obj):
    return []


class WeakValueDictionary(dict):
    pass


class WeakKeyDictionary(dict):
    pass


class WeakSet(set):
    pass


class WeakMethod(ref):
    pass


class finalize:
    def __init__(self, obj, func, /, *args, **kwargs):
        self._func = func
        self._args = args
        self._kwargs = kwargs
        self.alive = True

    def __call__(self, _=None):
        if self.alive:
            self.alive = False
            return self._func(*self._args, **self._kwargs)

    def detach(self):
        self.alive = False
