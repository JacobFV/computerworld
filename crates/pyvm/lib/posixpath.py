"""os.path (POSIX semantics) for the simulated interpreter."""
import _os

sep = '/'
curdir = '.'
pardir = '..'
extsep = '.'
pathsep = ':'
defpath = '/bin:/usr/bin'
altsep = None
devnull = '/dev/null'


def _fspath(p):
    if isinstance(p, str):
        return p
    if isinstance(p, bytes):
        return p.decode()
    f = getattr(type(p), '__fspath__', None)
    if f is None:
        raise TypeError('expected str, bytes or os.PathLike object, not ' + type(p).__name__)
    return f(p)


def normcase(s):
    return _fspath(s)


def isabs(s):
    return _fspath(s).startswith('/')


def join(a, *p):
    a = _fspath(a)
    path = a
    for b in map(_fspath, p):
        if b.startswith('/'):
            path = b
        elif not path or path.endswith('/'):
            path += b
        else:
            path += '/' + b
    return path


def split(p):
    p = _fspath(p)
    i = p.rfind('/') + 1
    head, tail = p[:i], p[i:]
    if head and head != '/' * len(head):
        head = head.rstrip('/')
    return head, tail


def splitext(p):
    p = _fspath(p)
    sep_index = p.rfind('/')
    dot_index = p.rfind('.')
    if dot_index > sep_index:
        filename_index = sep_index + 1
        while filename_index < dot_index:
            if p[filename_index] != '.':
                return p[:dot_index], p[dot_index:]
            filename_index += 1
    return p, ''


def splitdrive(p):
    return '', _fspath(p)


def basename(p):
    p = _fspath(p)
    i = p.rfind('/') + 1
    return p[i:]


def dirname(p):
    p = _fspath(p)
    i = p.rfind('/') + 1
    head = p[:i]
    if head and head != '/' * len(head):
        head = head.rstrip('/')
    return head


def normpath(path):
    path = _fspath(path)
    if not path:
        return '.'
    initial_slashes = path.startswith('/')
    if initial_slashes and path.startswith('//') and not path.startswith('///'):
        initial_slashes = 2
    comps = path.split('/')
    new_comps = []
    for comp in comps:
        if comp in ('', '.'):
            continue
        if (comp != '..' or (not initial_slashes and not new_comps) or
                (new_comps and new_comps[-1] == '..')):
            new_comps.append(comp)
        elif new_comps:
            new_comps.pop()
    comps = new_comps
    path = '/'.join(comps)
    if initial_slashes:
        path = '/' * initial_slashes + path
    return path or '.'


def abspath(path):
    path = _fspath(path)
    if not isabs(path):
        path = join(_os.getcwd(), path)
    return normpath(path)


def realpath(path, *, strict=False):
    return abspath(path)


def relpath(path, start=None):
    if not path:
        raise ValueError("no path specified")
    start = curdir if start is None else _fspath(start)
    start_list = [x for x in abspath(start).split('/') if x]
    path_list = [x for x in abspath(path).split('/') if x]
    i = len(commonprefix([start_list, path_list]))
    rel_list = [pardir] * (len(start_list) - i) + path_list[i:]
    if not rel_list:
        return curdir
    return join(*rel_list)


def commonprefix(m):
    if not m:
        return ''
    s1 = min(m)
    s2 = max(m)
    for i, c in enumerate(s1):
        if c != s2[i]:
            return s1[:i]
    return s1


def commonpath(paths):
    if not paths:
        raise ValueError('commonpath() arg is an empty sequence')
    paths = [_fspath(p) for p in paths]
    split_paths = [[c for c in p.split('/') if c and c != '.'] for p in paths]
    isabs_ = paths[0].startswith('/')
    s1 = min(split_paths)
    s2 = max(split_paths)
    common = s1
    for i, c in enumerate(s1):
        if c != s2[i]:
            common = s1[:i]
            break
    prefix = '/' if isabs_ else ''
    return prefix + '/'.join(common)


def expanduser(path):
    path = _fspath(path)
    if not path.startswith('~'):
        return path
    i = path.find('/', 1)
    if i < 0:
        i = len(path)
    if i == 1:
        home = _os.environ.get('HOME', '/home/' + _os.user)
    else:
        home = '/home/' + path[1:i]
    return (home.rstrip('/') or '/') + path[i:]


def expandvars(path):
    import re
    path = _fspath(path)
    if '$' not in path:
        return path

    def repl(m):
        name = m.group(1) or m.group(2)
        return _os.environ.get(name, m.group(0))
    return re.sub(r'\$(\w+)|\$\{([^}]*)\}', repl, path)


def exists(path):
    try:
        return _os.exists(_fspath(path))
    except (OSError, ValueError, TypeError):
        return False


lexists = exists


def _mode(path, follow=True):
    try:
        return _os.stat(_fspath(path), follow)[0]
    except (OSError, ValueError):
        return None


def isdir(path):
    m = _mode(path)
    return m is not None and (m & 0o170000) == 0o040000


def isfile(path):
    m = _mode(path)
    return m is not None and (m & 0o170000) == 0o100000


def islink(path):
    m = _mode(path, False)
    return m is not None and (m & 0o170000) == 0o120000


def ismount(path):
    return abspath(path) == '/'


def getsize(filename):
    return _os.stat(_fspath(filename), True)[6]


def getmtime(filename):
    return _os.stat(_fspath(filename), True)[8]


def getatime(filename):
    return _os.stat(_fspath(filename), True)[7]


def getctime(filename):
    return _os.stat(_fspath(filename), True)[9]


def samefile(f1, f2):
    return _os.stat(_fspath(f1), True)[1] == _os.stat(_fspath(f2), True)[1]
