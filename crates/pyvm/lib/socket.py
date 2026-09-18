"""socket for the simulated interpreter.

Name resolution is the world's DNS. A TCP connection to another machine is
checked by the world (DNS, routes, a listening service) and then carries HTTP:
every simulated service speaks HTTP, so bytes written to the socket are parsed
as HTTP/1.x requests, each complete request is sent through the world's network,
and the response comes back on the socket as HTTP/1.1 bytes. Anything that is
not HTTP gets `400 Bad Request` and the connection is closed, as a web server
would do. Sockets bound and listening inside this program accept connections
from this same program (loopback), which with threads makes an in-process
client/server work. Blocking waits use the deterministic thread scheduler.
"""
import _cw
import os
import sys

AF_UNSPEC = 0
AF_UNIX = 1
AF_INET = 2
AF_INET6 = 10
SOCK_STREAM = 1
SOCK_DGRAM = 2
SOCK_RAW = 3
SOCK_NONBLOCK = 2048
SOCK_CLOEXEC = 524288
IPPROTO_IP = 0
IPPROTO_TCP = 6
IPPROTO_UDP = 17
IPPROTO_IPV6 = 41
SOL_SOCKET = 1
SOL_TCP = 6
SO_REUSEADDR = 2
SO_KEEPALIVE = 9
SO_BROADCAST = 6
SO_LINGER = 13
SO_RCVBUF = 8
SO_SNDBUF = 7
SO_REUSEPORT = 15
SO_ERROR = 4
SO_TYPE = 3
TCP_NODELAY = 1
SHUT_RD = 0
SHUT_WR = 1
SHUT_RDWR = 2
SOMAXCONN = 4096
AI_PASSIVE = 1
AI_CANONNAME = 2
AI_NUMERICHOST = 4
AI_NUMERICSERV = 1024
NI_NUMERICHOST = 1
NI_NUMERICSERV = 2
NI_NAMEREQD = 8
INADDR_ANY = 0
INADDR_LOOPBACK = 0x7f000001
has_ipv6 = True
MSG_PEEK = 2
MSG_DONTWAIT = 64


class AddressFamily(int):
    _names = {0: 'AF_UNSPEC', 1: 'AF_UNIX', 2: 'AF_INET', 10: 'AF_INET6'}

    def __repr__(self):
        return '<AddressFamily.%s: %d>' % (self._names.get(int(self), '?'), int(self))

    def __str__(self):
        return 'AddressFamily.' + self._names.get(int(self), '?')

    @property
    def name(self):
        return self._names.get(int(self), '?')


class SocketKind(int):
    _names = {1: 'SOCK_STREAM', 2: 'SOCK_DGRAM', 3: 'SOCK_RAW'}

    def __repr__(self):
        return '<SocketKind.%s: %d>' % (self._names.get(int(self), '?'), int(self))

    def __str__(self):
        return 'SocketKind.' + self._names.get(int(self), '?')

    @property
    def name(self):
        return self._names.get(int(self), '?')


error = OSError


class herror(OSError):
    pass


class gaierror(OSError):
    pass


timeout = TimeoutError

_GLOBAL_DEFAULT_TIMEOUT = object()
_default_timeout = None


def getdefaulttimeout():
    return _default_timeout


def setdefaulttimeout(t):
    global _default_timeout
    if t is not None:
        t = float(t)
        if t < 0:
            raise ValueError('Timeout value out of range')
    _default_timeout = t


def gethostname():
    return os.uname().nodename


def getfqdn(name=''):
    name = name.strip()
    if not name or name == '0.0.0.0':
        name = gethostname()
    return name


def _is_ipv4(s):
    parts = s.split('.')
    if len(parts) != 4:
        return False
    for p in parts:
        if not p.isdigit() or int(p) > 255:
            return False
    return True


def _resolve(host):
    if host is None or host == '':
        return ['0.0.0.0']
    if isinstance(host, bytes):
        host = host.decode()
    if _is_ipv4(host):
        return [host]
    if host == '<broadcast>':
        return ['255.255.255.255']
    try:
        return _cw.resolve(host)
    except OSError as e:
        if e.errno == -2 or e.errno is None:
            raise gaierror(-2, 'Name or service not known') from None
        if e.errno == 101:
            raise gaierror(-3, 'Temporary failure in name resolution') from None
        raise


def gethostbyname(hostname):
    return _resolve(hostname)[0]


def gethostbyname_ex(hostname):
    addrs = _resolve(hostname)
    return (hostname, [], list(addrs))


def gethostbyaddr(ip):
    if ip in ('127.0.0.1', 'localhost'):
        return ('localhost', [], ['127.0.0.1'])
    raise herror(1, 'Unknown host')


def getaddrinfo(host, port, family=0, type=0, proto=0, flags=0):
    if isinstance(port, str):
        if port.isdigit():
            port = int(port)
        else:
            port = getservbyname(port)
    elif port is None:
        port = 0
    if host is None:
        addrs = ['0.0.0.0' if flags & AI_PASSIVE else '127.0.0.1']
    else:
        addrs = _resolve(host)
    if family not in (0, AF_INET):
        if family == AF_INET6:
            raise gaierror(-9, 'Address family for hostname not supported')
    out = []
    kinds = [(SOCK_STREAM, IPPROTO_TCP), (SOCK_DGRAM, IPPROTO_UDP), (SOCK_RAW, 0)]
    for a in addrs:
        for kind, pr in kinds:
            if type and type != kind:
                continue
            if proto and proto != pr:
                continue
            out.append((AddressFamily(AF_INET), SocketKind(kind), pr, '', (a, port)))
    return out


def getnameinfo(sockaddr, flags):
    host, port = sockaddr[0], sockaddr[1]
    return (host, str(port))


_SERVICES = {'http': 80, 'https': 443, 'ftp': 21, 'ssh': 22, 'smtp': 25, 'domain': 53,
             'pop3': 110, 'imap': 143, 'telnet': 23}


def getservbyname(name, proto=None):
    try:
        return _SERVICES[name]
    except KeyError:
        raise OSError('service/proto not found') from None


def getservbyport(port, proto=None):
    for k, v in _SERVICES.items():
        if v == port:
            return k
    raise OSError('port/proto not found')


def inet_aton(s):
    if not _is_ipv4(s):
        raise OSError('illegal IP address string passed to inet_aton')
    return bytes(int(p) for p in s.split('.'))


def inet_ntoa(b):
    if len(b) != 4:
        raise OSError('packed IP wrong length for inet_ntoa')
    return '.'.join(str(x) for x in b)


def inet_pton(family, s):
    if family == AF_INET:
        return inet_aton(s)
    raise OSError('address family not supported')


def inet_ntop(family, b):
    if family == AF_INET:
        return inet_ntoa(b)
    raise ValueError('unknown address family %d' % family)


def htons(x):
    return ((x & 0xff) << 8) | ((x >> 8) & 0xff)


ntohs = htons


def htonl(x):
    return int.from_bytes(x.to_bytes(4, 'little'), 'big')


ntohl = htonl

# ------------------------------------------------------------------ plumbing

_fds = [2]
_listeners = {}   # (port, type) -> listening socket
_local = None


def _next_fd():
    _fds[0] += 1
    return _fds[0]


def _local_address():
    global _local
    if _local is None:
        _local = _cw.local_address()
    return _local


def _is_local(addr):
    return addr in ('127.0.0.1', '0.0.0.0', 'localhost', '', _local_address()) or \
        addr.startswith('127.')


_ports = [40000]


def _ephemeral():
    _ports[0] += 1
    return _ports[0]


def _block(ready, sock, what):
    """Wait until `ready()`; other threads run meanwhile. A socket timeout turns
    into `TimeoutError` after that much virtual time; with nothing left that could
    make it ready, a blocking wait is a deadlock and is reported as one."""
    if ready():
        return
    t = sock._timeout
    if t == 0.0:
        raise BlockingIOError(11, 'Resource temporarily unavailable')
    try:
        import _thread
        waited = _thread._wait_until(ready, t)
    except ImportError:
        waited = False
    if waited:
        return
    if t is not None:
        _cw.advance(t)
        raise TimeoutError('timed out')
    raise OSError(35, 'Resource deadlock avoided: %s would block forever '
                      '(nothing else in this program can wake it)' % what)


def _parse_request(buf):
    """(request, rest) when `buf` starts with a complete HTTP/1.x request, (None,
    buf) when more bytes are needed, ('bad', b'') when it is not HTTP."""
    end = buf.find(b'\r\n\r\n')
    sep = 4
    if end < 0:
        end2 = buf.find(b'\n\n')
        if end2 >= 0:
            end, sep = end2, 2
    first_nl = buf.find(b'\n')
    if first_nl >= 0:
        line = buf[:first_nl].rstrip(b'\r').decode('latin-1')
        parts = line.split(' ')
        if len(parts) != 3 or not parts[2].startswith('HTTP/1.') or not parts[0].isalpha():
            return 'bad', b''
    elif len(buf) > 8192:
        return 'bad', b''
    if end < 0:
        return None, buf
    head = buf[:end].decode('latin-1').replace('\r\n', '\n').split('\n')
    method, target, version = head[0].split(' ')
    headers = []
    for h in head[1:]:
        k, _, v = h.partition(':')
        headers.append((k.strip(), v.strip()))
    body_start = end + sep
    length = 0
    chunked = False
    for k, v in headers:
        if k.lower() == 'content-length':
            length = int(v or '0')
        if k.lower() == 'transfer-encoding' and 'chunked' in v.lower():
            chunked = True
    rest = buf[body_start:]
    if chunked:
        body = b''
        pos = 0
        while True:
            nl = rest.find(b'\r\n', pos)
            if nl < 0:
                return None, buf
            size = int(rest[pos:nl].split(b';')[0] or b'0', 16)
            if len(rest) < nl + 2 + size + 2:
                return None, buf
            body += rest[nl + 2:nl + 2 + size]
            pos = nl + 2 + size + 2
            if size == 0:
                break
        return (method, target, version, headers, body), rest[pos:]
    if len(rest) < length:
        return None, buf
    return (method, target, version, headers, rest[:length]), rest[length:]


def _reason(status):
    try:
        from http.client import responses
        return responses.get(status, 'Unknown')
    except Exception:
        return 'OK' if status == 200 else 'Unknown'


class _Remote:
    """The far end of a TCP connection to a simulated service: an HTTP server."""

    def __init__(self, host, port, raddr):
        self.host = host
        self.port = port
        self.raddr = raddr
        self.out = b''
        self.closed = False

    def feed(self, sock, data):
        if self.closed:
            raise BrokenPipeError(32, 'Broken pipe')
        self.out += data
        while self.out:
            req, rest = _parse_request(self.out)
            if req is None:
                return
            if req == 'bad':
                self.out = b''
                sock._inbuf += (b'HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n'
                                b'Connection: close\r\n\r\n')
                self.closed = True
                return
            self.out = rest
            method, target, version, headers, body = req
            host = self.host
            for k, v in headers:
                if k.lower() == 'host':
                    host = v
            if target.startswith('http://') or target.startswith('https://'):
                url = target
            else:
                if ':' not in host and self.port != 80:
                    host = '%s:%d' % (host, self.port)
                url = 'http://' + host + target
            t = sock._timeout
            try:
                status, rh, payload = _cw.http(method, url, headers, body, t)
            except TimeoutError:
                raise TimeoutError('timed out') from None
            except OSError:
                self.closed = True
                raise ConnectionResetError(104, 'Connection reset by peer') from None
            if method == 'HEAD':
                payload = b''
            close = version == 'HTTP/1.0' or any(
                k.lower() == 'connection' and v.lower() == 'close' for k, v in headers)
            lines = ['HTTP/1.1 %d %s' % (status, _reason(status))]
            have_len = False
            for k, v in rh:
                if k.lower() == 'content-length':
                    have_len = True
                    v = str(len(payload)) if method != 'HEAD' else v
                if k.lower() in ('connection', 'transfer-encoding'):
                    continue
                lines.append('%s: %s' % (k, v))
            if not have_len:
                lines.append('Content-Length: %d' % len(payload))
            lines.append('Connection: close' if close else 'Connection: keep-alive')
            sock._inbuf += ('\r\n'.join(lines) + '\r\n\r\n').encode('latin-1') + payload
            if close:
                self.closed = True
                return


class socket:
    def __init__(self, family=AF_INET, type=SOCK_STREAM, proto=0, fileno=None):
        if family not in (AF_INET, AF_INET6, AF_UNIX):
            raise OSError(97, 'Address family not supported by protocol')
        self.family = AddressFamily(family)
        self.type = SocketKind(type & 0xf)
        if proto == 0:
            proto = IPPROTO_TCP if self.type == SOCK_STREAM else (
                IPPROTO_UDP if self.type == SOCK_DGRAM else 0)
        self.proto = proto
        self._fd = _next_fd() if fileno is None else fileno
        self._timeout = _default_timeout
        if type & SOCK_NONBLOCK:
            self._timeout = 0.0
        self._closed = False
        self._laddr = None
        self._raddr = None
        self._remote = None
        self._peer = None
        self._inbuf = b''
        self._eof = False
        self._listening = False
        self._backlog = []
        self._dgrams = []
        self._opts = {}
        self._shut_wr = False

    # --- identity
    def fileno(self):
        return -1 if self._closed else self._fd

    def __repr__(self):
        s = '<socket.socket fd=%d, family=%d, type=%d, proto=%d' % (
            self.fileno(), int(self.family), int(self.type), self.proto)
        if self._closed:
            return '<socket.socket [closed] fd=-1, family=%d, type=%d, proto=%d>' % (
                int(self.family), int(self.type), self.proto)
        if self._laddr:
            s += ', laddr=%r' % (self._laddr,)
        if self._raddr:
            s += ', raddr=%r' % (self._raddr,)
        return s + '>'

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def _check(self):
        if self._closed:
            raise OSError(9, 'Bad file descriptor')

    # --- options
    def settimeout(self, value):
        if value is not None:
            value = float(value)
            if value < 0:
                raise ValueError('Timeout value out of range')
        self._timeout = value

    def gettimeout(self):
        return self._timeout

    def setblocking(self, flag):
        self._timeout = None if flag else 0.0

    def getblocking(self):
        return self._timeout != 0.0

    def setsockopt(self, level, optname, value, optlen=None):
        self._opts[(level, optname)] = value

    def getsockopt(self, level, optname, buflen=None):
        if (level, optname) == (SOL_SOCKET, SO_TYPE):
            return int(self.type)
        if (level, optname) == (SOL_SOCKET, SO_ERROR):
            return 0
        return self._opts.get((level, optname), 0)

    # --- addresses
    def bind(self, address):
        self._check()
        host, port = address[0], address[1]
        if host not in ('', '0.0.0.0', 'localhost') and not _is_local(host):
            raise OSError(99, 'Cannot assign requested address')
        host = '0.0.0.0' if host == '' else ('127.0.0.1' if host == 'localhost' else host)
        if port == 0:
            port = _ephemeral()
        key = (port, int(self.type))
        if key in _listeners and _listeners[key] is not self:
            raise OSError(98, 'Address already in use')
        _listeners[key] = self
        self._laddr = (host, port)

    def listen(self, backlog=SOMAXCONN):
        self._check()
        if self._laddr is None:
            self.bind(('0.0.0.0', 0))
        self._listening = True

    def getsockname(self):
        self._check()
        return self._laddr or ('0.0.0.0', 0)

    def getpeername(self):
        self._check()
        if self._raddr is None:
            raise OSError(107, 'Transport endpoint is not connected')
        return self._raddr

    # --- connections
    def connect(self, address):
        self._check()
        if self.type == SOCK_DGRAM:
            self._raddr = (gethostbyname(address[0]), address[1])
            if self._laddr is None:
                self.bind(('0.0.0.0', 0))
            return
        host, port = address[0], address[1]
        if not isinstance(port, int):
            raise TypeError("'str' object cannot be interpreted as an integer")
        addrs = _resolve(host)
        addr = addrs[0]
        lst = _listeners.get((port, SOCK_STREAM))
        if _is_local(addr) and lst is not None and lst._listening:
            server_side = socket(self.family, SOCK_STREAM)
            lport = _ephemeral()
            laddr = ('127.0.0.1', lport)
            server_side._laddr = (('127.0.0.1' if lst._laddr[0] == '0.0.0.0' else lst._laddr[0]), port)
            server_side._raddr = laddr
            server_side._peer = self
            self._peer = server_side
            self._laddr = laddr
            self._raddr = (addr if addr != '0.0.0.0' else '127.0.0.1', port)
            lst._backlog.append(server_side)
            return
        if _is_local(addr):
            raise ConnectionRefusedError(111, 'Connection refused')
        remote, rport, laddr, lport = _cw.connect(host, port)
        self._remote = _Remote(host, port, (remote, rport))
        self._raddr = (remote, rport)
        self._laddr = (laddr, lport)

    def connect_ex(self, address):
        try:
            self.connect(address)
            return 0
        except OSError as e:
            return e.errno or 11

    def accept(self):
        self._check()
        if not self._listening:
            raise OSError(22, 'Invalid argument')
        _block(lambda: self._backlog or self._closed, self, 'accept()')
        conn = self._backlog.pop(0)
        return conn, conn._raddr

    def _accept(self):
        conn, addr = self.accept()
        return conn._fd, addr

    # --- data
    def send(self, data, flags=0):
        self._check()
        data = bytes(data)
        if self._shut_wr:
            raise BrokenPipeError(32, 'Broken pipe')
        if self._remote is not None:
            self._remote.feed(self, data)
            return len(data)
        if self._peer is not None:
            if self._peer._closed:
                raise BrokenPipeError(32, 'Broken pipe')
            self._peer._inbuf += data
            return len(data)
        if self.type == SOCK_DGRAM and self._raddr:
            return self.sendto(data, self._raddr)
        raise OSError(32, 'Broken pipe') if self._raddr else OSError(107, 'Transport endpoint is not connected')

    def sendall(self, data, flags=0):
        self.send(data, flags)

    def sendto(self, data, flags_or_addr, addr=None):
        address = addr if addr is not None else flags_or_addr
        data = bytes(data)
        if self._laddr is None:
            self.bind(('0.0.0.0', 0))
        target = gethostbyname(address[0])
        lst = _listeners.get((address[1], SOCK_DGRAM))
        if _is_local(target) and lst is not None:
            src = ('127.0.0.1', self._laddr[1])
            lst._dgrams.append((data, src))
        # Datagrams to other machines are sent and, with no UDP service there, lost.
        return len(data)

    def _readable(self):
        if self._inbuf:
            return True
        if self._remote is not None:
            return self._remote.closed or not self._remote.out
        if self._peer is not None:
            return self._peer._closed or self._peer._shut_wr
        return self._eof or self._closed

    def recv(self, bufsize, flags=0):
        self._check()
        if self.type == SOCK_DGRAM:
            return self.recvfrom(bufsize, flags)[0]
        if not self._inbuf and self._remote is None and self._peer is None:
            raise OSError(107, 'Transport endpoint is not connected')
        _block(self._readable, self, 'recv()')
        out = self._inbuf[:bufsize]
        if not (flags & MSG_PEEK):
            self._inbuf = self._inbuf[len(out):]
        return out

    def recv_into(self, buffer, nbytes=0, flags=0):
        n = nbytes or len(buffer)
        data = self.recv(n, flags)
        buffer[:len(data)] = data
        return len(data)

    def recvfrom(self, bufsize, flags=0):
        self._check()
        if self.type == SOCK_DGRAM:
            _block(lambda: bool(self._dgrams), self, 'recvfrom()')
            data, src = self._dgrams.pop(0)
            return data[:bufsize], src
        return self.recv(bufsize, flags), self._raddr

    def makefile(self, mode='r', buffering=None, *, encoding=None, errors=None, newline=None):
        return _SocketFile(self, mode, encoding)

    def shutdown(self, how):
        self._check()
        if how in (SHUT_WR, SHUT_RDWR):
            self._shut_wr = True

    def close(self):
        if self._closed:
            return
        self._closed = True
        for k, v in list(_listeners.items()):
            if v is self:
                del _listeners[k]

    def detach(self):
        fd = self._fd
        self._closed = True
        return fd

    def dup(self):
        return self


class _SocketFile:
    def __init__(self, sock, mode, encoding):
        self._sock = sock
        self._text = 'b' not in mode
        self._encoding = encoding or 'utf-8'
        self.mode = mode
        self.closed = False
        self._wbuf = b''

    def _fill(self):
        chunk = self._sock.recv(65536)
        return chunk

    def read(self, n=-1):
        data = b''
        while n is None or n < 0 or len(data) < n:
            chunk = self._sock.recv(65536 if n is None or n < 0 else n - len(data))
            if not chunk:
                break
            data += chunk
        return data.decode(self._encoding) if self._text else data

    def readline(self, limit=-1):
        data = b''
        while not data.endswith(b'\n'):
            if limit is not None and 0 <= limit <= len(data):
                break
            chunk = self._sock.recv(1)
            if not chunk:
                break
            data += chunk
        return data.decode(self._encoding) if self._text else data

    def __iter__(self):
        return self

    def __next__(self):
        line = self.readline()
        if not line:
            raise StopIteration
        return line

    def write(self, data):
        if isinstance(data, str):
            data = data.encode(self._encoding)
        self._wbuf += data
        return len(data)

    def flush(self):
        if self._wbuf:
            self._sock.sendall(self._wbuf)
            self._wbuf = b''

    def close(self):
        self.flush()
        self.closed = True

    def __enter__(self):
        return self

    def __exit__(self, *a):
        self.close()


SocketType = socket


def create_connection(address, timeout=_GLOBAL_DEFAULT_TIMEOUT, source_address=None, *,
                      all_errors=False):
    host, port = address
    sock = socket(AF_INET, SOCK_STREAM)
    if timeout is not _GLOBAL_DEFAULT_TIMEOUT:
        sock.settimeout(timeout)
    if source_address:
        sock.bind(source_address)
    sock.connect((host, port))
    return sock


def create_server(address, *, family=AF_INET, backlog=None, reuse_port=False,
                  dualstack_ipv6=False):
    sock = socket(family, SOCK_STREAM)
    sock.setsockopt(SOL_SOCKET, SO_REUSEADDR, 1)
    sock.bind(address)
    sock.listen(backlog if backlog is not None else SOMAXCONN)
    return sock


def socketpair(family=AF_UNIX, type=SOCK_STREAM, proto=0):
    a = socket(AF_INET, type)
    b = socket(AF_INET, type)
    a._peer, b._peer = b, a
    a._laddr = b._raddr = ('127.0.0.1', _ephemeral())
    b._laddr = a._raddr = ('127.0.0.1', _ephemeral())
    return a, b


def fromfd(fd, family, type, proto=0):
    return socket(family, type, proto, fd)
