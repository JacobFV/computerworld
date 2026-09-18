"""glob and fnmatch-style matching for the simulated interpreter."""
import os
import re

__all__ = ['glob', 'iglob', 'escape', 'has_magic']

magic_check = re.compile('([*?[])')


def has_magic(s):
    return magic_check.search(s) is not None


def translate(pat):
    i, n = 0, len(pat)
    res = ''
    while i < n:
        c = pat[i]
        i += 1
        if c == '*':
            res += '.*'
        elif c == '?':
            res += '.'
        elif c == '[':
            j = i
            if j < n and pat[j] == '!':
                j += 1
            if j < n and pat[j] == ']':
                j += 1
            while j < n and pat[j] != ']':
                j += 1
            if j >= n:
                res += '\\['
            else:
                stuff = pat[i:j].replace('\\', '\\\\')
                i = j + 1
                if stuff[0] == '!':
                    stuff = '^' + stuff[1:]
                res += '[' + stuff + ']'
        else:
            res += re.escape(c)
    return '(?s:' + res + r')\Z'


def fnmatch(name, pat):
    return re.match(translate(pat), name) is not None


def _glob_in_dir(dirname, pattern, dironly):
    try:
        names = os.listdir(dirname or '.')
    except OSError:
        return []
    if not pattern.startswith('.'):
        names = [n for n in names if not n.startswith('.')]
    out = [n for n in names if fnmatch(n, pattern)]
    if dironly:
        out = [n for n in out if os.path.isdir(os.path.join(dirname, n))]
    return out


def _rlistdir(dirname):
    try:
        names = os.listdir(dirname or '.')
    except OSError:
        return
    for x in names:
        if x.startswith('.'):
            continue
        yield x
        path = os.path.join(dirname, x) if dirname else x
        if os.path.isdir(path):
            for y in _rlistdir(path):
                yield os.path.join(x, y)


def iglob(pathname, *, root_dir=None, recursive=False, include_hidden=False):
    return iter(glob(pathname, root_dir=root_dir, recursive=recursive))


def glob(pathname, *, root_dir=None, dir_fd=None, recursive=False, include_hidden=False):
    return list(_iglob(pathname, recursive))


def _iglob(pathname, recursive):
    dirname, basename = os.path.split(pathname)
    if not has_magic(pathname):
        if basename:
            if os.path.exists(pathname):
                yield pathname
        elif os.path.isdir(dirname):
            yield pathname
        return
    if not dirname:
        if recursive and basename == '**':
            yield from sorted(_rlistdir(''))
        else:
            yield from sorted(_glob_in_dir('', basename, False))
        return
    if dirname != pathname and has_magic(dirname):
        dirs = list(_iglob(dirname, recursive))
    else:
        dirs = [dirname]
    for d in dirs:
        if recursive and basename == '**':
            names = [''] + sorted(_rlistdir(d))
        elif has_magic(basename):
            names = sorted(_glob_in_dir(d, basename, False))
        else:
            names = [basename] if os.path.exists(os.path.join(d, basename)) else []
        for name in names:
            yield os.path.join(d, name)


def escape(pathname):
    return re.sub(r'([*?[])', r'[\1]', pathname)
