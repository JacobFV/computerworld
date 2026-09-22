"""Threads: threading, queue and concurrent.futures.

Every result here is forced by a lock, an event, a queue or a join, so the
output is the same on CPython and on the simulated scheduler.
"""
import threading
import time
import queue
from concurrent.futures import (ThreadPoolExecutor, as_completed, wait,
                                FIRST_COMPLETED, Future, CancelledError)

# --- names, identities and enumerate ---------------------------------------
main = threading.current_thread()
print(main.name, main.daemon, main.is_alive())
print(threading.main_thread() is main, isinstance(threading.get_ident(), int))

order = []
step = threading.Lock()


def worker(n):
    with step:
        order.append(n)


ts = [threading.Thread(target=worker, args=(i,), name='w%d' % i) for i in range(3)]
print([t.name for t in ts], [t.is_alive() for t in ts])
for t in ts:
    t.start()
for t in ts:
    t.join()
print(sorted(order), [t.is_alive() for t in ts])
print(threading.active_count())

# --- a lock keeps increments atomic ----------------------------------------
counter = 0
lock = threading.Lock()


def bump(times):
    global counter
    for _ in range(times):
        with lock:
            counter += 1


ts = [threading.Thread(target=bump, args=(1000,)) for _ in range(4)]
for t in ts:
    t.start()
for t in ts:
    t.join()
print('counter', counter)

# --- reentrant locks --------------------------------------------------------
rlock = threading.RLock()
with rlock:
    with rlock:
        print('rlock reentered')

# --- events -----------------------------------------------------------------
events = []
ready = threading.Event()
done = threading.Event()


def waiter():
    events.append('waiting')
    ready.wait()
    events.append('woken')
    done.set()


w = threading.Thread(target=waiter)
w.start()
while 'waiting' not in events:
    time.sleep(0.001)
events.append('setting')
ready.set()
done.wait()
w.join()
print(events, ready.is_set())

# --- conditions -------------------------------------------------------------
cond = threading.Condition()
shared = []
seen = []


def consumer():
    with cond:
        cond.wait_for(lambda: len(shared) >= 2)
        seen.append(list(shared))


c = threading.Thread(target=consumer)
c.start()
for value in (1, 2):
    time.sleep(0.005)
    with cond:
        shared.append(value)
        cond.notify_all()
c.join()
print('condition', seen)

# --- semaphores bound concurrency ------------------------------------------
sem = threading.Semaphore(2)
active = []
peak = [0]
guard = threading.Lock()


def limited(i):
    with sem:
        with guard:
            active.append(i)
            peak[0] = max(peak[0], len(active))
        time.sleep(0.01)
        with guard:
            active.remove(i)


ts = [threading.Thread(target=limited, args=(i,)) for i in range(6)]
for t in ts:
    t.start()
for t in ts:
    t.join()
print('peak', peak[0], 'left', active)

# --- a bounded queue with producer and consumer -----------------------------
q = queue.Queue(maxsize=2)
got = []


def producer():
    for i in range(5):
        q.put(i)
    q.put(None)


def consumer2():
    while True:
        item = q.get()
        q.task_done()
        if item is None:
            break
        got.append(item)


p = threading.Thread(target=producer)
c = threading.Thread(target=consumer2)
p.start()
c.start()
p.join()
c.join()
q.join()
print('queue', got, q.empty(), q.qsize())

try:
    queue.Queue(maxsize=1).get_nowait()
except queue.Empty:
    print('queue empty raises')
full = queue.Queue(maxsize=1)
full.put('x')
try:
    full.put_nowait('y')
except queue.Full:
    print('queue full raises')

lifo = queue.LifoQueue()
prio = queue.PriorityQueue()
simple = queue.SimpleQueue()
for i in (3, 1, 2):
    lifo.put(i)
    prio.put(i)
    simple.put(i)
print([lifo.get() for _ in range(3)], [prio.get() for _ in range(3)],
      [simple.get() for _ in range(3)])

# --- thread-local storage ---------------------------------------------------
tl = threading.local()
tl.value = 'main'
seen_local = []


def check_local():
    try:
        seen_local.append(tl.value)
    except AttributeError:
        seen_local.append('unset')
    tl.value = 'thread'
    seen_local.append(tl.value)


t = threading.Thread(target=check_local)
t.start()
t.join()
print(seen_local, tl.value)

# --- a thread that raises does not stop the program -------------------------


def boom():
    raise ValueError('thread failure')


hooked = []
threading.excepthook = lambda args: hooked.append(
    (args.exc_type.__name__, str(args.exc_value), args.thread.name))
b = threading.Thread(target=boom, name='exploder')
b.start()
b.join()
threading.excepthook = threading.__excepthook__
print('after failure', b.is_alive(), hooked)

# --- daemon threads do not hold the interpreter open ------------------------
d = threading.Thread(target=lambda: time.sleep(30), daemon=True)
d.start()
print('daemon', d.daemon, d.is_alive())

# --- concurrent.futures -----------------------------------------------------
with ThreadPoolExecutor(max_workers=3, thread_name_prefix='pool') as ex:
    squares = [ex.submit(lambda n: n * n, i) for i in range(5)]
    print([f.result() for f in squares])
    print([f.done() for f in squares], squares[0].exception())
    print(list(ex.map(str.upper, ['a', 'b', 'c'])))

    slow = [ex.submit(time.sleep, 0.01) for _ in range(3)]
    print('as_completed', len(list(as_completed(slow))))

    finished, pending = wait([ex.submit(lambda: 1)], return_when=FIRST_COMPLETED)
    print('wait', len(finished), len(pending))

    failing = ex.submit(lambda: 1 // 0)
    print(type(failing.exception()).__name__)
    try:
        failing.result()
    except ZeroDivisionError as exc:
        print('result raises', exc)

f = Future()
print(f.cancel(), f.cancelled(), f.done())
try:
    f.result(timeout=0)
except CancelledError:
    print('cancelled future raises')

f2 = Future()
notified = []
f2.add_done_callback(lambda fut: notified.append(fut.result()))
f2.set_result(7)
print(f2.result(), notified, f2.done(), f2.running())

ex = ThreadPoolExecutor(max_workers=2)
results = []
ex.submit(lambda: results.append('first'))
ex.shutdown(wait=True)
print(results)
try:
    ex.submit(lambda: None)
except RuntimeError as exc:
    print('submit after shutdown:', exc)

print('done')
