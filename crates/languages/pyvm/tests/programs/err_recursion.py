import sys


def depth(n):
    return depth(n + 1)


def count(n):
    return 0 if n == 0 else 1 + count(n - 1)


print(count(900))
try:
    depth(0)
except RecursionError as e:
    print("RecursionError:", e)
print(sys.getrecursionlimit())


def boom(n):
    if n == 0:
        raise ValueError("bottom")
    return boom(n - 1)


boom(10)
