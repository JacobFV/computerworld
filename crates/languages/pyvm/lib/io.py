"""io for the simulated interpreter: in-memory streams; open() is the builtin."""
import builtins as _builtins

open = _builtins.open
SEEK_SET = 0
SEEK_CUR = 1
SEEK_END = 2
DEFAULT_BUFFER_SIZE = 8192
TextIOWrapper = type(_builtins.open.__call__) if False else None


class UnsupportedOperation(OSError, ValueError):
    pass


class IOBase:
    def __enter__(self):
        self._checkClosed()
        return self

    def __exit__(self, *args):
        self.close()

    def __iter__(self):
        self._checkClosed()
        return self

    def __next__(self):
        line = self.readline()
        if not line:
            raise StopIteration
        return line

    def close(self):
        self.closed = True

    def _checkClosed(self):
        if getattr(self, 'closed', False):
            raise ValueError("I/O operation on closed file.")

    def readable(self):
        return True

    def writable(self):
        return True

    def seekable(self):
        return True

    def isatty(self):
        return False

    def flush(self):
        self._checkClosed()

    def readlines(self, hint=-1):
        lines = []
        for line in self:
            lines.append(line)
        return lines

    def writelines(self, lines):
        for line in lines:
            self.write(line)


class StringIO(IOBase):
    def __init__(self, initial_value='', newline='\n'):
        if initial_value is None:
            initial_value = ''
        if not isinstance(initial_value, str):
            raise TypeError(f"initial_value must be str or None, not {type(initial_value).__name__}")
        self._buf = initial_value
        self._pos = 0
        self.closed = False

    def getvalue(self):
        self._checkClosed()
        return self._buf

    def write(self, s):
        self._checkClosed()
        if not isinstance(s, str):
            raise TypeError(f"string argument expected, got '{type(s).__name__}'")
        if self._pos == len(self._buf):
            self._buf += s
        else:
            if self._pos > len(self._buf):
                self._buf += '\0' * (self._pos - len(self._buf))
            self._buf = self._buf[:self._pos] + s + self._buf[self._pos + len(s):]
        self._pos += len(s)
        return len(s)

    def read(self, size=-1):
        self._checkClosed()
        if size is None or size < 0:
            data = self._buf[self._pos:]
        else:
            data = self._buf[self._pos:self._pos + size]
        self._pos += len(data)
        return data

    def readline(self, size=-1):
        self._checkClosed()
        end = self._buf.find('\n', self._pos)
        end = len(self._buf) if end < 0 else end + 1
        if size is not None and size >= 0:
            end = min(end, self._pos + size)
        data = self._buf[self._pos:end]
        self._pos = end
        return data

    def seek(self, pos, whence=0):
        self._checkClosed()
        if whence == 1:
            pos += self._pos
        elif whence == 2:
            pos += len(self._buf)
        self._pos = max(0, pos)
        return self._pos

    def tell(self):
        self._checkClosed()
        return self._pos

    def truncate(self, size=None):
        if size is None:
            size = self._pos
        self._buf = self._buf[:size]
        return size

    def __repr__(self):
        return '<_io.StringIO object at 0x7f0000001000>'


class BytesIO(IOBase):
    def __init__(self, initial_bytes=b''):
        self._buf = bytearray(initial_bytes or b'')
        self._pos = 0
        self.closed = False

    def getvalue(self):
        self._checkClosed()
        return bytes(self._buf)

    def getbuffer(self):
        return self._buf

    def write(self, b):
        self._checkClosed()
        b = bytes(b)
        end = self._pos + len(b)
        if self._pos > len(self._buf):
            self._buf.extend(b'\0' * (self._pos - len(self._buf)))
        self._buf = self._buf[:self._pos] + bytearray(b) + self._buf[end:]
        self._pos = end
        return len(b)

    def read(self, size=-1):
        self._checkClosed()
        if size is None or size < 0:
            data = self._buf[self._pos:]
        else:
            data = self._buf[self._pos:self._pos + size]
        self._pos += len(data)
        return bytes(data)

    def readline(self, size=-1):
        self._checkClosed()
        end = self._buf.find(b'\n', self._pos) if hasattr(self._buf, 'find') else -1
        if end < 0:
            rest = bytes(self._buf[self._pos:])
            i = rest.find(b'\n')
            end = len(self._buf) if i < 0 else self._pos + i + 1
        else:
            end += 1
        data = bytes(self._buf[self._pos:end])
        self._pos = end
        return data

    def seek(self, pos, whence=0):
        if whence == 1:
            pos += self._pos
        elif whence == 2:
            pos += len(self._buf)
        self._pos = max(0, pos)
        return self._pos

    def tell(self):
        return self._pos


class TextIOWrapper(IOBase):
    def __init__(self, buffer, encoding=None, errors=None, newline=None, line_buffering=False):
        self.buffer = buffer
        self.encoding = encoding or 'utf-8'
        self.closed = False

    def read(self, size=-1):
        return self.buffer.read(size).decode(self.encoding)

    def readline(self, size=-1):
        return self.buffer.readline(size).decode(self.encoding)

    def write(self, s):
        self.buffer.write(s.encode(self.encoding))
        return len(s)


BufferedReader = BytesIO
BufferedWriter = BytesIO
RawIOBase = IOBase
BufferedIOBase = IOBase
TextIOBase = IOBase
