"""traceback for the simulated interpreter (reports use the VM's traceback data)."""
import sys

__all__ = ['format_exc', 'format_exception', 'format_exception_only', 'print_exc',
           'print_exception', 'format_tb', 'print_tb', 'print_stack', 'format_stack',
           'extract_stack', 'extract_tb', 'TracebackException']


def _value_of(exc, value=None):
    if value is None or isinstance(exc, BaseException):
        return exc if isinstance(exc, BaseException) else value
    return value


def format_exception(exc, /, value=None, tb=None, limit=None, chain=True):
    v = exc if isinstance(exc, BaseException) else value
    if v is None:
        return ['NoneType: None\n']
    text = sys._format_exception(v, chain)
    return [line + '\n' for line in text.rstrip('\n').split('\n')]


def format_exception_only(exc, /, value=None):
    v = exc if isinstance(exc, BaseException) else value
    if v is None:
        return ['NoneType: None\n']
    lines = format_exception(v, chain=False)
    out = []
    for line in lines:
        if line.startswith('Traceback') or line.startswith('  '):
            continue
        out.append(line)
    return out[-1:] if out else ['NoneType: None\n']


def format_exc(limit=None, chain=True):
    return ''.join(format_exception(sys.exception(), limit=limit, chain=chain))


def print_exception(exc, /, value=None, tb=None, limit=None, file=None, chain=True):
    if file is None:
        file = sys.stderr
    for line in format_exception(exc, value, tb, limit=limit, chain=chain):
        file.write(line)


def print_exc(limit=None, file=None, chain=True):
    print_exception(sys.exception(), limit=limit, file=file, chain=chain)


def format_tb(tb, limit=None):
    return []


def print_tb(tb, limit=None, file=None):
    pass


def extract_tb(tb, limit=None):
    return []


def format_stack(f=None, limit=None):
    out = []
    for filename, lineno, name in reversed(sys._stack()[1:]):
        out.append(f'  File "{filename}", line {lineno}, in {name}\n')
    return out


def print_stack(f=None, limit=None, file=None):
    if file is None:
        file = sys.stderr
    for line in format_stack():
        file.write(line)


class FrameSummary:
    def __init__(self, filename, lineno, name, line=None):
        self.filename = filename
        self.lineno = lineno
        self.name = name
        self.line = line

    def __iter__(self):
        return iter((self.filename, self.lineno, self.name, self.line))

    def __repr__(self):
        return f"<FrameSummary file {self.filename}, line {self.lineno} in {self.name}>"


def extract_stack(f=None, limit=None):
    return [FrameSummary(fn, ln, nm) for fn, ln, nm in reversed(sys._stack()[1:])]


class TracebackException:
    def __init__(self, exc_type, exc_value, exc_traceback, **kwargs):
        self.exc_type = exc_type
        self._value = exc_value

    @classmethod
    def from_exception(cls, exc, **kwargs):
        return cls(type(exc), exc, None, **kwargs)

    def format(self, *, chain=True):
        return iter(format_exception(self._value, chain=chain))

    def format_exception_only(self):
        return iter(format_exception_only(self._value))
