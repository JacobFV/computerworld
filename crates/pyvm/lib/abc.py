"""abc for the simulated interpreter. Abstract methods are enforced at
instantiation for classes whose metaclass derives from ABCMeta."""


def abstractmethod(funcobj):
    funcobj.__isabstractmethod__ = True
    return funcobj


class abstractclassmethod(classmethod):
    __isabstractmethod__ = True


class abstractstaticmethod(staticmethod):
    __isabstractmethod__ = True


class abstractproperty(property):
    __isabstractmethod__ = True


class ABCMeta(type):
    def __new__(mcls, name, bases, namespace, /, **kwargs):
        cls = super().__new__(mcls, name, bases, namespace, **kwargs)
        cls._abc_registry = []
        return cls

    def register(cls, subclass):
        cls._abc_registry.append(subclass)
        return subclass

    def __instancecheck__(cls, instance):
        return cls.__subclasscheck__(type(instance))

    def __subclasscheck__(cls, subclass):
        if cls in getattr(subclass, '__mro__', ()):
            return True
        for r in cls._abc_registry:
            if issubclass(subclass, r):
                return True
        hook = cls.__dict__.get('__subclasshook__')
        if hook is not None:
            ok = hook.__func__(cls, subclass) if hasattr(hook, '__func__') else hook(subclass)
            if ok is not NotImplemented:
                return bool(ok)
        return False


class ABC(metaclass=ABCMeta):
    __slots__ = ()


def get_cache_token():
    return 0


def update_abstractmethods(cls):
    return cls
