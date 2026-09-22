#!/usr/bin/env node
// Compare Chromium's dump of a fixture with the engine's, write a Markdown report and a
// side-by-side picture (Chromium | engine | difference).
//
//   node scripts/web-parity/compare.mjs <name> [--threshold 0.9]
//   node scripts/web-parity/compare.mjs --all
//
// Reads crates/web/engine/tests/parity/<name>.chromium.json (+ .chromium.png, .fonts.json) and
// crates/web/engine/target-parity/<name>.engine.json (+ .engine.png, written by
// `cargo test -p cw-web --features pipeline --test parity`), writes
// crates/web/engine/target-parity/<name>.report.md and <name>.compare.png. Exits 1 when the
// pass rate is below the threshold (default: the fixture's entry in thresholds.json).
//
// The rules are the same as the Rust runner's (crates/web/engine/tests/support/mod.rs):
// - every rect edge within RECT_PX (1 px) of Chromium's;
// - widths and heights of text-dependent boxes (inline, table parts, floats, absolutes)
//   within TEXT_PX (2 px), since the faces differ and advances are quantised;
// - keyword properties equal as strings after normalisation; length properties within
//   the same tolerances; font-family reported, not compared.
import {readFile, writeFile, readdir} from 'node:fs/promises';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {join} from 'node:path';
import {PROPERTIES, LENGTH_PROPERTIES, INFORMATIONAL, RECT_PX, TEXT_PX, textDependent, normalise, px} from './common.mjs';
import {encodePNG, decodePNG} from '../dom-to-site/png.mjs';

const root = fileURLToPath(new URL('../..', import.meta.url));
const fixtures = join(root, 'crates/web/engine/tests/parity');
const outDir = join(root, 'crates/web/engine/target-parity');

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i < 0 ? fallback : args[i + 1];
};
const names = args.includes('--all')
  ? (await readdir(fixtures)).filter(f => f.endsWith('.html')).map(f => f.replace(/\.html$/, '')).sort()
  : args.filter((a, i) => !a.startsWith('--') && !args[i - 1]?.startsWith('--'));
if (names.length === 0) {
  console.error('usage: compare.mjs <name> [--threshold 0.9] | --all');
  process.exit(2);
}
const thresholds = JSON.parse(await readFile(join(fixtures, 'thresholds.json'), 'utf8'));

function rectMismatches(expected, got, sizeTol, prefix, out) {
  for (const [name, tol] of [['x', RECT_PX], ['y', RECT_PX], ['width', sizeTol], ['height', sizeTol]]) {
    const d = Math.abs(expected[name] - got[name]);
    if (d > tol + 1e-9) out.push({what: `${prefix}${name}`, expected: String(expected[name]), got: String(got[name]), delta: d});
  }
}

/// Same rules as `support::compare` in the Rust runner.
export function compare(expected, got) {
  const byPath = new Map(got.nodes.map(n => [n.path, n]));
  const report = {fixture: expected.fixture, total: expected.nodes.length, passed: 0, missing: 0, byProperty: {}, nodes: []};
  for (const node of expected.nodes) {
    const mismatches = [];
    const other = byPath.get(node.path);
    if (!other) {
      report.missing++;
      mismatches.push({what: 'node', expected: 'present', got: 'missing', delta: 1000});
    } else if (node.kind !== other.kind) {
      mismatches.push({what: 'kind', expected: node.kind, got: other.kind, delta: 1000});
    } else if (node.kind === 'element') {
      const textDep = textDependent(node);
      // A <br> generates no box in the engine; only its computed values are compared.
      if (node.tag !== 'br') rectMismatches(node.rect, other.rect, textDep ? TEXT_PX : RECT_PX, 'rect.', mismatches);
      for (const p of PROPERTIES) {
        if (INFORMATIONAL.has(p)) continue;
        const e = normalise(p, node.computed[p]), g = normalise(p, other.computed?.[p]);
        if (e === g) continue;
        if (LENGTH_PROPERTIES.has(p)) {
          const ep = px(e), gp = px(g);
          if (ep !== null && gp !== null) {
            const tol = textDep && (p === 'width' || p === 'height') ? TEXT_PX : RECT_PX;
            const d = Math.abs(ep - gp);
            if (d <= tol + 1e-9) continue;
            mismatches.push({what: p, expected: e, got: g, delta: d});
            continue;
          }
        }
        mismatches.push({what: p, expected: e, got: g, delta: 1});
      }
    } else {
      if (node.rects.length !== other.rects.length) {
        mismatches.push({what: 'lines', expected: String(node.rects.length), got: String(other.rects.length), delta: Math.abs(node.rects.length - other.rects.length) * 10});
      } else {
        node.rects.forEach((r, i) => rectMismatches(r, other.rects[i], TEXT_PX, `line[${i}].`, mismatches));
      }
    }
    const passed = mismatches.length === 0;
    if (passed) report.passed++;
    for (const m of mismatches) {
      const key = m.what.split('.')[0];
      report.byProperty[key] = (report.byProperty[key] ?? 0) + 1;
    }
    report.nodes.push({path: node.path, passed, mismatches, score: mismatches.reduce((a, m) => a + m.delta, 0)});
  }
  return report;
}

function markdown(report, fonts, engineFonts, threshold) {
  const rate = report.total ? report.passed / report.total : 0;
  const lines = [];
  lines.push(`# Parity: ${report.fixture}`, '');
  lines.push(`- nodes: ${report.total} (elements and text nodes in Chromium's dump)`);
  lines.push(`- passed: ${report.passed} (${(rate * 100).toFixed(1)}%)`);
  lines.push(`- missing from the engine's dump: ${report.missing}`);
  lines.push(`- threshold: ${(threshold * 100).toFixed(1)}%`);
  lines.push(`- verdict: ${rate >= threshold ? 'pass' : 'FAIL'}`, '');
  lines.push('Tolerances: rect edges within 1 px; widths and heights of text-dependent boxes (inline, table parts, floats, absolutes) within 2 px; keyword properties exact; `font-family` informational.', '');
  lines.push('## Fonts', '', '| font-family | Chromium shaped with | engine face |', '|---|---|---|');
  const engineBy = new Map((engineFonts ?? []).map(f => [f.family, f.engine]));
  for (const f of fonts) {
    const used = f.chromium.length ? f.chromium.map(u => `${u.family} (${u.glyphs})`).join(', ') : '?';
    lines.push(`| \`${f.family.replace(/\|/g, '\\|')}\` | ${used} | ${engineBy.get(f.family) ?? '?'} |`);
  }
  lines.push('', '## Mismatches by property', '', '| property | nodes |', '|---|---|');
  for (const [k, v] of Object.entries(report.byProperty).sort()) lines.push(`| ${k} | ${v} |`);
  lines.push('', '## Worst offenders', '');
  const worst = report.nodes.filter(n => !n.passed).sort((a, b) => b.score - a.score || a.path.localeCompare(b.path)).slice(0, 40);
  for (const n of worst) {
    lines.push(`### \`${n.path}\` (score ${n.score.toFixed(2)})`, '');
    for (const m of n.mismatches) lines.push(`- ${m.what}: expected \`${m.expected}\`, got \`${m.got}\``);
    lines.push('');
  }
  return lines.join('\n') + '\n';
}

/// Chromium | engine | difference, side by side; missing pictures are left blank.
function sideBySide(a, b) {
  const width = Math.max(a?.width ?? 0, b?.width ?? 0), height = Math.max(a?.height ?? 0, b?.height ?? 0);
  if (!width || !height) return null;
  const out = new Uint8Array(width * 3 * height * 4).fill(255);
  const put = (img, panel) => {
    if (!img) return;
    for (let y = 0; y < img.height; y++) for (let x = 0; x < img.width; x++) {
      const s = (y * img.width + x) * 4, d = (y * width * 3 + panel * width + x) * 4;
      out[d] = img.rgba[s]; out[d + 1] = img.rgba[s + 1]; out[d + 2] = img.rgba[s + 2]; out[d + 3] = 255;
    }
  };
  put(a, 0);
  put(b, 1);
  if (a && b) {
    for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
      const inA = x < a.width && y < a.height, inB = x < b.width && y < b.height;
      const sa = (y * a.width + x) * 4, sb = (y * b.width + x) * 4;
      const d = (y * width * 3 + 2 * width + x) * 4;
      let diff = 0;
      for (let c = 0; c < 3; c++) diff = Math.max(diff, Math.abs((inA ? a.rgba[sa + c] : 255) - (inB ? b.rgba[sb + c] : 255)));
      // White where equal, red where different, scaled so small differences show.
      const v = 255 - Math.min(255, diff * 3);
      out[d] = 255; out[d + 1] = v; out[d + 2] = v; out[d + 3] = 255;
    }
  }
  return {width: width * 3, height, rgba: out};
}

let failed = 0;
for (const name of names) {
  const expected = JSON.parse(await readFile(join(fixtures, `${name}.chromium.json`), 'utf8'));
  const enginePath = join(outDir, `${name}.engine.json`);
  if (!existsSync(enginePath)) {
    console.error(`${name}: no ${enginePath}; run \`cargo test -p cw-web --features pipeline --test parity\` first`);
    failed++;
    continue;
  }
  const got = JSON.parse(await readFile(enginePath, 'utf8'));
  const fonts = existsSync(join(fixtures, `${name}.fonts.json`)) ? JSON.parse(await readFile(join(fixtures, `${name}.fonts.json`), 'utf8')) : [];
  const threshold = Number(flag('--threshold', thresholds[name] ?? 0));
  const report = compare(expected, got);
  const rate = report.total ? report.passed / report.total : 0;
  await writeFile(join(outDir, `${name}.report.md`), markdown(report, fonts, got.fonts, threshold));
  const read = async p => existsSync(p) ? decodePNG(await readFile(p)) : null;
  const picture = sideBySide(await read(join(fixtures, `${name}.chromium.png`)), await read(join(outDir, `${name}.engine.png`)));
  if (picture) await writeFile(join(outDir, `${name}.compare.png`), encodePNG(picture));
  const verdict = rate >= threshold ? 'pass' : 'FAIL';
  console.log(`${name}: ${report.passed}/${report.total} (${(rate * 100).toFixed(1)}%, threshold ${(threshold * 100).toFixed(1)}%) ${verdict} -> ${join(outDir, `${name}.report.md`)}`);
  if (rate < threshold) failed++;
}
process.exit(failed ? 1 : 0);
