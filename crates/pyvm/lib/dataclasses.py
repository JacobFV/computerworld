"""dataclasses for the simulated interpreter (generated methods as in CPython)."""
import copy as _copy

__all__ = ['dataclass', 'field', 'Field', 'FrozenInstanceError', 'InitVar',
           'KW_ONLY', 'MISSING', 'fields', 'asdict', 'astuple', 'make_dataclass',
           'replace', 'is_dataclass']


class FrozenInstanceError(AttributeError):
    pass


class _HAS_DEFAULT_FACTORY_CLASS:
    def __repr__(self):
        return '<factory>'


_HAS_DEFAULT_FACTORY = _HAS_DEFAULT_FACTORY_CLASS()


class _MISSING_TYPE:
    def __repr__(self):
        return 'MISSING'


MISSING = _MISSING_TYPE()


class _KW_ONLY_TYPE:
    def __repr__(self):
        return 'KW_ONLY'


KW_ONLY = _KW_ONLY_TYPE()


class InitVar:
    def __init__(self, type):
        self.type = type

    def __class_getitem__(cls, type):
        return InitVar(type)

    def __repr__(self):
        return f'dataclasses.InitVar[{getattr(self.type, "__name__", self.type)}]'


class Field:
    def __init__(self, default, default_factory, init, repr, hash, compare, metadata, kw_only):
        self.name = None
        self.type = None
        self.default = default
        self.default_factory = default_factory
        self.init = init
        self.repr = repr
        self.hash = hash
        self.compare = compare
        self.metadata = metadata if metadata is not None else {}
        self.kw_only = kw_only
        self._field_type = None

    def __repr__(self):
        return ('Field('
                f'name={self.name!r},'
                f'type={self.type!r},'
                f'default={self.default!r},'
                f'default_factory={self.default_factory!r},'
                f'init={self.init!r},'
                f'repr={self.repr!r},'
                f'hash={self.hash!r},'
                f'compare={self.compare!r},'
                f'metadata={self.metadata!r},'
                f'kw_only={self.kw_only!r},'
                f'_field_type={self._field_type})')

    def __set_name__(self, owner, name):
        pass


def field(*, default=MISSING, default_factory=MISSING, init=True, repr=True,
          hash=None, compare=True, metadata=None, kw_only=MISSING):
    if default is not MISSING and default_factory is not MISSING:
        raise ValueError('cannot specify both default and default_factory')
    return Field(default, default_factory, init, repr, hash, compare, metadata, kw_only)


_FIELDS = '__dataclass_fields__'
_PARAMS = '__dataclass_params__'


def _is_classvar(a_type):
    if isinstance(a_type, str):
        return a_type.startswith('ClassVar') or a_type.startswith('typing.ClassVar')
    r = repr(a_type)
    return r.startswith('typing.ClassVar')


def _is_initvar(a_type):
    if isinstance(a_type, InitVar) or a_type is InitVar:
        return True
    return isinstance(a_type, str) and a_type.startswith('InitVar')


def _get_field(cls, a_name, a_type, default_kw_only):
    default = getattr(cls, a_name, MISSING)
    if isinstance(default, Field):
        f = default
    else:
        f = field(default=default)
    f.name = a_name
    f.type = a_type
    f._field_type = '_FIELD'
    if _is_classvar(a_type):
        f._field_type = '_FIELD_CLASSVAR'
    elif _is_initvar(a_type):
        f._field_type = '_FIELD_INITVAR'
    if f._field_type == '_FIELD' and f.kw_only is MISSING:
        f.kw_only = default_kw_only
    if f._field_type == '_FIELD' and f.default is not MISSING and type(f.default) in (list, dict, set):
        raise ValueError(f'mutable default {type(f.default)} for field '
                         f'{f.name} is not allowed: use default_factory')
    return f


def _fields_in_init_order(fields):
    return (tuple(f for f in fields if f.init and not f.kw_only),
            tuple(f for f in fields if f.init and f.kw_only))


def _init_fn(cls, fields, std_fields, kw_only_fields, frozen, has_post_init, self_name, globals):
    seen_default = False
    for f in std_fields:
        if f.init:
            if not (f.default is MISSING and f.default_factory is MISSING):
                seen_default = True
            elif seen_default:
                raise TypeError(f'non-default argument {f.name!r} follows default argument')
    locals = {'MISSING': MISSING, '_HAS_DEFAULT_FACTORY': _HAS_DEFAULT_FACTORY,
              '__dataclass_builtins_object__': object}
    lines = []
    for f in fields:
        if f._field_type == '_FIELD_CLASSVAR':
            continue
        if f.default_factory is not MISSING:
            locals[f'__dataclass_dflt_{f.name}__'] = f.default_factory
            if f.init:
                value = f'__dataclass_dflt_{f.name}__() if {f.name} is _HAS_DEFAULT_FACTORY else {f.name}'
            else:
                value = f'__dataclass_dflt_{f.name}__()'
        elif f.init:
            if f.default is not MISSING:
                locals[f'__dataclass_dflt_{f.name}__'] = f.default
            value = f.name
        elif f.default is not MISSING:
            locals[f'__dataclass_dflt_{f.name}__'] = f.default
            value = f'__dataclass_dflt_{f.name}__'
        else:
            continue
        if f._field_type == '_FIELD_INITVAR':
            continue
        if frozen:
            lines.append(f'__dataclass_builtins_object__.__setattr__({self_name},{f.name!r},{value})')
        else:
            lines.append(f'{self_name}.{f.name}={value}')
    if has_post_init:
        params = ','.join(f.name for f in fields if f._field_type == '_FIELD_INITVAR')
        lines.append(f'{self_name}.__post_init__({params})')
    if not lines:
        lines = ['pass']

    def param(f):
        if f.default is MISSING and f.default_factory is MISSING:
            return f.name
        if f.default_factory is not MISSING:
            return f'{f.name}=_HAS_DEFAULT_FACTORY'
        return f'{f.name}=__dataclass_dflt_{f.name}__'
    params = [self_name] + [param(f) for f in std_fields]
    if kw_only_fields:
        params += ['*'] + [param(f) for f in kw_only_fields]
    src = f"def __init__({', '.join(params)}):\n" + ''.join(f'  {l}\n' for l in lines)
    ns = dict(locals)
    exec(src, ns)
    fn = ns['__init__']
    fn.__qualname__ = f'{cls.__qualname__}.__init__'
    return fn


def _repr_fn(fields):
    def __repr__(self):
        return (self.__class__.__qualname__ + '(' +
                ', '.join(f'{f.name}={getattr(self, f.name)!r}' for f in fields) + ')')
    return __repr__


def _tuple_of(obj, fields):
    return tuple(getattr(obj, f.name) for f in fields)


def _cmp_fn(name, op, fields):
    def fn(self, other):
        if other.__class__ is self.__class__:
            a = _tuple_of(self, fields)
            b = _tuple_of(other, fields)
            if op == '==':
                return a == b
            if op == '!=':
                return a != b
            if op == '<':
                return a < b
            if op == '<=':
                return a <= b
            if op == '>':
                return a > b
            return a >= b
        return NotImplemented
    fn.__name__ = name
    return fn


def _frozen_get_del_attr(cls, fields):
    names = tuple(f.name for f in fields)

    def __setattr__(self, name, value):
        if type(self) is cls or name in names:
            raise FrozenInstanceError(f'cannot assign to field {name!r}')
        super(cls, self).__setattr__(name, value)

    def __delattr__(self, name):
        if type(self) is cls or name in names:
            raise FrozenInstanceError(f'cannot delete field {name!r}')
        super(cls, self).__delattr__(name)
    return __setattr__, __delattr__


def _process_class(cls, init, repr, eq, order, unsafe_hash, frozen, match_args, kw_only, slots):
    fields = {}
    globals = {}
    for b in cls.__mro__[-1:0:-1]:
        base_fields = getattr(b, _FIELDS, None)
        if base_fields is not None:
            for f in base_fields.values():
                fields[f.name] = f
    cls_annotations = cls.__dict__.get('__annotations__', {})
    cls_fields = []
    KW = kw_only
    for name, type_ in cls_annotations.items():
        if type_ is KW_ONLY or (isinstance(type_, str) and type_ == 'KW_ONLY'):
            KW = True
            continue
        cls_fields.append(_get_field(cls, name, type_, KW))
    for f in cls_fields:
        fields[f.name] = f
        if isinstance(getattr(cls, f.name, None), Field):
            if f.default is MISSING:
                delattr(cls, f.name)
            else:
                setattr(cls, f.name, f.default)
    for name, value in cls.__dict__.items():
        if isinstance(value, Field) and name not in cls_annotations:
            raise TypeError(f'{name!r} is a field but has no type annotation')
    setattr(cls, _FIELDS, fields)
    setattr(cls, _PARAMS, {'init': init, 'repr': repr, 'eq': eq, 'order': order,
                           'unsafe_hash': unsafe_hash, 'frozen': frozen})
    all_fields = [f for f in fields.values() if f._field_type in ('_FIELD', '_FIELD_INITVAR')]
    real_fields = [f for f in fields.values() if f._field_type == '_FIELD']
    if init:
        has_post_init = hasattr(cls, '__post_init__')
        std, kw = _fields_in_init_order(all_fields)
        cls.__init__ = _init_fn(cls, all_fields, std, kw, frozen, has_post_init, 'self', globals)
    if repr:
        flds = [f for f in real_fields if f.repr]
        fn = _repr_fn(flds)
        fn.__qualname__ = f'{cls.__qualname__}.__repr__'
        if '__repr__' not in cls.__dict__:
            cls.__repr__ = fn
    cmp_fields = [f for f in real_fields if f.compare]
    if eq and '__eq__' not in cls.__dict__:
        cls.__eq__ = _cmp_fn('__eq__', '==', cmp_fields)
    if order:
        for name, op in [('__lt__', '<'), ('__le__', '<='), ('__gt__', '>'), ('__ge__', '>=')]:
            if name in cls.__dict__:
                raise TypeError(f'Cannot overwrite attribute {name} in class {cls.__name__}. Consider using functools.total_ordering')
            setattr(cls, name, _cmp_fn(name, op, cmp_fields))
    if frozen:
        sa, da = _frozen_get_del_attr(cls, real_fields)
        cls.__setattr__ = sa
        cls.__delattr__ = da
    hash_fields = [f for f in real_fields if (f.compare if f.hash is None else f.hash)]
    has_explicit_hash = '__hash__' in cls.__dict__ and cls.__dict__['__hash__'] is not None
    if unsafe_hash or (eq and frozen):
        if not has_explicit_hash or unsafe_hash:
            cls.__hash__ = lambda self: hash(_tuple_of(self, hash_fields))
    elif eq and not has_explicit_hash:
        cls.__hash__ = None
    if match_args and '__match_args__' not in cls.__dict__:
        cls.__match_args__ = tuple(f.name for f in real_fields if f.init and not f.kw_only)
    return cls


def dataclass(cls=None, /, *, init=True, repr=True, eq=True, order=False,
              unsafe_hash=False, frozen=False, match_args=True,
              kw_only=False, slots=False, weakref_slot=False):
    def wrap(cls):
        return _process_class(cls, init, repr, eq, order, unsafe_hash,
                              frozen, match_args, kw_only, slots)
    if cls is None:
        return wrap
    return wrap(cls)


def fields(class_or_instance):
    try:
        fields = getattr(class_or_instance, _FIELDS)
    except AttributeError:
        raise TypeError('must be called with a dataclass type or instance') from None
    return tuple(f for f in fields.values() if f._field_type == '_FIELD')


def is_dataclass(obj):
    cls = obj if isinstance(obj, type) else type(obj)
    return hasattr(cls, _FIELDS)


def _is_dataclass_instance(obj):
    return hasattr(type(obj), _FIELDS)


def asdict(obj, *, dict_factory=dict):
    if not _is_dataclass_instance(obj):
        raise TypeError("asdict() should be called on dataclass instances")
    return _asdict_inner(obj, dict_factory)


def _asdict_inner(obj, dict_factory):
    if _is_dataclass_instance(obj):
        result = []
        for f in fields(obj):
            value = _asdict_inner(getattr(obj, f.name), dict_factory)
            result.append((f.name, value))
        return dict_factory(result)
    elif isinstance(obj, tuple) and hasattr(obj, '_fields'):
        return type(obj)(*[_asdict_inner(v, dict_factory) for v in obj])
    elif isinstance(obj, (list, tuple)):
        return type(obj)(_asdict_inner(v, dict_factory) for v in obj)
    elif isinstance(obj, dict):
        return type(obj)((_asdict_inner(k, dict_factory), _asdict_inner(v, dict_factory))
                         for k, v in obj.items())
    else:
        return _copy.deepcopy(obj)


def astuple(obj, *, tuple_factory=tuple):
    if not _is_dataclass_instance(obj):
        raise TypeError("astuple() should be called on dataclass instances")
    return _astuple_inner(obj, tuple_factory)


def _astuple_inner(obj, tuple_factory):
    if _is_dataclass_instance(obj):
        return tuple_factory([_astuple_inner(getattr(obj, f.name), tuple_factory) for f in fields(obj)])
    elif isinstance(obj, (list, tuple)):
        return type(obj)(_astuple_inner(v, tuple_factory) for v in obj)
    elif isinstance(obj, dict):
        return type(obj)((_astuple_inner(k, tuple_factory), _astuple_inner(v, tuple_factory))
                         for k, v in obj.items())
    else:
        return _copy.deepcopy(obj)


def replace(obj, /, **changes):
    if not _is_dataclass_instance(obj):
        raise TypeError("replace() should be called on dataclass instances")
    for f in getattr(obj, _FIELDS).values():
        if f._field_type == '_FIELD_CLASSVAR':
            continue
        if not f.init:
            if f.name in changes:
                raise ValueError(f'field {f.name} is declared with init=False, it cannot be specified with replace()')
            continue
        if f.name not in changes:
            changes[f.name] = getattr(obj, f.name)
    return obj.__class__(**changes)


def make_dataclass(cls_name, fields, *, bases=(), namespace=None, **kwargs):
    namespace = dict(namespace or {})
    annotations = {}
    for item in fields:
        if isinstance(item, str):
            name, tp = item, 'typing.Any'
        elif len(item) == 2:
            name, tp = item
        else:
            name, tp, spec = item
            namespace[name] = spec
        annotations[name] = tp
    namespace['__annotations__'] = annotations
    cls = type(cls_name, bases, namespace)
    return dataclass(cls, **kwargs)
