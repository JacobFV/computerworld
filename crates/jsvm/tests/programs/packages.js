// A small project on disk: node_modules resolution, package.json, cycles, JSON, cache.
const fs = require('fs');
const path = require('path');
const write = (p, s) => {
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, s);
};
write('app/node_modules/greeter/package.json', JSON.stringify({ name: 'greeter', version: '1.2.3', main: './lib/greet.js' }));
write('app/node_modules/greeter/lib/greet.js', `
const { shout } = require('./util');
module.exports = function greet(name) { return shout('hello ' + name); };
module.exports.version = require('../package.json').version;
`);
write('app/node_modules/greeter/lib/util.js', 'exports.shout = (s) => s.toUpperCase() + "!";');
write('app/node_modules/@scope/pkg/index.js', 'module.exports = { scoped: true, dir: __dirname.split("/").slice(-2).join("/") };');
write('app/src/a.js', `
exports.name = 'a';
const b = require('./b');
exports.fromB = b.name + ' saw a.name=' + b.sawDone;
exports.done = true;
`);
write('app/src/b.js', `
const a = require('./a');
exports.name = 'b';
exports.sawDone = a.name;
`);
write('app/src/index.js', `
const greet = require('greeter');
console.log(greet('world'), greet.version);
console.log(require('@scope/pkg'));
const a = require('./a');
console.log(a.name, a.fromB, a.done);
console.log(require('./data.json'), module.id === __filename, path.relative(process.cwd(), __filename));
console.log(module.parent === undefined ? 'no parent prop' : typeof module.parent, module.children.length, typeof module.paths);
module.exports = { ok: true };
`.replace('path.relative', "require('path').relative"));
write('app/src/data.json', '{ "list": [1, 2, 3], "nested": { "deep": true } }');

const entry = require('./app/src');
console.log('entry exports', entry);
try {
  require('not-installed');
} catch (e) {
  console.log(e.code, e.message.split('\n')[0]);
}
try {
  require('./app/src/missing.js');
} catch (e) {
  console.log(e.code, e.requireStack.map((p) => path.basename(p)));
}
write('app/broken.json', '{ "a": 1,, }');
try {
  require('./app/broken.json');
} catch (e) {
  console.log(e.name, e.message.replace(process.cwd(), '<cwd>'));
}
console.log(Object.keys(require.cache).map((p) => path.relative(process.cwd(), p)).sort());
