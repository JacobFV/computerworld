"""logging for the simulated interpreter (records carry world-clock time)."""
import sys
import time as _time

CRITICAL = 50
FATAL = CRITICAL
ERROR = 40
WARNING = 30
WARN = WARNING
INFO = 20
DEBUG = 10
NOTSET = 0

_levelToName = {CRITICAL: 'CRITICAL', ERROR: 'ERROR', WARNING: 'WARNING', INFO: 'INFO',
                DEBUG: 'DEBUG', NOTSET: 'NOTSET'}
_nameToLevel = {'CRITICAL': CRITICAL, 'FATAL': FATAL, 'ERROR': ERROR, 'WARN': WARNING,
                'WARNING': WARNING, 'INFO': INFO, 'DEBUG': DEBUG, 'NOTSET': NOTSET}

BASIC_FORMAT = "%(levelname)s:%(name)s:%(message)s"
_start = _time.time()


def getLevelName(level):
    result = _levelToName.get(level)
    if result is not None:
        return result
    result = _nameToLevel.get(level)
    if result is not None:
        return result
    return "Level %s" % level


def addLevelName(level, levelName):
    _levelToName[level] = levelName
    _nameToLevel[levelName] = level


def _checkLevel(level):
    if isinstance(level, int):
        return level
    if str(level) == level:
        if level not in _nameToLevel:
            raise ValueError("Unknown level: %r" % level)
        return _nameToLevel[level]
    raise TypeError("Level not an integer or a valid string: %r" % (level,))


class LogRecord:
    def __init__(self, name, level, pathname, lineno, msg, args, exc_info, func=None, sinfo=None):
        ct = _time.time()
        self.name = name
        self.msg = msg
        if args and len(args) == 1 and isinstance(args[0], dict) and args[0]:
            args = args[0]
        self.args = args
        self.levelname = getLevelName(level)
        self.levelno = level
        self.pathname = pathname
        self.filename = pathname.rsplit('/', 1)[-1]
        self.module = self.filename.rsplit('.', 1)[0]
        self.exc_info = exc_info
        self.exc_text = None
        self.lineno = lineno
        self.funcName = func
        self.created = ct
        self.msecs = int((ct - int(ct)) * 1000) + 0.0
        self.relativeCreated = (ct - _start) * 1000
        self.thread = 1
        self.threadName = 'MainThread'
        self.process = 1
        self.processName = 'MainProcess'

    def getMessage(self):
        msg = str(self.msg)
        if self.args:
            msg = msg % self.args
        return msg

    def __repr__(self):
        return '<LogRecord: %s, %s, %s, %s, "%s">' % (self.name, self.levelno, self.pathname, self.lineno, self.msg)


class PercentStyle:
    default_format = '%(message)s'

    def __init__(self, fmt):
        self._fmt = fmt or self.default_format

    def usesTime(self):
        return '%(asctime)' in self._fmt

    def format(self, record):
        return self._fmt % record.__dict__


class StrFormatStyle(PercentStyle):
    default_format = '{message}'

    def usesTime(self):
        return '{asctime' in self._fmt

    def format(self, record):
        return self._fmt.format(**record.__dict__)


_STYLES = {'%': PercentStyle, '{': StrFormatStyle}


class Formatter:
    default_time_format = '%Y-%m-%d %H:%M:%S'
    default_msec_format = '%s,%03d'

    def __init__(self, fmt=None, datefmt=None, style='%', validate=True, *, defaults=None):
        self._style = _STYLES[style](fmt)
        self._fmt = self._style._fmt
        self.datefmt = datefmt

    def formatTime(self, record, datefmt=None):
        ct = _time.gmtime(record.created)
        if datefmt:
            return _time.strftime(datefmt, ct)
        s = _time.strftime(self.default_time_format, ct)
        return self.default_msec_format % (s, record.msecs)

    def formatException(self, ei):
        import traceback
        return ''.join(traceback.format_exception(*ei)).rstrip('\n')

    def usesTime(self):
        return self._style.usesTime()

    def formatMessage(self, record):
        return self._style.format(record)

    def format(self, record):
        record.message = record.getMessage()
        if self.usesTime():
            record.asctime = self.formatTime(record, self.datefmt)
        s = self.formatMessage(record)
        if record.exc_info:
            if not record.exc_text:
                record.exc_text = self.formatException(record.exc_info)
        if record.exc_text:
            if s[-1:] != "\n":
                s = s + "\n"
            s = s + record.exc_text
        return s


_defaultFormatter = Formatter()


class Filter:
    def __init__(self, name=''):
        self.name = name
        self.nlen = len(name)

    def filter(self, record):
        if self.nlen == 0:
            return True
        if self.name == record.name:
            return True
        return record.name.startswith(self.name + '.')


class Filterer:
    def __init__(self):
        self.filters = []

    def addFilter(self, filter):
        if filter not in self.filters:
            self.filters.append(filter)

    def removeFilter(self, filter):
        if filter in self.filters:
            self.filters.remove(filter)

    def filter(self, record):
        for f in self.filters:
            result = f.filter(record) if hasattr(f, 'filter') else f(record)
            if not result:
                return False
        return True


class Handler(Filterer):
    def __init__(self, level=NOTSET):
        Filterer.__init__(self)
        self.level = _checkLevel(level)
        self.formatter = None
        self.name = None

    def setLevel(self, level):
        self.level = _checkLevel(level)

    def setFormatter(self, fmt):
        self.formatter = fmt

    def format(self, record):
        fmt = self.formatter or _defaultFormatter
        return fmt.format(record)

    def handle(self, record):
        if self.filter(record):
            self.emit(record)
        return True

    def emit(self, record):
        raise NotImplementedError('emit must be implemented by Handler subclasses')

    def flush(self):
        pass

    def close(self):
        pass

    def handleError(self, record):
        pass


class StreamHandler(Handler):
    terminator = '\n'

    def __init__(self, stream=None):
        Handler.__init__(self)
        self._stream = stream

    @property
    def stream(self):
        return self._stream if self._stream is not None else sys.stderr

    def emit(self, record):
        msg = self.format(record)
        self.stream.write(msg + self.terminator)

    def setStream(self, stream):
        old = self._stream
        self._stream = stream
        return old


class FileHandler(StreamHandler):
    def __init__(self, filename, mode='a', encoding=None, delay=False, errors=None):
        self.baseFilename = filename
        self.mode = mode
        StreamHandler.__init__(self, open(filename, mode, encoding=encoding))

    def close(self):
        self._stream.close()


class NullHandler(Handler):
    def emit(self, record):
        pass


class _StderrHandler(StreamHandler):
    def __init__(self, level=NOTSET):
        Handler.__init__(self, level)

    @property
    def stream(self):
        return sys.stderr


lastResort = _StderrHandler(WARNING)


class Logger(Filterer):
    def __init__(self, name, level=NOTSET):
        Filterer.__init__(self)
        self.name = name
        self.level = _checkLevel(level)
        self.parent = None
        self.propagate = True
        self.handlers = []
        self.disabled = False

    def setLevel(self, level):
        self.level = _checkLevel(level)

    def getEffectiveLevel(self):
        logger = self
        while logger:
            if logger.level:
                return logger.level
            logger = logger.parent
        return NOTSET

    def isEnabledFor(self, level):
        if _disable_level >= level:
            return False
        return level >= self.getEffectiveLevel()

    def _log(self, level, msg, args, exc_info=None, extra=None, stack_info=False, stacklevel=1):
        if exc_info:
            if isinstance(exc_info, BaseException):
                exc_info = (type(exc_info), exc_info, exc_info.__traceback__)
            elif not isinstance(exc_info, tuple):
                exc_info = sys.exc_info()
        record = LogRecord(self.name, level, sys.argv[0] if sys.argv and sys.argv[0] else '<stdin>', 0, msg, args, exc_info)
        if extra:
            for key in extra:
                record.__dict__[key] = extra[key]
        self.handle(record)

    def handle(self, record):
        if self.disabled or not self.filter(record):
            return
        c = self
        found = 0
        while c:
            for hdlr in c.handlers:
                found += 1
                if record.levelno >= hdlr.level:
                    hdlr.handle(record)
            if not c.propagate:
                c = None
            else:
                c = c.parent
        if found == 0 and lastResort and record.levelno >= lastResort.level:
            lastResort.handle(record)

    def debug(self, msg, *args, **kwargs):
        if self.isEnabledFor(DEBUG):
            self._log(DEBUG, msg, args, **kwargs)

    def info(self, msg, *args, **kwargs):
        if self.isEnabledFor(INFO):
            self._log(INFO, msg, args, **kwargs)

    def warning(self, msg, *args, **kwargs):
        if self.isEnabledFor(WARNING):
            self._log(WARNING, msg, args, **kwargs)

    warn = warning

    def error(self, msg, *args, **kwargs):
        if self.isEnabledFor(ERROR):
            self._log(ERROR, msg, args, **kwargs)

    def exception(self, msg, *args, exc_info=True, **kwargs):
        self.error(msg, *args, exc_info=exc_info, **kwargs)

    def critical(self, msg, *args, **kwargs):
        if self.isEnabledFor(CRITICAL):
            self._log(CRITICAL, msg, args, **kwargs)

    fatal = critical

    def log(self, level, msg, *args, **kwargs):
        if self.isEnabledFor(level):
            self._log(level, msg, args, **kwargs)

    def addHandler(self, hdlr):
        if hdlr not in self.handlers:
            self.handlers.append(hdlr)

    def removeHandler(self, hdlr):
        if hdlr in self.handlers:
            self.handlers.remove(hdlr)

    def hasHandlers(self):
        c = self
        while c:
            if c.handlers:
                return True
            c = c.parent if c.propagate else None
        return False

    def getChild(self, suffix):
        return getLogger(self.name + '.' + suffix if self.name != 'root' else suffix)

    def __repr__(self):
        return '<%s %s (%s)>' % (self.__class__.__name__, self.name, getLevelName(self.getEffectiveLevel()))


class RootLogger(Logger):
    def __init__(self, level):
        Logger.__init__(self, "root", level)


root = RootLogger(WARNING)
_loggers = {}
_disable_level = 0


def getLogger(name=None):
    if not name or name == 'root':
        return root
    if name in _loggers:
        return _loggers[name]
    logger = Logger(name)
    _loggers[name] = logger
    parent = root
    parts = name.split('.')
    for i in range(len(parts) - 1, 0, -1):
        pname = '.'.join(parts[:i])
        if pname in _loggers:
            parent = _loggers[pname]
            break
    logger.parent = parent
    for other in _loggers.values():
        if other.name.startswith(name + '.') and other.parent is parent:
            other.parent = logger
    return logger


def basicConfig(**kwargs):
    force = kwargs.pop('force', False)
    if force:
        for h in root.handlers[:]:
            root.removeHandler(h)
    if root.handlers:
        return
    handlers = kwargs.pop('handlers', None)
    if handlers is None:
        filename = kwargs.pop('filename', None)
        mode = kwargs.pop('filemode', 'a')
        if filename:
            h = FileHandler(filename, mode, encoding=kwargs.pop('encoding', None))
        else:
            h = StreamHandler(kwargs.pop('stream', None))
        handlers = [h]
    dfs = kwargs.pop('datefmt', None)
    style = kwargs.pop('style', '%')
    fs = kwargs.pop('format', BASIC_FORMAT if style == '%' else '{levelname}:{name}:{message}')
    fmt = Formatter(fs, dfs, style)
    for h in handlers:
        if h.formatter is None:
            h.setFormatter(fmt)
        root.addHandler(h)
    level = kwargs.pop('level', None)
    if level is not None:
        root.setLevel(level)


def debug(msg, *args, **kwargs):
    if len(root.handlers) == 0:
        basicConfig()
    root.debug(msg, *args, **kwargs)


def info(msg, *args, **kwargs):
    if len(root.handlers) == 0:
        basicConfig()
    root.info(msg, *args, **kwargs)


def warning(msg, *args, **kwargs):
    if len(root.handlers) == 0:
        basicConfig()
    root.warning(msg, *args, **kwargs)


warn = warning


def error(msg, *args, **kwargs):
    if len(root.handlers) == 0:
        basicConfig()
    root.error(msg, *args, **kwargs)


def exception(msg, *args, exc_info=True, **kwargs):
    error(msg, *args, exc_info=exc_info, **kwargs)


def critical(msg, *args, **kwargs):
    if len(root.handlers) == 0:
        basicConfig()
    root.critical(msg, *args, **kwargs)


fatal = critical


def log(level, msg, *args, **kwargs):
    if len(root.handlers) == 0:
        basicConfig()
    root.log(level, msg, *args, **kwargs)


def disable(level=CRITICAL):
    global _disable_level
    _disable_level = level


def shutdown():
    pass


def captureWarnings(capture):
    pass


class LoggerAdapter:
    def __init__(self, logger, extra=None):
        self.logger = logger
        self.extra = extra

    def process(self, msg, kwargs):
        kwargs["extra"] = self.extra
        return msg, kwargs

    def log(self, level, msg, *args, **kwargs):
        msg, kwargs = self.process(msg, kwargs)
        self.logger.log(level, msg, *args, **kwargs)

    def debug(self, msg, *args, **kwargs):
        self.log(DEBUG, msg, *args, **kwargs)

    def info(self, msg, *args, **kwargs):
        self.log(INFO, msg, *args, **kwargs)

    def warning(self, msg, *args, **kwargs):
        self.log(WARNING, msg, *args, **kwargs)

    def error(self, msg, *args, **kwargs):
        self.log(ERROR, msg, *args, **kwargs)

    def critical(self, msg, *args, **kwargs):
        self.log(CRITICAL, msg, *args, **kwargs)
