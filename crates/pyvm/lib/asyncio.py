"""asyncio for the simulated interpreter: a deterministic single-threaded event
loop. Sleeping advances the process's simulated clock, never the host's."""
import time as _time
import heapq as _heapq
from collections import deque as _deque

__all__ = ['run', 'sleep', 'gather', 'create_task', 'wait_for', 'wait', 'Future', 'Task',
           'Queue', 'Event', 'Lock', 'Semaphore', 'TimeoutError', 'CancelledError',
           'get_event_loop', 'new_event_loop', 'get_running_loop', 'as_completed',
           'iscoroutine', 'iscoroutinefunction', 'shield', 'current_task', 'all_tasks']


class CancelledError(BaseException):
    pass


class InvalidStateError(Exception):
    pass


TimeoutError = TimeoutError
_PENDING, _CANCELLED, _FINISHED = 'PENDING', 'CANCELLED', 'FINISHED'
_current_loop = None


class Future:
    def __init__(self, *, loop=None):
        self._loop = loop or get_event_loop()
        self._state = _PENDING
        self._result = None
        self._exception = None
        self._callbacks = []

    def done(self):
        return self._state != _PENDING

    def cancelled(self):
        return self._state == _CANCELLED

    def result(self):
        if self._state == _CANCELLED:
            raise CancelledError()
        if self._state != _FINISHED:
            raise InvalidStateError('Result is not ready.')
        if self._exception is not None:
            raise self._exception
        return self._result

    def exception(self):
        if self._state != _FINISHED:
            raise InvalidStateError('Exception is not set.')
        return self._exception

    def set_result(self, result):
        if self._state != _PENDING:
            raise InvalidStateError('invalid state')
        self._result = result
        self._state = _FINISHED
        self._schedule_callbacks()

    def set_exception(self, exception):
        if self._state != _PENDING:
            raise InvalidStateError('invalid state')
        if isinstance(exception, type):
            exception = exception()
        self._exception = exception
        self._state = _FINISHED
        self._schedule_callbacks()

    def cancel(self, msg=None):
        if self._state != _PENDING:
            return False
        self._state = _CANCELLED
        self._schedule_callbacks()
        return True

    def add_done_callback(self, fn, *, context=None):
        if self.done():
            self._loop.call_soon(fn, self)
        else:
            self._callbacks.append(fn)

    def remove_done_callback(self, fn):
        n = len(self._callbacks)
        self._callbacks = [f for f in self._callbacks if f != fn]
        return n - len(self._callbacks)

    def _schedule_callbacks(self):
        callbacks = self._callbacks[:]
        self._callbacks[:] = []
        for cb in callbacks:
            self._loop.call_soon(cb, self)

    def __await__(self):
        if not self.done():
            yield self
        return self.result()

    __iter__ = __await__

    def get_loop(self):
        return self._loop


class Task(Future):
    _counter = 0

    def __init__(self, coro, *, loop=None, name=None):
        super().__init__(loop=loop)
        self._coro = coro
        Task._counter += 1
        self._name = name or f'Task-{Task._counter}'
        self._must_cancel = False
        self._loop.call_soon(self._step)
        self._loop._tasks.append(self)

    def get_name(self):
        return self._name

    def set_name(self, value):
        self._name = str(value)

    def get_coro(self):
        return self._coro

    def cancel(self, msg=None):
        if self.done():
            return False
        self._must_cancel = True
        return True

    def _step(self, value=None, exc=None):
        global _current_task
        if self.done():
            return
        if self._must_cancel:
            exc = CancelledError()
            self._must_cancel = False
        prev = _current_task
        _current_task = self
        try:
            if exc is not None:
                result = self._coro.throw(exc)
            else:
                result = self._coro.send(value)
        except StopIteration as e:
            super().set_result(e.value)
        except CancelledError:
            super().cancel()
        except BaseException as e:
            super().set_exception(e)
        else:
            if isinstance(result, Future):
                result.add_done_callback(self._wakeup)
            elif result is None:
                self._loop.call_soon(self._step)
            else:
                self._loop.call_soon(self._step, None, RuntimeError(f'Task got bad yield: {result!r}'))
        finally:
            _current_task = prev

    def _wakeup(self, future):
        try:
            value = future.result()
        except BaseException as e:
            self._step(None, e)
        else:
            self._step(value)

    def __repr__(self):
        return f'<Task {self._state.lower()} name={self._name!r}>'


_current_task = None


class Handle:
    def __init__(self, callback, args):
        self._callback = callback
        self._args = args
        self._cancelled = False

    def cancel(self):
        self._cancelled = True

    def cancelled(self):
        return self._cancelled

    def _run(self):
        self._callback(*self._args)


class TimerHandle(Handle):
    def __init__(self, when, callback, args):
        super().__init__(callback, args)
        self._when = when

    def when(self):
        return self._when


class AbstractEventLoop:
    pass


class BaseEventLoop(AbstractEventLoop):
    def __init__(self):
        self._ready = _deque()
        self._timers = []
        self._seq = 0
        self._tasks = []
        self._running = False
        self._closed = False

    def time(self):
        return _time.monotonic()

    def call_soon(self, callback, *args, context=None):
        h = Handle(callback, args)
        self._ready.append(h)
        return h

    def call_later(self, delay, callback, *args, context=None):
        return self.call_at(self.time() + delay, callback, *args)

    def call_at(self, when, callback, *args, context=None):
        h = TimerHandle(when, callback, args)
        self._seq += 1
        _heapq.heappush(self._timers, (when, self._seq, h))
        return h

    def create_future(self):
        return Future(loop=self)

    def create_task(self, coro, *, name=None, context=None):
        return Task(coro, loop=self, name=name)

    def _run_once(self):
        if not self._ready and self._timers:
            when = self._timers[0][0]
            now = self.time()
            if when > now:
                _time.sleep(when - now)
        now = self.time()
        while self._timers and self._timers[0][0] <= now + 1e-9:
            _, _, h = _heapq.heappop(self._timers)
            if not h._cancelled:
                self._ready.append(h)
        n = len(self._ready)
        for _ in range(n):
            h = self._ready.popleft()
            if not h._cancelled:
                h._run()

    def run_until_complete(self, future):
        global _current_loop
        if iscoroutine(future):
            future = self.create_task(future)
        prev = _current_loop
        _current_loop = self
        self._running = True
        try:
            while not future.done():
                if not self._ready and not self._timers:
                    raise RuntimeError('Event loop stopped before Future completed.')
                self._run_once()
        finally:
            self._running = False
            _current_loop = prev
        return future.result()

    def run_forever(self):
        while self._ready or self._timers:
            self._run_once()

    def is_running(self):
        return self._running

    def is_closed(self):
        return self._closed

    def close(self):
        self._closed = True

    def stop(self):
        self._ready.clear()
        self._timers.clear()


_default_loop = None


def new_event_loop():
    return BaseEventLoop()


def get_event_loop():
    global _default_loop
    if _current_loop is not None:
        return _current_loop
    if _default_loop is None or _default_loop.is_closed():
        _default_loop = new_event_loop()
    return _default_loop


def set_event_loop(loop):
    global _default_loop
    _default_loop = loop


def get_running_loop():
    if _current_loop is None:
        raise RuntimeError('no running event loop')
    return _current_loop


def iscoroutine(obj):
    return type(obj).__name__ == 'coroutine' or hasattr(obj, 'send') and hasattr(obj, 'throw') and type(obj).__name__ == 'generator'


def iscoroutinefunction(func):
    code = getattr(func, '__code__', None)
    return False if code is None else False


def run(main, *, debug=None):
    if _current_loop is not None:
        raise RuntimeError('asyncio.run() cannot be called from a running event loop')
    if not iscoroutine(main):
        raise ValueError(f'a coroutine was expected, got {main!r}')
    loop = new_event_loop()
    set_event_loop(loop)
    try:
        return loop.run_until_complete(main)
    finally:
        loop.close()


def ensure_future(coro_or_future, *, loop=None):
    if isinstance(coro_or_future, Future):
        return coro_or_future
    return (loop or get_event_loop()).create_task(coro_or_future)


def create_task(coro, *, name=None, context=None):
    return get_running_loop().create_task(coro, name=name)


def current_task(loop=None):
    return _current_task


def all_tasks(loop=None):
    loop = loop or get_running_loop()
    return {t for t in loop._tasks if not t.done()}


class _SleepHandle:
    pass


async def sleep(delay, result=None):
    loop = get_running_loop()
    if delay <= 0:
        await _yield_once()
        return result
    future = loop.create_future()
    loop.call_later(delay, _set_result_unless_cancelled, future, result)
    return await future


class _YieldOnce:
    def __await__(self):
        yield None


def _yield_once():
    return _YieldOnce()


def _set_result_unless_cancelled(fut, result):
    if not fut.cancelled() and not fut.done():
        fut.set_result(result)


async def gather(*aws, return_exceptions=False):
    loop = get_running_loop()
    futures = [ensure_future(a, loop=loop) for a in aws]
    results = []
    for f in futures:
        try:
            results.append(await f)
        except BaseException as e:
            if return_exceptions and not isinstance(e, CancelledError):
                results.append(e)
            else:
                raise
    return results


async def wait_for(aw, timeout):
    loop = get_running_loop()
    fut = ensure_future(aw, loop=loop)
    if timeout is None:
        return await fut
    timed_out = [False]

    def on_timeout():
        if not fut.done():
            timed_out[0] = True
            fut.cancel()
            waiter.done() or waiter.set_result(None)
    waiter = loop.create_future()
    fut.add_done_callback(lambda f: waiter.done() or waiter.set_result(None))
    handle = loop.call_later(timeout, on_timeout)
    await waiter
    handle.cancel()
    if timed_out[0]:
        raise TimeoutError()
    return fut.result()


FIRST_COMPLETED = 'FIRST_COMPLETED'
FIRST_EXCEPTION = 'FIRST_EXCEPTION'
ALL_COMPLETED = 'ALL_COMPLETED'


async def wait(fs, *, timeout=None, return_when=ALL_COMPLETED):
    loop = get_running_loop()
    fs = {ensure_future(f, loop=loop) for f in fs}
    if return_when == ALL_COMPLETED:
        for f in list(fs):
            try:
                await f
            except BaseException:
                pass
        return fs, set()
    waiter = loop.create_future()
    for f in fs:
        f.add_done_callback(lambda _f: waiter.done() or waiter.set_result(None))
    await waiter
    done = {f for f in fs if f.done()}
    return done, fs - done


def as_completed(fs, *, timeout=None):
    loop = get_event_loop()
    futures = [ensure_future(f, loop=loop) for f in fs]
    done_order = []

    async def _wait_one():
        while True:
            for f in futures:
                if f.done() and f not in done_order:
                    done_order.append(f)
                    return f.result()
            await sleep(0)
    return [_wait_one() for _ in futures]


async def shield(arg):
    return await arg


class Event:
    def __init__(self):
        self._value = False
        self._waiters = []

    def is_set(self):
        return self._value

    def set(self):
        if not self._value:
            self._value = True
            for fut in self._waiters:
                if not fut.done():
                    fut.set_result(True)

    def clear(self):
        self._value = False

    async def wait(self):
        if self._value:
            return True
        fut = get_running_loop().create_future()
        self._waiters.append(fut)
        try:
            await fut
            return True
        finally:
            self._waiters.remove(fut)


class Lock:
    def __init__(self):
        self._locked = False
        self._waiters = _deque()

    def locked(self):
        return self._locked

    async def acquire(self):
        if not self._locked:
            self._locked = True
            return True
        fut = get_running_loop().create_future()
        self._waiters.append(fut)
        await fut
        self._locked = True
        return True

    def release(self):
        if not self._locked:
            raise RuntimeError('Lock is not acquired.')
        self._locked = False
        while self._waiters:
            fut = self._waiters.popleft()
            if not fut.done():
                fut.set_result(True)
                break

    async def __aenter__(self):
        await self.acquire()
        return None

    async def __aexit__(self, exc_type, exc, tb):
        self.release()


class Semaphore:
    def __init__(self, value=1):
        self._value = value
        self._waiters = _deque()

    def locked(self):
        return self._value == 0

    async def acquire(self):
        while self._value <= 0:
            fut = get_running_loop().create_future()
            self._waiters.append(fut)
            await fut
        self._value -= 1
        return True

    def release(self):
        self._value += 1
        while self._waiters:
            fut = self._waiters.popleft()
            if not fut.done():
                fut.set_result(True)
                break

    async def __aenter__(self):
        await self.acquire()

    async def __aexit__(self, exc_type, exc, tb):
        self.release()


class QueueEmpty(Exception):
    pass


class QueueFull(Exception):
    pass


class Queue:
    def __init__(self, maxsize=0):
        self._maxsize = maxsize
        self._queue = _deque()
        self._getters = _deque()
        self._putters = _deque()
        self._unfinished_tasks = 0
        self._finished = Event()
        self._finished.set()

    def qsize(self):
        return len(self._queue)

    @property
    def maxsize(self):
        return self._maxsize

    def empty(self):
        return not self._queue

    def full(self):
        return 0 < self._maxsize <= self.qsize()

    def _wakeup_next(self, waiters):
        while waiters:
            waiter = waiters.popleft()
            if not waiter.done():
                waiter.set_result(None)
                break

    async def put(self, item):
        while self.full():
            putter = get_running_loop().create_future()
            self._putters.append(putter)
            await putter
        return self.put_nowait(item)

    def put_nowait(self, item):
        if self.full():
            raise QueueFull
        self._queue.append(item)
        self._unfinished_tasks += 1
        self._finished.clear()
        self._wakeup_next(self._getters)

    async def get(self):
        while self.empty():
            getter = get_running_loop().create_future()
            self._getters.append(getter)
            await getter
        return self.get_nowait()

    def get_nowait(self):
        if self.empty():
            raise QueueEmpty
        item = self._queue.popleft()
        self._wakeup_next(self._putters)
        return item

    def task_done(self):
        if self._unfinished_tasks <= 0:
            raise ValueError('task_done() called too many times')
        self._unfinished_tasks -= 1
        if self._unfinished_tasks == 0:
            self._finished.set()

    async def join(self):
        if self._unfinished_tasks > 0:
            await self._finished.wait()
