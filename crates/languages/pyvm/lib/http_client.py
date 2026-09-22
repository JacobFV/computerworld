"""http.client over the simulated network.

Every exchange travels the world's network like any other client of the machine
(DNS, routes, service availability, failures, latency). `https://` goes to port
443 exactly as the browser sends it; the world decides whether anything answers.
"""
import _cw
import io
import socket
from urllib.parse import urlsplit

__all__ = ["HTTPResponse", "HTTPConnection", "HTTPException", "NotConnected",
           "UnknownProtocol", "UnknownTransferEncoding", "UnimplementedFileMode",
           "IncompleteRead", "InvalidURL", "ImproperConnectionState",
           "CannotSendRequest", "CannotSendHeader", "ResponseNotReady",
           "BadStatusLine", "LineTooLong", "RemoteDisconnected", "error",
           "responses", "HTTPSConnection", "HTTPMessage"]

HTTP_PORT = 80
HTTPS_PORT = 443

try:
    from http import HTTPStatus as _HS
    responses = {int(v): v.phrase for v in _HS}
except Exception:
    responses = {}

_METHODS_EXPECTING_BODY = {'PATCH', 'POST', 'PUT'}
_UNKNOWN = 'UNKNOWN'


class HTTPException(Exception):
    pass


class NotConnected(HTTPException):
    pass


class InvalidURL(HTTPException):
    pass


class UnknownProtocol(HTTPException):
    def __init__(self, version):
        self.args = version,
        self.version = version


class UnknownTransferEncoding(HTTPException):
    pass


class UnimplementedFileMode(HTTPException):
    pass


class IncompleteRead(HTTPException):
    def __init__(self, partial, expected=None):
        self.args = partial,
        self.partial = partial
        self.expected = expected

    def __repr__(self):
        if self.expected is not None:
            e = ', %i more expected' % self.expected
        else:
            e = ''
        return '%s(%i bytes read%s)' % (self.__class__.__name__, len(self.partial), e)

    __str__ = object.__str__


class ImproperConnectionState(HTTPException):
    pass


class CannotSendRequest(ImproperConnectionState):
    pass


class CannotSendHeader(ImproperConnectionState):
    pass


class ResponseNotReady(ImproperConnectionState):
    pass


class BadStatusLine(HTTPException):
    def __init__(self, line):
        if not line:
            line = repr(line)
        self.args = line,
        self.line = line


class LineTooLong(HTTPException):
    def __init__(self, line_type):
        HTTPException.__init__(self, "got more than %d bytes when reading %s"
                               % (65536, line_type))


class RemoteDisconnected(ConnectionResetError, BadStatusLine):
    def __init__(self, *pos, **kw):
        BadStatusLine.__init__(self, "")
        ConnectionResetError.__init__(self, *pos, **kw)


error = HTTPException


class HTTPMessage:
    """The headers of a message: an ordered, case-insensitive multi-map (the
    subset of email.message.Message that HTTP code uses)."""

    def __init__(self, items=None):
        self._headers = list(items or [])

    def __len__(self):
        return len(self._headers)

    def __contains__(self, name):
        n = name.lower()
        return any(k.lower() == n for k, v in self._headers)

    def __getitem__(self, name):
        return self.get(name)

    def __setitem__(self, name, val):
        self._headers.append((name, val))

    def __delitem__(self, name):
        n = name.lower()
        self._headers = [(k, v) for k, v in self._headers if k.lower() != n]

    def __iter__(self):
        for k, v in self._headers:
            yield k

    def get(self, name, failobj=None):
        n = name.lower()
        for k, v in self._headers:
            if k.lower() == n:
                return v
        return failobj

    def get_all(self, name, failobj=None):
        n = name.lower()
        vals = [v for k, v in self._headers if k.lower() == n]
        return vals if vals else failobj

    def getallmatchingheaders(self, name):
        n = name.lower()
        return ['%s: %s' % (k, v) for k, v in self._headers if k.lower() == n]

    def keys(self):
        return [k for k, v in self._headers]

    def values(self):
        return [v for k, v in self._headers]

    def items(self):
        return list(self._headers)

    def add_header(self, name, value):
        self._headers.append((name, value))

    def replace_header(self, name, value):
        n = name.lower()
        for i, (k, v) in enumerate(self._headers):
            if k.lower() == n:
                self._headers[i] = (k, value)
                return
        raise KeyError(name)

    def get_content_type(self):
        ct = self.get('content-type')
        if ct is None:
            return 'text/plain'
        ct = ct.split(';')[0].strip().lower()
        return ct if ct.count('/') == 1 else 'text/plain'

    def get_content_maintype(self):
        return self.get_content_type().split('/')[0]

    def get_content_subtype(self):
        return self.get_content_type().split('/')[1]

    def get_param(self, param, failobj=None, header='content-type', unquote=True):
        v = self.get(header)
        if v is None:
            return failobj
        for part in v.split(';')[1:]:
            k, _, val = part.strip().partition('=')
            if k.strip().lower() == param.lower():
                val = val.strip()
                if unquote and len(val) >= 2 and val[0] == val[-1] == '"':
                    val = val[1:-1]
                return val
        return failobj

    def get_content_charset(self, failobj=None):
        v = self.get_param('charset')
        return v.lower() if v else failobj

    def get_filename(self, failobj=None):
        return self.get_param('filename', failobj, 'content-disposition')

    def as_string(self):
        return ''.join('%s: %s\n' % (k, v) for k, v in self._headers) + '\n'

    def __str__(self):
        return self.as_string()


class HTTPResponse(io.BufferedIOBase if hasattr(io, 'BufferedIOBase') else object):
    def __init__(self, status=200, headers=(), body=b'', method=None, url=None):
        self.status = status
        self.code = status
        self.reason = responses.get(status, '')
        self.version = 11
        self.headers = self.msg = HTTPMessage(headers)
        self._method = method
        self._body = b'' if method == 'HEAD' else body
        self._pos = 0
        self.url = url
        self.chunked = False
        self.will_close = True
        self.length = len(self._body)
        self.closed = False
        self.debuglevel = 0

    def _check(self):
        pass

    def read(self, amt=None):
        if self.closed:
            return b''
        if amt is None or amt < 0:
            out = self._body[self._pos:]
            self._pos = len(self._body)
        else:
            out = self._body[self._pos:self._pos + amt]
            self._pos += len(out)
        self.length = len(self._body) - self._pos
        if self._pos >= len(self._body) and (amt is None or amt < 0):
            self.close()
        return out

    def read1(self, n=-1):
        return self.read(n if n >= 0 else None)

    def peek(self, n=-1):
        return self._body[self._pos:]

    def readinto(self, b):
        data = self.read(len(b))
        b[:len(data)] = data
        return len(data)

    def readline(self, limit=-1):
        i = self._body.find(b'\n', self._pos)
        end = len(self._body) if i < 0 else i + 1
        if limit is not None and limit >= 0:
            end = min(end, self._pos + limit)
        out = self._body[self._pos:end]
        self._pos = end
        self.length = len(self._body) - self._pos
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

    def getheader(self, name, default=None):
        if self.headers is None:
            raise ResponseNotReady()
        headers = self.headers.get_all(name) or default
        if isinstance(headers, str) or not hasattr(headers, '__iter__'):
            return headers
        return ', '.join(headers)

    def getheaders(self):
        return list(self.headers.items())

    def fileno(self):
        return 3

    def isclosed(self):
        return self.closed

    def close(self):
        self.closed = True

    def readable(self):
        return True

    def flush(self):
        pass

    def info(self):
        return self.headers

    def geturl(self):
        return self.url

    def getcode(self):
        return self.status

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def __repr__(self):
        return '<http.client.HTTPResponse object at 0x%x>' % id(self)


def _encode_body(body, method):
    if body is None:
        return None
    if isinstance(body, str):
        return body.encode('iso-8859-1')
    if isinstance(body, (bytes, bytearray)):
        return bytes(body)
    if hasattr(body, 'read'):
        data = body.read()
        return data.encode('iso-8859-1') if isinstance(data, str) else data
    parts = []
    for chunk in body:
        parts.append(chunk.encode('iso-8859-1') if isinstance(chunk, str) else bytes(chunk))
    return b''.join(parts)


class HTTPConnection:
    _http_vsn = 11
    _http_vsn_str = 'HTTP/1.1'
    response_class = HTTPResponse
    default_port = HTTP_PORT
    auto_open = 1
    debuglevel = 0
    _scheme = 'http'

    def __init__(self, host, port=None, timeout=socket._GLOBAL_DEFAULT_TIMEOUT,
                 source_address=None, blocksize=8192):
        self.timeout = timeout
        self.source_address = source_address
        self.blocksize = blocksize
        self.sock = None
        self._buffer = []
        self.__response = None
        self.__state = 'Idle'
        self._method = None
        self._tunnel_host = None
        self._tunnel_port = None
        self._tunnel_headers = {}
        (self.host, self.port) = self._get_hostport(host, port)
        self._validate_host(self.host)
        self._pending = None

    def _get_hostport(self, host, port):
        if port is None:
            i = host.rfind(':')
            j = host.rfind(']')
            if i > j:
                try:
                    port = int(host[i + 1:])
                except ValueError:
                    if host[i + 1:] == "":
                        port = self.default_port
                    else:
                        raise InvalidURL("nonnumeric port: '%s'" % host[i + 1:])
                host = host[:i]
            else:
                port = self.default_port
        if host and host[0] == '[' and host[-1] == ']':
            host = host[1:-1]
        return (host, port)

    def _validate_host(self, host):
        for c in host:
            if ord(c) <= 32 or ord(c) == 127:
                raise InvalidURL(f"URL can't contain control characters. {host!r} "
                                 f"(found at least {c!r})")

    def set_debuglevel(self, level):
        self.debuglevel = level

    def set_tunnel(self, host, port=None, headers=None):
        self._tunnel_host, self._tunnel_port = self._get_hostport(host, port)
        self._tunnel_headers = dict(headers or {})

    def connect(self):
        _cw.connect(self.host, self.port)
        self.sock = True

    def close(self):
        self.sock = None
        self.__state = 'Idle'
        response = self.__response
        if response:
            self.__response = None
            response.close()

    def putrequest(self, method, url, skip_host=False, skip_accept_encoding=False):
        if self.__response and self.__response.isclosed():
            self.__response = None
        if self.__state == 'Idle':
            self.__state = 'Request-started'
        else:
            raise CannotSendRequest(self.__state)
        for c in method:
            if ord(c) <= 32 or ord(c) == 127:
                raise ValueError(f"method can't contain control characters. {method!r} "
                                 f"(found at least {c!r})")
        self._method = method
        url = url or '/'
        for c in url:
            if ord(c) <= 32 or ord(c) == 127:
                raise InvalidURL(f"URL can't contain control characters. {url!r} "
                                 f"(found at least {c!r})")
        self._url = url
        self._headers = []
        if not skip_host:
            netloc = ''
            if url.startswith('http'):
                nil, netloc, nil, nil, nil = urlsplit(url)
            if netloc:
                self._headers.append(('Host', netloc))
            else:
                host = self.host
                if ':' in host:
                    host = '[' + host + ']'
                if self.port == self.default_port:
                    self._headers.append(('Host', host))
                else:
                    self._headers.append(('Host', '%s:%s' % (host, self.port)))
        if not skip_accept_encoding:
            self._headers.append(('Accept-Encoding', 'identity'))

    def putheader(self, header, *values):
        if self.__state != 'Request-started':
            raise CannotSendHeader()
        if hasattr(header, 'encode'):
            header = header
        else:
            header = header.decode('ascii')
        vals = []
        for v in values:
            if isinstance(v, bytes):
                vals.append(v.decode('latin-1'))
            elif isinstance(v, int):
                vals.append(str(v))
            else:
                vals.append(v)
        self._headers.append((header, '\r\n\t'.join(vals)))

    def endheaders(self, message_body=None, *, encode_chunked=False):
        if self.__state == 'Request-started':
            self.__state = 'Request-sent'
        else:
            raise CannotSendHeader()
        self._send_request_now(message_body)

    def _send_request_now(self, body):
        url = self._url
        if not url.startswith('http://') and not url.startswith('https://'):
            host = self.host if ':' not in self.host else '[' + self.host + ']'
            if self.port == self.default_port:
                url = '%s://%s%s' % (self._scheme, host, url)
            else:
                url = '%s://%s:%s%s' % (self._scheme, host, self.port, url)
        timeout = self.timeout
        if timeout is socket._GLOBAL_DEFAULT_TIMEOUT:
            timeout = socket.getdefaulttimeout()
        data = _encode_body(body, self._method) or b''
        try:
            status, headers, payload = _cw.http(self._method, url, self._headers, data, timeout)
        except TimeoutError:
            raise socket.timeout('timed out')
        except OSError as e:
            if e.errno == -2:
                raise socket.gaierror(-2, 'Name or service not known')
            raise
        self.sock = True
        self._pending = (status, headers, payload, url)

    def send(self, data):
        self._extra = data

    def request(self, method, url, body=None, headers={}, *, encode_chunked=False):
        header_names = frozenset(k.lower() for k in headers)
        skips = {}
        if 'host' in header_names:
            skips['skip_host'] = 1
        if 'accept-encoding' in header_names:
            skips['skip_accept_encoding'] = 1
        self.putrequest(method, url, **skips)
        data = _encode_body(body, method)
        if 'content-length' not in header_names:
            if 'transfer-encoding' not in header_names:
                if data is None:
                    if method.upper() in _METHODS_EXPECTING_BODY:
                        self.putheader('Content-Length', '0')
                else:
                    self.putheader('Content-Length', str(len(data)))
        for hdr, value in headers.items():
            self.putheader(hdr, value)
        self.endheaders(data)

    def getresponse(self):
        if self.__response and self.__response.isclosed():
            self.__response = None
        if self.__state != 'Request-sent' or self.__response:
            raise ResponseNotReady(self.__state)
        status, headers, payload, url = self._pending
        self._pending = None
        response = self.response_class(status, headers, payload, self._method, url)
        self.__state = 'Idle'
        self.__response = None
        return response


class HTTPSConnection(HTTPConnection):
    default_port = HTTPS_PORT
    _scheme = 'https'

    def __init__(self, host, port=None, *, timeout=socket._GLOBAL_DEFAULT_TIMEOUT,
                 source_address=None, context=None, blocksize=8192, **kw):
        super().__init__(host, port, timeout, source_address, blocksize=blocksize)
        self._context = context


def parse_headers(fp, _class=HTTPMessage):
    headers = []
    while True:
        line = fp.readline()
        if line in (b'\r\n', b'\n', b'', '\r\n', '\n', ''):
            break
        if isinstance(line, bytes):
            line = line.decode('iso-8859-1')
        k, _, v = line.partition(':')
        headers.append((k.strip(), v.strip()))
    return _class(headers)
