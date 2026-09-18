"""contextlib for the simulated interpreter."""
import sys as _sys
from functools import wraps


class AbstractContextManager:
    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        return None


class ContextDecorator:
    def _recreate_cm(self):
        return self

    def __call__(self, func):
        @wraps(func)
        def inner(*args, **kwds):
            with self._recreate_cm():
                return func(*args, **kwds)
        return inner


class _GeneratorContextManager(ContextDecorator, AbstractContextManager):
    def __init__(self, func, args, kwds):
        self.gen = func(*args, **kwds)
        self.func, self.args, self.kwds = func, args, kwds
        doc = getattr(func, "__doc__", None)
        if doc is None:
            doc = type(self).__doc__
        self.__doc__ = doc

    def _recreate_cm(self):
        return self.__class__(self.func, self.args, self.kwds)

    def __enter__(self):
        del self.args, self.kwds, self.func
        try:
            return next(self.gen)
        except StopIteration:
            raise RuntimeError("generator didn't yield") from None

    def __exit__(self, typ, value, traceback):
        if typ is None:
            try:
                next(self.gen)
            except StopIteration:
                return False
            else:
                raise RuntimeError("generator didn't stop")
        else:
            if value is None:
                value = typ()
            try:
                self.gen.throw(value)
            except StopIteration as exc:
                return exc is not value
            except RuntimeError as exc:
                if exc is value:
                    exc.__traceback__ = traceback
                    return False
                if isinstance(value, StopIteration) and exc.__cause__ is value:
                    value.__traceback__ = traceback
                    return False
                raise
            except BaseException as exc:
                if exc is not value:
                    raise
                exc.__traceback__ = traceback
                return False
            raise RuntimeError("generator didn't stop after throw()")


def contextmanager(func):
    @wraps(func)
    def helper(*args, **kwds):
        return _GeneratorContextManager(func, args, kwds)
    return helper


class closing(AbstractContextManager):
    def __init__(self, thing):
        self.thing = thing

    def __enter__(self):
        return self.thing

    def __exit__(self, *exc_info):
        self.thing.close()


class _RedirectStream(AbstractContextManager):
    _stream = None

    def __init__(self, new_target):
        self._new_target = new_target
        self._old_targets = []

    def __enter__(self):
        self._old_targets.append(getattr(_sys, self._stream))
        setattr(_sys, self._stream, self._new_target)
        return self._new_target

    def __exit__(self, exctype, excinst, exctb):
        setattr(_sys, self._stream, self._old_targets.pop())


class redirect_stdout(_RedirectStream):
    _stream = "stdout"


class redirect_stderr(_RedirectStream):
    _stream = "stderr"


class suppress(AbstractContextManager):
    def __init__(self, *exceptions):
        self._exceptions = exceptions

    def __enter__(self):
        pass

    def __exit__(self, exctype, excinst, exctb):
        return exctype is not None and issubclass(exctype, self._exceptions)


class nullcontext(AbstractContextManager):
    def __init__(self, enter_result=None):
        self.enter_result = enter_result

    def __enter__(self):
        return self.enter_result

    def __exit__(self, *excinfo):
        pass


class chdir(AbstractContextManager):
    def __init__(self, path):
        self.path = path
        self._old_cwd = []

    def __enter__(self):
        import os
        self._old_cwd.append(os.getcwd())
        os.chdir(self.path)

    def __exit__(self, *excinfo):
        import os
        os.chdir(self._old_cwd.pop())


class ExitStack(AbstractContextManager):
    def __init__(self):
        self._exit_callbacks = []

    def pop_all(self):
        new_stack = type(self)()
        new_stack._exit_callbacks = self._exit_callbacks
        self._exit_callbacks = []
        return new_stack

    def push(self, exit):
        _cb_type = type(exit)
        try:
            exit_method = _cb_type.__exit__
        except AttributeError:
            self._exit_callbacks.append(exit)
        else:
            self._exit_callbacks.append(lambda *exc: exit_method(exit, *exc))
        return exit

    def enter_context(self, cm):
        cls = type(cm)
        result = cls.__enter__(cm)
        self._exit_callbacks.append(lambda *exc: cls.__exit__(cm, *exc))
        return result

    def callback(self, callback, /, *args, **kwds):
        def _exit_wrapper(exc_type, exc, tb):
            callback(*args, **kwds)
        self._exit_callbacks.append(_exit_wrapper)
        return callback

    def close(self):
        self.__exit__(None, None, None)

    def __exit__(self, *exc_details):
        received_exc = exc_details[0] is not None
        suppressed_exc = False
        pending_raise = False
        while self._exit_callbacks:
            cb = self._exit_callbacks.pop()
            try:
                if cb(*exc_details):
                    suppressed_exc = True
                    pending_raise = False
                    exc_details = (None, None, None)
            except BaseException as e:
                exc_details = (type(e), e, None)
                pending_raise = True
        if pending_raise:
            raise exc_details[1]
        return received_exc and suppressed_exc
