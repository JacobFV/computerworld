"""string for the simulated interpreter."""
import re as _re

__all__ = ["ascii_letters", "ascii_lowercase", "ascii_uppercase", "capwords",
           "digits", "hexdigits", "octdigits", "printable", "punctuation",
           "whitespace", "Formatter", "Template"]

whitespace = ' \t\n\r\x0b\x0c'
ascii_lowercase = 'abcdefghijklmnopqrstuvwxyz'
ascii_uppercase = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
ascii_letters = ascii_lowercase + ascii_uppercase
digits = '0123456789'
hexdigits = digits + 'abcdef' + 'ABCDEF'
octdigits = '01234567'
punctuation = r"""!"#$%&'()*+,-./:;<=>?@[\]^_`{|}~"""
printable = digits + ascii_letters + punctuation + whitespace


def capwords(s, sep=None):
    return (sep or ' ').join(map(str.capitalize, s.split(sep)))


class Template:
    delimiter = '$'
    idpattern = r'(?a:[_a-z][_a-z0-9]*)'
    braceidpattern = None
    flags = _re.IGNORECASE

    def __init__(self, template):
        self.template = template
        delim = _re.escape(self.delimiter)
        id = self.idpattern
        bid = self.braceidpattern or self.idpattern
        pattern = fr"""
            {delim}(?:
              (?P<escaped>{delim})  |
              (?P<named>{id})       |
              {{(?P<braced>{bid})}} |
              (?P<invalid>)
            )
            """
        self.pattern = _re.compile(pattern, self.flags | _re.VERBOSE)

    def _invalid(self, mo):
        i = mo.start('invalid')
        lines = self.template[:i].splitlines(keepends=True)
        if not lines:
            colno = 1
            lineno = 1
        else:
            colno = i - len(''.join(lines[:-1]))
            lineno = len(lines)
        raise ValueError('Invalid placeholder in string: line %d, col %d' % (lineno, colno))

    def substitute(self, mapping=None, /, **kws):
        if mapping is None:
            mapping = kws
        elif kws:
            mapping = {**mapping, **kws}

        def convert(mo):
            named = mo.group('named') or mo.group('braced')
            if named is not None:
                return str(mapping[named])
            if mo.group('escaped') is not None:
                return self.delimiter
            if mo.group('invalid') is not None:
                self._invalid(mo)
            raise ValueError('Unrecognized named group in pattern', self.pattern)
        return self.pattern.sub(convert, self.template)

    def safe_substitute(self, mapping=None, /, **kws):
        if mapping is None:
            mapping = kws
        elif kws:
            mapping = {**mapping, **kws}

        def convert(mo):
            named = mo.group('named') or mo.group('braced')
            if named is not None:
                try:
                    return str(mapping[named])
                except KeyError:
                    return mo.group()
            if mo.group('escaped') is not None:
                return self.delimiter
            if mo.group('invalid') is not None:
                return mo.group()
            raise ValueError('Unrecognized named group in pattern', self.pattern)
        return self.pattern.sub(convert, self.template)

    def get_identifiers(self):
        ids = []
        for mo in self.pattern.finditer(self.template):
            named = mo.group('named') or mo.group('braced')
            if named is not None and named not in ids:
                ids.append(named)
        return ids

    def is_valid(self):
        for mo in self.pattern.finditer(self.template):
            if mo.group('invalid') is not None:
                return False
        return True


class Formatter:
    def format(self, format_string, /, *args, **kwargs):
        return self.vformat(format_string, args, kwargs)

    def vformat(self, format_string, args, kwargs):
        return format_string.format(*args, **kwargs)
