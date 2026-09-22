"""urllib.request over the simulated network (CPython 3.12's request, handler and
redirect semantics). `http:`/`https:` go through http.client and the world's
network; `file:` reads the machine's files; `data:` URLs decode inline."""
import base64
import io
import os
import socket
import sys
import http.client
from urllib.error import URLError, HTTPError
from urllib.parse import (urlparse, urlsplit, urljoin, unquote, quote, _splittype,
                          _splithost, urlunparse, unquote_to_bytes)
from urllib.response import addinfourl, addclosehook

__all__ = ['Request', 'OpenerDirector', 'BaseHandler', 'HTTPDefaultErrorHandler',
           'HTTPRedirectHandler', 'HTTPCookieProcessor', 'ProxyHandler', 'HTTPHandler',
           'HTTPSHandler', 'FileHandler', 'DataHandler', 'UnknownHandler',
           'HTTPErrorProcessor', 'urlopen', 'install_opener', 'build_opener',
           'pathname2url', 'url2pathname', 'getproxies', 'urlretrieve', 'urlcleanup']

__version__ = '%d.%d' % sys.version_info[:2]

_opener = None


def urlopen(url, data=None, timeout=socket._GLOBAL_DEFAULT_TIMEOUT, *, context=None):
    global _opener
    if _opener is None:
        _opener = opener = build_opener()
    else:
        opener = _opener
    return opener.open(url, data, timeout)


def install_opener(opener):
    global _opener
    _opener = opener


def urlretrieve(url, filename=None, reporthook=None, data=None):
    with urlopen(url, data) as fp:
        headers = fp.info()
        body = fp.read()
    if filename is None:
        import tempfile
        filename = tempfile.mktemp()
    with open(filename, 'wb') as f:
        f.write(body)
    if reporthook:
        reporthook(1, len(body), len(body))
    return filename, headers


def urlcleanup():
    global _opener
    _opener = None


def request_host(request):
    url = request.full_url
    host = urlparse(url)[1]
    if host == "":
        host = request.get_header("Host", "")
    return host.rpartition(':')[0].lower() if ':' in host and not host.endswith(']') else host.lower()


class Request:
    def __init__(self, url, data=None, headers={}, origin_req_host=None,
                 unverifiable=False, method=None):
        self.full_url = url
        self.headers = {}
        self.unredirected_hdrs = {}
        self._data = None
        self.data = data
        self._tunnel_host = None
        for key, value in headers.items():
            self.add_header(key, value)
        if origin_req_host is None:
            origin_req_host = request_host(self)
        self.origin_req_host = origin_req_host
        self.unverifiable = unverifiable
        if method:
            self.method = method

    @property
    def full_url(self):
        if self.fragment:
            return '{}#{}'.format(self._full_url, self.fragment)
        return self._full_url

    @full_url.setter
    def full_url(self, url):
        self._full_url = url
        self._full_url, _, self.fragment = self._full_url.partition('#')
        if not _:
            self.fragment = None
        self._parse()

    @full_url.deleter
    def full_url(self):
        self._full_url = None
        self.fragment = None
        self.selector = ''

    @property
    def data(self):
        return self._data

    @data.setter
    def data(self, data):
        if data != self._data:
            self._data = data
            if self.has_header("Content-length"):
                self.remove_header("Content-length")

    @data.deleter
    def data(self):
        self.data = None

    def _parse(self):
        self.type, rest = _splittype(self._full_url)
        if self.type is None:
            raise ValueError("unknown url type: %r" % self.full_url)
        self.host, self.selector = _splithost(rest)
        if self.host:
            self.host = unquote(self.host)

    def get_method(self):
        default_method = "POST" if self.data is not None else "GET"
        return getattr(self, 'method', default_method)

    def get_full_url(self):
        return self.full_url

    def set_proxy(self, host, type):
        self.host = host
        self.type = type

    def has_proxy(self):
        return False

    def add_header(self, key, val):
        self.headers[key.capitalize()] = val

    def add_unredirected_header(self, key, val):
        self.unredirected_hdrs[key.capitalize()] = val

    def has_header(self, header_name):
        return (header_name in self.headers or header_name in self.unredirected_hdrs)

    def get_header(self, header_name, default=None):
        return self.headers.get(header_name,
                                self.unredirected_hdrs.get(header_name, default))

    def remove_header(self, header_name):
        self.headers.pop(header_name, None)
        self.unredirected_hdrs.pop(header_name, None)

    def header_items(self):
        hdrs = {**self.unredirected_hdrs, **self.headers}
        return list(hdrs.items())

    def __repr__(self):
        return '<urllib.request.Request object at 0x%x>' % id(self)


class OpenerDirector:
    def __init__(self):
        client_version = "Python-urllib/%s" % __version__
        self.addheaders = [('User-agent', client_version)]
        self.handlers = []

    def add_handler(self, handler):
        self.handlers.append(handler)
        self.handlers.sort(key=lambda h: h.handler_order)
        handler.add_parent(self)

    def close(self):
        pass

    def _find(self, prefix, suffix):
        for h in self.handlers:
            m = getattr(h, prefix + '_' + suffix, None)
            if m is not None:
                yield m

    def open(self, fullurl, data=None, timeout=socket._GLOBAL_DEFAULT_TIMEOUT):
        if isinstance(fullurl, str):
            req = Request(fullurl, data)
        else:
            req = fullurl
            if data is not None:
                req.data = data
        req.timeout = timeout
        protocol = req.type
        for pre in self._find(protocol, 'request'):
            req = pre(req)
        response = self._open(req, data)
        for post in self._find(protocol, 'response'):
            response = post(req, response)
        return response

    def _open(self, req, data=None):
        for m in self._find(req.type, 'open'):
            r = m(req)
            if r is not None:
                return r
        for m in self._find('unknown', 'open'):
            r = m(req)
            if r is not None:
                return r
        raise URLError('unknown url type: %s' % req.type)

    def error(self, proto, *args):
        if proto in ('http', 'https'):
            code = args[3]
            for m in self._find('http_error', str(code)):
                r = m(*args)
                if r is not None:
                    return r
            for m in self._find('http_error', 'default'):
                r = m(*args)
                if r is not None:
                    return r
        return None


def build_opener(*handlers):
    opener = OpenerDirector()
    default_classes = [ProxyHandler, UnknownHandler, HTTPHandler, HTTPDefaultErrorHandler,
                       HTTPRedirectHandler, FTPHandler, FileHandler, HTTPErrorProcessor,
                       DataHandler, HTTPSHandler]
    skip = set()
    for klass in default_classes:
        for check in handlers:
            if isinstance(check, type):
                if issubclass(check, klass):
                    skip.add(klass)
            elif isinstance(check, klass):
                skip.add(klass)
    for klass in skip:
        default_classes.remove(klass)
    for klass in default_classes:
        opener.add_handler(klass())
    for h in handlers:
        if isinstance(h, type):
            h = h()
        opener.add_handler(h)
    return opener


class BaseHandler:
    handler_order = 500

    def add_parent(self, parent):
        self.parent = parent

    def close(self):
        pass

    def __lt__(self, other):
        return self.handler_order < getattr(other, 'handler_order', 500)


class HTTPErrorProcessor(BaseHandler):
    handler_order = 1000

    def http_response(self, request, response):
        code, msg, hdrs = response.code, response.msg, response.info()
        if not (200 <= code < 300):
            response = self.parent.error('http', request, response, code, msg, hdrs)
        return response

    https_response = http_response


class HTTPDefaultErrorHandler(BaseHandler):
    def http_error_default(self, req, fp, code, msg, hdrs):
        raise HTTPError(req.full_url, code, msg, hdrs, fp)


class HTTPRedirectHandler(BaseHandler):
    max_repeats = 4
    max_redirections = 10

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        m = req.get_method()
        if (not (code in (301, 302, 303, 307, 308) and m in ("GET", "HEAD")
                 or code in (301, 302, 303) and m == "POST")):
            raise HTTPError(req.full_url, code, msg, headers, fp)
        newurl = newurl.replace(' ', '%20')
        CONTENT_HEADERS = ("content-length", "content-type")
        newheaders = {k: v for k, v in req.headers.items() if k.lower() not in CONTENT_HEADERS}
        return Request(newurl, method="HEAD" if m == "HEAD" else "GET", headers=newheaders,
                       origin_req_host=req.origin_req_host, unverifiable=True)

    def http_error_302(self, req, fp, code, msg, headers):
        if "location" in headers:
            newurl = headers["location"]
        elif "uri" in headers:
            newurl = headers["uri"]
        else:
            return
        urlparts = urlparse(newurl)
        if urlparts.scheme not in ('http', 'https', 'ftp', ''):
            raise HTTPError(newurl, code,
                            "%s - Redirection to url '%s' is not allowed" % (msg, newurl),
                            headers, fp)
        if not urlparts.path and urlparts.netloc:
            urlparts = list(urlparts)
            urlparts[2] = "/"
        newurl = urlunparse(urlparts)
        newurl = quote(newurl, encoding="iso-8859-1", safe="/%#:;?&=+$,@!~*'()[]")
        newurl = urljoin(req.full_url, newurl)
        new = self.redirect_request(req, fp, code, msg, headers, newurl)
        if new is None:
            return
        if hasattr(req, 'redirect_dict'):
            visited = new.redirect_dict = req.redirect_dict
            if (visited.get(newurl, 0) >= self.max_repeats or
                    len(visited) >= self.max_redirections):
                raise HTTPError(req.full_url, code, self.inf_msg + msg, headers, fp)
        else:
            visited = new.redirect_dict = req.redirect_dict = {}
        visited[newurl] = visited.get(newurl, 0) + 1
        fp.read()
        fp.close()
        return self.parent.open(new, timeout=req.timeout)

    http_error_301 = http_error_303 = http_error_307 = http_error_308 = http_error_302

    inf_msg = "The HTTP server returned a redirect error that would " \
              "lead to an infinite loop.\n" \
              "The last 30x error message was:\n"


class ProxyHandler(BaseHandler):
    handler_order = 100

    def __init__(self, proxies=None):
        self.proxies = proxies or {}


class HTTPCookieProcessor(BaseHandler):
    def __init__(self, cookiejar=None):
        self.cookiejar = cookiejar
        self.cookies = {}

    def http_request(self, request):
        host = request.host
        if host in self.cookies and self.cookies[host]:
            request.add_unredirected_header(
                'Cookie', '; '.join('%s=%s' % kv for kv in self.cookies[host].items()))
        return request

    def http_response(self, request, response):
        for v in response.info().get_all('Set-Cookie') or []:
            k, _, rest = v.partition('=')
            self.cookies.setdefault(request.host, {})[k.strip()] = rest.split(';')[0]
        return response

    https_request = http_request
    https_response = http_response


class AbstractHTTPHandler(BaseHandler):
    def __init__(self, debuglevel=None):
        self._debuglevel = debuglevel or 0

    def set_http_debuglevel(self, level):
        self._debuglevel = level

    def do_request_(self, request):
        host = request.host
        if not host:
            raise URLError('no host given')
        if request.data is not None:
            data = request.data
            if isinstance(data, str):
                msg = "POST data should be bytes, an iterable of bytes, or a file object. " \
                      "It cannot be of type str."
                raise TypeError(msg)
            if not request.has_header('Content-type'):
                request.add_unredirected_header(
                    'Content-type', 'application/x-www-form-urlencoded')
            if (not request.has_header('Content-length')
                    and not request.has_header('Transfer-encoding')):
                if isinstance(data, (bytes, bytearray)):
                    request.add_unredirected_header('Content-length', '%d' % len(data))
                else:
                    try:
                        request.add_unredirected_header('Content-length',
                                                        '%d' % len(memoryview(data)))
                    except Exception:
                        request.add_unredirected_header('Transfer-encoding', 'chunked')
        sel_host = host
        request.add_unredirected_header('Host', sel_host) if not request.has_header('Host') else None
        for name, value in self.parent.addheaders:
            name = name.capitalize()
            if not request.has_header(name):
                request.add_unredirected_header(name, value)
        return request

    def do_open(self, http_class, req, **http_conn_args):
        host = req.host
        if not host:
            raise URLError('no host given')
        h = http_class(host, timeout=req.timeout, **http_conn_args)
        h.set_debuglevel(self._debuglevel)
        headers = dict(req.unredirected_hdrs)
        headers.update({k: v for k, v in req.headers.items() if k not in headers})
        headers["Connection"] = "close"
        headers = {name.title(): val for name, val in headers.items()}
        try:
            try:
                h.request(req.get_method(), req.selector, req.data, headers,
                          encode_chunked=req.has_header('Transfer-encoding'))
            except OSError as err:
                raise URLError(err)
            r = h.getresponse()
        except:
            h.close()
            raise
        r.url = req.get_full_url()
        r.msg = r.reason
        return r


class HTTPHandler(AbstractHTTPHandler):
    def http_open(self, req):
        return self.do_open(http.client.HTTPConnection, req)

    http_request = AbstractHTTPHandler.do_request_


class HTTPSHandler(AbstractHTTPHandler):
    def __init__(self, debuglevel=None, context=None, check_hostname=None):
        AbstractHTTPHandler.__init__(self, debuglevel)
        self._context = context

    def https_open(self, req):
        return self.do_open(http.client.HTTPSConnection, req, context=self._context)

    https_request = AbstractHTTPHandler.do_request_


class UnknownHandler(BaseHandler):
    def unknown_open(self, req):
        type = req.type
        raise URLError('unknown url type: %s' % type)


class FTPHandler(BaseHandler):
    def ftp_open(self, req):
        raise URLError('ftp error: no FTP service is simulated')


def url2pathname(pathname):
    return unquote(pathname)


def pathname2url(pathname):
    return quote(pathname)


_MIME = {'.txt': 'text/plain', '.html': 'text/html', '.htm': 'text/html',
         '.json': 'application/json', '.py': 'text/x-python', '.js': 'text/javascript',
         '.css': 'text/css', '.csv': 'text/csv', '.png': 'image/png', '.jpg': 'image/jpeg',
         '.gif': 'image/gif', '.pdf': 'application/pdf', '.xml': 'application/xml'}


class FileHandler(BaseHandler):
    def file_open(self, req):
        localfile = url2pathname(req.selector)
        try:
            stats = os.stat(localfile)
            size = stats.st_size
            mtype = _MIME.get(os.path.splitext(localfile)[1].lower())
            headers = http.client.HTTPMessage([
                ('Content-type', mtype or 'text/plain'),
                ('Content-length', str(size)),
            ])
            origurl = 'file://' + (req.host or '') + req.selector
            return addinfourl(open(localfile, 'rb'), headers, origurl)
        except OSError as exp:
            raise URLError(exp, exp.filename)


class DataHandler(BaseHandler):
    def data_open(self, req):
        url = req.full_url
        scheme, data = url.split(":", 1)
        mediatype, data = data.split(",", 1)
        if mediatype.endswith(";base64"):
            data = base64.decodebytes(data.encode('ascii')) if hasattr(base64, 'decodebytes') \
                else base64.b64decode(data)
            mediatype = mediatype[:-7]
        else:
            data = unquote_to_bytes(data)
        if not mediatype:
            mediatype = "text/plain;charset=US-ASCII"
        headers = http.client.HTTPMessage([("Content-type", mediatype),
                                           ("Content-length", str(len(data)))])
        return addinfourl(io.BytesIO(data), headers, url)


def getproxies():
    return {}


def proxy_bypass(host):
    return True
