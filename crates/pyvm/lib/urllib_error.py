"""urllib.error: URLError, HTTPError, ContentTooShortError."""
import urllib.response

__all__ = ['URLError', 'HTTPError', 'ContentTooShortError']


class URLError(OSError):
    def __init__(self, reason, filename=None):
        self.args = reason,
        self.reason = reason
        if filename is not None:
            self.filename = filename

    def __str__(self):
        return '<urlopen error %s>' % self.reason


class HTTPError(URLError, urllib.response.addinfourl):
    """Raised for an HTTP error status; it is also a response (read(), headers)."""
    __super_init = urllib.response.addinfourl.__init__

    def __init__(self, url, code, msg, hdrs, fp):
        self.code = code
        self.msg = msg
        self.hdrs = hdrs
        self.fp = fp
        if fp is not None:
            self.__super_init(fp, hdrs, url, code)
        else:
            self.url = url
            self.headers = hdrs

    def __str__(self):
        return 'HTTP Error %s: %s' % (self.code, self.msg)

    def __repr__(self):
        return '<HTTPError %s: %r>' % (self.code, self.msg)

    @property
    def reason(self):
        return self.msg

    @reason.setter
    def reason(self, value):
        pass

    def info(self):
        return self.hdrs


class ContentTooShortError(URLError):
    def __init__(self, message, content):
        URLError.__init__(self, message)
        self.content = content
