"""os for the simulated interpreter: every call acts on the simulated machine's
filesystem and environment."""
import _os
import sys
import posixpath as path

sys.modules['os.path'] = path

name = 'posix'
sep = '/'
altsep = None
extsep = '.'
pathsep = ':'
linesep = '\n'
curdir = '.'
pardir = '..'
defpath = '/bin:/usr/bin'
devnull = '/dev/null'
F_OK = 0
R_OK = 4
W_OK = 2
X_OK = 1
SEEK_SET = 0
SEEK_CUR = 1
SEEK_END = 2
error = OSError


class _Environ(dict):
    def __repr__(self):
        return 'environ(' + dict.__repr__(self) + ')'

    def copy(self):
        return dict(self)


environ = _Environ(_os.environ)


def getenv(key, default=None):
    return environ.get(key, default)


def putenv(key, value):
    environ[key] = value


def unsetenv(key):
    environ.pop(key, None)


def fspath(p):
    if isinstance(p, (str, bytes)):
        return p
    f = getattr(type(p), '__fspath__', None)
    if f is None:
        raise TypeError('expected str, bytes or os.PathLike object, not ' + type(p).__name__)
    return f(p)


class PathLike:
    def __fspath__(self):
        raise NotImplementedError


getcwd = _os.getcwd


def getcwdb():
    return getcwd().encode()


def chdir(p):
    _os.chdir(fspath(p))


def listdir(p='.'):
    return _os.listdir(fspath(p))


def mkdir(p, mode=0o777, *, dir_fd=None):
    _os.mkdir(fspath(p))


def makedirs(name, mode=0o777, exist_ok=False):
    name = fspath(name)
    if path.isdir(name):
        if exist_ok:
            return
        raise FileExistsError(17, 'File exists', name)
    if path.exists(name):
        raise FileExistsError(17, 'File exists', name)
    head, tail = path.split(name)
    if not tail:
        head, tail = path.split(head)
    if head and tail and not path.exists(head):
        makedirs(head, exist_ok=True)
    _os.mkdir(name)


def remove(p, *, dir_fd=None):
    _os.remove(fspath(p))


unlink = remove


def rmdir(p, *, dir_fd=None):
    _os.rmdir(fspath(p))


def removedirs(name):
    rmdir(name)
    head, tail = path.split(name)
    if not tail:
        head, tail = path.split(head)
    while head and tail:
        try:
            rmdir(head)
        except OSError:
            break
        head, tail = path.split(head)


def rename(src, dst, *, src_dir_fd=None, dst_dir_fd=None):
    _os.rename(fspath(src), fspath(dst))


replace = rename


class stat_result(tuple):
    _fields = ('st_mode', 'st_ino', 'st_dev', 'st_nlink', 'st_uid', 'st_gid',
               'st_size', 'st_atime', 'st_mtime', 'st_ctime')

    def __new__(cls, values):
        return tuple.__new__(cls, values)

    st_mode = property(lambda s: s[0])
    st_ino = property(lambda s: s[1])
    st_dev = property(lambda s: s[2])
    st_nlink = property(lambda s: s[3])
    st_uid = property(lambda s: s[4])
    st_gid = property(lambda s: s[5])
    st_size = property(lambda s: s[6])
    st_atime = property(lambda s: s[7])
    st_mtime = property(lambda s: s[8])
    st_ctime = property(lambda s: s[9])
    st_atime_ns = property(lambda s: int(s[7] * 1e9))
    st_mtime_ns = property(lambda s: int(s[8] * 1e9))
    st_ctime_ns = property(lambda s: int(s[9] * 1e9))

    def __repr__(self):
        vals = [int(v) if i >= 7 else v for i, v in enumerate(self)]
        return 'os.stat_result(' + ', '.join(f'{f}={v!r}' for f, v in zip(self._fields, vals)) + ')'


def stat(p, *, dir_fd=None, follow_symlinks=True):
    return stat_result(_os.stat(fspath(p), follow_symlinks))


def lstat(p, *, dir_fd=None):
    return stat_result(_os.stat(fspath(p), False))


def access(p, mode, *, dir_fd=None, effective_ids=False, follow_symlinks=True):
    try:
        st = stat(p)
    except OSError:
        return False
    if mode == F_OK:
        return True
    bits = st.st_mode
    ok = True
    if mode & R_OK:
        ok = ok and bool(bits & 0o444)
    if mode & W_OK:
        ok = ok and bool(bits & 0o222)
    if mode & X_OK:
        ok = ok and bool(bits & 0o111)
    return ok


class DirEntry:
    def __init__(self, dirpath, name):
        self.name = name
        self.path = path.join(dirpath, name)

    def is_dir(self, *, follow_symlinks=True):
        return path.isdir(self.path)

    def is_file(self, *, follow_symlinks=True):
        return path.isfile(self.path)

    def is_symlink(self):
        return path.islink(self.path)

    def stat(self, *, follow_symlinks=True):
        return stat(self.path)

    def inode(self):
        return self.stat().st_ino

    def __fspath__(self):
        return self.path

    def __repr__(self):
        return f'<DirEntry {self.name!r}>'


class _ScandirIterator:
    def __init__(self, p):
        self._entries = [DirEntry(p, n) for n in listdir(p)]
        self._i = 0

    def __iter__(self):
        return self

    def __next__(self):
        if self._i >= len(self._entries):
            raise StopIteration
        e = self._entries[self._i]
        self._i += 1
        return e

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def close(self):
        pass


def scandir(p='.'):
    return _ScandirIterator(fspath(p))


def walk(top, topdown=True, onerror=None, followlinks=False):
    top = fspath(top)
    try:
        names = listdir(top)
    except OSError as err:
        if onerror is not None:
            onerror(err)
        return
    dirs, nondirs = [], []
    for n in names:
        if path.isdir(path.join(top, n)):
            dirs.append(n)
        else:
            nondirs.append(n)
    if topdown:
        yield top, dirs, nondirs
        for d in dirs:
            yield from walk(path.join(top, d), topdown, onerror, followlinks)
    else:
        for d in dirs:
            yield from walk(path.join(top, d), topdown, onerror, followlinks)
        yield top, dirs, nondirs


getpid = _os.getpid
getppid = _os.getppid
cpu_count = _os.cpu_count
urandom = _os.urandom


def getlogin():
    return _os.getlogin()


def getuid():
    return 1000


def geteuid():
    return 1000


def getgid():
    return 1000


def umask(mask):
    return 0o022


def isatty(fd):
    return False


def strerror(code):
    return {2: 'No such file or directory', 13: 'Permission denied', 17: 'File exists',
            20: 'Not a directory', 21: 'Is a directory', 22: 'Invalid argument',
            39: 'Directory not empty'}.get(code, f'Unknown error {code}')


class terminal_size(tuple):
    columns = property(lambda s: s[0])
    lines = property(lambda s: s[1])

    def __repr__(self):
        return f'os.terminal_size(columns={self[0]}, lines={self[1]})'


def get_terminal_size(fd=1):
    return terminal_size((80, 24))


class _Uname(tuple):
    sysname = property(lambda s: s[0])
    nodename = property(lambda s: s[1])
    release = property(lambda s: s[2])
    version = property(lambda s: s[3])
    machine = property(lambda s: s[4])


def uname():
    return _Uname(('Linux', _os.gethostname(), '6.8.0', '#1 SMP', 'x86_64'))


def get_exec_path(env=None):
    return (env or environ).get('PATH', defpath).split(pathsep)


def fsencode(filename):
    return fspath(filename).encode()


def fsdecode(filename):
    f = fspath(filename)
    return f.decode() if isinstance(f, bytes) else f


supports_follow_symlinks = set()


# ---------------------------------------------------------------- processes
# Children run through the machine's own shell (nested python3/node included);
# they finish before the call returns, so a "running" child is never observed.

def _wait_status(code):
    return (code & 0xff) << 8


def WIFEXITED(status):
    return (status & 0x7f) == 0


def WEXITSTATUS(status):
    return (status >> 8) & 0xff


def WIFSIGNALED(status):
    return ((status & 0x7f) + 1) >> 1 > 0 and (status & 0x7f) != 0


def WTERMSIG(status):
    return status & 0x7f


def waitstatus_to_exitcode(status):
    if WIFEXITED(status):
        return WEXITSTATUS(status)
    return -WTERMSIG(status)


def system(command):
    """Run `command` in the machine's shell; its output goes straight to ours."""
    import _cw
    out, err, code, _ = _cw.spawn(command, True, '', None, list(environ.items()))
    _cw.child_output(out, err)
    return _wait_status(code)


class _wrap_close:
    def __init__(self, stream, code):
        self._stream = stream
        self._code = code

    def close(self):
        self._stream.close()
        if self._code == 0:
            return None
        return _wait_status(self._code)

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def __getattr__(self, name):
        return getattr(self._stream, name)

    def __iter__(self):
        return iter(self._stream)


def popen(cmd, mode='r', buffering=-1):
    import io
    import _cw
    if not isinstance(cmd, str):
        raise TypeError('invalid cmd type (%s, expected string)' % type(cmd))
    if mode not in ('r', 'w'):
        raise ValueError('invalid mode %r' % mode)
    if mode == 'r':
        out, err, code, _ = _cw.spawn(cmd, True, '', None, list(environ.items()))
        _cw.child_output('', err)
        return _wrap_close(io.StringIO(out), code)

    class _Writer(io.StringIO):
        def close(self):
            if not self.closed:
                data = self.getvalue()
                out, err, code, _ = _cw.spawn(cmd, True, data, None, list(environ.items()))
                _cw.child_output(out, err)
                self._code = code
            io.StringIO.close(self)

    w = _Writer()
    w._code = 0

    class _WClose(_wrap_close):
        def close(self):
            self._stream.close()
            c = self._stream._code
            return None if c == 0 else _wait_status(c)

    return _WClose(w, 0)
