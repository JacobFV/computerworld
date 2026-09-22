"""base64 for the simulated interpreter."""

_B64 = b'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
_URLSAFE = b'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_'


class Error(ValueError):
    pass


def _bytes(s):
    if isinstance(s, str):
        try:
            return s.encode('ascii')
        except UnicodeEncodeError:
            raise ValueError('string argument should contain only ASCII characters')
    return bytes(s)


def b64encode(s, altchars=None):
    s = bytes(s)
    alphabet = _B64
    if altchars is not None:
        alphabet = _B64[:62] + bytes(altchars)
    out = bytearray()
    for i in range(0, len(s), 3):
        chunk = s[i:i + 3]
        n = int.from_bytes(chunk + b'\0' * (3 - len(chunk)), 'big')
        enc = [alphabet[(n >> sh) & 63] for sh in (18, 12, 6, 0)]
        if len(chunk) < 3:
            enc = enc[:len(chunk) + 1] + [61] * (3 - len(chunk))
        out.extend(enc)
    return bytes(out)


def b64decode(s, altchars=None, validate=False):
    s = _bytes(s)
    alphabet = _B64
    if altchars is not None:
        alphabet = _B64[:62] + bytes(altchars)
    table = {c: i for i, c in enumerate(alphabet)}
    vals = []
    pad = 0
    for c in s:
        if c == 61:
            pad += 1
            continue
        if c in table:
            if pad:
                raise Error('Incorrect padding')
            vals.append(table[c])
        elif validate:
            raise Error('Non-base64 digit found')
    if (len(vals) + pad) % 4 or len(vals) % 4 == 1:
        raise Error('Incorrect padding')
    out = bytearray()
    for i in range(0, len(vals), 4):
        chunk = vals[i:i + 4]
        n = 0
        for v in chunk:
            n = (n << 6) | v
        n <<= 6 * (4 - len(chunk))
        data = n.to_bytes(3, 'big')
        out.extend(data[:len(chunk) - 1] if len(chunk) < 4 else data)
    return bytes(out)


def standard_b64encode(s):
    return b64encode(s)


def standard_b64decode(s):
    return b64decode(s)


def urlsafe_b64encode(s):
    return b64encode(s, b'-_')


def urlsafe_b64decode(s):
    return b64decode(s, b'-_')


def b16encode(s):
    return bytes(s).hex().upper().encode()


def b16decode(s, casefold=False):
    s = _bytes(s)
    if casefold:
        s = s.upper()
    return bytes.fromhex(s.decode())


def b32encode(s):
    alphabet = b'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'
    s = bytes(s)
    out = bytearray()
    for i in range(0, len(s), 5):
        chunk = s[i:i + 5]
        n = int.from_bytes(chunk + b'\0' * (5 - len(chunk)), 'big')
        enc = [alphabet[(n >> (35 - 5 * k)) & 31] for k in range(8)]
        keep = {0: 0, 1: 2, 2: 4, 3: 5, 4: 7, 5: 8}[len(chunk)]
        out.extend(enc[:keep] + [61] * (8 - keep))
    return bytes(out)


def encodebytes(s):
    enc = b64encode(s)
    lines = [enc[i:i + 76] + b'\n' for i in range(0, len(enc), 76)]
    return b''.join(lines)


def decodebytes(s):
    return b64decode(s)
