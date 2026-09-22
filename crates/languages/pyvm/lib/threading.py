"""threading for the simulated interpreter.

Threads are real interpreter threads on a deterministic scheduler: the
interpreter switches between them at a fixed instruction quantum taken from the
world's seed, so a race interleaves the same way in every replay. Blocking runs
the other threads; when they are all waiting on time, the clock jumps forward.
"""
import _thread
import sys

__all__ = ['get_ident', 'active_count', 'Condition', 'current_thread', 'enumerate',
           'main_thread', 'TIMEOUT_MAX', 'Event', 'Lock', 'RLock', 'Semaphore',
           'BoundedSemaphore', 'Thread', 'Barrier', 'BrokenBarrierError', 'Timer',
           'ThreadError', 'setprofile', 'settrace', 'local', 'stack_size',
           'excepthook', 'ExceptHookArgs', 'gettrace', 'getprofile']

get_ident = _thread.get_ident
get_native_id = _thread.get_native_id
TIMEOUT_MAX = _thread.TIMEOUT_MAX
ThreadError = _thread.error
stack_size = _thread.stack_size


class _LockBase:
    __slots__ = ('_lock',)

    def acquire(self, blocking=True, timeout=-1):
        return _thread.acquire(self._lock, blocking, timeout)

    def release(self):
        _thread.release(self._lock)

    def locked(self):
        return _thread.locked(self._lock)

    def __enter__(self):
        self.acquire()
        return self

    def __exit__(self, *args):
        self.release()
        return False

    def _is_owned(self):
        return _thread.owner(self._lock) == get_ident()


class Lock(_LockBase):
    __slots__ = ()

    def __init__(self):
        self._lock = _thread.allocate_lock()

    def __repr__(self):
        return '<unlocked _thread.lock object at 0x%x>' % id(self) if not self.locked() \
            else '<locked _thread.lock object at 0x%x>' % id(self)


class RLock(_LockBase):
    __slots__ = ()

    def __init__(self):
        self._lock = _thread.RLock()

    def _acquire_restore(self, state):
        while not _thread.acquire(self._lock, True, -1):
            pass

    def _release_save(self):
        _thread.release(self._lock)
        return None

    def __repr__(self):
        return '<%s %s.RLock object owner=%s at 0x%x>' % (
            'locked' if self.locked() else 'unlocked', __name__,
            _thread.owner(self._lock), id(self))


def allocate_lock():
    return Lock()


class Condition:
    def __init__(self, lock=None):
        if lock is None:
            lock = RLock()
        self._lock = lock
        self.acquire = lock.acquire
        self.release = lock.release
        self._waiters = []

    def __enter__(self):
        return self._lock.__enter__()

    def __exit__(self, *args):
        return self._lock.__exit__(*args)

    def _is_owned(self):
        return getattr(self._lock, '_is_owned', lambda: True)()

    def wait(self, timeout=None):
        if not self._is_owned():
            raise RuntimeError("cannot wait on un-acquired lock")
        waiter = [False]
        self._waiters.append(waiter)
        self.release()
        try:
            got = _thread._wait_until(lambda: waiter[0], timeout)
        finally:
            while True:
                if self.acquire(True, -1):
                    break
        if waiter in self._waiters:
            self._waiters.remove(waiter)
        return got

    def wait_for(self, predicate, timeout=None):
        import time
        endtime = None if timeout is None else time.monotonic() + timeout
        result = predicate()
        while not result:
            if endtime is not None:
                waittime = endtime - time.monotonic()
                if waittime <= 0:
                    break
                self.wait(waittime)
            else:
                self.wait(None)
            result = predicate()
        return result

    def notify(self, n=1):
        if not self._is_owned():
            raise RuntimeError("cannot notify on un-acquired lock")
        for waiter in self._waiters[:n]:
            waiter[0] = True
            self._waiters.remove(waiter)

    def notify_all(self):
        self.notify(len(self._waiters))

    notifyAll = notify_all

    def __repr__(self):
        return '<Condition(%s, %d)>' % (self._lock, len(self._waiters))


class Semaphore:
    def __init__(self, value=1):
        if value < 0:
            raise ValueError("semaphore initial value must be >= 0")
        self._cond = Condition(Lock())
        self._value = value

    def acquire(self, blocking=True, timeout=None):
        if not blocking and timeout is not None:
            raise ValueError("can't specify timeout for non-blocking acquire")
        import time
        endtime = None
        with self._cond:
            while self._value == 0:
                if not blocking:
                    return False
                if timeout is not None:
                    if endtime is None:
                        endtime = time.monotonic() + timeout
                    else:
                        timeout = endtime - time.monotonic()
                        if timeout <= 0:
                            return False
                self._cond.wait(timeout)
            self._value -= 1
            return True

    __enter__ = acquire

    def release(self, n=1):
        if n < 1:
            raise ValueError('n must be one or more')
        with self._cond:
            self._value += n
            self._cond.notify(n)

    def __exit__(self, *args):
        self.release()
        return False


class BoundedSemaphore(Semaphore):
    def __init__(self, value=1):
        Semaphore.__init__(self, value)
        self._initial_value = value

    def release(self, n=1):
        if n < 1:
            raise ValueError('n must be one or more')
        with self._cond:
            if self._value + n > self._initial_value:
                raise ValueError("Semaphore released too many times")
            self._value += n
            self._cond.notify(n)


class Event:
    def __init__(self):
        self._cond = Condition(Lock())
        self._flag = False

    def is_set(self):
        return self._flag

    isSet = is_set

    def set(self):
        with self._cond:
            self._flag = True
            self._cond.notify_all()

    def clear(self):
        with self._cond:
            self._flag = False

    def wait(self, timeout=None):
        with self._cond:
            if not self._flag:
                self._cond.wait_for(lambda: self._flag, timeout)
            return self._flag

    def __repr__(self):
        return '<threading.Event at 0x%x: %s>' % (id(self), 'set' if self._flag else 'unset')


class BrokenBarrierError(RuntimeError):
    pass


class Barrier:
    def __init__(self, parties, action=None, timeout=None):
        self._cond = Condition(Lock())
        self._action = action
        self._timeout = timeout
        self._parties = parties
        self._state = 0
        self._count = 0

    def wait(self, timeout=None):
        if timeout is None:
            timeout = self._timeout
        with self._cond:
            self._enter()
            index = self._count
            self._count += 1
            try:
                if index + 1 == self._parties:
                    self._release()
                else:
                    self._wait(timeout)
                return index
            finally:
                self._count -= 1
                self._exit()

    def _enter(self):
        while self._state in (-1, 1):
            self._cond.wait()
        if self._state < 0:
            raise BrokenBarrierError

    def _release(self):
        if self._action:
            self._action()
        self._state = 1
        self._cond.notify_all()

    def _wait(self, timeout):
        if not self._cond.wait_for(lambda: self._state != 0, timeout):
            self._break()
            raise BrokenBarrierError
        if self._state < 0:
            raise BrokenBarrierError

    def _exit(self):
        if self._count == 0:
            if self._state in (-1, 1):
                self._state = 0
                self._cond.notify_all()

    def reset(self):
        with self._cond:
            if self._count > 0:
                self._state = -1 if self._state != -2 else -2
            else:
                self._state = 0
            self._cond.notify_all()

    def abort(self):
        with self._cond:
            self._break()

    def _break(self):
        self._state = -2
        self._cond.notify_all()

    @property
    def parties(self):
        return self._parties

    @property
    def n_waiting(self):
        return self._count if self._state == 0 else 0

    @property
    def broken(self):
        return self._state == -2


class local:
    """Thread-local data: each thread sees its own attributes."""

    def __init__(self, **kw):
        object.__setattr__(self, '_values', {})
        for k, v in kw.items():
            setattr(self, k, v)

    def _dict(self):
        values = object.__getattribute__(self, '_values')
        return values.setdefault(get_ident(), {})

    def __getattr__(self, name):
        d = object.__getattribute__(self, '_dict')()
        if name in d:
            return d[name]
        raise AttributeError(
            "'_thread._local' object has no attribute '%s'" % name)

    def __setattr__(self, name, value):
        if name == '_values':
            return object.__setattr__(self, name, value)
        object.__getattribute__(self, '_dict')()[name] = value

    def __delattr__(self, name):
        d = object.__getattribute__(self, '_dict')()
        if name in d:
            del d[name]
        else:
            raise AttributeError(name)


class ExceptHookArgs(tuple):
    exc_type = property(lambda s: s[0])
    exc_value = property(lambda s: s[1])
    exc_traceback = property(lambda s: s[2])
    thread = property(lambda s: s[3])


def excepthook(args, /):
    """The interpreter prints a failing thread's traceback itself; this hook is
    here for programs that replace it."""
    import traceback
    if args.exc_type is SystemExit:
        return
    print(f"Exception in thread {args.thread.name if args.thread else '?'}:",
          file=sys.stderr)
    traceback.print_exception(args.exc_type, args.exc_value, args.exc_traceback,
                              file=sys.stderr)


__excepthook__ = excepthook


def setprofile(func):
    pass


def settrace(func):
    pass


def gettrace():
    return None


def getprofile():
    return None


class Thread:
    _counter = 0

    def __init__(self, group=None, target=None, name=None, args=(), kwargs=None, *,
                 daemon=None):
        if group is not None:
            raise ValueError("group argument must be None for now")
        Thread._counter += 1
        self._target = target
        self._args = args
        self._kwargs = kwargs if kwargs is not None else {}
        if name is None:
            base = getattr(target, '__name__', None)
            name = f"Thread-{Thread._counter - 1}" + (f" ({base})" if base else "")
        self._name = str(name)
        self._daemon = bool(daemon) if daemon is not None else current_thread().daemon
        self._ident = None
        self._started = Event()
        self._is_stopped = False
        self._native_id = None

    def start(self):
        if self._ident is not None:
            raise RuntimeError("threads can only be started once")
        self._ident = _thread.start_new_thread(self._run, (), {}, self._name,
                                               self._daemon, self)
        self._started.set()

    def _run(self):
        try:
            if self._target is not None:
                self._target(*self._args, **self._kwargs)
        except BaseException as exc:
            # The interpreter reports a failing thread itself; a program that
            # replaced the hook gets it called instead, as CPython does.
            if excepthook is __excepthook__:
                raise
            excepthook(ExceptHookArgs(
                (type(exc), exc, exc.__traceback__, self)))
        finally:
            self._is_stopped = True
            del self._target, self._args, self._kwargs

    def run(self):
        if self._target is not None:
            self._target(*self._args, **self._kwargs)

    def join(self, timeout=None):
        if self._ident is None:
            raise RuntimeError("cannot join thread before it is started")
        if self._ident == get_ident():
            raise RuntimeError("cannot join current thread")
        _thread.join(self._ident, timeout)

    def is_alive(self):
        return self._ident is not None and _thread.is_alive(self._ident)

    isAlive = is_alive

    @property
    def name(self):
        return self._name

    @name.setter
    def name(self, value):
        self._name = str(value)
        if self._ident is not None:
            _thread._set_name(self._ident, self._name)

    @property
    def ident(self):
        return self._ident

    @property
    def native_id(self):
        return self._ident

    @property
    def daemon(self):
        return self._daemon

    @daemon.setter
    def daemon(self, value):
        if self._ident is not None:
            raise RuntimeError("cannot set daemon status of active thread")
        self._daemon = bool(value)

    def setDaemon(self, daemonic):
        self.daemon = daemonic

    def isDaemon(self):
        return self.daemon

    def getName(self):
        return self.name

    def setName(self, name):
        self.name = name

    def __repr__(self):
        status = "initial"
        if self._ident is not None:
            status = "started" if self.is_alive() else "stopped"
        if self._daemon:
            status += " daemon"
        if self._ident is not None:
            status += " %s" % self._ident
        return "<%s(%s, %s)>" % (self.__class__.__name__, self._name, status)


class Timer(Thread):
    def __init__(self, interval, function, args=None, kwargs=None):
        Thread.__init__(self)
        self.interval = interval
        self.function = function
        self.args = args if args is not None else []
        self.kwargs = kwargs if kwargs is not None else {}
        self.finished = Event()
        self._target = self._fire
        self._args = ()
        self._kwargs = {}

    def _fire(self):
        self.finished.wait(self.interval)
        if not self.finished.is_set():
            self.function(*self.args, **self.kwargs)
        self.finished.set()

    def cancel(self):
        self.finished.set()


class _MainThread(Thread):
    def __init__(self):
        Thread.__init__(self, name="MainThread", daemon=False)
        self._started.set()
        self._ident = get_ident()


class _DummyThread(Thread):
    def __init__(self, ident, name, daemon):
        Thread.__init__(self, name=name, daemon=daemon)
        self._started.set()
        self._ident = ident


_main_thread = None


def main_thread():
    global _main_thread
    if _main_thread is None:
        _main_thread = _MainThread()
        _thread._set_object(_main_thread._ident, _main_thread)
    return _main_thread


def current_thread():
    ident, name, daemon, obj = _thread._current()
    if isinstance(obj, Thread):
        return obj
    if ident == 1:
        return main_thread()
    t = _DummyThread(ident, name, daemon)
    _thread._set_object(ident, t)
    return t


currentThread = current_thread


def enumerate():
    out = []
    for ident, name, daemon, obj in _thread._enumerate():
        if isinstance(obj, Thread):
            out.append(obj)
        elif ident == 1:
            out.append(main_thread())
        else:
            out.append(_DummyThread(ident, name, daemon))
    return out


def active_count():
    return len(enumerate())


activeCount = active_count
