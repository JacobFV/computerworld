"""urllib.parse: URL parsing, joining, quoting and query strings (CPython 3.12
semantics)."""
from collections import namedtuple

__all__ = ["urlparse", "urlunparse", "urljoin", "urldefrag", "urlsplit", "urlunsplit",
           "urlencode", "parse_qs", "parse_qsl", "quote", "quote_plus", "quote_from_bytes",
           "unquote", "unquote_plus", "unquote_to_bytes", "ParseResult", "SplitResult",
           "DefragResult"]

uses_relative = ['', 'ftp', 'http', 'gopher', 'nntp', 'imap', 'wais', 'file', 'https',
                 'shttp', 'mms', 'prospero', 'rtsp', 'rtspu', 'sftp', 'svn', 'svn+ssh',
                 'ws', 'wss']
uses_netloc = ['', 'ftp', 'http', 'gopher', 'nntp', 'telnet', 'imap', 'wais', 'file', 'mms',
               'https', 'shttp', 'snews', 'prospero', 'rtsp', 'rtspu', 'rsync', 'svn',
               'svn+ssh', 'sftp', 'nfs', 'git', 'git+ssh', 'ws', 'wss']
uses_params = ['', 'ftp', 'hdl', 'prospero', 'http', 'imap', 'https', 'shttp', 'rtsp',
               'rtspu', 'sip', 'sips', 'mms', 'sftp', 'tel']
scheme_chars = ('abcdefghijklmnopqrstuvwxyz'
                'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
                '0123456789'
                '+-.')
_UNSAFE_URL_BYTES_TO_REMOVE = ['\t', '\r', '\n']
_WHATWG_C0_CONTROL_OR_SPACE = '\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f '


class _NetlocResultMixinStr:
    @property
    def username(self):
        return self._userinfo[0]

    @property
    def password(self):
        return self._userinfo[1]

    @property
    def hostname(self):
        hostname = self._hostinfo[0]
        if not hostname:
            return None
        separator = '%' if isinstance(hostname, str) else b'%'
        hostname, percent, zone = hostname.partition(separator)
        return hostname.lower() + percent + zone

    @property
    def port(self):
        port = self._hostinfo[1]
        if port is not None:
            if port.isdigit() and port.isascii():
                port = int(port)
            else:
                raise ValueError(f"Port could not be cast to integer value as {port!r}")
            if not (0 <= port <= 65535):
                raise ValueError("Port out of range 0-65535")
        return port

    @property
    def _userinfo(self):
        netloc = self.netloc
        userinfo, have_info, hostinfo = netloc.rpartition('@')
        if have_info:
            username, have_password, password = userinfo.partition(':')
            if not have_password:
                password = None
        else:
            username = password = None
        return username, password

    @property
    def _hostinfo(self):
        netloc = self.netloc
        _, _, hostinfo = netloc.rpartition('@')
        _, have_open_br, bracketed = hostinfo.partition('[')
        if have_open_br:
            hostname, _, port = bracketed.partition(']')
            _, _, port = port.partition(':')
        else:
            hostname, _, port = hostinfo.partition(':')
        if not port:
            port = None
        return hostname, port


_DefragResultBase = namedtuple('DefragResult', 'url fragment')
_SplitResultBase = namedtuple('SplitResult', 'scheme netloc path query fragment')
_ParseResultBase = namedtuple('ParseResult', 'scheme netloc path params query fragment')


class DefragResult(_DefragResultBase):
    __slots__ = ()

    def geturl(self):
        if self.fragment:
            return self.url + '#' + self.fragment
        return self.url


class SplitResult(_SplitResultBase, _NetlocResultMixinStr):
    __slots__ = ()

    def geturl(self):
        return urlunsplit(self)


class ParseResult(_ParseResultBase, _NetlocResultMixinStr):
    __slots__ = ()

    def geturl(self):
        return urlunparse(self)


def _coerce_args(*args):
    str_input = isinstance(args[0], str)
    for arg in args[1:]:
        if arg and isinstance(arg, str) != str_input:
            raise TypeError("Cannot mix str and non-str arguments")
    if str_input:
        return args + (lambda x: x,)
    return tuple(x.decode('ascii', 'strict') if x else '' for x in args) + \
        (lambda x: x.encode('ascii', 'strict') if isinstance(x, str) else
         type(x)(*[v.encode('ascii') if isinstance(v, str) else v for v in x])
         if isinstance(x, tuple) else x,)


def urlparse(url, scheme='', allow_fragments=True):
    url, scheme, _coerce_result = _coerce_args(url, scheme)
    scheme, netloc, url, query, fragment = urlsplit(url, scheme, allow_fragments)
    if scheme in uses_params and ';' in url:
        url, params = _splitparams(url)
    else:
        params = ''
    return _coerce_result(ParseResult(scheme, netloc, url, params, query, fragment))


def _splitparams(url):
    if '/' in url:
        i = url.find(';', url.rfind('/'))
        if i < 0:
            return url, ''
    else:
        i = url.find(';')
    return url[:i], url[i + 1:]


def _splitnetloc(url, start=0):
    delim = len(url)
    for c in '/?#':
        wdelim = url.find(c, start)
        if wdelim >= 0:
            delim = min(delim, wdelim)
    return url[start:delim], url[delim:]


def _checknetloc(netloc):
    pass


def urlsplit(url, scheme='', allow_fragments=True):
    url, scheme, _coerce_result = _coerce_args(url, scheme)
    url = url.lstrip(_WHATWG_C0_CONTROL_OR_SPACE)
    scheme = scheme.strip(_WHATWG_C0_CONTROL_OR_SPACE)
    for b in _UNSAFE_URL_BYTES_TO_REMOVE:
        url = url.replace(b, "")
        scheme = scheme.replace(b, "")
    allow_fragments = bool(allow_fragments)
    netloc = query = fragment = ''
    i = url.find(':')
    if i > 0 and url[0].isascii() and url[0].isalpha():
        for c in url[:i]:
            if c not in scheme_chars:
                break
        else:
            scheme, url = url[:i].lower(), url[i + 1:]
    if url[:2] == '//':
        netloc, url = _splitnetloc(url, 2)
        if (('[' in netloc and ']' not in netloc) or
                (']' in netloc and '[' not in netloc)):
            raise ValueError("Invalid IPv6 URL")
    if allow_fragments and '#' in url:
        url, fragment = url.split('#', 1)
    if '?' in url:
        url, query = url.split('?', 1)
    v = SplitResult(scheme, netloc, url, query, fragment)
    return _coerce_result(v)


def urlunparse(components):
    scheme, netloc, url, params, query, fragment, _coerce_result = (_coerce_args(*components))
    if params:
        url = "%s;%s" % (url, params)
    return _coerce_result(urlunsplit((scheme, netloc, url, query, fragment)))


def urlunsplit(components):
    scheme, netloc, url, query, fragment, _coerce_result = (_coerce_args(*components))
    if netloc or (scheme and scheme in uses_netloc and url[:2] != '//'):
        if url and url[:1] != '/':
            url = '/' + url
        url = '//' + (netloc or '') + url
    if scheme:
        url = scheme + ':' + url
    if query:
        url = url + '?' + query
    if fragment:
        url = url + '#' + fragment
    return _coerce_result(url)


def urljoin(base, url, allow_fragments=True):
    if not base:
        return url
    if not url:
        return base
    base, url, _coerce_result = _coerce_args(base, url)
    bscheme, bnetloc, bpath, bparams, bquery, bfragment = urlparse(base, '', allow_fragments)
    scheme, netloc, path, params, query, fragment = urlparse(url, bscheme, allow_fragments)
    if scheme != bscheme or scheme not in uses_relative:
        return _coerce_result(url)
    if scheme in uses_netloc:
        if netloc:
            return _coerce_result(urlunparse((scheme, netloc, path, params, query, fragment)))
        netloc = bnetloc
    if not path and not params:
        path = bpath
        params = bparams
        if not query:
            query = bquery
        return _coerce_result(urlunparse((scheme, netloc, path, params, query, fragment)))
    base_parts = bpath.split('/')
    if base_parts[-1] != '':
        del base_parts[-1]
    if path[:1] == '/':
        segments = path.split('/')
    else:
        segments = base_parts + path.split('/')
        segments[1:-1] = filter(None, segments[1:-1])
    resolved_path = []
    for seg in segments:
        if seg == '..':
            try:
                resolved_path.pop()
            except IndexError:
                pass
        elif seg == '.':
            continue
        else:
            resolved_path.append(seg)
    if segments[-1] in ('.', '..'):
        resolved_path.append('')
    return _coerce_result(urlunparse((scheme, netloc, '/'.join(resolved_path) or '/',
                                      params, query, fragment)))


def urldefrag(url):
    url, _coerce_result = _coerce_args(url)
    if '#' in url:
        s, n, p, a, q, frag = urlparse(url)
        defrag = urlunparse((s, n, p, a, q, ''))
    else:
        frag = ''
        defrag = url
    return _coerce_result(DefragResult(defrag, frag))


_hexdig = '0123456789ABCDEFabcdef'
_hextobyte = None


_HEX = frozenset(b'0123456789ABCDEFabcdef')


def unquote_to_bytes(string):
    if not string:
        return b''
    if isinstance(string, str):
        string = string.encode('utf-8')
    bits = string.split(b'%')
    if len(bits) == 1:
        return string
    res = bytearray(bits[0])
    for item in bits[1:]:
        h = item[:2]
        if len(h) == 2 and h[0] in _HEX and h[1] in _HEX:
            res.append(int(h.decode('ascii'), 16))
            res += item[2:]
        else:
            res += b'%'
            res += item
    return bytes(res)


def unquote(string, encoding='utf-8', errors='replace'):
    if isinstance(string, bytes):
        return unquote_to_bytes(string).decode(encoding, errors)
    if '%' not in string:
        string.split
        return string
    if encoding is None:
        encoding = 'utf-8'
    if errors is None:
        errors = 'replace'
    return unquote_to_bytes(string).decode(encoding, errors)


def parse_qs(qs, keep_blank_values=False, strict_parsing=False,
             encoding='utf-8', errors='replace', max_num_fields=None, separator='&'):
    parsed_result = {}
    pairs = parse_qsl(qs, keep_blank_values, strict_parsing,
                      encoding=encoding, errors=errors,
                      max_num_fields=max_num_fields, separator=separator)
    for name, value in pairs:
        if name in parsed_result:
            parsed_result[name].append(value)
        else:
            parsed_result[name] = [value]
    return parsed_result


def parse_qsl(qs, keep_blank_values=False, strict_parsing=False,
              encoding='utf-8', errors='replace', max_num_fields=None, separator='&'):
    if isinstance(qs, bytes):
        qs = qs.decode('ascii')
    if not separator or not isinstance(separator, (str, bytes)):
        raise ValueError("Separator must be of type string or bytes.")
    if max_num_fields is not None:
        num_fields = 1 + qs.count(separator) if qs else 0
        if max_num_fields < num_fields:
            raise ValueError('Max number of fields exceeded')
    r = []
    query_args = qs.split(separator) if qs else []
    for name_value in query_args:
        if not name_value and not strict_parsing:
            continue
        nv = name_value.split('=', 1)
        if len(nv) != 2:
            if strict_parsing:
                raise ValueError("bad query field: %r" % (name_value,))
            if keep_blank_values:
                nv.append('')
            else:
                continue
        if len(nv[1]) or keep_blank_values:
            name = nv[0].replace('+', ' ')
            name = unquote(name, encoding=encoding, errors=errors)
            value = nv[1].replace('+', ' ')
            value = unquote(value, encoding=encoding, errors=errors)
            r.append((name, value))
    return r


def unquote_plus(string, encoding='utf-8', errors='replace'):
    string = string.replace('+', ' ')
    return unquote(string, encoding, errors)


_ALWAYS_SAFE = frozenset(b'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
                         b'abcdefghijklmnopqrstuvwxyz'
                         b'0123456789'
                         b'_.-~')
_ALWAYS_SAFE_BYTES = bytes(_ALWAYS_SAFE)


def quote(string, safe='/', encoding=None, errors=None):
    if isinstance(string, str):
        if not string:
            return string
        if encoding is None:
            encoding = 'utf-8'
        if errors is None:
            errors = 'strict'
        string = string.encode(encoding, errors)
    else:
        if encoding is not None:
            raise TypeError("quote() doesn't support 'encoding' for bytes")
        if errors is not None:
            raise TypeError("quote() doesn't support 'errors' for bytes")
    return quote_from_bytes(string, safe)


def quote_plus(string, safe='', encoding=None, errors=None):
    if ((isinstance(string, str) and ' ' not in string) or
            (isinstance(string, bytes) and b' ' not in string)):
        return quote(string, safe, encoding, errors)
    if isinstance(safe, str):
        space = ' '
    else:
        space = b' '
    string = quote(string, safe + space, encoding, errors)
    return string.replace(' ', '+')


def quote_from_bytes(bs, safe='/'):
    if not isinstance(bs, (bytes, bytearray)):
        raise TypeError("quote_from_bytes() expected bytes")
    if not bs:
        return ''
    if isinstance(safe, str):
        safe = safe.encode('ascii', 'ignore')
    else:
        safe = bytes([c for c in safe if c < 128])
    safe_set = set(_ALWAYS_SAFE) | set(safe)
    out = []
    for c in bs:
        if c in safe_set:
            out.append(chr(c))
        else:
            out.append('%{:02X}'.format(c))
    return ''.join(out)


def urlencode(query, doseq=False, safe='', encoding=None, errors=None,
              quote_via=quote_plus):
    if hasattr(query, "items"):
        query = query.items()
    else:
        try:
            if len(query) and not isinstance(query[0], tuple):
                raise TypeError
        except TypeError as err:
            raise TypeError("not a valid non-string sequence "
                            "or mapping object") from err
    l = []
    if not doseq:
        for k, v in query:
            if isinstance(k, bytes):
                k = quote_via(k, safe)
            else:
                k = quote_via(str(k), safe, encoding, errors)
            if isinstance(v, bytes):
                v = quote_via(v, safe)
            else:
                v = quote_via(str(v), safe, encoding, errors)
            l.append(k + '=' + v)
    else:
        for k, v in query:
            if isinstance(k, bytes):
                k = quote_via(k, safe)
            else:
                k = quote_via(str(k), safe, encoding, errors)
            if isinstance(v, bytes):
                v = quote_via(v, safe)
                l.append(k + '=' + v)
            elif isinstance(v, str):
                v = quote_via(v, safe, encoding, errors)
                l.append(k + '=' + v)
            else:
                try:
                    x = len(v)
                except TypeError:
                    v = quote_via(str(v), safe, encoding, errors)
                    l.append(k + '=' + v)
                else:
                    for elt in v:
                        if isinstance(elt, bytes):
                            elt = quote_via(elt, safe)
                        else:
                            elt = quote_via(str(elt), safe, encoding, errors)
                        l.append(k + '=' + elt)
    return '&'.join(l)


def splittype(url):
    scheme, _, rest = url.partition(':')
    if _ and scheme and all(c in scheme_chars for c in scheme):
        return scheme.lower(), rest
    return None, url


def _splittype(url):
    return splittype(url)


def _splithost(url):
    if url[:2] == '//':
        netloc, path = _splitnetloc(url, 2)
        if path and path[0] != '/':
            path = '/' + path
        return netloc, path
    return None, url


def _splitport(host):
    host, _, port = host.rpartition(':') if ':' in host and not host.endswith(']') else (host, '', '')
    if not _:
        return host, None
    if port.isdigit():
        return host, port
    return host + ':' + port, None
