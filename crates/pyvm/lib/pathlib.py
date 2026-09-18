"""pathlib (POSIX flavour) for the simulated interpreter."""
import os
import posixpath
import glob as _glob

__all__ = ['PurePath', 'PurePosixPath', 'Path', 'PosixPath', 'PureWindowsPath', 'WindowsPath']


class PurePath:
    def __new__(cls, *args):
        if cls is PurePath:
            cls = PurePosixPath
        self = object.__new__(cls)
        parts = []
        for a in args:
            if isinstance(a, PurePath):
                parts.append(a._str)
            else:
                a = os.fspath(a)
                if not isinstance(a, str):
                    raise TypeError("argument should be a str or an os.PathLike object "
                                    f"where __fspath__ returns a str, not {type(a).__name__!r}")
                parts.append(a)
        path = posixpath.join(*parts) if parts else ''
        self._str = self._normalize(path)
        return self

    @staticmethod
    def _normalize(path):
        if not path:
            return '.'
        root = '/' if path.startswith('/') else ''
        if path.startswith('//') and not path.startswith('///'):
            root = '//'
        comps = [c for c in path.split('/') if c and c != '.']
        s = root + '/'.join(comps)
        return s or '.'

    def __str__(self):
        return self._str

    def __fspath__(self):
        return self._str

    def as_posix(self):
        return self._str

    def __repr__(self):
        return f"{type(self).__name__}({self._str!r})"

    def __bytes__(self):
        return self._str.encode()

    def __hash__(self):
        return hash(self._str)

    def __eq__(self, other):
        if not isinstance(other, PurePath):
            return NotImplemented
        return self._str == other._str

    def __lt__(self, other):
        if not isinstance(other, PurePath):
            return NotImplemented
        return self.parts < other.parts

    def __le__(self, other):
        return self == other or self < other

    def __gt__(self, other):
        if not isinstance(other, PurePath):
            return NotImplemented
        return self.parts > other.parts

    def __ge__(self, other):
        return self == other or self > other

    def __truediv__(self, key):
        try:
            return type(self)(self, key)
        except TypeError:
            return NotImplemented

    def __rtruediv__(self, key):
        try:
            return type(self)(key, self)
        except TypeError:
            return NotImplemented

    @property
    def parts(self):
        if self._str == '.':
            return ()
        if self._str.startswith('/'):
            return ('/',) + tuple(p for p in self._str.split('/') if p)
        return tuple(self._str.split('/'))

    @property
    def drive(self):
        return ''

    @property
    def root(self):
        return '/' if self._str.startswith('/') else ''

    @property
    def anchor(self):
        return self.root

    @property
    def name(self):
        if self._str in ('.', '/'):
            return ''
        return self._str.rsplit('/', 1)[-1]

    @property
    def suffix(self):
        name = self.name
        i = name.rfind('.')
        if 0 < i < len(name) - 1:
            return name[i:]
        return ''

    @property
    def suffixes(self):
        name = self.name
        if name.endswith('.'):
            return []
        name = name.lstrip('.')
        return ['.' + s for s in name.split('.')[1:]]

    @property
    def stem(self):
        name = self.name
        i = name.rfind('.')
        if 0 < i < len(name) - 1:
            return name[:i]
        return name

    @property
    def parent(self):
        s = self._str
        if s in ('.', '/'):
            return self
        head = s.rsplit('/', 1)[0] if '/' in s else '.'
        if s.startswith('/') and head == '':
            head = '/'
        return type(self)(head)

    @property
    def parents(self):
        out = []
        p = self
        while True:
            q = p.parent
            if q == p:
                break
            out.append(q)
            p = q
        return tuple(out)

    def is_absolute(self):
        return self._str.startswith('/')

    def is_relative_to(self, other):
        try:
            self.relative_to(other)
            return True
        except ValueError:
            return False

    def relative_to(self, other, *_deprecated, walk_up=False):
        other = type(self)(other)
        a, b = self.parts, other.parts
        if a[:len(b)] != b:
            raise ValueError(f"{str(self)!r} is not in the subpath of {str(other)!r}")
        return type(self)(*a[len(b):]) if len(a) > len(b) else type(self)('.')

    def joinpath(self, *other):
        return type(self)(self, *other)

    def with_name(self, name):
        if not self.name:
            raise ValueError(f"{self!r} has an empty name")
        return self.parent / name

    def with_stem(self, stem):
        return self.with_name(stem + self.suffix)

    def with_suffix(self, suffix):
        if suffix and not suffix.startswith('.') or suffix == '.':
            raise ValueError(f"Invalid suffix {suffix!r}")
        return self.with_name(self.stem + suffix)

    def match(self, pattern):
        pat_parts = [p for p in pattern.split('/') if p]
        parts = [p for p in self.parts if p != '/']
        if pattern.startswith('/'):
            if len(pat_parts) != len(parts):
                return False
        elif len(pat_parts) > len(parts):
            return False
        for part, pat in zip(reversed(parts), reversed(pat_parts)):
            if not _glob.fnmatch(part, pat):
                return False
        return True


class PurePosixPath(PurePath):
    pass


class PureWindowsPath(PurePath):
    pass


class Path(PurePath):
    def __new__(cls, *args, **kwargs):
        if cls is Path:
            cls = PosixPath
        return PurePath.__new__(cls, *args)

    @classmethod
    def cwd(cls):
        return cls(os.getcwd())

    @classmethod
    def home(cls):
        return cls(os.path.expanduser('~'))

    def absolute(self):
        if self.is_absolute():
            return self
        return type(self)(os.getcwd(), self)

    def resolve(self, strict=False):
        return type(self)(os.path.abspath(self._str))

    def expanduser(self):
        return type(self)(os.path.expanduser(self._str))

    def stat(self, *, follow_symlinks=True):
        return os.stat(self._str)

    def exists(self, *, follow_symlinks=True):
        return os.path.exists(self._str)

    def is_file(self):
        return os.path.isfile(self._str)

    def is_dir(self):
        return os.path.isdir(self._str)

    def is_symlink(self):
        return os.path.islink(self._str)

    def iterdir(self):
        for name in os.listdir(self._str):
            yield self / name

    def glob(self, pattern):
        base = self._str
        for p in _glob.glob(os.path.join(base, pattern), recursive='**' in pattern):
            yield type(self)(p)

    def rglob(self, pattern):
        return self.glob('**/' + pattern)

    def open(self, mode='r', buffering=-1, encoding=None, errors=None, newline=None):
        return open(self._str, mode, encoding=encoding)

    def read_text(self, encoding=None, errors=None):
        with open(self._str, encoding=encoding) as f:
            return f.read()

    def read_bytes(self):
        with open(self._str, 'rb') as f:
            return f.read()

    def write_text(self, data, encoding=None, errors=None, newline=None):
        if not isinstance(data, str):
            raise TypeError('data must be str, not %s' % data.__class__.__name__)
        with open(self._str, 'w', encoding=encoding) as f:
            return f.write(data)

    def write_bytes(self, data):
        with open(self._str, 'wb') as f:
            return f.write(bytes(data))

    def touch(self, mode=0o666, exist_ok=True):
        if self.exists():
            if not exist_ok:
                raise FileExistsError(17, 'File exists', self._str)
            return
        with open(self._str, 'a'):
            pass

    def mkdir(self, mode=0o777, parents=False, exist_ok=False):
        if parents:
            os.makedirs(self._str, exist_ok=exist_ok)
            return
        try:
            os.mkdir(self._str)
        except FileExistsError:
            if not exist_ok or not self.is_dir():
                raise

    def unlink(self, missing_ok=False):
        try:
            os.remove(self._str)
        except FileNotFoundError:
            if not missing_ok:
                raise

    def rmdir(self):
        os.rmdir(self._str)

    def rename(self, target):
        os.rename(self._str, str(target))
        return type(self)(target)

    def replace(self, target):
        return self.rename(target)

    def samefile(self, other_path):
        return os.path.samefile(self._str, str(other_path))


class PosixPath(Path, PurePosixPath):
    pass


class WindowsPath(Path, PureWindowsPath):
    pass
