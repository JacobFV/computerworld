"""locale: the conventions of the locales this machine has.

`setlocale` answers from a table recorded from a real glibc, so `localeconv`,
`currency`, `format_string` and the names `time.strftime` uses are what CPython
would give on such a machine. A locale the table does not have raises
`locale.Error`, as CPython does for one that is not installed.
"""
import _locale

# The module defines its own `str`, as CPython's does; this keeps the builtin.
_builtin_str = str

__all__ = ['Error', 'setlocale', 'getlocale', 'getdefaultlocale', 'getpreferredencoding',
           'localeconv', 'currency', 'format_string', 'str', 'atof', 'atoi', 'delocalize',
           'localize', 'normalize', 'resetlocale', 'strcoll', 'strxfrm', 'nl_langinfo',
           'LC_ALL', 'LC_COLLATE', 'LC_CTYPE', 'LC_MESSAGES', 'LC_MONETARY',
           'LC_NUMERIC', 'LC_TIME', 'CHAR_MAX']

LC_CTYPE = 0
LC_NUMERIC = 1
LC_TIME = 2
LC_COLLATE = 3
LC_MONETARY = 4
LC_MESSAGES = 5
LC_ALL = 6
CHAR_MAX = 127

_CATEGORIES = (LC_CTYPE, LC_NUMERIC, LC_TIME, LC_COLLATE, LC_MONETARY, LC_MESSAGES)
_NAMES = {
    LC_CTYPE: 'LC_CTYPE', LC_NUMERIC: 'LC_NUMERIC', LC_TIME: 'LC_TIME',
    LC_COLLATE: 'LC_COLLATE', LC_MONETARY: 'LC_MONETARY', LC_MESSAGES: 'LC_MESSAGES',
    LC_ALL: 'LC_ALL',
}

# What glibc leaves unset in the C locale: its own `CHAR_MAX`, which is 255
# where `char` is unsigned (`locale.CHAR_MAX` is Python's own 127).
_UNSET = 255
_C_CONV = {
    'int_curr_symbol': '', 'currency_symbol': '', 'mon_decimal_point': '',
    'mon_thousands_sep': '', 'mon_grouping': [], 'positive_sign': '',
    'negative_sign': '', 'int_frac_digits': _UNSET, 'frac_digits': _UNSET,
    'p_cs_precedes': _UNSET, 'p_sep_by_space': _UNSET,
    'n_cs_precedes': _UNSET, 'n_sep_by_space': _UNSET,
    'p_sign_posn': _UNSET, 'n_sign_posn': _UNSET,
    'decimal_point': '.', 'thousands_sep': '', 'grouping': [],
}


class Error(Exception):
    """A locale this machine does not have."""


# The locale of each category; `C` until a program sets one.
_current = {c: 'C' for c in _CATEGORIES}


def _resolve(name):
    """The recorded locale `name` refers to, or None for the C locale."""
    if name in (None, '', 'C', 'POSIX', 'C.UTF-8'):
        return None
    data = _locale.data(name)
    if data is None:
        raise Error('unsupported locale setting')
    return data


def setlocale(category=LC_ALL, locale=None):
    if locale is None:
        return _current[LC_CTYPE] if category == LC_ALL else _current[category]
    if not isinstance(locale, _builtin_str):
        locale = normalize('.'.join(p for p in locale if p))
    data = _resolve(locale)
    name = 'C' if data is None else data['name']
    if category == LC_ALL:
        for c in _CATEGORIES:
            _current[c] = name
    else:
        if category not in _CATEGORIES:
            raise Error('invalid locale category')
        _current[category] = name
    if category in (LC_ALL, LC_TIME):
        _locale.set_time_locale(name)
    return name


def resetlocale(category=LC_ALL):
    setlocale(category, 'C')


def getlocale(category=LC_CTYPE):
    name = _current[LC_CTYPE] if category == LC_ALL else _current[category]
    if name in ('C', 'POSIX'):
        return (None, None)
    language, _, encoding = name.partition('.')
    return (language, encoding or 'UTF-8')


def getdefaultlocale(envvars=('LC_ALL', 'LC_CTYPE', 'LANG', 'LANGUAGE')):
    import os
    for var in envvars:
        value = os.environ.get(var)
        if value:
            try:
                data = _resolve(value)
            except Error:
                continue
            if data is None:
                return (None, None)
            language, _, encoding = data['name'].partition('.')
            return (language, encoding or 'UTF-8')
    return (None, 'UTF-8')


def getpreferredencoding(do_setlocale=True):
    return 'UTF-8'


def _data(category):
    return _locale.data(_current[category])


def localeconv():
    data = _data(LC_NUMERIC)
    monetary = _data(LC_MONETARY)
    out = dict(_C_CONV)
    if data is not None:
        for key in ('decimal_point', 'thousands_sep', 'grouping'):
            out[key] = data['conv'][key]
    if monetary is not None:
        for key, value in monetary['conv'].items():
            if key in ('decimal_point', 'thousands_sep', 'grouping'):
                continue
            out[key] = value
    return out


def nl_langinfo(key):
    data = _data(LC_TIME)
    if isinstance(key, _builtin_str):
        name = key
    else:
        name = _LANGINFO.get(key)
    if data is None:
        return _C_LANGINFO.get(name, '')
    if name in ('D_T_FMT', 'D_FMT', 'T_FMT', 'T_FMT_AMPM', 'CODESET'):
        return data[name.lower()]
    if name and name.startswith('DAY_'):
        return data['days'][int(name[4:]) - 1]
    if name and name.startswith('ABDAY_'):
        return data['abdays'][int(name[6:]) - 1]
    if name and name.startswith('MON_'):
        return data['months'][int(name[4:]) - 1]
    if name and name.startswith('ABMON_'):
        return data['abmonths'][int(name[6:]) - 1]
    if name == 'AM_STR':
        return data['am_pm'][0]
    if name == 'PM_STR':
        return data['am_pm'][1]
    if name == 'RADIXCHAR':
        return localeconv()['decimal_point']
    if name == 'THOUSEP':
        return localeconv()['thousands_sep']
    if name == 'CRNCYSTR':
        return localeconv()['currency_symbol']
    return ''


_LANGINFO = {}
_C_LANGINFO = {
    'D_T_FMT': '%a %b %e %H:%M:%S %Y', 'D_FMT': '%m/%d/%y', 'T_FMT': '%H:%M:%S',
    'T_FMT_AMPM': '%I:%M:%S %p', 'AM_STR': 'AM', 'PM_STR': 'PM', 'CODESET': 'UTF-8',
    'RADIXCHAR': '.', 'THOUSEP': '',
}
_NEXT_KEY = 100
for _i, _name in enumerate(
        ['CODESET', 'D_T_FMT', 'D_FMT', 'T_FMT', 'T_FMT_AMPM', 'AM_STR', 'PM_STR',
         'DAY_1', 'DAY_2', 'DAY_3', 'DAY_4', 'DAY_5', 'DAY_6', 'DAY_7',
         'ABDAY_1', 'ABDAY_2', 'ABDAY_3', 'ABDAY_4', 'ABDAY_5', 'ABDAY_6', 'ABDAY_7',
         'MON_1', 'MON_2', 'MON_3', 'MON_4', 'MON_5', 'MON_6', 'MON_7', 'MON_8',
         'MON_9', 'MON_10', 'MON_11', 'MON_12',
         'ABMON_1', 'ABMON_2', 'ABMON_3', 'ABMON_4', 'ABMON_5', 'ABMON_6', 'ABMON_7',
         'ABMON_8', 'ABMON_9', 'ABMON_10', 'ABMON_11', 'ABMON_12',
         'RADIXCHAR', 'THOUSEP', 'CRNCYSTR']):
    _LANGINFO[_NEXT_KEY + _i] = _name
    globals()[_name] = _NEXT_KEY + _i
    __all__.append(_name)


def _group(text, monetary=False):
    """Inserts the locale's thousands separators into a digit string."""
    conv = localeconv()
    grouping = conv['mon_grouping' if monetary else 'grouping']
    separator = conv['mon_thousands_sep' if monetary else 'thousands_sep']
    if not grouping or not separator:
        return text, 0
    result = ''
    seen = 0
    sizes = list(grouping)
    size = sizes.pop(0) if sizes else 0
    while text and size:
        if size == CHAR_MAX:
            break
        if len(text) <= size:
            break
        result = separator + text[-size:] + result
        text = text[:-size]
        seen += 1
        if sizes:
            nxt = sizes.pop(0)
            if nxt == 0:
                sizes = [size]
            else:
                size = nxt
    return text + result, seen * len(separator)


def format_string(f, val, grouping=False, monetary=False):
    """`%`-formats with the locale's separators, as CPython's does."""
    import re as _re
    percent = _re.compile(r'%(?:\((?P<key>.*?)\))?'
                          r'(?P<modifiers>[-#0-9 +*.hlL]*?)[eEfFgGdiouxXcrsa%]')

    def repl(match):
        spec = match.group(0)
        if spec[-1] == '%':
            return '%'
        if isinstance(val, dict):
            piece = spec % val
        else:
            piece = spec % repl.values[repl.index]
            repl.index += 1
        return _localize_piece(piece, spec, grouping, monetary)

    repl.values = val if isinstance(val, tuple) else (val,)
    repl.index = 0
    return percent.sub(repl, f)


def _localize_piece(piece, spec, grouping, monetary):
    conv = localeconv()
    if spec[-1] in 'eEfFgG':
        # The C locale has no monetary decimal point at all, and says so.
        point = conv['mon_decimal_point' if monetary else 'decimal_point']
        piece = piece.replace('.', point)
    if grouping and spec[-1] in 'diufFeEgG':
        sign = ''
        body = piece
        stripped = body.lstrip()
        pad = body[:len(body) - len(stripped)]
        body = stripped
        if body[:1] in '+-':
            sign, body = body[0], body[1:]
        point = conv['mon_decimal_point' if monetary else 'decimal_point'] or '.'
        head, sep, tail = body.partition(point)
        head, _ = _group(head, monetary)
        piece = pad + sign + head + sep + tail
    return piece


def currency(val, symbol=True, grouping=False, international=False):
    conv = localeconv()
    digits = conv['int_frac_digits' if international else 'frac_digits']
    if digits == CHAR_MAX:
        digits = 2
    s = format_string('%%.%if' % digits, abs(val), grouping, monetary=True)
    s = '<' + s + '>'
    if symbol:
        smb = conv['int_curr_symbol' if international else 'currency_symbol']
        precedes = conv['n_cs_precedes' if val < 0 else 'p_cs_precedes']
        separated = conv['n_sep_by_space' if val < 0 else 'p_sep_by_space']
        if precedes:
            s = smb + (' ' if separated else '') + s
        else:
            if international and smb[-1] == ' ':
                smb = smb[:-1]
            s = s + (' ' if separated else '') + smb
    sign_pos = conv['n_sign_posn' if val < 0 else 'p_sign_posn']
    sign = conv['negative_sign'] if val < 0 else conv['positive_sign']
    if sign_pos == 0:
        s = '(' + s + ')'
    elif sign_pos == 1:
        s = sign + s
    elif sign_pos == 2:
        s = s + sign
    elif sign_pos == 3:
        s = s.replace('<', sign, 1)
    elif sign_pos == 4:
        s = s.replace('<', sign, 1)
    else:
        s = sign + s
    return s.replace('<', '').replace('>', '')


def str(val):
    return format_string('%.12g', val)


def delocalize(string):
    conv = localeconv()
    ts = conv['thousands_sep']
    if ts:
        string = string.replace(ts, '')
    dd = conv['decimal_point']
    if dd:
        string = string.replace(dd, '.')
    return string


def localize(string, grouping=False, monetary=False):
    return _localize_piece(string, '%s', grouping, monetary)


def atof(string, func=float):
    return func(delocalize(string))


def atoi(string):
    return int(delocalize(string))


def strcoll(a, b):
    return (a > b) - (a < b)


def strxfrm(s):
    return s


_ALIASES = {
    'en': 'en_US.UTF-8', 'de': 'de_DE.UTF-8', 'fr': 'fr_FR.UTF-8', 'es': 'es_ES.UTF-8',
    'it': 'it_IT.UTF-8', 'pt': 'pt_BR.UTF-8', 'ja': 'ja_JP.UTF-8', 'zh': 'zh_CN.UTF-8',
    'ko': 'ko_KR.UTF-8', 'ru': 'ru_RU.UTF-8', 'ar': 'ar_EG.UTF-8', 'hi': 'hi_IN.UTF-8',
}


def normalize(localename):
    name = localename.strip()
    if not name:
        return name
    lowered = name.lower().replace('-', '_')
    base, _, encoding = lowered.partition('.')
    if base in _ALIASES and not encoding:
        return _ALIASES[base]
    data = _locale.data(name)
    if data is not None:
        return data['name']
    if '.' not in name and '_' in name:
        return name + '.ISO8859-1'
    return name
