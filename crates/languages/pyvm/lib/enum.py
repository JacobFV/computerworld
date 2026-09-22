"""enum for the simulated interpreter (Enum, IntEnum, StrEnum, Flag, IntFlag, auto)."""

__all__ = ['EnumType', 'EnumMeta', 'Enum', 'IntEnum', 'StrEnum', 'Flag', 'IntFlag',
           'auto', 'unique', 'ReprEnum', 'verify', 'member', 'nonmember']


class auto:
    _counter = 0

    def __init__(self, value=None):
        self.value = value
        auto._counter += 1
        self._order = auto._counter

    def __repr__(self):
        return "auto(%r)" % (self.value,)


class member:
    def __init__(self, value):
        self.value = value


class nonmember:
    def __init__(self, value):
        self.value = value


def _is_descriptor(obj):
    return (hasattr(obj, '__get__') or hasattr(obj, '__set__') or hasattr(obj, '__delete__'))


def _is_dunder(name):
    return len(name) > 4 and name[:2] == name[-2:] == '__' and name[2] != '_' and name[-3] != '_'


def _is_sunder(name):
    return len(name) > 2 and name[0] == name[-1] == '_' and name[1:2] != '_' and name[-2:-1] != '_'


def _is_private(cls_name, name):
    pattern = '_%s__' % (cls_name.lstrip('_'),)
    return len(name) > len(pattern) and name.startswith(pattern)


class EnumType(type):
    @classmethod
    def __prepare__(metacls, cls, bases, **kwds):
        return {}

    def __new__(metacls, cls, bases, classdict, *, boundary=None, _simple=False, **kwds):
        member_names = []
        members = {}
        ignore = classdict.get('_ignore_', [])
        if isinstance(ignore, str):
            ignore = ignore.replace(',', ' ').split()
        for key, value in list(classdict.items()):
            if key in ignore or key == '_ignore_':
                continue
            if isinstance(value, nonmember):
                classdict[key] = value.value
                continue
            if isinstance(value, member):
                value = value.value
            elif _is_dunder(key) or _is_sunder(key) or _is_private(cls, key) or _is_descriptor(value):
                continue
            members[key] = value
            member_names.append(key)
        for key in member_names:
            del classdict[key]
        for key in ignore:
            classdict.pop(key, None)
        # The first data type among the bases (int for IntEnum, str for StrEnum).
        member_type = object
        for base in bases:
            for c in base.__mro__:
                if c in (int, str, float) :
                    member_type = c
                    break
            if member_type is not object:
                break
        classdict['_member_type_'] = member_type
        enum_class = super().__new__(metacls, cls, bases, classdict, **kwds)
        enum_class._member_names_ = []
        enum_class._member_map_ = {}
        enum_class._value2member_map_ = {}
        last_values = []
        gnv = getattr(enum_class, '_generate_next_value_', None)
        for name in member_names:
            value = members[name]
            if isinstance(value, auto):
                if value.value is None:
                    value = gnv(name, 1, len(last_values), last_values[:]) if gnv else len(last_values) + 1
                else:
                    value = value.value
            if isinstance(value, tuple) and any(isinstance(v, auto) for v in value):
                value = tuple(gnv(name, 1, len(last_values), last_values[:]) if isinstance(v, auto) else v for v in value)
            last_values.append(value)
            args = value if isinstance(value, tuple) else (value,)
            custom_new = enum_class.__dict__.get('__new__') if '__new__' in enum_class.__dict__ else None
            if custom_new is not None:
                enum_member = custom_new(enum_class, *args)
                if not hasattr(enum_member, '_value_'):
                    enum_member._value_ = value if member_type is object else member_type(*args)
            elif member_type is object:
                enum_member = object.__new__(enum_class)
                enum_member._value_ = value
            else:
                enum_member = member_type.__new__(enum_class, *args)
                enum_member._value_ = member_type(*args)
            value = enum_member._value_
            enum_member._name_ = name
            enum_member.__objclass__ = enum_class
            init = getattr(enum_class, '__init__', None)
            if init is not None and init is not object.__init__ and '__init__' in _all_dicts(enum_class):
                enum_member.__init__(*args)
            existing = None
            for m in enum_class._member_map_.values():
                if m._value_ == value:
                    existing = m
                    break
            if existing is not None:
                enum_member = existing
            else:
                enum_class._member_names_.append(name)
            enum_class._member_map_[name] = enum_member
            try:
                enum_class._value2member_map_.setdefault(value, enum_member)
            except TypeError:
                pass
            type.__setattr__(enum_class, name, enum_member)
        return enum_class

    def __call__(cls, value, names=None, *, module=None, qualname=None, type=None, start=1, boundary=None):
        if names is None:
            if isinstance(value, cls):
                return value
            try:
                return cls._value2member_map_[value]
            except (KeyError, TypeError):
                for m in cls._member_map_.values():
                    if m._value_ == value:
                        return m
            result = cls._missing_(value)
            if isinstance(result, cls):
                return result
            raise ValueError(f"{value!r} is not a valid {cls.__qualname__}")
        return cls._create_(value, names, module=module, qualname=qualname, type=type, start=start)

    def _create_(cls, class_name, names, *, module=None, qualname=None, type=None, start=1):
        bases = (cls,) if type is None else (type, cls)
        classdict = {}
        if isinstance(names, str):
            names = names.replace(',', ' ').split()
        if isinstance(names, (tuple, list)) and names and isinstance(names[0], str):
            names = [(e, i + start) for (i, e) in enumerate(names)]
        if isinstance(names, dict):
            names = names.items()
        for item in names:
            member_name, member_value = item
            classdict[member_name] = member_value
        return EnumType.__new__(EnumType, class_name, bases, classdict)

    def __getitem__(cls, name):
        return cls._member_map_[name]

    def __iter__(cls):
        return (cls._member_map_[name] for name in cls._member_names_)

    def __reversed__(cls):
        return (cls._member_map_[name] for name in reversed(cls._member_names_))

    def __len__(cls):
        return len(cls._member_names_)

    def __contains__(cls, value):
        if isinstance(value, cls):
            return True
        try:
            return value in cls._value2member_map_
        except TypeError:
            return False

    def __bool__(cls):
        return True

    def __repr__(cls):
        if Flag is not None and issubclass(cls, Flag):
            return "<flag %r>" % cls.__name__
        return "<enum %r>" % cls.__name__

    @property
    def __members__(cls):
        return dict(cls._member_map_)

    def __setattr__(cls, name, value):
        member_map = cls.__dict__.get('_member_map_', {})
        if name in member_map:
            raise AttributeError('cannot reassign member %r' % (name,))
        super().__setattr__(name, value)

    def __delattr__(cls, attr):
        if attr in cls._member_map_:
            raise AttributeError("%r cannot delete member %r." % (cls.__name__, attr))
        super().__delattr__(attr)


def _all_dicts(cls):
    names = set()
    for c in cls.__mro__:
        if c in (Enum, object) or c.__module__ == 'builtins':
            continue
        names.update(c.__dict__)
    return names


EnumMeta = EnumType
Flag = None


class Enum(metaclass=EnumType):
    @staticmethod
    def _generate_next_value_(name, start, count, last_values):
        if not last_values:
            return start
        try:
            return max(v for v in last_values if isinstance(v, int)) + 1
        except ValueError:
            return start

    @classmethod
    def _missing_(cls, value):
        return None

    def __repr__(self):
        v_repr = self.__class__._value_repr_ or repr
        return "<%s.%s: %s>" % (self.__class__.__name__, self._name_, v_repr(self._value_))

    def __str__(self):
        return "%s.%s" % (self.__class__.__name__, self._name_)

    def __format__(self, format_spec):
        return str.__format__(str(self), format_spec)

    def __hash__(self):
        return hash(self._name_)

    def __reduce_ex__(self, proto):
        return self.__class__, (self._value_,)

    def __dir__(self):
        return ['__class__', '__doc__', '__eq__', '__hash__', '__module__', 'name', 'value']

    @property
    def name(self):
        return self._name_

    @property
    def value(self):
        return self._value_

    _value_repr_ = None


class ReprEnum(Enum):
    pass


class IntEnum(int, ReprEnum):
    def __str__(self):
        return int.__repr__(self._value_)

    def __format__(self, format_spec):
        return int.__format__(self._value_, format_spec)

    _value_repr_ = None


class StrEnum(str, ReprEnum):
    def __str__(self):
        return self._value_

    def __format__(self, format_spec):
        return str.__format__(self._value_, format_spec)

    @staticmethod
    def _generate_next_value_(name, start, count, last_values):
        return name.lower()


class Flag(Enum):
    @staticmethod
    def _generate_next_value_(name, start, count, last_values):
        if not count:
            return start if start is not None else 1
        high = max(v for v in last_values if isinstance(v, int))
        return 1 << high.bit_length()

    @classmethod
    def _missing_(cls, value):
        if not isinstance(value, int):
            raise ValueError("%r is not a valid %s" % (value, cls.__qualname__))
        pseudo = object.__new__(cls) if cls._member_type_ is object else cls._member_type_.__new__(cls, value)
        pseudo._value_ = value
        names = [m._name_ for m in cls if m._value_ and (m._value_ & value) == m._value_]
        pseudo._name_ = '|'.join(names) if names else None
        cls._value2member_map_[value] = pseudo
        return pseudo

    def __contains__(self, other):
        return other._value_ & self._value_ == other._value_

    def __iter__(self):
        for m in self.__class__:
            if m._value_ and (m._value_ & self._value_) == m._value_:
                yield m

    def __len__(self):
        return sum(1 for _ in self)

    def __repr__(self):
        cls = self.__class__
        if self._name_ is None:
            return "<%s: %r>" % (cls.__name__, self._value_)
        return "<%s.%s: %r>" % (cls.__name__, self._name_, self._value_)

    def __str__(self):
        cls = self.__class__
        if self._name_ is None:
            return '%s(%r)' % (cls.__name__, self._value_)
        return "%s.%s" % (cls.__name__, self._name_)

    def __bool__(self):
        return bool(self._value_)

    def __or__(self, other):
        if not isinstance(other, self.__class__):
            return NotImplemented
        return self.__class__(self._value_ | other._value_)

    def __and__(self, other):
        if not isinstance(other, self.__class__):
            return NotImplemented
        return self.__class__(self._value_ & other._value_)

    def __xor__(self, other):
        if not isinstance(other, self.__class__):
            return NotImplemented
        return self.__class__(self._value_ ^ other._value_)

    def __invert__(self):
        total = 0
        for m in self.__class__:
            total |= m._value_
        return self.__class__(total & ~self._value_)


class IntFlag(int, ReprEnum, Flag):
    def __str__(self):
        return int.__repr__(self._value_)

    def __format__(self, format_spec):
        return int.__format__(self._value_, format_spec)

    def __or__(self, other):
        if isinstance(other, int):
            other = other._value_ if isinstance(other, IntFlag) else other
            return self.__class__(self._value_ | other)
        return NotImplemented

    def __and__(self, other):
        if isinstance(other, int):
            other = other._value_ if isinstance(other, IntFlag) else other
            return self.__class__(self._value_ & other)
        return NotImplemented

    __ror__ = __or__
    __rand__ = __and__


def unique(enumeration):
    duplicates = []
    for name, member in enumeration.__members__.items():
        if name != member.name:
            duplicates.append((name, member.name))
    if duplicates:
        alias_details = ', '.join(["%s -> %s" % (alias, name) for (alias, name) in duplicates])
        raise ValueError('duplicate values found in %r: %s' % (enumeration, alias_details))
    return enumeration


def verify(*checks):
    def decorator(enumeration):
        return enumeration
    return decorator


class EnumCheck:
    CONTINUOUS = 'no skipped integer values'
    NAMED_FLAGS = 'multi-flag aliases may not contain unnamed flags'
    UNIQUE = 'one name per value'


CONTINUOUS = EnumCheck.CONTINUOUS
NAMED_FLAGS = EnumCheck.NAMED_FLAGS
UNIQUE = EnumCheck.UNIQUE
