# Classic algorithms: sorting, searching, graphs, dynamic programming.
import heapq
from collections import deque, defaultdict


def quicksort(xs):
    if len(xs) <= 1:
        return xs
    pivot, *rest = xs
    return quicksort([x for x in rest if x < pivot]) + [pivot] + quicksort([x for x in rest if x >= pivot])


def merge_sort(xs):
    if len(xs) <= 1:
        return list(xs)
    mid = len(xs) // 2
    left, right = merge_sort(xs[:mid]), merge_sort(xs[mid:])
    out = []
    i = j = 0
    while i < len(left) and j < len(right):
        if left[i] <= right[j]:
            out.append(left[i])
            i += 1
        else:
            out.append(right[j])
            j += 1
    out.extend(left[i:])
    out.extend(right[j:])
    return out


def binary_search(xs, target):
    lo, hi = 0, len(xs) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        if xs[mid] == target:
            return mid
        elif xs[mid] < target:
            lo = mid + 1
        else:
            hi = mid - 1
    return -1


def sieve(n):
    is_prime = [True] * (n + 1)
    is_prime[0] = is_prime[1] = False
    for i in range(2, int(n ** 0.5) + 1):
        if is_prime[i]:
            for j in range(i * i, n + 1, i):
                is_prime[j] = False
    return [i for i, p in enumerate(is_prime) if p]


def dijkstra(graph, start):
    dist = {start: 0}
    pq = [(0, start)]
    while pq:
        d, node = heapq.heappop(pq)
        if d > dist.get(node, float('inf')):
            continue
        for nxt, w in graph[node]:
            nd = d + w
            if nd < dist.get(nxt, float('inf')):
                dist[nxt] = nd
                heapq.heappush(pq, (nd, nxt))
    return dist


def bfs(grid, start, goal):
    rows, cols = len(grid), len(grid[0])
    q = deque([(start, 0)])
    seen = {start}
    while q:
        (r, c), steps = q.popleft()
        if (r, c) == goal:
            return steps
        for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            nr, nc = r + dr, c + dc
            if 0 <= nr < rows and 0 <= nc < cols and grid[nr][nc] == '.' and (nr, nc) not in seen:
                seen.add((nr, nc))
                q.append(((nr, nc), steps + 1))
    return -1


def lcs(a, b):
    dp = [[0] * (len(b) + 1) for _ in range(len(a) + 1)]
    for i in range(1, len(a) + 1):
        for j in range(1, len(b) + 1):
            if a[i - 1] == b[j - 1]:
                dp[i][j] = dp[i - 1][j - 1] + 1
            else:
                dp[i][j] = max(dp[i - 1][j], dp[i][j - 1])
    return dp[-1][-1]


def knapsack(items, capacity):
    best = [0] * (capacity + 1)
    for weight, value in items:
        for c in range(capacity, weight - 1, -1):
            best[c] = max(best[c], best[c - weight] + value)
    return best[capacity]


def topo_sort(edges):
    indeg = defaultdict(int)
    adj = defaultdict(list)
    nodes = set()
    for a, b in edges:
        adj[a].append(b)
        indeg[b] += 1
        nodes |= {a, b}
    ready = sorted(n for n in nodes if indeg[n] == 0)
    order = []
    while ready:
        n = ready.pop(0)
        order.append(n)
        for m in adj[n]:
            indeg[m] -= 1
            if indeg[m] == 0:
                ready.append(m)
                ready.sort()
    return order


def permutations(xs):
    if not xs:
        yield []
        return
    for i, x in enumerate(xs):
        for p in permutations(xs[:i] + xs[i + 1:]):
            yield [x] + p


def hanoi(n, src, dst, via, moves):
    if n == 0:
        return
    hanoi(n - 1, src, via, dst, moves)
    moves.append((src, dst))
    hanoi(n - 1, via, dst, src, moves)


data = [38, 27, 43, 3, 9, 82, 10, 3]
print("quicksort:", quicksort(data))
print("merge_sort:", merge_sort(data))
print("sorted desc:", sorted(data, reverse=True))
print("search 43:", binary_search(sorted(data), 43), "search 5:", binary_search(sorted(data), 5))
print("primes:", sieve(60))
graph = {'A': [('B', 7), ('C', 9), ('F', 14)], 'B': [('A', 7), ('C', 10), ('D', 15)],
         'C': [('A', 9), ('B', 10), ('D', 11), ('F', 2)], 'D': [('B', 15), ('C', 11), ('E', 6)],
         'E': [('D', 6), ('F', 9)], 'F': [('A', 14), ('C', 2), ('E', 9)]}
print("dijkstra:", sorted(dijkstra(graph, 'A').items()))
grid = ["..#....", ".##.##.", "....#..", "#.#...#", "......."]
print("bfs steps:", bfs(grid, (0, 0), (4, 6)))
print("lcs:", lcs("AGGTAB", "GXTXAYB"))
print("knapsack:", knapsack([(1, 1), (3, 4), (4, 5), (5, 7)], 7))
print("topo:", topo_sort([("shirt", "tie"), ("tie", "jacket"), ("pants", "shoes"), ("pants", "belt"), ("belt", "jacket"), ("shirt", "belt")]))
print("perms:", [''.join(p) for p in permutations(list("abc"))])
moves = []
hanoi(4, 'A', 'C', 'B', moves)
print("hanoi moves:", len(moves), moves[:3])
fib = [0, 1]
while len(fib) < 90:
    fib.append(fib[-1] + fib[-2])
print("fib[89]:", fib[89], "2**200:", 2 ** 200)
print("factorial 30:", __import__('math').factorial(30))
print("gcd chain:", [__import__('math').gcd(a, b) for a, b in [(48, 18), (17, 5), (100, 75)]])
