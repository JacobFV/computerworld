"""urllib.response: file-like wrappers with headers and a URL."""

__all__ = ['addbase', 'addclosehook', 'addinfo', 'addinfourl']


class addbase:
    def __init__(self, fp):
        self.fp = fp
        self.closed = False

    def __repr__(self):
        return '<%s at %r whose fp = %r>' % (self.__class__.__name__, id(self), self.fp)

    def read(self, *a):
        return self.fp.read(*a)

    def readline(self, *a):
        return self.fp.readline(*a)

    def readlines(self, *a):
        return self.fp.readlines(*a)

    def __iter__(self):
        return iter(self.fp)

    def __enter__(self):
        if self.closed:
            raise ValueError("I/O operation on closed file")
        return self

    def __exit__(self, type, value, traceback):
        self.close()

    def close(self):
        self.closed = True
        if self.fp is not None and hasattr(self.fp, 'close'):
            self.fp.close()


class addclosehook(addbase):
    def __init__(self, fp, closehook, *hookargs):
        super().__init__(fp)
        self.closehook = closehook
        self.hookargs = hookargs

    def close(self):
        try:
            if self.closehook:
                self.closehook(*self.hookargs)
        finally:
            super().close()


class addinfo(addbase):
    def __init__(self, fp, headers):
        super().__init__(fp)
        self.headers = headers

    def info(self):
        return self.headers


class addinfourl(addinfo):
    def __init__(self, fp, headers, url, code=None):
        super().__init__(fp, headers)
        self.url = url
        self.code = code

    @property
    def status(self):
        return self.code

    def getcode(self):
        return self.code

    def geturl(self):
        return self.url
