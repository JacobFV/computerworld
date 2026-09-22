// Modules and the filesystem
const fs = require('fs');
const path = require('path');
const os = require('os');
const dir = path.join(process.cwd(), 'work');
fs.mkdirSync(path.join(dir, 'lib'), { recursive: true });
fs.writeFileSync(path.join(dir, 'lib', 'math.js'), `
const PI = 3.14159;
function area(r) { return PI * r * r; }
module.exports = { PI, area };
console.log('math loaded', __filename.endsWith('math.js'), path === undefined);
`.replace('path === undefined', "typeof path"));
fs.writeFileSync(path.join(dir, 'lib', 'counter.js'), `
let count = 0;
exports.inc = () => ++count;
exports.get = () => count;
`);
fs.writeFileSync(path.join(dir, 'data.json'), JSON.stringify({ name: 'demo', items: [1, 2, 3] }, null, 2));
fs.writeFileSync(path.join(dir, 'lib', 'index.js'), `module.exports = 'lib index';`);
const math = require('./work/lib/math');
const math2 = require('./work/lib/math.js');
console.log(math.area(2).toFixed(3), math === math2, math.PI);
const counter = require('./work/lib/counter');
counter.inc(); counter.inc();
console.log(require('./work/lib/counter').get(), require('./work/data.json').items, require('./work/lib'));
try { require('./work/missing'); } catch (e) { console.log(e.code, e.message.split('\n')[0]); }
console.log(typeof require.resolve, require.resolve('./work/lib/math').endsWith('work/lib/math.js'), require('node:path') === path);
// fs APIs
const file = path.join(dir, 'notes.txt');
fs.writeFileSync(file, 'line1\n');
fs.appendFileSync(file, 'line2\n');
console.log(JSON.stringify(fs.readFileSync(file, 'utf8')), fs.readFileSync(file).length, fs.existsSync(file), fs.existsSync(file + '.nope'));
console.log(fs.readdirSync(dir).sort(), fs.readdirSync(dir, { withFileTypes: true }).map((d) => d.name + (d.isDirectory() ? '/' : '')).sort());
const st = fs.statSync(file);
console.log(st.isFile(), st.isDirectory(), st.size, fs.statSync(dir).isDirectory());
fs.renameSync(file, file + '.bak');
fs.copyFileSync(file + '.bak', file);
fs.unlinkSync(file + '.bak');
console.log(fs.readdirSync(dir).sort());
try { fs.readFileSync(path.join(dir, 'ghost.txt')); } catch (e) { console.log(e.code, e.syscall, e.errno, e.message.replace(dir, '<dir>')); }
try { fs.mkdirSync(dir); } catch (e) { console.log(e.code, e.message.replace(dir, '<dir>')); }
try { fs.rmdirSync(dir); } catch (e) { console.log(e.code); }
fs.rmSync(path.join(dir, 'lib'), { recursive: true, force: true });
console.log(fs.readdirSync(dir).sort(), fs.existsSync(path.join(dir, 'lib')));
fs.promises.readFile(path.join(dir, 'data.json'), 'utf8').then((s) => console.log('promise read', JSON.parse(s).name));
fs.readFile(path.join(dir, 'data.json'), 'utf8', (err, data) => {
  console.log('callback read', err, data.length);
  fs.readFile(path.join(dir, 'none.json'), (err) => console.log('callback err', err.code));
});
(async () => {
  const fsp = require('fs/promises');
  await fsp.writeFile(path.join(dir, 'async.txt'), 'async data');
  console.log('async', await fsp.readFile(path.join(dir, 'async.txt'), 'utf8'));
  try { await fsp.readFile(path.join(dir, 'nope')); } catch (e) { console.log('async err', e.code); }
})();
console.log(path.basename('/a/b/c.txt'), path.basename('/a/b/c.txt', '.txt'), path.extname('x.tar.gz'), path.dirname('/a/b/c'), path.isAbsolute('a'));
console.log(path.normalize('/a//b/../c/./d/'), path.relative('/a/b/c', '/a/d'), path.join('a', '..', '..', 'b'), path.resolve('/x', 'y', '../z'));
console.log(path.parse('/home/user/file.test.js'), path.format({ dir: '/tmp', name: 'f', ext: '.md' }), path.sep, path.delimiter);
console.log(os.EOL === '\n', os.platform(), os.type(), typeof os.cpus().length, typeof os.totalmem(), os.tmpdir(), typeof os.hostname());
