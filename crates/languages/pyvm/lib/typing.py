"""typing for the simulated interpreter.

Annotations are accepted and introspectable enough for common code; the special
forms subscript to themselves rather than building full generic aliases.
"""
import collections as _collections


class _SpecialForm:
    def __init__(self, name):
        self._name = name

    def __repr__(self):
        return 'typing.' + self._name

    def __getitem__(self, params):
        return _GenericAlias(self, params)

    def __call__(self, *args, **kwargs):
        raise TypeError(f"Cannot instantiate {self!r}")

    def __or__(self, other):
        return _GenericAlias(Union, (self, other))

    def __ror__(self, other):
        return _GenericAlias(Union, (other, self))


class _GenericAlias:
    def __init__(self, origin, params, name=None):
        self.__origin__ = origin
        if not isinstance(params, tuple):
            params = (params,)
        self.__args__ = params
        self._alias_name = name

    def __repr__(self):
        def r(a):
            if isinstance(a, type):
                if a.__module__ == 'builtins':
                    return a.__qualname__
                return f'{a.__module__}.{a.__qualname__}'
            if a is Ellipsis:
                return '...'
            return repr(a)
        o = self.__origin__
        if self._alias_name is not None:
            name = self._alias_name
        else:
            name = r(o) if not isinstance(o, _SpecialForm) else repr(o)
        if o is Optional and len(self.__args__) == 1:
            return f"typing.Optional[{r(self.__args__[0])}]"
        return f"{name}[{', '.join(r(a) for a in self.__args__)}]"

    def __getitem__(self, params):
        return _GenericAlias(self.__origin__, params, self._alias_name)

    def __call__(self, *args, **kwargs):
        return self.__origin__(*args, **kwargs)

    def __eq__(self, other):
        return isinstance(other, _GenericAlias) and self.__origin__ == other.__origin__ and self.__args__ == other.__args__

    def __hash__(self):
        return hash((self.__origin__, self.__args__))

    def __or__(self, other):
        return _GenericAlias(Union, (self, other))

    def __mro_entries__(self, bases):
        o = self.__origin__
        return (o,) if isinstance(o, type) else ()

    def __instancecheck__(self, obj):
        return isinstance(obj, self.__origin__)


Any = _SpecialForm('Any')
Union = _SpecialForm('Union')
Optional = _SpecialForm('Optional')
ClassVar = _SpecialForm('ClassVar')
Final = _SpecialForm('Final')
Literal = _SpecialForm('Literal')
Annotated = _SpecialForm('Annotated')
NoReturn = _SpecialForm('NoReturn')
Never = _SpecialForm('Never')
Self = _SpecialForm('Self')
LiteralString = _SpecialForm('LiteralString')
TypeAlias = _SpecialForm('TypeAlias')
Concatenate = _SpecialForm('Concatenate')
TypeGuard = _SpecialForm('TypeGuard')
Required = _SpecialForm('Required')
NotRequired = _SpecialForm('NotRequired')
Unpack = _SpecialForm('Unpack')


class _Alias:
    def __init__(self, origin, name):
        self.__origin__ = origin
        self._name = name

    def __getitem__(self, params):
        return _GenericAlias(self.__origin__, params, 'typing.' + self._name)

    def __repr__(self):
        return 'typing.' + self._name

    def __call__(self, *args, **kwargs):
        return self.__origin__(*args, **kwargs)

    def __mro_entries__(self, bases):
        return (self.__origin__,)

    def __instancecheck__(self, obj):
        return isinstance(obj, self.__origin__)


List = _Alias(list, 'List')
Dict = _Alias(dict, 'Dict')
Set = _Alias(set, 'Set')
FrozenSet = _Alias(frozenset, 'FrozenSet')
Tuple = _Alias(tuple, 'Tuple')
Type = _Alias(type, 'Type')
DefaultDict = _Alias(_collections.defaultdict, 'DefaultDict')
OrderedDict = _Alias(_collections.OrderedDict, 'OrderedDict')
Counter = _Alias(_collections.Counter, 'Counter')
Deque = _Alias(_collections.deque, 'Deque')
ChainMap = _Alias(_collections.ChainMap, 'ChainMap')
Callable = _Alias(object, 'Callable')
Iterable = _Alias(object, 'Iterable')
Iterator = _Alias(object, 'Iterator')
Generator = _Alias(object, 'Generator')
Sequence = _Alias(object, 'Sequence')
MutableSequence = _Alias(list, 'MutableSequence')
Mapping = _Alias(object, 'Mapping')
MutableMapping = _Alias(dict, 'MutableMapping')
AbstractSet = _Alias(object, 'AbstractSet')
MutableSet = _Alias(set, 'MutableSet')
Collection = _Alias(object, 'Collection')
Container = _Alias(object, 'Container')
Hashable = _Alias(object, 'Hashable')
Sized = _Alias(object, 'Sized')
Reversible = _Alias(object, 'Reversible')
Awaitable = _Alias(object, 'Awaitable')
Coroutine = _Alias(object, 'Coroutine')
AsyncIterator = _Alias(object, 'AsyncIterator')
AsyncIterable = _Alias(object, 'AsyncIterable')
AsyncGenerator = _Alias(object, 'AsyncGenerator')
IO = _Alias(object, 'IO')
TextIO = _Alias(object, 'TextIO')
BinaryIO = _Alias(object, 'BinaryIO')
Pattern = _Alias(object, 'Pattern')
Match = _Alias(object, 'Match')
Text = str
AnyStr = str
SupportsInt = _Alias(object, 'SupportsInt')
SupportsFloat = _Alias(object, 'SupportsFloat')
SupportsAbs = _Alias(object, 'SupportsAbs')
SupportsIndex = _Alias(object, 'SupportsIndex')
ContextManager = _Alias(object, 'ContextManager')


class TypeVar:
    def __init__(self, name, *constraints, bound=None, covariant=False, contravariant=False, infer_variance=False):
        self.__name__ = name
        self.__constraints__ = constraints
        self.__bound__ = bound
        self.__covariant__ = covariant
        self.__contravariant__ = contravariant

    def __repr__(self):
        prefix = '+' if self.__covariant__ else '-' if self.__contravariant__ else '~'
        return prefix + self.__name__

    def __or__(self, other):
        return _GenericAlias(Union, (self, other))


class ParamSpec(TypeVar):
    args = object()
    kwargs = object()


class TypeVarTuple(TypeVar):
    pass


class Generic:
    def __class_getitem__(cls, params):
        return cls

    def __init_subclass__(cls, *args, **kwargs):
        pass


class Protocol(Generic):
    pass


def runtime_checkable(cls):
    return cls


def cast(typ, val):
    return val


def assert_type(val, typ, /):
    return val


def assert_never(arg, /):
    raise AssertionError(f"Expected code to be unreachable, but got: {arg!r}")


def reveal_type(obj, /):
    import sys
    print(f"Runtime type is {type(obj).__name__!r}", file=sys.stderr)
    return obj


def overload(func):
    return func


def final(f):
    return f


def override(method, /):
    return method


def no_type_check(arg):
    return arg


def get_type_hints(obj, globalns=None, localns=None, include_extras=False):
    hints = {}
    if isinstance(obj, type):
        for base in reversed(obj.__mro__):
            hints.update(base.__dict__.get('__annotations__', {}))
        return hints
    return dict(getattr(obj, '__annotations__', {}) or {})


def get_origin(tp):
    return getattr(tp, '__origin__', None)


def get_args(tp):
    return getattr(tp, '__args__', ())


def NewType(name, tp):
    def new_type(x):
        return x
    new_type.__name__ = name
    new_type.__supertype__ = tp
    return new_type


def NamedTuple(typename, fields=None, /, **kwargs):
    if fields is None:
        fields = kwargs.items()
    return _collections.namedtuple(typename, [f for f, _ in fields])


class _NamedTupleMeta(type):
    pass


def _namedtuple_mro_entries(bases):
    return (_NamedTupleBase,)


class _NamedTupleBase:
    def __init_subclass__(cls, **kwargs):
        pass


def TypedDict(typename, fields=None, /, *, total=True, **kwargs):
    return dict


TYPE_CHECKING = False


def _nt_mro_entries(bases):
    return (_NamedTupleBuilder,)


class _NamedTupleBuilder(tuple):
    """Base for `class Point(NamedTuple): x: int` definitions."""

    def __init_subclass__(cls, **kwargs):
        ann = cls.__dict__.get('__annotations__', {})
        fields = tuple(ann)
        defaults = {f: cls.__dict__[f] for f in fields if f in cls.__dict__}
        nt = _collections.namedtuple(cls.__name__, fields,
                                     defaults=[defaults[f] for f in fields if f in defaults] or None)
        for name in ('__new__', '_make', '_replace', '__repr__', '_asdict', '__getnewargs__', '_fields', '_field_defaults', '__match_args__'):
            setattr(cls, name, nt.__dict__[name])
        for i, f in enumerate(fields):
            setattr(cls, f, property(lambda self, _i=i: self[_i]))
        cls.__annotations__ = dict(ann)


NamedTuple.__mro_entries__ = _nt_mro_entries


def _td_mro_entries(bases):
    return (dict,)


TypedDict.__mro_entries__ = _td_mro_entries
