"""queue for the simulated (single-threaded) interpreter."""
from collections import deque
import heapq

__all__ = ['Empty', 'Full', 'Queue', 'PriorityQueue', 'LifoQueue', 'SimpleQueue']


class Empty(Exception):
    pass


class Full(Exception):
    pass


class Queue:
    def __init__(self, maxsize=0):
        self.maxsize = maxsize
        self._init(maxsize)
        self.unfinished_tasks = 0

    def _init(self, maxsize):
        self.queue = deque()

    def _qsize(self):
        return len(self.queue)

    def _put(self, item):
        self.queue.append(item)

    def _get(self):
        return self.queue.popleft()

    def task_done(self):
        if self.unfinished_tasks <= 0:
            raise ValueError('task_done() called too many times')
        self.unfinished_tasks -= 1

    def join(self):
        pass

    def qsize(self):
        return self._qsize()

    def empty(self):
        return not self._qsize()

    def full(self):
        return 0 < self.maxsize <= self._qsize()

    def put(self, item, block=True, timeout=None):
        if self.maxsize > 0 and self._qsize() >= self.maxsize:
            raise Full
        self._put(item)
        self.unfinished_tasks += 1

    def get(self, block=True, timeout=None):
        if not self._qsize():
            raise Empty
        return self._get()

    def put_nowait(self, item):
        return self.put(item, block=False)

    def get_nowait(self):
        return self.get(block=False)


class PriorityQueue(Queue):
    def _init(self, maxsize):
        self.queue = []

    def _put(self, item):
        heapq.heappush(self.queue, item)

    def _get(self):
        return heapq.heappop(self.queue)


class LifoQueue(Queue):
    def _init(self, maxsize):
        self.queue = []

    def _put(self, item):
        self.queue.append(item)

    def _get(self):
        return self.queue.pop()


class SimpleQueue(Queue):
    pass
