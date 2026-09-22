"""warnings for the simulated interpreter."""
import sys

filters = []
_onceregistry = {}
_seen = set()


def formatwarning(message, category, filename, lineno, line=None):
    s = f"{filename}:{lineno}: {category.__name__}: {message}\n"
    if line is None:
        line = sys._source_line(filename, lineno)
    if line:
        s += f"  {line.strip()}\n"
    return s


def showwarning(message, category, filename, lineno, file=None, line=None):
    if file is None:
        file = sys.stderr
    file.write(formatwarning(message, category, filename, lineno, line))


def _action_for(category, message):
    for action, msg, cat, mod, lineno in filters:
        if (msg is None or msg.match(str(message))) and issubclass(category, cat):
            return action
    if issubclass(category, (DeprecationWarning, PendingDeprecationWarning, ImportWarning, ResourceWarning)):
        return 'ignore'
    return 'default'


def warn(message, category=None, stacklevel=1, source=None, *, skip_file_prefixes=()):
    if isinstance(message, Warning):
        category = message.__class__
    if category is None:
        category = UserWarning
    loc = sys._stack()
    idx = min(stacklevel, len(loc) - 1)
    filename, lineno, _ = loc[idx] if loc else ('sys', 1, '')
    action = _action_for(category, message)
    if action == 'error':
        raise category(message) if not isinstance(message, Warning) else message
    if action == 'ignore':
        return
    key = (str(message), category, filename, lineno)
    if action in ('default', 'module', 'once'):
        if key in _seen:
            return
        _seen.add(key)
    showwarning(message, category, filename, lineno)


def warn_explicit(message, category, filename, lineno, module=None, registry=None, module_globals=None, source=None):
    showwarning(message, category, filename, lineno)


def filterwarnings(action, message="", category=Warning, module="", lineno=0, append=False):
    import re
    item = (action, re.compile(message, re.I) if message else None, category, module, lineno)
    if append:
        filters.append(item)
    else:
        filters.insert(0, item)


def simplefilter(action, category=Warning, lineno=0, append=False):
    item = (action, None, category, None, lineno)
    if append:
        filters.append(item)
    else:
        filters.insert(0, item)


def resetwarnings():
    filters[:] = []


class catch_warnings:
    def __init__(self, *, record=False, module=None, action=None, category=Warning, lineno=0, append=False):
        self._record = record
        self._action = action
        self._category = category

    def __enter__(self):
        global showwarning
        self._filters = filters[:]
        self._showwarning = showwarning
        if self._action:
            simplefilter(self._action, self._category)
        if self._record:
            log = []

            def _record(message, category, filename, lineno, file=None, line=None):
                log.append(WarningMessage(message, category, filename, lineno))
            showwarning = _record
            return log
        return None

    def __exit__(self, *exc_info):
        global showwarning
        filters[:] = self._filters
        showwarning = self._showwarning


class WarningMessage:
    def __init__(self, message, category, filename, lineno, file=None, line=None, source=None):
        self.message = message if isinstance(message, Warning) else category(message)
        self.category = category
        self.filename = filename
        self.lineno = lineno


def deprecated(msg, /, *, category=DeprecationWarning, stacklevel=1):
    def decorator(arg):
        return arg
    return decorator
