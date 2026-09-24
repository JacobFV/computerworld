// The Node side of the framework micro-benchmark that examples/dom_bench.rs
// runs on cw-jsvm: the same DOM shim, bundles, app and interactions, timed
// phase by phase, each iteration in a fresh process.
//
//     node crates/languages/jsvm/bench/driver.mjs [react|vue] [runs]
//     node --jitless crates/languages/jsvm/bench/driver.mjs [react|vue] [runs]
//
// Phases: load (evaluate the framework bundles), mount (evaluate the app and
// let the scheduler finish), click (a re-rendering click on #check-2), key
// (one keystroke into the controlled #new-task input). Prints the median of
// each in milliseconds and checks the resulting DOM.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
const vendor = join(here, '../../../web/engine/tests/vendor');
export const EXPECTED = 'task|task done|task done|task|task|task done / x';
const BUNDLES = {
  react: ['react-18.3.1.production.min.js', 'react-dom-18.3.1.production.min.js'],
  vue: ['vue-3.4.38.global.prod.js'],
};

const [, , which = 'react', runsArg = '15', child] = process.argv;

function run(file, path) {
  vm.runInThisContext(readFileSync(path, 'utf8'), { filename: file });
}

// Lets queued immediates, timers and microtasks run (React's scheduler uses
// setImmediate under Node; Vue flushes in microtasks).
async function drain() {
  for (let i = 0; i < 3; i++) await new Promise((r) => setImmediate(r));
}

async function once() {
  run('dom-shim.js', join(here, 'dom-shim.js'));
  run('phases.js', join(here, 'phases.js'));
  const t = {};
  let t0 = performance.now();
  for (const b of BUNDLES[which]) run(b, join(vendor, b));
  t.load = performance.now() - t0;
  t0 = performance.now();
  run(`${which}-app.js`, join(here, `${which}-app.js`));
  await drain();
  t.mount = performance.now() - t0;
  t0 = performance.now();
  globalThis.benchClick('check-2');
  await drain();
  t.click = performance.now() - t0;
  t0 = performance.now();
  globalThis.benchKey();
  await drain();
  t.key = performance.now() - t0;
  t.summary = globalThis.benchSummary();
  return t;
}

if (child === '--child') {
  process.stdout.write(JSON.stringify(await once()));
} else {
  const runs = Number(runsArg);
  const all = [];
  for (let i = 0; i < runs; i++) {
    const out = execFileSync(process.execPath, [...process.execArgv, fileURLToPath(import.meta.url), which, '1', '--child']);
    const t = JSON.parse(out.toString());
    if (t.summary !== EXPECTED) throw new Error(`${which}: DOM after the phases is ${t.summary}, expected ${EXPECTED}`);
    all.push(t);
  }
  const med = (k) => {
    const v = all.map((t) => t[k]).sort((a, b) => a - b);
    return v[v.length >> 1].toFixed(2);
  };
  const mode = process.execArgv.includes('--jitless') ? 'node --jitless' : 'node';
  console.log(`${which} load ${med('load')} mount ${med('mount')} click ${med('click')} key ${med('key')} (ms, median of ${runs}, ${mode})`);
}
