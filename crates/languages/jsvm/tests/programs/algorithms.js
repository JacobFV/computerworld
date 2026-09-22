// Algorithms (performance smoke test)
function sieve(n) {
  const is = new Array(n + 1).fill(true);
  is[0] = is[1] = false;
  for (let i = 2; i * i <= n; i++) if (is[i]) for (let j = i * i; j <= n; j += i) is[j] = false;
  return is.reduce((c, v) => c + (v ? 1 : 0), 0);
}
console.log('primes', sieve(200000));
const memo = new Map();
function fibm(n) { if (n < 2) return BigInt(n); if (memo.has(n)) return memo.get(n); const v = fibm(n - 1) + fibm(n - 2); memo.set(n, v); return v; }
console.log('fib(300)', fibm(300).toString());
function quicksort(a) { if (a.length < 2) return a; const [p, ...r] = a; return [...quicksort(r.filter((x) => x < p)), p, ...quicksort(r.filter((x) => x >= p))]; }
let seed = 42;
const rand = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;
const data = Array.from({ length: 5000 }, () => Math.floor(rand() * 100000));
const sorted = quicksort(data);
console.log('sorted ok', sorted.every((v, i) => i === 0 || sorted[i - 1] <= v), sorted.slice(0, 5));
const words = 'lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor'.split(' ');
const freq = {};
for (let i = 0; i < 100000; i++) { const w = words[i % words.length]; freq[w] = (freq[w] || 0) + 1; }
console.log(Object.entries(freq).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 3));
let s = '';
for (let i = 0; i < 20000; i++) s += String.fromCharCode(97 + (i % 26));
console.log(s.length, s.slice(0, 30));
const grid = Array.from({ length: 100 }, (_, y) => Array.from({ length: 100 }, (_, x) => (x * y) % 7 === 0 ? 1 : 0));
function bfs(g) {
  const q = [[0, 0, 0]]; const seen = new Set(['0,0']);
  while (q.length) {
    const [x, y, d] = q.shift();
    if (x === 99 && y === 99) return d;
    for (const [dx, dy] of [[1, 0], [0, 1], [-1, 0], [0, -1]]) {
      const nx = x + dx, ny = y + dy, k = nx + ',' + ny;
      if (nx >= 0 && ny >= 0 && nx < 100 && ny < 100 && !seen.has(k)) { seen.add(k); q.push([nx, ny, d + 1]); }
    }
  }
  return -1;
}
console.log('bfs', bfs(grid));
class Matrix {
  constructor(n) { this.n = n; this.a = new Float64Array(n * n); }
  static identity(n) { const m = new Matrix(n); for (let i = 0; i < n; i++) m.a[i * n + i] = 1; return m; }
  mul(o) { const n = this.n, r = new Matrix(n); for (let i = 0; i < n; i++) for (let k = 0; k < n; k++) { const v = this.a[i * n + k]; if (v) for (let j = 0; j < n; j++) r.a[i * n + j] += v * o.a[k * n + j]; } return r; }
}
const m = Matrix.identity(40); m.a[1] = 2;
let p = m; for (let i = 0; i < 5; i++) p = p.mul(m);
console.log('matrix', p.a[1], p.a[0]);
const json = JSON.stringify(Array.from({ length: 2000 }, (_, i) => ({ id: i, name: 'item' + i, tags: ['a', 'b'] })));
console.log('json', json.length, JSON.parse(json).filter((o) => o.id % 500 === 0).map((o) => o.name));
function* permutations(arr) { if (arr.length <= 1) { yield arr; return; } for (let i = 0; i < arr.length; i++) for (const p of permutations([...arr.slice(0, i), ...arr.slice(i + 1)])) yield [arr[i], ...p]; }
let count = 0; for (const _ of permutations([1, 2, 3, 4, 5, 6, 7])) count++;
console.log('perms', count);
const re = /(\d+)-(\d+)/g; let total = 0;
const txt = Array.from({ length: 3000 }, (_, i) => `${i}-${i * 2}`).join(' ');
for (const mm of txt.matchAll(re)) total += Number(mm[2]) - Number(mm[1]);
console.log('regex total', total);
