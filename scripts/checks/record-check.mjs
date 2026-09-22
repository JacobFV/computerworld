/** The one moving picture on the project site: a bounded check, its verdict, and the
 * counterexample replayed on the machine it was found on.
 *
 *   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/usr/bin/google-chrome \
 *     node scripts/checks/record-check.mjs
 *
 * Nothing in the frame is drawn from imagination.
 *
 *   The search      `scripts/checks/cw-check.mjs --depth 2 --progress 1` is run as a child
 *                   process and its own progress lines are parsed. Every number on
 *                   screen -- states checked, distinct, frontier, duplicates, states
 *                   that left the scope, policies broken, elapsed seconds -- is read
 *                   off those lines, and the fan of dots is drawn one dot per state
 *                   the checker actually generated, in the order it generated them,
 *                   with each expansion's real branching factor.
 *   The verdict     The P2 row of the report that run wrote, counterexample included.
 *   The replay      A world booted from the same definition at the same seed, seeded
 *                   with the same two opening actions, driven by the counterexample's
 *                   own `pointer.v1` primitives, and pictured with `env.render()`.
 *                   The script asserts through `world.inspect()` that pull request #15
 *                   really does go from open to merged with bmartinez's request for
 *                   changes still standing, and refuses to write a video if it does not.
 *
 * The only liberties are of the camera: the search's 81 seconds are replayed over a
 * few (the real elapsed clock is on screen throughout), and the machine's 1440x2000
 * display is cropped and panned to fit a 1280x800 frame.
 *
 * There are two ways out of the page, and the script takes the better one it can.
 *
 *   With ffmpeg    Every frame is drawn at an exact timestamp -- i/fps, not the wall
 *                  clock -- handed out of the page as a lossless PNG, and encoded by
 *                  libx264. Deterministic timing, a lossless source, and a real
 *                  rate-control loop that fits the picture to a byte budget.
 *   Without it     Chrome's own MediaRecorder records the canvas live in H.264/MP4.
 *                  It writes a *fragmented* MP4, whose movie header claims no duration
 *                  at all, so `progressive()` below rewrites it as an ordinary one --
 *                  one moov with real sample tables, then one mdat -- which is what a
 *                  bare <video> with a scrub bar wants.
 *
 * Either way the poster is one frame of the same storyboard, taken at --poster-at.
 */
import {spawn, spawnSync} from 'node:child_process';
import {readFile, writeFile, mkdir, rm, stat} from 'node:fs/promises';
import {createServer} from 'node:net';
import {dirname, join} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('../..', import.meta.url));

const usage = `Record the bounded check: the search, the verdict, the replay.

  node scripts/checks/record-check.mjs [options]

  --depth <k>        depth to run the checker at            (default: 2)
  --log <path>       reuse a previously captured run instead of running the checker;
                     the file is the checker's stdout, one line per line, and
                     --report must point at the report that run wrote
  --report <path>    the report that goes with --log
  --keep <dir>       where a fresh run's log and report are kept
                     (default: a cw-check-run directory under the system temp dir; the
                     video is the deliverable, the run is only its evidence)
  --video <path>     (default: site/media/b2-check.mp4)
  --poster <path>    (default: site/media/b2-check.jpg)
  --poster-at <s>    the moment the poster is taken          (default: 10.2)
  --ffmpeg <path>    encode with this ffmpeg instead of MediaRecorder; by default an
                     ffmpeg on PATH is used if there is one, and MediaRecorder if not
  --no-ffmpeg        force the MediaRecorder path even when ffmpeg is there
  --crf <n>          libx264 quality, lower is better        (default: 23)
  --max-bytes <n>    the video must come in under this; if a CRF encode does not, it is
                     re-encoded two-pass at the bitrate that does  (default: 1000000)
  --bitrate <bps>    video bitrate handed to MediaRecorder   (default: 1500000)
  --fps <n>          capture rate                            (default: 25)
  --quality <0..1>   poster JPEG quality                     (default: 0.82)
  --headful          show the browser doing it
  --help

Needs PLAYWRIGHT_MODULE (there is no playwright package in the project) and CHROME_BIN.
The machine that gets replayed is the Wasm bundle in site/pkg/, so that bundle is what
the picture is of.
`;

const options = {
  depth: 2, log: null, report: null, keep: null, ffmpeg: null, noFfmpeg: false,
  crf: 23, maxBytes: 1000000,
  video: `${root}site/media/b2-check.mp4`,
  poster: `${root}site/media/b2-check.jpg`,
  posterAt: 10.2, bitrate: 1500000, fps: 25, quality: 0.82, headful: false,
};
const numbers = new Set(['depth', 'posterAt', 'bitrate', 'fps', 'quality', 'crf', 'maxBytes']);
const flags = new Set(['headful', 'noFfmpeg']);
const alias = {'poster-at': 'posterAt', 'max-bytes': 'maxBytes', 'no-ffmpeg': 'noFfmpeg'};
for (let i = 2; i < process.argv.length; i++) {
  const argument = process.argv[i];
  if (argument === '--help' || argument === '-h') { process.stdout.write(usage); process.exit(0); }
  const raw = argument.startsWith('--') ? argument.slice(2) : null;
  const name = alias[raw] ?? raw;
  if (!name || !(name in options)) { process.stderr.write(`unknown option ${argument}\n\n${usage}`); process.exit(2); }
  if (flags.has(name)) { options[name] = true; continue; }
  const value = process.argv[++i];
  if (value === undefined) { process.stderr.write(`${argument} wants a value\n`); process.exit(2); }
  options[name] = numbers.has(name) ? Number(value) : value;
}
if ((options.log === null) !== (options.report === null)) {
  process.stderr.write('--log and --report go together\n'); process.exit(2);
}

// ---------------------------------------------------------------------------
// the encoder
// ---------------------------------------------------------------------------
/** An ffmpeg that runs here, or null. Only PATH is searched: a video the project cannot
 * rebuild from a clean checkout is not much of a deliverable, so the ffmpeg path is an
 * improvement when one is installed and never a requirement. */
function findFfmpeg() {
  if (options.noFfmpeg) return null;
  const candidates = options.ffmpeg ? [options.ffmpeg] : ['ffmpeg'];
  for (const candidate of candidates) {
    const probe = spawnSync(candidate, ['-hide_banner', '-encoders'], {encoding: 'utf8'});
    if (probe.status === 0 && /\blibx264\b/.test(probe.stdout)) return candidate;
  }
  if (options.ffmpeg) {
    process.stderr.write(`${options.ffmpeg} is not an ffmpeg with libx264 in it\n`);
    process.exit(2);
  }
  return null;
}
const ffmpeg = findFfmpeg();

// ---------------------------------------------------------------------------
// the search -- run the real checker, keep its own words
// ---------------------------------------------------------------------------
const keep = options.keep ?? join(tmpdir(), 'cw-check-run');

async function runChecker() {
  await mkdir(keep, {recursive: true});
  const logPath = `${keep}/cw-check-depth${options.depth}.log`;
  const reportPath = `${keep}/policies-depth${options.depth}.json`;
  process.stdout.write(`cw-check --depth ${options.depth}: running (this is the search the video shows)\n`);
  const child = spawn(process.execPath, [
    `${root}scripts/checks/cw-check.mjs`, '--depth', String(options.depth), '--progress', '1', '--out', reportPath,
  ], {cwd: root, stdio: ['ignore', 'pipe', 'inherit']});
  let text = '', rest = '';
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', chunk => {
    rest += chunk;
    const lines = rest.split('\n');
    rest = lines.pop();
    for (const line of lines) text += line + '\n';
    const last = lines.filter(l => l.trim()).pop();
    if (last) process.stdout.write(`  ${last.trim()}\n`);
  });
  const code = await new Promise(resolve => child.on('close', resolve));
  if (rest) text += rest + '\n';
  if (code !== 0) { process.stderr.write(`cw-check exited ${code}\n`); process.exit(1); }
  await writeFile(logPath, text);
  return {logPath, reportPath};
}

const {logPath, reportPath} = options.log
  ? {logPath: options.log, reportPath: options.report}
  : await runChecker();

const log = await readFile(logPath, 'utf8');
const report = JSON.parse(await readFile(reportPath, 'utf8'));

/** The checker's progress lines, one per expanded state, as the numbers they carry.
 * `checked` is cumulative, so the difference between two lines is exactly the number of
 * successors the expansion in between generated: the branching factor of that state. */
const series = [];
for (const line of log.split('\n')) {
  const m = line.match(/depth (\d+)\/(\d+)\s+expanded (\d+)\s+checked (\d+)\s+distinct (\d+)\s+frontier (\d+)\s+dup (\d+)\s+out-of-scope (\d+)\s+violated (\d+)\/(\d+)\s+(\d+)s/);
  if (!m) continue;
  const [, depth, , expanded, checked, distinct, frontier, dup, oos, violated, policies, seconds] = m.map(Number);
  series.push({depth, expanded, checked, distinct, frontier, dup, oos, violated, policies, seconds});
}
if (series.length < 2) { process.stderr.write(`no progress lines in ${logPath}\n`); process.exit(1); }

const policy = report.policies.find(p => p.id === 'P2');
if (!policy || policy.status !== 'violated' || !policy.counterexample?.replay_verified) {
  process.stderr.write(`the depth-${report.depth} run did not leave a replay-verified P2 counterexample; nothing honest to record\n`);
  process.exit(1);
}
process.stdout.write(`cw-check: depth ${report.depth} exhausted, ${report.states_explored} states checked, ` +
  `${report.policies_held} hold, ${report.policies_violated} violated\n`);
process.stdout.write(`P2 counterexample (${policy.counterexample.length} actions): ${policy.counterexample.explanation}\n`);

/** The first expansion whose line reports more policies broken than the one before it
 * reported: the expansion during which this violation came to light. */
const brokeAt = (() => {
  for (let i = 1; i < series.length; i++) if (series[i].violated > series[i - 1].violated) return series[i];
  return series[series.length - 1];
})();

// ---------------------------------------------------------------------------
// the page
// ---------------------------------------------------------------------------
const freePort = () => new Promise((resolve, reject) => {
  const probe = createServer();
  probe.on('error', reject);
  probe.listen(0, '127.0.0.1', () => { const {port} = probe.address(); probe.close(() => resolve(port)); });
});
const port = await freePort();
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const server = spawn(process.execPath, [`${root}scripts/site/serve-site.mjs`, String(port)], {stdio: 'ignore'});
const up = async () => {
  for (let tries = 0; tries < 60; tries++) {
    try { if ((await fetch(`http://127.0.0.1:${port}/`)).ok) return true; } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  return false;
};
if (!await up()) { server.kill(); console.error(`the site server did not come up on ${port}`); process.exit(1); }

const worldJson = await readFile(`${root}${report.world === 'company-2026' ? 'worlds/company-2026/world.json' : `worlds/${report.world}/world.json`}`, 'utf8');

/** Everything that happens in the tab. It boots the machine, replays the counterexample,
 * lays the storyboard out over the real numbers, paints it in real time and records the
 * canvas with MediaRecorder. */
async function record({run, options: o, worldUrl}) {
  // --- the machine ---------------------------------------------------------
  const {default: init, World, installFont, fontPackStatus} = await import('/pkg/computerworld.js');
  await init();
  await Promise.all(fontPackStatus().files.filter(f => !f.installed).map(async f => {
    const response = await fetch(`/pkg/${f.path}`);
    if (response.ok) installFont(new Uint8Array(await response.arrayBuffer()));
  }));
  const definition = await (await fetch(worldUrl)).json();
  definition.metadata = definition.metadata || {};
  definition.metadata.desktop_themes = {...(definition.metadata.desktop_themes || {}), [run.machine]: 'virtual-macos-golden-gate'};

  const [SW, SH] = [run.viewport.width, run.viewport.height];
  const world = new World(definition, run.seed);
  const env = world.environment({
    actor: run.actor, machines: [run.machine],
    actions: ['application.v1', 'keyboard.v1', 'pointer.v1', 'browser.v1'],
    observations: ['semantic.v1'],
  });
  const step = action => env.step([{...action, machine: run.machine}]).outcomes[0];
  for (const action of [
    {family: 'application.v1', op: 'launch', payload: {kind: 'browser', argument: `${run.scope}/pulls`}},
    {family: 'application.v1', op: 'maximize', payload: {window: 0}},
  ]) { const outcome = step(action); if (!outcome.success) throw new Error(`seed action failed: ${JSON.stringify(outcome.error)}`); }

  /** Pull request 15 as the world itself reports it. */
  const pull = () => {
    const repository = world.inspect().state.services.github.state.repositories.atlas;
    const thread = repository.pull_requests['15'];
    return {
      state: thread.state,
      merged_by: thread.merged_by || '',
      standing: (thread.reviews || []).filter(r => r.decision === 'request_changes').map(r => r.author),
      main: repository.refs['refs/heads/main'],
    };
  };

  const source = document.createElement('canvas');
  source.width = SW; source.height = SH;
  const into = source.getContext('2d');
  const shoot = async () => {
    const frame = env.render(SW, SH);
    into.putImageData(new ImageData(new Uint8ClampedArray(frame.rgba), SW, SH), 0, 0);
    frame.free();
    return createImageBitmap(source);
  };

  // The counterexample, driven one primitive at a time, pictured either side of each.
  const shots = [await shoot()];
  const facts = [pull()];
  for (const primitive of run.primitives) {
    const outcome = step(primitive);
    if (!outcome.success) throw new Error(`the counterexample would not replay: ${JSON.stringify(outcome.error)}`);
    shots.push(await shoot());
    facts.push(pull());
  }
  // If the machine did not actually do the forbidden thing there is no video to make.
  const before = facts[0], after = facts[facts.length - 1];
  if (before.state !== 'open') throw new Error(`pull request 15 started ${before.state}, not open`);
  if (after.state !== 'merged') throw new Error(`pull request 15 ended ${after.state}, not merged`);
  if (after.merged_by !== run.actor) throw new Error(`pull request 15 was merged by ${after.merged_by}, not ${run.actor}`);
  if (!after.standing.length) throw new Error('no request for changes was standing at the merge');
  if (after.main === before.main) throw new Error('refs/heads/main did not move');

  // --- the storyboard ------------------------------------------------------
  const W = 1280, H = 800;
  const BG = '#0c0b0a', INK = '#ece7dd', DIM = '#8d877c', FAINT = '#5f5a52';
  const LINE = '#262320', LINE2 = '#3a3631', ACCENT = '#ffb454', BREAKS = '#d4705a', HOLDS = '#9bb07e';
  const MONO = 'ui-monospace, "DejaVu Sans Mono", "Liberation Mono", monospace';
  const SANS = '"DejaVu Sans", system-ui, -apple-system, "Liberation Sans", sans-serif';
  const mono = (size, weight = '') => `${weight} ${size}px ${MONO}`.trim();
  const sans = (size, weight = '') => `${weight} ${size}px ${SANS}`.trim();

  const canvas = document.createElement('canvas');
  canvas.width = W; canvas.height = H;
  document.body.style.cssText = 'margin:0;background:#000';
  document.body.append(canvas);
  const ctx = canvas.getContext('2d', {alpha: false});

  // Beats, in seconds. The replay gets the most of them, and the camera is kept as
  // still as it can be: every second the frame does not move is a second the encoder
  // spends sharpening the machine's own text rather than chasing a pan.
  const T = {
    searchFrom: 0.35, searchTo: 3.70,
    verdictIn: 4.05,
    replayIn: 5.30,
    click1: 6.00, page1: 6.30, pan1From: 6.30, pan1To: 6.90,
    click2: 8.45, page2: 8.75,
    pan2From: 9.30, pan2To: 9.85,
    // The tail is half a second of one unchanging frame: the encoder needs the stillness
    // to sharpen it back up, and it is the frame the loop returns to.
    outFrom: 10.70, outTo: 11.05, end: 11.55,
  };
  const END = T.end;

  const clamp = (x, a, b) => Math.max(a, Math.min(b, x));
  const lerp = (a, b, t) => a + (b - a) * t;
  const ease = t => t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
  const ramp = (t, a, b) => clamp((t - a) / (b - a), 0, 1);

  // --- the fan -------------------------------------------------------------
  // One dot per state the checker generated, in generation order. The root's successors
  // sit on the inner ring; the successors of each state the checker went on to expand sit
  // on the outer ring, in a wedge of its own whose width is that state's real branching
  // factor. Colour is the checker's own bookkeeping: a state that joined the frontier, a
  // state it had already seen, a state that left the scope.
  const CX = 858, CY = 430, R1 = 104, R2 = 278;
  const first = run.series[0];
  const expansions = run.series.slice(1);              // every expansion after the root's
  const rootFan = first.checked - 1;                   // successors of the start state
  const wedges = expansions.map((s, i) => {
    const previous = i === 0 ? {checked: first.checked, dup: first.dup, oos: first.oos, frontier: 0} : expansions[i - 1];
    return {
      line: s,
      total: s.checked - previous.checked,
      fresh: s.frontier - previous.frontier,
      dup: s.dup - previous.dup,
      oos: s.oos - previous.oos,
    };
  });
  const stubs = rootFan - wedges.length;               // root successors nobody expanded
  const weight = wedges.map(w => w.total);
  const total = weight.reduce((a, b) => a + b, 0) + stubs * 9;
  // a deterministic shuffle, so a wedge does not read as a tidy gradient it never was
  const mix = (items, seed) => {
    let s = seed >>> 0 || 1;
    const out = items.slice();
    for (let i = out.length - 1; i > 0; i--) {
      s = (s * 1664525 + 1013904223) >>> 0;
      const j = s % (i + 1);
      [out[i], out[j]] = [out[j], out[i]];
    }
    return out;
  };

  const dots = [];          // in the order the checker generated them
  const inner = [];
  {
    // lay the inner ring out first: every expanded state at the middle of its own wedge,
    // every unexpanded one in the gaps between them
    let cursor = -Math.PI / 2, at = 0;
    const slices = [];
    for (let i = 0; i < wedges.length; i++) {
      slices.push({kind: 'wedge', index: i, span: (weight[i] / total) * Math.PI * 2});
      if (at < stubs && i % Math.max(1, Math.round(wedges.length / Math.max(stubs, 1))) === 0) {
        slices.push({kind: 'stub', span: (9 / total) * Math.PI * 2}); at++;
      }
    }
    while (at < stubs) { slices.push({kind: 'stub', span: (9 / total) * Math.PI * 2}); at++; }
    const kinds = [];
    for (const slice of slices) {
      slice.a0 = cursor; slice.a1 = cursor + slice.span; cursor = slice.a1;
      slice.mid = (slice.a0 + slice.a1) / 2;
      kinds.push(slice);
    }
    // 21 of the root's 32 successors joined the frontier and were expanded; the rest were
    // duplicates or out of scope, in the counts the first progress line reports.
    const spare = mix([...Array(first.dup).fill('dup'), ...Array(first.oos).fill('oos')], 7);
    let spareAt = 0;
    for (const slice of kinds) {
      inner.push({
        a: slice.mid, kind: slice.kind === 'wedge' ? 'fresh' : (spare[spareAt++] ?? 'dup'),
        slice,
      });
    }
    for (const d of inner) dots.push({ring: 1, a: d.a, kind: d.kind, parent: null});
    for (let i = 0; i < wedges.length; i++) {
      const slice = kinds.find(k => k.kind === 'wedge' && k.index === i);
      const w = wedges[i];
      const kindsOut = mix([
        ...Array(Math.max(w.fresh, 0)).fill('fresh'),
        ...Array(Math.max(w.dup, 0)).fill('dup'),
        ...Array(Math.max(w.oos, 0)).fill('oos'),
      ], 1013 + i * 31);
      const pad = slice.span * 0.06;
      for (let k = 0; k < w.total; k++) {
        const t = w.total === 1 ? 0.5 : k / (w.total - 1);
        dots.push({
          ring: 2, a: lerp(slice.a0 + pad, slice.a1 - pad, t),
          kind: kindsOut[k] ?? 'fresh', parent: slice.mid, wedge: i,
        });
      }
    }
  }
  const brokeWedge = wedges.findIndex(w => w.line.expanded === run.brokeAt.expanded);
  const brokeSlice = brokeWedge >= 0 ? dots.find(d => d.ring === 2 && d.wedge === brokeWedge) : null;

  // The fan is painted once, cumulatively, on a canvas of its own: each animation frame
  // only adds the dots that have appeared since the last one.
  const fan = document.createElement('canvas');
  fan.width = W; fan.height = H;
  const fx = fan.getContext('2d');
  let painted = 0;
  const colour = {fresh: ACCENT, dup: '#6d665c', oos: '#4c463e'};
  const paintTo = n => {
    for (; painted < n && painted < dots.length; painted++) {
      const d = dots[painted];
      const r = d.ring === 1 ? R1 : R2;
      const x = CX + Math.cos(d.a) * r, y = CY + Math.sin(d.a) * r;
      fx.strokeStyle = d.kind === 'fresh' ? 'rgba(255,180,84,0.20)' : 'rgba(150,140,126,0.10)';
      fx.lineWidth = 1;
      fx.beginPath();
      if (d.ring === 1) { fx.moveTo(CX, CY); fx.lineTo(x, y); }
      else { fx.moveTo(CX + Math.cos(d.parent) * R1, CY + Math.sin(d.parent) * R1); fx.lineTo(x, y); }
      fx.stroke();
      fx.fillStyle = colour[d.kind];
      fx.beginPath();
      fx.arc(x, y, d.kind === 'fresh' ? 2.6 : 2.1, 0, Math.PI * 2);
      fx.fill();
    }
  };

  // --- small painters ------------------------------------------------------
  const text = (s, x, y, font, fill, align = 'left') => {
    ctx.font = font; ctx.fillStyle = fill; ctx.textAlign = align; ctx.textBaseline = 'alphabetic';
    ctx.fillText(s, x, y);
  };
  const tracked = (s, x, y, font, fill, spacing) => {
    ctx.font = font; ctx.fillStyle = fill; ctx.textAlign = 'left'; ctx.textBaseline = 'alphabetic';
    let cursor = x;
    for (const ch of s) { ctx.fillText(ch, cursor, y); cursor += ctx.measureText(ch).width + spacing; }
    return cursor - spacing - x;
  };
  const rule = (x0, y, x1, stroke) => {
    ctx.strokeStyle = stroke; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(x0, y + 0.5); ctx.lineTo(x1, y + 0.5); ctx.stroke();
  };
  const wrap = (s, font, width) => {
    ctx.font = font;
    const out = []; let line = '';
    for (const word of s.split(' ')) {
      const trial = line ? `${line} ${word}` : word;
      if (ctx.measureText(trial).width > width && line) { out.push(line); line = word; } else line = trial;
    }
    if (line) out.push(line);
    return out;
  };

  const header = (right, rightColour) => {
    text(`cw-check  --depth ${run.depth}   ·   ${run.world}   ·   seed ${run.seed}   ·   engine ${run.engine}`,
      56, 36, mono(14), FAINT);
    if (right) {
      ctx.font = mono(14);
      const w = ctx.measureText(right).width;
      ctx.fillStyle = rightColour;
      ctx.beginPath(); ctx.arc(W - 56 - w - 14, 31, 3.5, 0, Math.PI * 2); ctx.fill();
      text(right, W - 56, 36, mono(14), rightColour, 'right');
    }
    rule(56, 56, W - 56, LINE);
  };

  /** The counter and the four numbers under it, straight off the progress line the
   * search has reached, with the real elapsed clock beside them. */
  const readout = (now, alpha) => {
    ctx.globalAlpha = alpha;
    const x = 56;
    tracked('STATES CHECKED', x, 146, mono(13), FAINT, 2.6);
    text(String(now.checked).padStart(3, ' '), x - 4, 232, mono(76, 'bold'), INK);
    let y = 300;
    const row = (label, value, colour = INK) => {
      text(label, x, y, mono(16), DIM);
      text(value, x + 340, y, mono(16), colour, 'right');
      rule(x, y + 11, x + 340, '#1c1a18');
      y += 34;
    };
    row('distinct', String(now.distinct));
    row('frontier', String(now.frontier));
    row('already seen', String(now.dup));
    row('left the scope', String(now.oos));
    row('policies broken', `${now.violated} / ${now.policies}`, now.violated ? BREAKS : HOLDS);
    text(`elapsed  ${now.seconds.toFixed(1)} s`, x, y + 10, mono(14), FAINT);
    const lines = [
      'every action sequence to depth ' + run.depth,
      'from ' + run.start,
    ];
    let ly = y + 58;
    for (const l of lines) for (const part of wrap(l, mono(13), 360)) { text(part, x, ly, mono(13), '#6f6961'); ly += 19; }
    ctx.globalAlpha = 1;
  };

  const legend = alpha => {
    ctx.globalAlpha = alpha;
    const x = W - 56 - 210, y0 = H - 118;
    const key = (row, fill, label) => {
      const y = y0 + row * 24;
      ctx.fillStyle = fill; ctx.beginPath(); ctx.arc(x + 4, y - 4, 3, 0, Math.PI * 2); ctx.fill();
      text(label, x + 18, y, mono(13), '#6f6961');
    };
    key(0, colour.fresh, 'new — expanded');
    key(1, colour.dup, 'already seen');
    key(2, colour.oos, 'left the scope');
    ctx.globalAlpha = 1;
  };

  // --- the desktop ---------------------------------------------------------
  // One scale, chosen so the machine's full 1440-pixel width fits the frame; the camera
  // only ever slides up and down it.
  const SCALE = W / SW;                       // 0.888…
  const CROP = H / SCALE;                     // 900 source rows in view
  const toFrameX = sx => sx * SCALE;
  const toFrameY = (sy, top) => (sy - top) * SCALE;
  const desktop = (shot, top) => {
    ctx.drawImage(shot, 0, top, SW, CROP, 0, 0, W, H);
  };

  const STRIP = 88;
  const strip = (index, line1, line2, colour2) => {
    const y = H - STRIP;
    ctx.fillStyle = 'rgba(12,11,10,0.95)';
    ctx.fillRect(0, y, W, STRIP);
    rule(0, y, W, LINE2);
    if (index) {
      ctx.fillStyle = 'rgba(255,180,84,0.14)';
      ctx.fillRect(48, y + 22, 38, 38);
      text(index, 67, y + 48, mono(17, 'bold'), ACCENT, 'center');
    }
    text(line1, index ? 104 : 48, y + 38, mono(20), INK);
    if (line2) text(line2, index ? 104 : 48, y + 66, mono(15), colour2 ?? DIM);
    text(`${run.machine}  ·  ${SW}×${SH}`, W - 48, y + 38, mono(13), FAINT, 'right');
    text('env.render()', W - 48, y + 66, mono(13), FAINT, 'right');
  };

  /** A hairline round something the machine really painted -- an outline, never a fill,
   * and never anything that is not already on screen under it. */
  const box = (sx, sy, sw, sh, top, stroke, alpha = 1) => {
    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.strokeStyle = stroke; ctx.lineWidth = 2;
    ctx.strokeRect(Math.round(toFrameX(sx)) + 0.5, Math.round(toFrameY(sy, top)) + 0.5,
      Math.round(sw * SCALE), Math.round(sh * SCALE));
    ctx.restore();
  };

  /** Where the click landed, at the coordinate the counterexample names. */
  const ping = (sx, sy, top, t) => {
    if (t <= 0 || t >= 1) return;
    const x = toFrameX(sx), y = toFrameY(sy, top);
    ctx.save();
    ctx.strokeStyle = ACCENT;
    ctx.globalAlpha = (1 - t) * 0.95;
    ctx.lineWidth = 2.5;
    ctx.beginPath(); ctx.arc(x, y, 10 + t * 42, 0, Math.PI * 2); ctx.stroke();
    ctx.globalAlpha = Math.min(1, (1 - t) * 1.6);
    ctx.fillStyle = ACCENT;
    ctx.beginPath(); ctx.arc(x, y, 4.5, 0, Math.PI * 2); ctx.fill();
    ctx.restore();
  };

  // --- the verdict ---------------------------------------------------------
  const verdict = alpha => {
    ctx.save();
    ctx.globalAlpha = alpha;
    const x = 150;
    text(`VIOLATION — ${run.policy.id}`, x, 296, mono(42, 'bold'), BREAKS);
    let y = 352;
    for (const line of wrap(run.policy.statement, sans(21), 980)) { text(line, x, y, sans(21), INK); y += 30; }
    rule(x, y + 6, x + 980, LINE2);
    y += 52;
    for (const action of run.policy.counterexample.actions) {
      ctx.fillStyle = 'rgba(212,112,90,0.16)';
      ctx.fillRect(x, y - 21, 28, 28);
      text(String(action.step), x + 14, y, mono(15, 'bold'), BREAKS, 'center');
      text(action.description, x + 44, y, mono(20), INK);
      y += 40;
    }
    y += 22;
    text(`counterexample replayed from a fresh world and reproduced  ·  state hash ${run.policy.counterexample.replay_state_hash.slice(0, 16)}…`,
      x, y, mono(14), '#736d64');
    ctx.restore();
  };

  // --- one frame -----------------------------------------------------------
  const lastLine = run.series[run.series.length - 1];
  const at = t => {
    // interpolate the checker's own progress lines, endpoints exact
    const u = clamp((t - T.searchFrom) / (T.searchTo - T.searchFrom), 0, 1);
    const target = 1 + u * (lastLine.checked - 1);
    let i = 0;
    while (i < run.series.length - 1 && run.series[i].checked < target) i++;
    const hi = run.series[i], lo = i === 0 ? {checked: 1, distinct: 1, frontier: 1, dup: 0, oos: 0, violated: 0, policies: hi.policies, seconds: 0} : run.series[i - 1];
    const span = hi.checked - lo.checked || 1;
    const f = clamp((target - lo.checked) / span, 0, 1);
    return {
      checked: Math.round(target),
      distinct: Math.round(lerp(lo.distinct, hi.distinct, f)),
      frontier: Math.round(lerp(lo.frontier, hi.frontier, f)),
      dup: Math.round(lerp(lo.dup, hi.dup, f)),
      oos: Math.round(lerp(lo.oos, hi.oos, f)),
      violated: f > 0.999 ? hi.violated : lo.violated,
      policies: hi.policies,
      seconds: lerp(lo.seconds, hi.seconds, f),
      index: Math.round(u * dots.length),
    };
  };

  // Three positions of one camera on the machine's 2000-row display. `page` holds the
  // standing request for changes and the merge button in the same frame, so the second
  // click needs no camera move at all; `badge` rises afterwards to bring in the header
  // the merge rewrote.
  const PAN_TOP = {list: 0, page: 420, badge: 300};

  const draw = t => {
    ctx.fillStyle = BG;
    ctx.fillRect(0, 0, W, H);

    if (t < T.replayIn) {
      const now = at(t);
      paintTo(now.index);
      ctx.drawImage(fan, 0, 0);
      // the wedge the violation came to light in, marked once it has been drawn
      if (brokeSlice && now.index > dots.indexOf(brokeSlice)) {
        const found = ramp(t, T.searchFrom + (T.searchTo - T.searchFrom) * (dots.indexOf(brokeSlice) / dots.length), T.searchTo);
        ctx.save();
        ctx.globalAlpha = 0.45 + 0.55 * found;
        ctx.strokeStyle = BREAKS; ctx.lineWidth = 4;
        const slice = dots.filter(d => d.ring === 2 && d.wedge === brokeWedge);
        ctx.beginPath();
        ctx.arc(CX, CY, R2 + 15, slice[0].a, slice[slice.length - 1].a);
        ctx.stroke();
        const mid = (slice[0].a + slice[slice.length - 1].a) / 2;
        text(run.policy.id, CX + Math.cos(mid) * (R2 + 42), CY + Math.sin(mid) * (R2 + 42) + 5,
          mono(15, 'bold'), BREAKS, 'center');
        ctx.restore();
      }
      readout(now, 1);
      legend(0.9);
      header(now.violated ? `${now.violated} of ${now.policies} policies broken` : 'searching',
        now.violated ? BREAKS : ACCENT);

      if (t >= T.verdictIn) {
        const a = ramp(t, T.verdictIn, T.verdictIn + 0.35);
        ctx.fillStyle = `rgba(12,11,10,${0.99 * a})`;
        ctx.fillRect(0, 57, W, H - 57);
        verdict(a);
      }
      return;
    }

    // --- the replay ---
    let top, shot, index, l1, l2, c2, ringAt = null, ringT = -1, hilite = 0;
    const standing = `${run.standing.join(', ')} · request_changes · still standing`;
    if (t < T.page1) {
      top = PAN_TOP.list; shot = shots[0]; index = '1';
      l1 = run.policy.counterexample.actions[0].description;
      l2 = 'the open pull requests of northstar/atlas';
      ringAt = [run.primitives[0].payload.x, run.primitives[0].payload.y];
      ringT = ramp(t, T.click1, T.click1 + 0.26);
    } else if (t < T.page2) {
      top = lerp(PAN_TOP.list, PAN_TOP.page, ease(ramp(t, T.pan1From, T.pan1To)));
      shot = shots[1]; index = '2';
      l1 = run.policy.counterexample.actions[1].description;
      l2 = standing; c2 = BREAKS;
      hilite = ramp(t, T.pan1To - 0.15, T.pan1To + 0.3);
      ringAt = [run.primitives[1].payload.x, run.primitives[1].payload.y];
      ringT = ramp(t, T.click2, T.click2 + 0.26);
    } else {
      top = lerp(PAN_TOP.page, PAN_TOP.badge, ease(ramp(t, T.pan2From, T.pan2To)));
      shot = shots[2]; index = null;
      l1 = `pull request #15 merged by ${run.actor}`;
      l2 = `${run.policy.id} violated — ${run.standing.join(', ')}'s request for changes still stands`;
      c2 = BREAKS; hilite = 1;
    }
    desktop(shot, top);
    if (hilite > 0) {
      // bmartinez's standing request for changes, on the timeline and in the sidebar
      box(run.marks.review.x, run.marks.review.y, run.marks.review.w, run.marks.review.h, top, BREAKS, hilite * 0.95);
      box(run.marks.reviewer.x, run.marks.reviewer.y, run.marks.reviewer.w, run.marks.reviewer.h, top, BREAKS, hilite * 0.95);
    }
    if (ringAt) ping(ringAt[0], ringAt[1], top, ringT);
    strip(index, l1, l2, c2);

    // the loop: settle back onto the frame the video opens on
    const out = ramp(t, T.outFrom, T.outTo);
    if (out > 0) {
      ctx.fillStyle = `rgba(12,11,10,${Math.min(1, out / 0.55)})`;
      ctx.fillRect(0, 0, W, H);
      const back = ramp(out, 0.55, 1);
      if (back > 0) {
        ctx.save();
        ctx.globalAlpha = back;
        header('searching', ACCENT);
        ctx.restore();
        readout(at(0), back);
        legend(back * 0.9);
      }
    }
  };

  // --- roll ----------------------------------------------------------------
  // With an encoder waiting outside, every frame is drawn at an exact timestamp and
  // handed over losslessly: the picture no longer depends on this tab keeping up.
  if (o.frames) {
    const count = Math.round(END * o.fps);
    for (let i = 0; i < count; i++) {
      draw(i / o.fps);
      await window.__frame(i, canvas.toDataURL('image/png').split(',')[1]);
    }
    draw(o.posterAt);
    return {
      video: null, poster: canvas.toDataURL('image/jpeg', o.quality).split(',')[1], mime: 'image/png',
      stats: {frames: count, wall: END, fps: o.fps, slowest: 0, bytes: 0, dots: dots.length},
      facts: {before, after},
    };
  }

  const mimes = ['video/mp4;codecs=avc1.42E01E', 'video/mp4;codecs=avc1', 'video/mp4'];
  const mime = mimes.find(m => MediaRecorder.isTypeSupported(m));
  if (!mime) throw new Error('this Chrome cannot record H.264 in MP4');

  draw(0);
  const stream = canvas.captureStream(o.fps);
  const recorder = new MediaRecorder(stream, {mimeType: mime, videoBitsPerSecond: o.bitrate});
  const chunks = [];
  recorder.ondataavailable = e => e.data.size && chunks.push(e.data);
  const done = new Promise(resolve => { recorder.onstop = resolve; });
  recorder.start(200);

  // Driven by the wall clock, not by a frame counter: if a frame takes too long the
  // picture gets choppier, but it never gets slower than the seconds it claims.
  const started = performance.now();
  let frames = 0, slowest = 0;
  await new Promise(resolve => {
    const tick = () => {
      const t = (performance.now() - started) / 1000;
      if (t >= END) { resolve(); return; }
      const a = performance.now();
      draw(t);
      slowest = Math.max(slowest, performance.now() - a);
      frames++;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  const wall = (performance.now() - started) / 1000;
  await new Promise(r => setTimeout(r, 160));
  recorder.stop();
  await done;
  const blob = new Blob(chunks, {type: mime});
  const bytes = new Uint8Array(await blob.arrayBuffer());

  // the poster: the same storyboard, one frame of it
  draw(o.posterAt);
  const poster = canvas.toDataURL('image/jpeg', o.quality).split(',')[1];

  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return {
    video: btoa(binary), poster, mime,
    stats: {frames, wall, fps: frames / wall, slowest, bytes: bytes.length, dots: dots.length},
    facts: {before, after},
  };
}

// ---------------------------------------------------------------------------
// fragmented -> progressive
// ---------------------------------------------------------------------------
/** Flatten a fragmented MP4 (what MediaRecorder writes: a moov with no samples in it and
 * a moof/mdat pair per fragment) into an ordinary progressive one: a single moov whose
 * sample tables describe the whole track, followed by a single mdat. */
function progressive(input) {
  const view = new DataView(input.buffer, input.byteOffset, input.byteLength);
  const u32 = o => view.getUint32(o);
  const u64 = o => Number(view.getBigUint64(o));

  const boxes = (start, end) => {
    const out = [];
    for (let at = start; at + 8 <= end;) {
      let size = u32(at);
      const type = String.fromCharCode(input[at + 4], input[at + 5], input[at + 6], input[at + 7]);
      let head = 8;
      if (size === 1) { size = u64(at + 8); head = 16; }
      else if (size === 0) size = end - at;
      if (size < head) break;
      out.push({type, start: at, end: at + size, body: at + head});
      at += size;
    }
    return out;
  };
  const find = (list, type) => list.find(b => b.type === type);
  const child = (box, ...path) => {
    let here = box;
    for (const type of path) {
      const next = find(boxes(here.body, here.end), type);
      if (!next) return null;
      here = next;
    }
    return here;
  };

  const top = boxes(0, input.length);
  const ftyp = find(top, 'ftyp');
  const moov = find(top, 'moov');
  if (!ftyp || !moov) throw new Error('not an MP4');
  const trak = child(moov, 'trak');
  const mdhd = child(trak, 'mdia', 'mdhd');
  const stbl = child(trak, 'mdia', 'minf', 'stbl');
  const stsd = find(boxes(stbl.body, stbl.end), 'stsd');
  if (!trak || !mdhd || !stsd) throw new Error('no video track to flatten');
  const mdhdVersion = input[mdhd.body];
  const mediaTimescale = u32(mdhd.body + (mdhdVersion === 1 ? 20 : 12));
  const mvhd = child(moov, 'mvhd');
  const movieTimescale = u32(mvhd.body + (input[mvhd.body] === 1 ? 20 : 12));

  // defaults a fragment may lean on, out of mvex/trex
  const trex = child(moov, 'mvex', 'trex');
  const defaults = trex
    ? {duration: u32(trex.body + 12), size: u32(trex.body + 16), flags: u32(trex.body + 20)}
    : {duration: 0, size: 0, flags: 0};

  const samples = [];
  for (const moof of top.filter(b => b.type === 'moof')) {
    for (const traf of boxes(moof.body, moof.end).filter(b => b.type === 'traf')) {
      const kids = boxes(traf.body, traf.end);
      const tfhd = find(kids, 'tfhd');
      const tf = u32(tfhd.body) & 0xffffff;
      let at = tfhd.body + 8;                       // past version/flags and track_ID
      let base = moof.start;
      if (tf & 0x01) { base = u64(at); at += 8; }
      if (tf & 0x02) at += 4;
      const perTrack = {...defaults};
      if (tf & 0x08) { perTrack.duration = u32(at); at += 4; }
      if (tf & 0x10) { perTrack.size = u32(at); at += 4; }
      if (tf & 0x20) { perTrack.flags = u32(at); at += 4; }
      for (const trun of kids.filter(b => b.type === 'trun')) {
        const flags = u32(trun.body) & 0xffffff;
        const count = u32(trun.body + 4);
        let p = trun.body + 8;
        let offset = base;
        if (flags & 0x001) { offset = base + view.getInt32(p); p += 4; }
        let firstFlags = null;
        if (flags & 0x004) { firstFlags = u32(p); p += 4; }
        for (let i = 0; i < count; i++) {
          let duration = perTrack.duration, size = perTrack.size, flag = perTrack.flags, cto = 0;
          if (flags & 0x100) { duration = u32(p); p += 4; }
          if (flags & 0x200) { size = u32(p); p += 4; }
          if (flags & 0x400) { flag = u32(p); p += 4; }
          if (flags & 0x800) { cto = view.getInt32(p); p += 4; }
          if (i === 0 && firstFlags !== null) flag = firstFlags;
          samples.push({offset, size, duration, cto, sync: !(flag & 0x10000)});
          offset += size;
        }
      }
    }
  }
  if (!samples.length) return input;                 // already progressive; leave it alone

  const mediaDuration = samples.reduce((a, s) => a + s.duration, 0);
  const movieDuration = Math.round(mediaDuration * movieTimescale / mediaTimescale);

  // --- writers ---
  const box = (type, ...parts) => {
    const body = parts.flat();
    const length = body.reduce((a, b) => a + b.length, 0);
    const head = new Uint8Array(8);
    new DataView(head.buffer).setUint32(0, length + 8);
    for (let i = 0; i < 4; i++) head[4 + i] = type.charCodeAt(i);
    return concat([head, ...body]);
  };
  const concat = list => {
    const out = new Uint8Array(list.reduce((a, b) => a + b.length, 0));
    let at = 0;
    for (const part of list) { out.set(part, at); at += part.length; }
    return out;
  };
  const be32 = (...values) => {
    const out = new Uint8Array(values.length * 4);
    const dv = new DataView(out.buffer);
    values.forEach((v, i) => dv.setUint32(i * 4, v >>> 0));
    return out;
  };
  const full = (version, flags) => be32(((version & 0xff) << 24) | (flags & 0xffffff));
  const raw = b => input.subarray(b.start, b.end);

  // stts, as runs of equal durations
  const runs = [];
  for (const s of samples) {
    const last = runs[runs.length - 1];
    if (last && last[1] === s.duration) last[0]++;
    else runs.push([1, s.duration]);
  }
  const stts = box('stts', full(0, 0), be32(runs.length), be32(...runs.flat()));
  const sizes = samples.map(s => s.size);
  const uniform = sizes.every(s => s === sizes[0]);
  const stsz = box('stsz', full(0, 0), be32(uniform ? sizes[0] : 0, samples.length),
    uniform ? new Uint8Array(0) : be32(...sizes));
  const stsc = box('stsc', full(0, 0), be32(1), be32(1, samples.length, 1));
  const syncs = samples.map((s, i) => s.sync ? i + 1 : 0).filter(Boolean);
  const stss = syncs.length && syncs.length !== samples.length
    ? box('stss', full(0, 0), be32(syncs.length), be32(...syncs)) : new Uint8Array(0);
  let ctts = new Uint8Array(0);
  if (samples.some(s => s.cto !== 0)) {
    const entries = [];
    for (const s of samples) {
      const last = entries[entries.length - 1];
      if (last && last[1] === s.cto) last[0]++;
      else entries.push([1, s.cto]);
    }
    ctts = box('ctts', full(1, 0), be32(entries.length), be32(...entries.flat()));
  }

  // The chunk offset is only known once the header's own length is: the table is built
  // twice, the second time with the offset the first pass settled on.
  const assemble = chunkOffset => {
    const newStbl = box('stbl', raw(stsd), stts, ctts, stss, stsc, stsz,
      box('stco', full(0, 0), be32(1), be32(chunkOffset)));
    const minf = child(trak, 'mdia', 'minf');
    const minfKids = boxes(minf.body, minf.end).map(b => b.type === 'stbl' ? newStbl : raw(b));
    const mdia = child(trak, 'mdia');
    const mdiaKids = boxes(mdia.body, mdia.end).map(b => {
      if (b.type === 'minf') return box('minf', minfKids);
      if (b.type !== 'mdhd') return raw(b);
      const bytes = raw(b).slice();
      const dv = new DataView(bytes.buffer);
      // mdhd: after version/flags, v1 carries 8+8+4 before duration, v0 4+4+4
      if (input[b.body] === 1) dv.setBigUint64(8 + 4 + 20, BigInt(mediaDuration));
      else dv.setUint32(8 + 4 + 12, mediaDuration);
      return bytes;
    });
    const trakKids = boxes(trak.body, trak.end).map(b => {
      if (b.type === 'mdia') return box('mdia', mdiaKids);
      if (b.type !== 'tkhd') return raw(b);
      const bytes = raw(b).slice();
      const dv = new DataView(bytes.buffer);
      // tkhd: v1 carries 8+8+4+4 before duration, v0 4+4+4+4
      if (input[b.body] === 1) dv.setBigUint64(8 + 4 + 24, BigInt(movieDuration));
      else dv.setUint32(8 + 4 + 16, movieDuration);
      return bytes;
    });
    const moovKids = boxes(moov.body, moov.end).flatMap(b => {
      if (b.type === 'mvex') return [];               // no fragments left to describe
      if (b.type === 'trak') return [box('trak', trakKids)];
      if (b.type !== 'mvhd') return [raw(b)];
      const bytes = raw(b).slice();
      const dv = new DataView(bytes.buffer);
      if (input[b.body] === 1) dv.setBigUint64(8 + 4 + 16 + 4, BigInt(movieDuration));
      else dv.setUint32(8 + 4 + 8 + 4, movieDuration);
      return [bytes];
    });
    return concat([raw(ftyp), box('moov', moovKids)]);
  };
  let header = assemble(0);
  header = assemble(header.length + 8);

  const payload = new Uint8Array(sizes.reduce((a, b) => a + b, 0));
  let at = 0;
  for (const s of samples) { payload.set(input.subarray(s.offset, s.offset + s.size), at); at += s.size; }
  return concat([header, box('mdat', payload)]);
}

const browser = await chromium.launch({executablePath: process.env.CHROME_BIN, headless: !options.headful});
const page = await browser.newPage({viewport: {width: 1320, height: 860}});
await page.route('**/check-world.json', route => route.fulfill({contentType: 'application/json', body: worldJson}));
await page.route('**/record-check.html', route =>
  route.fulfill({contentType: 'text/html; charset=utf-8', body: '<!doctype html><meta charset="utf-8"><title>record</title>'}));
page.on('console', m => m.type() !== 'log' && console.warn(`  ${m.text()}`));
page.on('pageerror', e => console.warn(`  ${e.message.split('\n')[0]}`));

const frameDir = `${keep}/frames`;
if (ffmpeg) {
  await rm(frameDir, {recursive: true, force: true});
  await mkdir(frameDir, {recursive: true});
  await page.exposeFunction('__frame', (index, base64) =>
    writeFile(`${frameDir}/f-${String(index).padStart(5, '0')}.png`, Buffer.from(base64, 'base64')));
}

let result;
try {
  await page.goto(`http://127.0.0.1:${port}/record-check.html`);
  result = await page.evaluate(record, {
    worldUrl: '/check-world.json',
    options: {fps: options.fps, bitrate: options.bitrate, quality: options.quality,
      posterAt: options.posterAt, frames: !!ffmpeg},
    run: {
      depth: report.depth, seed: report.seed, engine: report.engine_version, world: report.world,
      actor: report.actor, machine: report.machine, viewport: report.viewport, scope: report.scope,
      start: 'Alice’s Mac, on /northstar/atlas/pulls',
      series, brokeAt, policy, primitives: policy.counterexample.primitives,
      standing: ['bmartinez'],
      // Where the standing request for changes is painted, in the machine's own
      // coordinates: the timeline row, and the reviewer in the sidebar.
      marks: {review: {x: 118, y: 630, w: 400, h: 34}, reviewer: {x: 1028, y: 494, w: 300, h: 30}},
    },
  });
} finally {
  await browser.close();
  server.kill();
}

await mkdir(dirname(options.video), {recursive: true});
await mkdir(dirname(options.poster), {recursive: true});

/** Run ffmpeg over the frames, and say how many bytes came out. */
async function encode(args) {
  const run = spawnSync(ffmpeg, [
    '-y', '-v', 'error', '-framerate', String(options.fps), '-i', `${frameDir}/f-%05d.png`,
    '-an', '-c:v', 'libx264', '-preset', 'veryslow', '-profile:v', 'high', '-level', '4.0',
    '-pix_fmt', 'yuv420p', '-g', String(options.fps * 2), '-movflags', '+faststart',
    ...args, options.video,
  ], {encoding: 'utf8'});
  if (run.status !== 0) { process.stderr.write(run.stderr || 'ffmpeg failed\n'); process.exit(1); }
  return (await stat(options.video)).size;
}

let video;
if (ffmpeg) {
  // Constant quality first, because it spends bits where the picture needs them. If that
  // overshoots the budget, the budget wins: a second, two-pass encode at exactly the
  // bitrate that fits.
  let bytes = await encode(['-crf', String(options.crf)]);
  console.log(`x264 crf ${options.crf}: ${(bytes / 1024).toFixed(0)} KB`);
  if (bytes > options.maxBytes) {
    const seconds = result.stats.frames / options.fps;
    const rate = Math.floor((options.maxBytes * 0.94 * 8) / seconds);
    console.log(`  over ${(options.maxBytes / 1024).toFixed(0)} KB — re-encoding two-pass at ${Math.round(rate / 1000)} kb/s`);
    const passlog = `${keep}/x264`;
    spawnSync(ffmpeg, ['-y', '-v', 'error', '-framerate', String(options.fps), '-i', `${frameDir}/f-%05d.png`,
      '-an', '-c:v', 'libx264', '-preset', 'veryslow', '-profile:v', 'high', '-level', '4.0',
      '-pix_fmt', 'yuv420p', '-g', String(options.fps * 2),
      '-b:v', String(rate), '-pass', '1', '-passlogfile', passlog, '-f', 'mp4', '/dev/null'], {encoding: 'utf8'});
    bytes = await encode(['-b:v', String(rate), '-pass', '2', '-passlogfile', passlog]);
    console.log(`x264 two-pass: ${(bytes / 1024).toFixed(0)} KB`);
  }
  video = await readFile(options.video);
  await rm(frameDir, {recursive: true, force: true});
} else {
  video = Buffer.from(progressive(new Uint8Array(Buffer.from(result.video, 'base64'))));
}
const poster = Buffer.from(result.poster, 'base64');
if (!ffmpeg) await writeFile(options.video, video);
await writeFile(options.poster, poster);

const {stats, facts} = result;
const kb = n => `${(n / 1024).toFixed(0)} KB`;
console.log(`replay: pull request 15  ${facts.before.state} → ${facts.after.state}, merged by ${facts.after.merged_by}, ` +
  `${facts.after.standing.join(', ')} still requesting changes, refs/heads/main moved`);
console.log(ffmpeg
  ? `drew ${stats.frames} frames at exact timestamps, ${stats.wall.toFixed(2)}s at ${options.fps} fps, ${stats.dots} dots`
  : `painted ${stats.frames} frames in ${stats.wall.toFixed(2)}s (${stats.fps.toFixed(1)} fps, slowest frame ${stats.slowest.toFixed(0)} ms), ${stats.dots} dots`);
console.log(`  ${options.video.replace(root, '')}   1280x800  ${ffmpeg ? 'h264 (libx264)' : result.mime}  ${kb(video.length)}`);
console.log(`  ${options.poster.replace(root, '')}   ${kb(poster.length)}`);
if (video.length > options.maxBytes) console.warn(`  the video is over the byte budget`);
if (!ffmpeg && stats.fps < options.fps * 0.8) console.warn(`  the page could not keep ${options.fps} fps: the picture will be choppy`);
