from urllib.parse import (urlparse, urlsplit, urlunparse, urljoin, urlencode, quote,
                          quote_plus, unquote, unquote_plus, parse_qs, parse_qsl, urldefrag)
import shlex
import http
import http.client

u = urlparse("https://user:pw@Example.COM:8443/a/b;params?x=1&y=2#frag")
print(u)
print(u.scheme, u.netloc, u.hostname, u.port, u.username, u.password, u.geturl())
print(urlsplit("//host/path?q"), urlsplit("mailto:someone@example.com"))
print(urlunparse(("http", "h", "/p", "", "a=b", "")))
base = "http://a/b/c/d;p?q"
for ref in ["g", "./g", "g/", "/g", "//g", "?y", "g?y", "#s", "g#s", "..", "../g", "../..", "../../g",
            "../../../g", "g.", ".g", "g..", "..g", "./../g", "g;x=1/../y", "http:g", ""]:
    print(repr(ref), urljoin(base, ref))
print(urlencode({"a": "1 2", "b": "x&y", "c": ["p", "q"]}))
print(urlencode({"c": ["p", "q"]}, doseq=True), urlencode([("k", "v/w")], safe="/"))
print(quote("a b/c?d=é"), quote("a b/c", safe=""), quote_plus("a b&c"), quote(b"\x00\xff"))
print(unquote("%E2%82%AC%20x%zz"), unquote_plus("a+b%2Bc"))
print(parse_qs("a=1&a=2&b=&c"), parse_qs("a=1&b=", keep_blank_values=True))
print(parse_qsl("x=%20y&z=a+b"), urldefrag("http://x/y#z"))
try:
    urlparse("http://[::1").hostname
except ValueError as e:
    print("ValueError", e)
print(shlex.split("echo 'hello world' \"a b\" c\\ d"), shlex.quote("it's"), shlex.join(["a b", "c"]))
print(http.HTTPStatus.NOT_FOUND, int(http.HTTPStatus.OK), http.HTTPStatus(404).phrase)
print(http.client.responses[418], http.client.HTTPS_PORT)
