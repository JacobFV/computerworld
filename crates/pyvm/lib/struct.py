"""struct: interpret bytes as packed binary data."""
import _struct

error = _struct.error
calcsize = _struct.calcsize
pack = _struct.pack

__all__ = ['calcsize', 'pack', 'pack_into', 'unpack', 'unpack_from', 'iter_unpack',
           'Struct', 'error']


def unpack(format, buffer, /):
    if not isinstance(buffer, (bytes, bytearray)):
        buffer = bytes(buffer)
    return _struct.unpack_from(format, buffer, 0, True)


def unpack_from(format, /, buffer, offset=0):
    if not isinstance(buffer, (bytes, bytearray)):
        buffer = bytes(buffer)
    return _struct.unpack_from(format, buffer, offset)


def pack_into(format, buffer, offset, /, *v):
    data = _struct.pack(format, *v)
    if offset < 0:
        offset += len(buffer)
    if offset + len(data) > len(buffer):
        raise error('pack_into requires a buffer of at least %d bytes for packing %d bytes '
                    'at offset %d (actual buffer size is %d)'
                    % (offset + len(data), len(data), offset, len(buffer)))
    buffer[offset:offset + len(data)] = data


def iter_unpack(format, buffer, /):
    size = calcsize(format)
    if size == 0:
        raise error('cannot iteratively unpack with a struct of length 0')
    if len(buffer) % size:
        raise error('iterative unpacking requires a buffer of a multiple of %d bytes' % size)
    return (unpack_from(format, buffer, i) for i in range(0, len(buffer), size))


class Struct:
    def __init__(self, format):
        if isinstance(format, bytes):
            format = format.decode('ascii')
        self.format = format
        self.size = calcsize(format)

    def pack(self, *v):
        return pack(self.format, *v)

    def pack_into(self, buffer, offset, *v):
        return pack_into(self.format, buffer, offset, *v)

    def unpack(self, buffer):
        return unpack(self.format, buffer)

    def unpack_from(self, buffer, offset=0):
        return unpack_from(self.format, buffer, offset)

    def iter_unpack(self, buffer):
        return iter_unpack(self.format, buffer)

    def __repr__(self):
        return 'Struct(%r)' % (self.format,)
