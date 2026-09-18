console.log('hello', 1 + 2, [1, 2, 3], { a: 1, b: 'x' });
let s = 0;
for (let i = 0; i < 10; i++) s += i;
console.log(s);
function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
console.log(fib(20));
const arr = [5, 3, 8, 1].map(x => x * 2).filter(x => x > 5).sort((a, b) => a - b);
console.log(arr, arr.length);
class Animal {
  constructor(name) { this.name = name; }
  speak() { return `${this.name} makes a sound`; }
}
class Dog extends Animal {
  speak() { return `${super.speak()} (woof)`; }
}
console.log(new Dog('Rex').speak(), new Dog('Rex'));
const { a, ...rest } = { a: 1, b: 2, c: 3 };
console.log(a, rest);
const m = new Map([['x', 1]]);
m.set('y', 2);
console.log(m, new Set([1, 2, 2, 3]));
try { null.x; } catch (e) { console.log(e.message); }
console.log(JSON.stringify({ a: [1, { b: 2 }] }), JSON.parse('[1,2]'));
console.log(typeof undefined, typeof null, typeof (() => 1), 0.1 + 0.2);
async function main() { await null; console.log('async done'); return 5; }
main().then(v => console.log('then', v));
console.log('sync end');
