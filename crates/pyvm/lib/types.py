"""types for the simulated interpreter."""
import sys as _sys


def _f():
    pass


FunctionType = type(_f)
LambdaType = type(lambda: None)
CodeType = type(_f.__code__)
BuiltinFunctionType = type(len)
BuiltinMethodType = type([].append)
MethodType = type(_sys.exc_info.__call__) if False else type((lambda self: None).__get__(1))
ModuleType = type(_sys)


def _g():
    yield 1


GeneratorType = type(_g())


async def _c():
    pass


_co = _c()
CoroutineType = type(_co)
del _co
NoneType = type(None)
NotImplementedType = type(NotImplemented)
EllipsisType = type(Ellipsis)
MappingProxyType = dict


class SimpleNamespace:
    def __init__(self, mapping_or_iterable=(), /, **kwargs):
        self.__dict__.update(dict(mapping_or_iterable))
        self.__dict__.update(kwargs)

    def __repr__(self):
        items = (f"{k}={v!r}" for k, v in self.__dict__.items())
        return "{}({})".format(type(self).__name__ if type(self) is not SimpleNamespace else "namespace", ", ".join(items))

    def __eq__(self, other):
        if isinstance(self, SimpleNamespace) and isinstance(other, SimpleNamespace):
            return self.__dict__ == other.__dict__
        return NotImplemented


def new_class(name, bases=(), kwds=None, exec_body=None):
    ns = {}
    if exec_body is not None:
        exec_body(ns)
    return type(name, tuple(bases), ns)


class DynamicClassAttribute(property):
    pass


def coroutine(func):
    return func
