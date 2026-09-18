// An ES module: imports, top-level await, dynamic import, import.meta.
import fs from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import path, { join } from 'path';
import * as util from 'util';
import { EventEmitter, once } from 'events';

console.log(typeof fs.readFileSync, typeof join, path.sep, typeof util.inspect);
console.log(import.meta.url.startsWith('file:///'), import.meta.url.endsWith('/main.mjs'));
console.log(typeof require, typeof module, typeof __filename, this);

const dir = path.dirname(new URL(import.meta.url).pathname);
await writeFile(join(dir, 'helper.mjs'), `
export const answer = 42;
export default function greet(name) { return 'hello ' + name; }
export class Point { constructor(x, y) { this.x = x; this.y = y; } toString() { return \`(\${this.x}, \${this.y})\`; } }
`);
const helper = await import('./helper.mjs');
console.log(helper.answer, helper.default('esm'), String(new helper.Point(1, 2)), Object.keys(helper));
console.log((await readFile(join(dir, 'helper.mjs'), 'utf8')).split('\n').length);

const em = new EventEmitter();
setTimeout(() => em.emit('ready', 'payload', 2), 5);
const args = await once(em, 'ready');
console.log('once resolved with', args);

try {
  await import('./does-not-exist.mjs');
} catch (e) {
  console.log(e.code);
}
export const late = 'exported';
console.log('module done');
