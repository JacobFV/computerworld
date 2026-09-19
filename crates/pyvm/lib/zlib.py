"""zlib for the simulated interpreter: a port of the zlib library CPython links,
so compressed bytes match CPython's exactly."""
import _zlib

error = _zlib.error

MAX_WBITS = 15
DEFLATED = 8
DEF_MEM_LEVEL = 8
DEF_BUF_SIZE = 16384
Z_NO_COMPRESSION = 0
Z_BEST_SPEED = 1
Z_BEST_COMPRESSION = 9
Z_DEFAULT_COMPRESSION = -1
Z_FILTERED = 1
Z_HUFFMAN_ONLY = 2
Z_RLE = 3
Z_FIXED = 4
Z_DEFAULT_STRATEGY = 0
Z_NO_FLUSH = 0
Z_PARTIAL_FLUSH = 1
Z_SYNC_FLUSH = 2
Z_FULL_FLUSH = 3
Z_FINISH = 4
Z_BLOCK = 5
Z_TREES = 6
ZLIB_VERSION = '1.3'
ZLIB_RUNTIME_VERSION = '1.3'


def _data(data):
    if isinstance(data, (bytes, bytearray)):
        return data
    if hasattr(data, 'tobytes'):
        return data.tobytes()
    return data


def compress(data, /, level=Z_DEFAULT_COMPRESSION, wbits=MAX_WBITS):
    return _zlib.compress(_data(data), level, wbits)


def decompress(data, /, wbits=MAX_WBITS, bufsize=DEF_BUF_SIZE):
    if bufsize < 0:
        raise ValueError('bufsize must be non-negative')
    return _zlib.decompress(_data(data), wbits, bufsize)


def crc32(data, value=0, /):
    return _zlib.crc32(_data(data), value & 0xFFFFFFFF)


def adler32(data, value=1, /):
    return _zlib.adler32(_data(data), value & 0xFFFFFFFF)


class Compress:
    def __init__(self, handle):
        self._h = handle
        self._finished = False

    def compress(self, data, /):
        if self._finished:
            raise error('Error -2 while compressing data: inconsistent stream state')
        return _zlib.c_compress(self._h, _data(data))

    def flush(self, mode=Z_FINISH, /):
        if self._finished and mode != Z_NO_FLUSH:
            raise error('Error -2 while flushing: inconsistent stream state')
        out = _zlib.c_flush(self._h, mode)
        if mode == Z_FINISH:
            self._finished = True
        return out

    def copy(self):
        c = Compress(_zlib.c_copy(self._h))
        c._finished = self._finished
        return c

    __copy__ = copy

    def __deepcopy__(self, memo):
        return self.copy()


Compress.__module__ = 'zlib'


def compressobj(level=Z_DEFAULT_COMPRESSION, method=DEFLATED, wbits=MAX_WBITS,
                memLevel=DEF_MEM_LEVEL, strategy=Z_DEFAULT_STRATEGY, zdict=None):
    if zdict is not None and not isinstance(zdict, (bytes, bytearray)) \
            and not hasattr(zdict, 'tobytes'):
        raise TypeError('zdict argument must support the buffer protocol')
    return Compress(_zlib.compressobj(level, method, wbits, memLevel, strategy,
                                      None if zdict is None else _data(zdict)))


class Decompress:
    def __init__(self, handle):
        self._h = handle
        self.unused_data = b''
        self.unconsumed_tail = b''
        self.eof = False

    def decompress(self, data, /, max_length=0):
        if max_length < 0:
            raise ValueError('max_length must be non-negative')
        data = _data(data)
        if self.eof:
            # zlib reports the end again without reading: CPython files the
            # input as unused, and (as its bookkeeping goes) as the tail too
            # when a tail was pending.
            self.unused_data += data
            if self.unconsumed_tail:
                self.unconsumed_tail = bytes(data)
            return b''
        pending_tail = bool(self.unconsumed_tail)
        out, eof, unused, tail = _zlib.d_decompress(self._h, data, max_length)
        self.eof = eof
        if eof:
            self.unused_data += unused
            self.unconsumed_tail = unused if pending_tail else b''
        else:
            self.unconsumed_tail = tail
        return out

    def flush(self, length=DEF_BUF_SIZE, /):
        if length <= 0:
            raise ValueError('length must be greater than zero')
        if self.eof:
            return b''
        out, eof, unused, tail = _zlib.d_decompress(self._h, self.unconsumed_tail, 0)
        self.eof = eof
        if eof:
            self.unused_data += unused
        self.unconsumed_tail = b''
        return out

    def copy(self):
        d = Decompress(_zlib.d_copy(self._h))
        d.unused_data = self.unused_data
        d.unconsumed_tail = self.unconsumed_tail
        d.eof = self.eof
        return d

    __copy__ = copy

    def __deepcopy__(self, memo):
        return self.copy()


Decompress.__module__ = 'zlib'


def decompressobj(wbits=MAX_WBITS, zdict=b''):
    return Decompress(_zlib.decompressobj(wbits, _data(zdict) if zdict else None))
