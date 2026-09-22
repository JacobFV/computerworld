"""shutil for the simulated interpreter (file copies over the simulated FS)."""
import os
import _os


class Error(OSError):
    pass


class SameFileError(Error):
    pass


def copyfileobj(fsrc, fdst, length=0):
    while True:
        buf = fsrc.read(length or 65536)
        if not buf:
            break
        fdst.write(buf)


def copyfile(src, dst, *, follow_symlinks=True):
    if os.path.abspath(src) == os.path.abspath(dst):
        raise SameFileError(f"{src!r} and {dst!r} are the same file")
    with open(src, 'rb') as fsrc, open(dst, 'wb') as fdst:
        fdst.write(fsrc.read())
    return dst


def copymode(src, dst, *, follow_symlinks=True):
    pass


def copystat(src, dst, *, follow_symlinks=True):
    pass


def copy(src, dst, *, follow_symlinks=True):
    if os.path.isdir(dst):
        dst = os.path.join(dst, os.path.basename(src))
    copyfile(src, dst)
    return dst


copy2 = copy


def copytree(src, dst, symlinks=False, ignore=None, copy_function=copy2,
             ignore_dangling_symlinks=False, dirs_exist_ok=False):
    names = os.listdir(src)
    ignored = ignore(src, names) if ignore is not None else set()
    os.makedirs(dst, exist_ok=dirs_exist_ok)
    for name in names:
        if name in ignored:
            continue
        s = os.path.join(src, name)
        d = os.path.join(dst, name)
        if os.path.isdir(s):
            copytree(s, d, symlinks, ignore, copy_function, dirs_exist_ok=dirs_exist_ok)
        else:
            copy_function(s, d)
    return dst


def rmtree(path, ignore_errors=False, onerror=None, *, onexc=None, dir_fd=None):
    try:
        _os.rmtree(os.fspath(path))
    except OSError:
        if not ignore_errors:
            raise


def move(src, dst, copy_function=copy2):
    if os.path.isdir(dst):
        dst = os.path.join(dst, os.path.basename(src))
    os.rename(src, dst)
    return dst


def which(cmd, mode=None, path=None):
    path = path or os.environ.get('PATH', os.defpath)
    for d in path.split(os.pathsep):
        p = os.path.join(d, cmd)
        if os.path.isfile(p):
            return p
    return None


def disk_usage(path):
    class _usage(tuple):
        total = property(lambda s: s[0])
        used = property(lambda s: s[1])
        free = property(lambda s: s[2])
    return _usage((68719476736, 0, 68719476736))


def get_terminal_size(fallback=(80, 24)):
    return os.get_terminal_size()
