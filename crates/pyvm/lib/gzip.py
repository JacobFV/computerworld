"""gzip for the simulated interpreter (CPython 3.12's format and behaviour)."""
import builtins
import io
import os
import struct
import time
import zlib

__all__ = ["BadGzipFile", "GzipFile", "open", "compress", "decompress"]

FTEXT, FHCRC, FEXTRA, FNAME, FCOMMENT = 1, 2, 4, 8, 16
READ, WRITE = 'rb', 'wb'
_COMPRESS_LEVEL_FAST = 1
_COMPRESS_LEVEL_TRADEOFF = 6
_COMPRESS_LEVEL_BEST = 9
READ_BUFFER_SIZE = 128 * 1024


class BadGzipFile(OSError):
    """Exception raised in some cases for invalid gzip files."""


def open(filename, mode="rb", compresslevel=_COMPRESS_LEVEL_BEST,
         encoding=None, errors=None, newline=None):
    if "t" in mode:
        if "b" in mode:
            raise ValueError("Invalid mode: %r" % (mode,))
    else:
        if encoding is not None:
            raise ValueError("Argument 'encoding' not supported in binary mode")
        if errors is not None:
            raise ValueError("Argument 'errors' not supported in binary mode")
        if newline is not None:
            raise ValueError("Argument 'newline' not supported in binary mode")
    gz_mode = mode.replace("t", "")
    if isinstance(filename, (str, bytes, os.PathLike)):
        binary_file = GzipFile(filename, gz_mode, compresslevel)
    elif hasattr(filename, "read") or hasattr(filename, "write"):
        binary_file = GzipFile(None, gz_mode, compresslevel, filename)
    else:
        raise TypeError("filename must be a str or bytes object, or a file")
    if "t" in mode:
        return _TextWrapper(binary_file, encoding or 'utf-8', errors or 'strict', newline)
    return binary_file


class _TextWrapper:
    def __init__(self, raw, encoding, errors, newline):
        self._raw = raw
        self._enc = encoding
        self._err = errors
        self._buf = None

    def _load(self):
        if self._buf is None:
            self._buf = io.StringIO(self._raw.read().decode(self._enc, self._err))
        return self._buf

    def read(self, n=-1):
        return self._load().read(n)

    def readline(self, *a):
        return self._load().readline(*a)

    def readlines(self):
        return self._load().readlines()

    def __iter__(self):
        return iter(self._load())

    def write(self, s):
        self._raw.write(s.encode(self._enc, self._err))
        return len(s)

    def close(self):
        self._raw.close()

    def __enter__(self):
        return self

    def __exit__(self, *a):
        self.close()


def write32u(output, value):
    output.write(struct.pack("<L", value))


class GzipFile:
    myfileobj = None

    def __init__(self, filename=None, mode=None, compresslevel=_COMPRESS_LEVEL_BEST,
                 fileobj=None, mtime=None):
        if mode and ('t' in mode or 'U' in mode):
            raise ValueError("Invalid mode: {!r}".format(mode))
        if mode and 'b' not in mode:
            mode += 'b'
        if fileobj is None:
            fileobj = self.myfileobj = builtins.open(filename, mode or 'rb')
        if filename is None:
            filename = getattr(fileobj, 'name', '')
            if not isinstance(filename, (str, bytes)):
                filename = ''
        else:
            filename = os.fspath(filename)
        origmode = mode
        if mode is None:
            mode = getattr(fileobj, 'mode', 'rb')
        if mode.startswith('r'):
            self.mode = READ
            self._data = _read_members(fileobj.read())
            self._pos = 0
            self.name = filename
        elif mode.startswith(('w', 'a', 'x')):
            if origmode is None:
                import warnings
                warnings.warn("GzipFile was opened for writing, but this will "
                              "change in future Python releases.  "
                              "Specify the mode argument for opening it for writing.",
                              FutureWarning, 2)
            self.mode = WRITE
            self._init_write(filename)
            self.compress = zlib.compressobj(compresslevel, zlib.DEFLATED, -zlib.MAX_WBITS,
                                             zlib.DEF_MEM_LEVEL, 0)
            self._write_mtime = mtime
            self._compresslevel = compresslevel
        else:
            raise ValueError("Invalid mode: {!r}".format(mode))
        self.fileobj = fileobj
        if self.mode == WRITE:
            self._write_gzip_header(compresslevel)

    @property
    def mtime(self):
        return getattr(self, '_last_mtime', None)

    def __repr__(self):
        s = repr(self.fileobj)
        return '<gzip ' + s[1:-1] + ' ' + hex(id(self)) + '>'

    def _init_write(self, filename):
        self.name = filename
        self.crc = zlib.crc32(b"")
        self.size = 0
        self.writebuf = []
        self.bufsize = 0
        self.offset = 0

    def _write_gzip_header(self, compresslevel):
        self.fileobj.write(b'\037\213')
        self.fileobj.write(b'\010')
        try:
            fname = os.path.basename(self.name)
            if not isinstance(fname, bytes):
                fname = fname.encode('latin-1')
            if fname.endswith(b'.gz'):
                fname = fname[:-3]
        except UnicodeEncodeError:
            fname = b''
        flags = 0
        if fname:
            flags = FNAME
        self.fileobj.write(chr(flags).encode('latin-1'))
        mtime = self._write_mtime
        if mtime is None:
            mtime = time.time()
        write32u(self.fileobj, int(mtime))
        if compresslevel == _COMPRESS_LEVEL_BEST:
            xfl = b'\002'
        elif compresslevel == _COMPRESS_LEVEL_FAST:
            xfl = b'\004'
        else:
            xfl = b'\000'
        self.fileobj.write(xfl)
        self.fileobj.write(b'\377')
        if fname:
            self.fileobj.write(fname + b'\000')

    def write(self, data):
        self._check_not_closed()
        if self.mode != WRITE:
            import errno
            raise OSError(errno.EBADF, "write() on read-only GzipFile object")
        if not isinstance(data, (bytes, bytearray)):
            data = data.tobytes() if hasattr(data, 'tobytes') else bytes(data)
        length = len(data)
        if length > 0:
            self.fileobj.write(self.compress.compress(data))
            self.size += length
            self.crc = zlib.crc32(data, self.crc)
            self.offset += length
        return length

    def _check_not_closed(self):
        if self.closed:
            raise ValueError("I/O operation on closed file.")

    def read(self, size=-1):
        self._check_not_closed()
        if self.mode != READ:
            import errno
            raise OSError(errno.EBADF, "read() on write-only GzipFile object")
        if size is None or size < 0:
            out = self._data[self._pos:]
        else:
            out = self._data[self._pos:self._pos + size]
        self._pos += len(out)
        return out

    def read1(self, size=-1):
        return self.read(size)

    def peek(self, n):
        return self._data[self._pos:self._pos + max(n, 1)]

    def readline(self, size=-1):
        i = self._data.find(b'\n', self._pos)
        end = len(self._data) if i < 0 else i + 1
        if size is not None and size >= 0:
            end = min(end, self._pos + size)
        out = self._data[self._pos:end]
        self._pos = end
        return out

    def readlines(self, hint=-1):
        out = []
        while True:
            line = self.readline()
            if not line:
                return out
            out.append(line)

    def __iter__(self):
        return self

    def __next__(self):
        line = self.readline()
        if not line:
            raise StopIteration
        return line

    @property
    def closed(self):
        return self.fileobj is None

    def close(self):
        fileobj = self.fileobj
        if fileobj is None:
            return
        self.fileobj = None
        try:
            if self.mode == WRITE:
                fileobj.write(self.compress.flush())
                write32u(fileobj, self.crc)
                write32u(fileobj, self.size & 0xffffffff)
        finally:
            myfileobj = self.myfileobj
            if myfileobj:
                self.myfileobj = None
                myfileobj.close()

    def flush(self, zlib_mode=zlib.Z_SYNC_FLUSH):
        self._check_not_closed()
        if self.mode == WRITE:
            self.fileobj.write(self.compress.flush(zlib_mode))
            self.fileobj.flush()

    def fileno(self):
        return self.fileobj.fileno()

    def rewind(self):
        if self.mode != READ:
            raise OSError("Can't rewind in write mode")
        self._pos = 0

    def readable(self):
        return self.mode == READ

    def writable(self):
        return self.mode == WRITE

    def seekable(self):
        return True

    def tell(self):
        return self._pos if self.mode == READ else self.offset

    def seek(self, offset, whence=io.SEEK_SET if hasattr(io, 'SEEK_SET') else 0):
        if self.mode == WRITE:
            raise OSError('Seek from end not supported')
        if whence == 1:
            offset = self._pos + offset
        elif whence == 2:
            offset = len(self._data) + offset
        self._pos = max(0, offset)
        return self._pos

    def __enter__(self):
        return self

    def __exit__(self, *a):
        self.close()


def _read_exact(fp, n):
    data = fp.read(n)
    while len(data) < n:
        b = fp.read(n - len(data))
        if not b:
            raise EOFError("Compressed file ended before the end-of-stream marker was reached")
        data += b
    return data


def _read_gzip_header(fp):
    magic = fp.read(2)
    if magic == b'':
        return None
    if magic != b'\037\213':
        raise BadGzipFile('Not a gzipped file (%r)' % magic)
    (method, flag, last_mtime) = struct.unpack("<BBIxx", _read_exact(fp, 8))
    if method != 8:
        raise BadGzipFile('Unknown compression method')
    if flag & FEXTRA:
        extra_len, = struct.unpack("<H", _read_exact(fp, 2))
        _read_exact(fp, extra_len)
    if flag & FNAME:
        while True:
            s = fp.read(1)
            if not s or s == b'\000':
                break
    if flag & FCOMMENT:
        while True:
            s = fp.read(1)
            if not s or s == b'\000':
                break
    if flag & FHCRC:
        _read_exact(fp, 2)
    return last_mtime


def _read_members(data):
    fp = io.BytesIO(data)
    out = []
    while True:
        pos = fp.tell()
        rest = data[pos:]
        if not rest.lstrip(b'\x00'):
            break
        if _read_gzip_header(fp) is None:
            break
        d = zlib.decompressobj(-zlib.MAX_WBITS)
        body = data[fp.tell():]
        chunk = d.decompress(body)
        if not d.eof:
            raise EOFError("Compressed file ended before the end-of-stream marker was reached")
        out.append(chunk)
        trailer = d.unused_data
        if len(trailer) < 8:
            raise EOFError("Compressed file ended before the end-of-stream marker was reached")
        crc32, isize = struct.unpack("<II", trailer[:8])
        if crc32 != zlib.crc32(chunk):
            raise BadGzipFile("CRC check failed %s != %s" % (hex(crc32), hex(zlib.crc32(chunk))))
        if isize != (len(chunk) & 0xffffffff):
            raise BadGzipFile("Incorrect length of data produced")
        fp = io.BytesIO(trailer[8:])
        data = trailer[8:]
    return b''.join(out)


def compress(data, compresslevel=_COMPRESS_LEVEL_BEST, *, mtime=None):
    if mtime == 0:
        return zlib.compress(data, level=compresslevel, wbits=31)
    if mtime is None:
        mtime = time.time()
    header = struct.pack("<BBBBLBB", 0x1f, 0x8b, 8, 0, int(mtime),
                         2 if compresslevel == _COMPRESS_LEVEL_BEST else
                         4 if compresslevel == _COMPRESS_LEVEL_FAST else 0, 255)
    trailer = struct.pack("<LL", zlib.crc32(data), (len(data) & 0xffffffff))
    return header + zlib.compress(data, level=compresslevel, wbits=-15) + trailer


def decompress(data):
    return _read_members(bytes(data))
