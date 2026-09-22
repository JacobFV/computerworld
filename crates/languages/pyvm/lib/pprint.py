"""pprint for the simulated interpreter (CPython's layout algorithm)."""
import sys as _sys

__all__ = ["pprint", "pformat", "isreadable", "isrecursive", "saferepr", "PrettyPrinter", "pp"]


def pprint(object, stream=None, indent=1, width=80, depth=None, *, compact=False, sort_dicts=True, underscore_numbers=False):
    printer = PrettyPrinter(stream=stream, indent=indent, width=width, depth=depth,
                            compact=compact, sort_dicts=sort_dicts)
    printer.pprint(object)


def pformat(object, indent=1, width=80, depth=None, *, compact=False, sort_dicts=True, underscore_numbers=False):
    return PrettyPrinter(indent=indent, width=width, depth=depth, compact=compact,
                         sort_dicts=sort_dicts).pformat(object)


def pp(object, *args, sort_dicts=False, **kwargs):
    pprint(object, *args, sort_dicts=sort_dicts, **kwargs)


def saferepr(object):
    return PrettyPrinter()._repr(object, {}, 0)


def isreadable(object):
    return True


def isrecursive(object):
    return False


class _safe_key:
    def __init__(self, obj):
        self.obj = obj

    def __lt__(self, other):
        try:
            return self.obj < other.obj
        except TypeError:
            return ((str(type(self.obj)), id(self.obj)) < (str(type(other.obj)), id(other.obj)))


def _safe_tuple(t):
    return _safe_key(t[0]), _safe_key(t[1])


class _Stream:
    def __init__(self):
        self.parts = []

    def write(self, s):
        self.parts.append(s)

    def getvalue(self):
        return ''.join(self.parts)


class PrettyPrinter:
    def __init__(self, indent=1, width=80, depth=None, stream=None, *, compact=False, sort_dicts=True, underscore_numbers=False):
        indent = int(indent)
        width = int(width)
        if indent < 0:
            raise ValueError('indent must be >= 0')
        if depth is not None and depth <= 0:
            raise ValueError('depth must be > 0')
        if not width:
            raise ValueError('width must be != 0')
        self._depth = depth
        self._indent_per_level = indent
        self._width = width
        self._stream = stream
        self._compact = bool(compact)
        self._sort_dicts = sort_dicts

    def pprint(self, object):
        s = _Stream()
        self._format(object, s, 0, 0, {}, 0)
        s.write("\n")
        out = self._stream if self._stream is not None else _sys.stdout
        out.write(s.getvalue())

    def pformat(self, object):
        s = _Stream()
        self._format(object, s, 0, 0, {}, 0)
        return s.getvalue()

    def _format(self, object, stream, indent, allowance, context, level):
        objid = id(object)
        if objid in context:
            stream.write('<Recursion on %s with id=%s>' % (type(object).__name__, objid))
            return
        rep = self._repr(object, context, level)
        max_width = self._width - indent - allowance
        if len(rep) > max_width:
            p = None
            t = type(object)
            if issubclass(t, dict) and t.__repr__ is dict.__repr__:
                p = PrettyPrinter._pprint_dict
            elif issubclass(t, list) and t.__repr__ is list.__repr__:
                p = PrettyPrinter._pprint_list
            elif issubclass(t, tuple) and t.__repr__ is tuple.__repr__:
                p = PrettyPrinter._pprint_tuple
            elif issubclass(t, (set, frozenset)) and t.__repr__ in (set.__repr__, frozenset.__repr__):
                p = PrettyPrinter._pprint_set
            elif t is str:
                p = PrettyPrinter._pprint_str
            if p is not None:
                context[objid] = 1
                p(self, object, stream, indent, allowance, context, level + 1)
                del context[objid]
                return
        stream.write(rep)

    def _pprint_dict(self, object, stream, indent, allowance, context, level):
        write = stream.write
        write('{')
        if self._indent_per_level > 1:
            write((self._indent_per_level - 1) * ' ')
        if len(object):
            if self._sort_dicts:
                items = sorted(object.items(), key=_safe_tuple)
            else:
                items = list(object.items())
            self._format_dict_items(items, stream, indent, allowance + 1, context, level)
        write('}')

    def _format_dict_items(self, items, stream, indent, allowance, context, level):
        write = stream.write
        indent += self._indent_per_level
        delimnl = ',\n' + ' ' * indent
        last_index = len(items) - 1
        for i, (key, ent) in enumerate(items):
            last = i == last_index
            rep = self._repr(key, context, level)
            write(rep)
            write(': ')
            self._format(ent, stream, indent + len(rep) + 2, allowance if last else 1, context, level)
            if not last:
                write(delimnl)

    def _pprint_list(self, object, stream, indent, allowance, context, level):
        stream.write('[')
        self._format_items(object, stream, indent, allowance + 1, context, level)
        stream.write(']')

    def _pprint_tuple(self, object, stream, indent, allowance, context, level):
        stream.write('(')
        endchar = ',)' if len(object) == 1 else ')'
        self._format_items(object, stream, indent, allowance + len(endchar), context, level)
        stream.write(endchar)

    def _pprint_set(self, object, stream, indent, allowance, context, level):
        if not len(object):
            stream.write(repr(object))
            return
        typ = object.__class__
        if typ is set:
            stream.write('{')
            endchar = '}'
        else:
            stream.write(typ.__name__ + '({')
            endchar = '})'
            indent += len(typ.__name__) + 1
        object = sorted(object, key=_safe_key)
        self._format_items(object, stream, indent, allowance + len(endchar), context, level)
        stream.write(endchar)

    def _pprint_str(self, object, stream, indent, allowance, context, level):
        write = stream.write
        if not len(object):
            write(repr(object))
            return
        chunks = []
        lines = object.splitlines(True)
        if level == 1:
            indent += 1
            allowance += 1
        max_width1 = max_width = self._width - indent
        for i, line in enumerate(lines):
            rep = repr(line)
            if i == len(lines) - 1:
                max_width1 -= allowance
            if len(rep) <= max_width1:
                chunks.append(rep)
            else:
                import re
                parts = re.findall(r'\S*\s*', line)
                assert parts
                assert not parts[-1]
                parts.pop()
                max_width2 = max_width
                current = ''
                for j, part in enumerate(parts):
                    candidate = current + part
                    if j == len(parts) - 1 and i == len(lines) - 1:
                        max_width2 -= allowance
                    if len(repr(candidate)) > max_width2:
                        if current:
                            chunks.append(repr(current))
                        current = part
                    else:
                        current = candidate
                if current:
                    chunks.append(repr(current))
        if len(chunks) == 1:
            write(rep)
            return
        if level == 1:
            write('(')
        for i, rep in enumerate(chunks):
            if i > 0:
                write('\n' + ' ' * indent)
            write(rep)
        if level == 1:
            write(')')

    def _format_items(self, items, stream, indent, allowance, context, level):
        write = stream.write
        indent += self._indent_per_level
        if self._indent_per_level > 1:
            write((self._indent_per_level - 1) * ' ')
        delimnl = ',\n' + ' ' * indent
        delim = ''
        width = max_width = self._width - indent + 1
        it = iter(items)
        try:
            next_ent = next(it)
        except StopIteration:
            return
        last = False
        while not last:
            ent = next_ent
            try:
                next_ent = next(it)
            except StopIteration:
                last = True
                max_width -= allowance
                width -= allowance
            if self._compact:
                rep = self._repr(ent, context, level)
                w = len(rep) + 2
                if width < w:
                    width = max_width
                    if delim:
                        delim = delimnl
                if width >= w:
                    width -= w
                    write(delim)
                    delim = ', '
                    write(rep)
                    continue
            write(delim)
            delim = delimnl
            self._format(ent, stream, indent, allowance if last else 1, context, level)

    def _repr(self, object, context, level):
        t = type(object)
        if self._depth is not None and level > self._depth and isinstance(object, (dict, list, tuple)) and object:
            return {dict: '{...}', list: '[...]', tuple: '(...)'}.get(t, '...')
        if issubclass(t, dict) and t.__repr__ is dict.__repr__:
            if not object:
                return '{}'
            items = sorted(object.items(), key=_safe_tuple) if self._sort_dicts else object.items()
            return '{' + ', '.join(self._repr(k, context, level + 1) + ': ' + self._repr(v, context, level + 1)
                                   for k, v in items) + '}'
        if issubclass(t, list) and t.__repr__ is list.__repr__:
            return '[' + ', '.join(self._repr(v, context, level + 1) for v in object) + ']'
        if issubclass(t, tuple) and t.__repr__ is tuple.__repr__:
            if len(object) == 1:
                return '(' + self._repr(object[0], context, level + 1) + ',)'
            return '(' + ', '.join(self._repr(v, context, level + 1) for v in object) + ')'
        return repr(object)
