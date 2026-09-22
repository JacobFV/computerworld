// An uncaught ReferenceError at the top level, after some output.
const items = [3, 1, 2];
items.sort((a, b) => a - b);
console.log('sorted', items);
console.error('about to fail');
const total = items.reduce((s, x) => s + x, 0) + missingOffset;
console.log(total);
