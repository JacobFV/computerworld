// No framework: a slideshow of live machines, a snippet to copy, and one star count.
import { cast, opening } from './cast.js';

const stage = document.getElementById('stage');
const track = document.getElementById('track');
const captionTitle = document.getElementById('caption-title');
const captionSummary = document.getElementById('caption-summary');
const captionWhere = document.getElementById('caption-where');

// EQUAL SHARE. No machine on a slide leads and none is anybody's thumbnail, and the
// picture they make together has no holes in it. A slide is cut in two, each half is cut
// in two, and so on down to the machines — beside each other or one above the other, never
// reordered — so every cut divides a rectangle into two rectangles that fill it exactly.
// There is no dead space anywhere in the composition by construction: a phone never gets a
// row of its own with margin either side, because a "row" is only ever the whole of the
// box it was given.
//
// Which of those cuts to take is the one judgement here, and it is made on the machines
// that come off worst. Two machines of the same shape always come out the same size — every
// desktop on a slide is one tile and every phone is another — and among the cuts that hold
// to that, the one that MAKES THE SMALLEST SCREEN ON THE SLIDE AS LARGE AS IT CAN BE wins.
// Raising the floor is what an equal share means when the shapes are mixed and no cut can
// make a phone and a desktop the same size without leaving a hole; where they are not
// mixed it is equal area exactly, because then every tile is the floor. Four desktops and
// a phone come out as a 2 x 2 block with the phone standing beside it at the block's full
// height; three desktops and four phones as a row of each, both rows the same width; a Mac
// and an iPhone side by side, both floor to ceiling; three desktops as three equal tiles.
//
// The shapes are enumerated once, at load, because they are all aspect ratio. A node's
// shape `a` is its width over its height: `a = aA + aB` for two side by side (they share a
// height) and `1/a = 1/aA + 1/aB` for two stacked (they share a width). `u` and `v` carry
// the smallest and the largest tile as a fraction of the node's own height squared, which
// is enough to tell two arrangements apart; the sizes that decide between them are
// measured properly, with the gutters, since those come out of the machines. A node's
// width is `m·H + c`, linear in the height it is given, and `c` is where the gutters live.
// `cast.js` documents all of this for whoever writes the next scene.

// The two things a tile carries besides its screen. `boot` is what covers the still while
// the simulator is on its way down: the picture darkened, a spinner, and the download's
// own percentage, and not a word more. `grow` is the control that gives one machine the
// whole screen; it is marked `data-keep` so live.js keeps it when it swaps the still for
// a canvas, and style.css shows it only once that machine is live.
const icon = path => `<svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true" fill="none"` +
  ` stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="${path}"/></svg>`;
const ENTER = 'M6.5 2H2v4.5M9.5 2H14v4.5M6.5 14H2V9.5M9.5 14H14V9.5';
const LEAVE = 'M6.5 2v4.5H2M9.5 2v4.5H14M6.5 14V9.5H2M9.5 14V9.5H14';
const ARRIVE = 'M8 2.5v7.5M4.5 6.5 8 10l3.5-3.5M3 13.5h10';

function overlay(label) {
  const boot = document.createElement('div');
  boot.className = 'boot';
  const spinner = document.createElement('span');
  spinner.className = 'spin';
  spinner.setAttribute('aria-hidden', 'true');
  const percent = document.createElement('p');
  percent.className = 'pct';
  percent.dataset.bootNote = '';
  // Save-Data: the same overlay, carrying the one control that starts the download, and
  // no paragraph explaining itself over the picture.
  const get = document.createElement('button');
  get.type = 'button';
  get.className = 'get';
  get.dataset.get = '';
  get.setAttribute('aria-label', `Download the simulator and start ${label}`);
  get.innerHTML = icon(ARRIVE);
  boot.append(spinner, percent, get);
  return boot;
}

function grower(tile, label) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'grow';
  button.dataset.keep = '';
  button.dataset.label = label;
  button.setAttribute('aria-label', `Fullscreen — ${label}`);
  button.innerHTML = icon(ENTER) + icon(LEAVE);
  button.addEventListener('click', event => {
    event.stopPropagation();                 // not a click on the slide behind it
    if (tile.classList.contains('full')) shrink(); else fill(tile);
  });
  return button;
}

const leaf = unit => ({ unit, a: unit.aspect, u: unit.aspect, v: unit.aspect });
/** A beside B, at one height. */
const beside = (A, B) => ({ A, B, row: true, a: A.a + B.a, u: Math.min(A.u, B.u), v: Math.max(A.v, B.v) });
/** A above B, at one width. Each half's height is the shared width over its own shape, so
 * its tiles scale by (a/aA)² against the node's. */
const over = (A, B) => {
  const a = 1 / (1 / A.a + 1 / B.a);
  const k = a * a;
  return {
    A, B, row: false, a,
    u: k * Math.min(A.u / (A.a * A.a), B.u / (B.a * B.a)),
    v: k * Math.max(A.v / (A.a * A.a), B.v / (B.a * B.a)),
  };
};

/** Every way of cutting `units` in two and two again, without reordering them. Two cuts
 * that come out the same shape with the same worst tile are the same arrangement as far
 * as anything downstream is concerned, so only one of them is kept. */
function arrangements(units) {
  const done = new Map();
  const walk = (from, to) => {
    const at = `${from}:${to}`;
    if (done.has(at)) return done.get(at);
    let out;
    if (to - from === 1) out = [leaf(units[from])];
    else {
      const seen = new Map();
      for (let cut = from + 1; cut < to; cut++)
        for (const A of walk(from, cut))
          for (const B of walk(cut, to))
            for (const node of [beside(A, B), over(A, B)]) {
              const key = `${node.a.toFixed(5)}|${node.u.toFixed(7)}|${node.v.toFixed(7)}`;
              if (!seen.has(key)) seen.set(key, node);
            }
      out = [...seen.values()];
    }
    done.set(at, out);
    return out;
  };
  return walk(0, units.length);
}

/** How wide a node comes out at height H: `m·H + c`, gutters and all. */
function measure(node, gut) {
  if (node.gut !== gut) {
    if (node.unit) { node.m = node.a; node.c = 0; }
    else {
      const A = measure(node.A, gut), B = measure(node.B, gut);
      if (node.row) { node.m = A.m + B.m; node.c = A.c + B.c + gut; }
      else {
        const inv = 1 / A.m + 1 / B.m;
        node.m = 1 / inv;
        node.c = (A.c / A.m + B.c / B.m - gut) / inv;
      }
    }
    node.gut = gut;
  }
  return node;
}

/** Give every tile under `node` its height; the widths follow from the aspect ratios. */
function place(node, height, gut, put) {
  if (node.unit) return put(node.unit, height);
  if (node.row) { place(node.A, height, gut, put); place(node.B, height, gut, put); return; }
  const width = measure(node, gut).m * height + measure(node, gut).c;
  for (const half of [node.A, node.B]) {
    const { m, c } = measure(half, gut);
    place(half, (width - c) / m, gut, put);
  }
}


/** The arrangement to draw in the room there is now.
 *
 * Two machines of the same shape are drawn at the same size, full stop: every desktop on
 * a slide is one tile and every phone is another, so nothing on a slide is a thumbnail of
 * the machine beside it. Among the arrangements that hold to that, take the one that makes
 * THE SMALLEST SCREEN ON THE SLIDE AS LARGE AS IT CAN BE. Raising the floor is what an
 * equal share means when the shapes are mixed and no cut can make a phone and a desktop
 * the same size without leaving a hole; where the shapes are not mixed it is equal area
 * exactly, because every tile is then the floor. The sizes are measured rather than
 * estimated from the aspect ratios alone: the gutters come out of the machines, and a cut
 * six ways loses more of them than one cut twice. */
function choose(shape, room, cap, gut) {
  const areas = shape.areas;
  let best = null, ragged = null;
  for (const node of shape.tried) {
    const { m, c } = measure(node, gut);
    // Half a pixel back, so a rounded-up tile at the right-hand edge still clears it.
    const height = Math.min(cap, (room - c - 0.5) / m);
    if (!(height > 0)) continue;
    place(node, height, gut, (unit, tall) => { areas[unit.at] = tall * tall * unit.aspect; });
    let least = Infinity;
    for (const area of areas) if (area < least) least = area;
    const even = shape.kinds.every(kind => {
      let low = Infinity, high = 0;
      for (const at of kind) { if (areas[at] < low) low = areas[at]; if (areas[at] > high) high = areas[at]; }
      return high <= low * 1.02;
    });
    if (even) { if (!best || least > best.least) best = { node, height, least }; }
    else if (!ragged || least > ragged.least) ragged = { node, height, least };
  }
  return best ?? ragged;
}

/** The elements for an arrangement, and a name for it so `fit` can tell when it changed. */
const frame = node => {
  if (node.unit) return node.unit.el;
  const box = document.createElement('div');
  box.className = node.row ? 'row' : 'col';
  box.append(frame(node.A), frame(node.B));
  return box;
};
const name = node => node.unit ? '.' : `${node.row ? 'h' : 'v'}(${name(node.A)}${name(node.B)})`;

// One slide per scene, one tile per machine in it, each holding the still its machine
// replaces once it is running. How a slide is cut up depends on how much room there is,
// so that is settled in `fit` rather than here; all that is worked out now is every cut
// it could take.
const shapes = [];
const slides = cast.map((scene, index) => {
  const slide = document.createElement('article');
  slide.className = 'slide';
  slide.id = `scene-${scene.id}`;   // not the bare id: the browser would scroll the strip to it
  slide.setAttribute('role', 'group');
  slide.setAttribute('aria-roledescription', 'slide');
  slide.setAttribute('aria-label', `${index + 1} of ${cast.length}: ${scene.title}`);
  const units = scene.machines.map((machine, at) => {
    const [width, height] = machine.size;
    const tile = document.createElement('div');
    const phone = height > width;
    tile.className = phone ? 'tile phone' : 'tile';
    tile.style.aspectRatio = `${width} / ${height}`;
    // The same shape as a number, for the one place a ratio will not do: a machine filling
    // the screen is as large as the viewport allows AT ITS OWN SHAPE, which is a width and
    // a height worked out from it rather than a box to fit into.
    tile.style.setProperty('--ar', (width / height).toFixed(6));
    tile.dataset.machine = machine.id;
    tile.dataset.label = machine.label ?? scene.title;
    tile.inert = true;
    const still = document.createElement('img');
    still.width = width;
    still.height = height;
    still.decoding = 'async';
    still.alt = machine.label ?? scene.title;
    // A machine whose still has not been rendered yet keeps its shape and shows an empty
    // screen, not a broken image, so a scene can land before its pictures do.
    still.addEventListener('error', () => tile.classList.add('blank'), { once: true });
    // Asked for by `warm`, not here: the ones the visitor can reach with one keypress go
    // first and the rest follow behind them. Never `loading="lazy"` — a still fetched
    // when its slide arrives is a still the visitor watches arrive.
    still.dataset.src = `./media/scenes/${machine.id}.jpg`;
    tile.append(still, overlay(tile.dataset.label), grower(tile, tile.dataset.label));
    return { el: tile, still, phone, at, aspect: width / height };
  });
  // Which machines are the same shape, so `choose` can insist they come out the same size.
  const kinds = new Map();
  units.forEach((unit, at) => {
    const kind = unit.aspect.toFixed(3);
    kinds.set(kind, [...(kinds.get(kind) ?? []), at]);
  });
  shapes.push({
    slide, units, tried: arrangements(units), key: '',
    kinds: [...kinds.values()], areas: new Float64Array(units.length),
  });
  track.append(slide);
  return slide;
});

// The room a slide has to live in stays in style.css, where the media queries and
// embed.css can reach it; these two read it back in pixels. Nothing is drawn in them.
const probe = document.createElement('div');
probe.style.cssText = 'position:absolute;top:0;left:0;visibility:hidden;pointer-events:none;width:var(--w-group);height:var(--h-desk)';
const probePhone = document.createElement('div');
probePhone.style.cssText = 'position:absolute;top:0;left:0;visibility:hidden;pointer-events:none;width:var(--gut);height:var(--h-phone)';
stage.append(probe, probePhone);

let fitted = '';

/** Size every slide's tiles for the room there is now: pick the arrangement, hand the
 * whole slide its height, and let the cuts divide it. A tile's width follows from its own
 * aspect ratio and its corners are rounded in proportion, so a phone shrinks to a phone
 * rather than to a rounded stamp. The frame is rebuilt only when the room has changed
 * which arrangement wins. */
function fit() {
  const room = probe.offsetWidth;
  const gut = probePhone.offsetWidth;
  // How tall a slide may stand. On a narrow window style.css gives a portrait tile more
  // height than a landscape one, and that is the taller of the two: a slide is one
  // picture, so it gets the room the tallest thing on the page is allowed, whether it
  // spends it on a phone or on desktops stacked up the screen.
  const cap = Math.max(probe.offsetHeight, probePhone.offsetHeight);
  // Nothing below depends on anything but these three, so a slideshow being turned does
  // not re-solve forty-four compositions on every keypress.
  const measured = `${room}|${cap}|${gut}`;
  if (measured === fitted) return false;
  fitted = measured;
  for (const shape of shapes) {
    const chosen = choose(shape, room, cap, gut);
    const key = name(chosen.node);
    if (key !== shape.key) {
      shape.key = key;
      shape.slide.replaceChildren(frame(chosen.node));
    }
    place(chosen.node, chosen.height, gut, (unit, height) => {
      unit.el.style.height = `${height.toFixed(1)}px`;
      const radius = unit.phone ? Math.max(4, height * 0.05) : Math.max(3, Math.min(10, height * 0.015));
      unit.el.style.borderRadius = `${radius.toFixed(1)}px`;
      // The shadow is cast by the tile, so it is the tile's size, not the slide's.
      unit.el.style.setProperty('--lift', `${Math.max(9, Math.min(40, height * 0.06)).toFixed(1)}px`);
      // A small screen gets a small note, and one too narrow for a sentence over it gets
      // none: style.css does both, off these two marks.
      unit.el.classList.toggle('small', height < 280);
      unit.el.classList.toggle('tight', height * unit.aspect < 150);
    });
  }
  return true;
}

// The cast is a ring, so every index into it is taken the short way round.
const at = index => (index % cast.length + cast.length) % cast.length;

// NOTHING IS FETCHED WHEN ITS SLIDE ARRIVES. All sixty-eight stills are 3.7 MB together
// and the largest is 104 KB, so the whole cast is asked for within a second or two of the
// page opening: the slide in the middle and the ones a keypress away first, at the head of
// the queue and decoded before they are needed, then the rest of the ring outward from
// there while the browser is idle. Turning the strip then costs a transform and nothing
// else. A browser asking for Save-Data is the exception — it gets the slide it is on and
// its neighbours, and nothing it did not ask for.
const thrifty = !!navigator.connection?.saveData;
const REACH = thrifty ? 1 : 2;

/** Fetch and decode the stills on one slide, at the head of the browser's queue. */
function warm(index) {
  const waiting = [];
  for (const { still } of shapes[index].units) {
    if (!still.src) { still.fetchPriority = 'high'; still.src = still.dataset.src; }
    else if (still.fetchPriority !== 'high') still.fetchPriority = 'high';
    if (!still.ready) waiting.push(still.decode().then(() => { still.ready = true; }, () => {}));
  }
  return waiting.length ? Promise.all(waiting) : Promise.resolve();
}

/** The slide in the middle and the ones either side of it, now. */
const warmNear = index => {
  const near = [];
  for (let step = -REACH; step <= REACH; step++) near.push(warm(at(index + step)));
  return Promise.all(near);
};

/** Ask for the rest of the ring, outward from `from`, a few slides at a time while the
 * browser is idle. A few at a time rather than all at once: a request already issued
 * cannot be moved up the queue, so leaving most of them unissued is what lets `warm` put
 * the slide a visitor has just turned to at the front of it. */
const WAVE = 3;
let arriving = Promise.resolve();
function sweep(from) {
  if (thrifty) return;
  const order = [];
  const seen = new Set();
  for (let step = 0; step <= REACH; step++) { seen.add(at(from + step)); seen.add(at(from - step)); }
  for (let step = REACH + 1; step <= cast.length; step++)
    for (const side of [from + step, from - step]) {
      const index = at(side);
      if (seen.has(index)) continue;
      seen.add(index);
      order.push(index);
    }
  let next = 0;
  arriving = new Promise(allIn => {
    const coming = [];
    const some = () => {
      for (let k = 0; k < WAVE && next < order.length; k++)
        for (const { still } of shapes[order[next++]].units) {
          if (still.src) continue;
          still.fetchPriority = 'low';
          still.src = still.dataset.src;
          coming.push(new Promise(done => {
            still.addEventListener('load', done, { once: true });
            still.addEventListener('error', done, { once: true });
          }));
        }
      if (next < order.length) idle().then(some);
      else Promise.all(coming).then(allIn);
    };
    some();
  });
}

// The pictures get the browser to themselves until they are all in — or five seconds,
// whichever comes first. This is about the line, not the thread: the machines are built
// and drawn in workers now, but the simulator is sixteen megabytes gzipped and a thumbnail
// queued behind it arrives when it arrives. Five seconds is long enough to have the whole
// cast on a decent line and short enough that a visitor who has settled still gets a
// machine while they look.
const pictures = () => Promise.race([arriving, new Promise(done => setTimeout(done, 5000))]);

// What the overlay is doing, said once for every tile at once, because there is one
// download for the whole page: `ask` is a Save-Data browser waiting to be told to go,
// `load` is the download itself, `work` is a world being built out of what came down, and
// no mark at all is no overlay. style.css reads it off the document.
const root = document.documentElement;
const notes = () => [...document.querySelectorAll('[data-boot-note]')];
const phase = (state, text = '') => {
  if (state) root.dataset.boot = state; else delete root.dataset.boot;
  notes().forEach(n => { n.textContent = text; });
};

// NOTHING WAITS ON A MACHINE. Every tile shows its pre-rendered still the moment the page
// parses, and the slideshow turns on the same frame the key is pressed: none of what
// follows is on the path of a click, an arrow or a `#link`. The simulator is sixteen
// megabytes gzipped, so it is not even asked for until the browser is idle, and a scene is
// started only once the strip has stood still for a moment — arrowing through twenty
// slides starts the one they stop on, not twenty worlds. Those worlds are built in workers
// and cost this thread nothing, but they are still twenty worlds.
let simulator = null;
const download = () => (simulator ??= (async () => {
  // A browser asking for Save-Data gets the stills and a way in, not a download it did
  // not ask for. The way in is the overlay's own button. Everyone else gets a machine.
  if (navigator.connection?.saveData) {
    phase('ask');
    await new Promise(resolve =>
      document.querySelectorAll('[data-get]').forEach(button => button.addEventListener('click', resolve, { once: true })));
  }
  phase('load');
  const live = await import('./live.js');
  // A whole percentage while the download can be counted, nothing but the spinner while
  // it cannot, and the spinner alone again once the bytes are in and a world is being
  // built out of them. No number is ever shown that is not the download's own.
  return live.boot(report => {
    if (report === false) phase('work');
    else phase('load', typeof report === 'number' ? `Downloading ${report}%` : '');
  });
})().catch(error => {
  // The stills are real renders of the simulator, so a browser that cannot run it is left
  // looking at them rather than at an apology written over them.
  phase(null);
  console.error(error);
}));

const idle = () => new Promise(resolve =>
  window.requestIdleCallback ? requestIdleCallback(resolve, { timeout: 600 }) : setTimeout(resolve, 200));

// How long the strip has to stand still before the machine under the visitor is worth
// starting. Shorter than a second thought, longer than a key repeat.
const SETTLE = 260;
let armed = 0;
let era = 0;

/** Ask for the scene at `index` to come up, and abandon whatever was coming up for the
 * last one. Returns at once; it is never awaited. */
function wake(index) {
  clearTimeout(armed);
  era++;                                   // every boot chain in flight gives up at its next await
  armed = setTimeout(() => come(index, era), SETTLE);
}

async function come(index, mine) {
  // The pictures come first: they are small, they are what the visitor is looking at, and
  // a machine nobody has asked for yet should not be taking the line from them.
  await warmNear(index);
  await pictures();
  if (era !== mine || current !== index) return;
  const start = await download();
  if (!start || era !== mine || current !== index) return;
  await start(cast[index], slides[index]);
  // Then its neighbours, while nothing else is happening, so the machines at the edges of
  // the strip are already running when they are reached. Only while nothing else is
  // happening, though: a hand still on the arrow key is not nothing.
  for (const near of [index + 1, index - 1]) {
    await idle();
    if (era !== mine || current !== index || performance.now() - moved < 900) return;
    await start(cast[at(near)], slides[at(near)], { spare: true });
  }
}

// When the strip last turned, so that a boot never lands on top of someone still moving.
let moved = 0;

let current = -1;

// The strip lies on the face of a very wide cylinder: how far a slide stands off centre is
// how far round the curve it has gone, so it is both turned away from the viewer and set
// that much further back, and the machine in the middle is square on at the front. `ARC`
// is the turn one place out; past that the angle eases towards a limit instead of piling
// up, which keeps the far edges an arc rather than a wall. style.css does the drawing,
// and drops all of it for a browser without 3D transforms or a reader who asked for less
// motion — the numbers below are still written, and simply go unread.
const ARC = 13 * Math.PI / 180;          // the turn at the neighbouring machine, in radians
const EASE = 1 - Math.exp(-1);           // so that one place out lands exactly on ARC
const turn = places => ARC * Math.sign(places) * (1 - Math.exp(-Math.abs(places))) / EASE;

// What the strip measured the last time it was cut up. A slide's width and height are
// settled by `fit` and by nothing else: the arc is a transform, `far` only hides, and a
// tile is sized off its own aspect ratio rather than off the still inside it. So these are
// read back from the page when the composition changes, and not once per keypress —
// forty-four slides is forty-four forced layouts, taken in the frame a held arrow key
// wanted for turning the strip.
let span = null, tall = null, gap = 0;

/** Lay the strip out around the current machine. It is a ring: each slide sits as many
 * places to the left or right as is shortest, so there is always a neighbour on both sides,
 * and one that changes sides does so out of sight. */
function arrange() {
  const count = slides.length;
  if (fit() || !span) {
    gap = parseFloat(getComputedStyle(track).columnGap) || 0;
    // Slides differ in width now that a slide may hold a team: a seven-machine group is
    // half as wide again as one laptop. Every number below comes off the measured widths,
    // so the gutter between neighbours is the same wherever the strip is standing.
    span = slides.map(slide => slide.offsetWidth);
    tall = slides.map(slide => slide.offsetHeight);
  }
  // What one place out is worth in pixels here, so the same arc comes out of a phone's
  // narrow strip as out of a wide window. Machines differ in width, so take the middle
  // one and the average of the two beside it.
  const step = (span[current] + (span[at(current + 1)] + span[at(current - 1)]) / 2) / 2 + gap;
  slides.forEach((slide, index) => {
    let places = at(index - current);
    if (places > count / 2) places -= count;
    const side = Math.sign(places);
    let x = 0;
    for (let k = 0; k !== places; k += side) x += side * (span[at(current + k)] / 2 + gap + span[at(current + k + side)] / 2);
    const far = Math.abs(places) > 3;
    if (far !== slide.classList.contains('far')) slide.classList.toggle('far', far);
    slide.style.setProperty('--x', `${Math.round(x)}px`);
    // Off the measured position, not the place count: a slide half way to the next one is
    // half way round, and the turn rides the same transition the slide's travel does. The
    // radius is whatever puts the neighbour at ARC; the depth is how far round that curve
    // has carried this slide back, which is what stops the turn shouldering the strip's
    // edges out of the window.
    const angle = step > 0 ? turn(x / step) : 0;
    slide.style.setProperty('--rot', `${(angle * 180 / Math.PI).toFixed(2)}deg`);
    slide.style.setProperty('--z', `${(-(step / ARC) * (1 - Math.cos(angle))).toFixed(1)}px`);
  });
  track.style.height = `${tall[current]}px`;
}
// ONE MACHINE, THE WHOLE SCREEN. Every live tile carries a control in its top right. It
// asks the Fullscreen API for that tile; where the API is missing or says no — a sandboxed
// frame, mostly — the tile is lifted into a fixed box of its own instead, which looks the
// same from the outside. Either way the tile keeps its canvas and every listener on it, so
// the machine takes the pointer and the keyboard full screen exactly as it did in the
// strip, and style.css gives the canvas as much of the viewport as its own shape allows.
let big = null;                       // { tile, host } while one machine has the screen

// Nothing is re-measured while one machine has the whole screen: going fullscreen changes
// the viewport, and the tile that went is out of the slide's flow while it is there, so
// anything measured then would come off a composition with a hole in it. `settle` is the
// one pass that puts it all back afterwards.
new ResizeObserver(() => { if (!big && current >= 0) arrange(); }).observe(stage);

function dress(tile, on) {
  tile.classList.toggle('full', on);
  const button = tile.querySelector('.grow');
  if (button) button.setAttribute('aria-label', on ? 'Leave fullscreen' : `Fullscreen — ${button.dataset.label}`);
}

/** Put the strip back exactly as it was. Every number in it — the widths, the arc and the
 * track's height — is measured, and nothing was measured while the screen was taken, so
 * the cache is dropped and one silent pass puts it all back without a slide sliding. */
function settle() {
  fitted = '';
  span = null;
  track.classList.add('still');
  arrange();
  requestAnimationFrame(() => requestAnimationFrame(() => track.classList.remove('still')));
}

/** The way in when the Fullscreen API is not on offer: the tile is moved into a fixed box
 * over the page, and a hidden copy of it holds its place in the slide, so the composition
 * behind is not re-cut and putting the machine back is one swap. A fixed box has to leave
 * the slide to be fixed at all — a transformed ancestor would otherwise be its viewport,
 * and the slides are turned on an arc. */
function lift(tile) {
  const gap = tile.cloneNode(false);
  gap.style.visibility = 'hidden';
  gap.classList.remove('live', 'full');
  const host = document.createElement('div');
  host.className = 'fs-host';
  tile.replaceWith(gap);
  host.append(tile);
  document.body.append(host);
  return { tile, gap, host };
}

function drop() {
  const { tile, gap, host } = big;
  gap.replaceWith(tile);
  host.remove();
  big = null;
  dress(tile, false);
  settle();
  tile.querySelector('.grow')?.focus();
}

async function fill(tile) {
  if (big) return;
  if (document.fullscreenEnabled && tile.requestFullscreen) {
    try {
      await tile.requestFullscreen({ navigationUI: 'hide' });
      return;                                    // `fullscreenchange` takes it from here
    } catch (error) {
      console.warn('fullscreen', error);         // a sandboxed frame, mostly
    }
  }
  big = lift(tile);
  dress(tile, true);
  tile.querySelector('canvas')?.focus();
}

function shrink() {
  if (!big) return;
  if (big.host) drop();
  else document.exitFullscreen?.().catch(() => {});
}

// The browser's own way out — Escape, a gesture, the window losing the screen — comes
// through here, and puts the strip back the same way the control would have.
document.addEventListener('fullscreenchange', () => {
  const taken = document.fullscreenElement;
  if (taken?.classList?.contains('tile')) {
    big = { tile: taken, host: null };
    dress(taken, true);
    taken.querySelector('canvas')?.focus();
  } else if (big && !big.host) {
    const { tile } = big;
    big = null;
    dress(tile, false);
    settle();
    tile.querySelector('.grow')?.focus();
  }
});

// Escape, caught before the machine can be typed at: a visitor pressing it means the
// screen back, not an Escape key sent into the simulation. The browser leaves its own
// fullscreen on Escape too, and asking it a second time is harmless — but asking is what
// makes the key work in an engine that does not, and in the box that is not the API's.
document.addEventListener('keydown', event => {
  if (!big || event.key !== 'Escape') return;
  event.stopPropagation();
  event.preventDefault();
  shrink();
}, true);

// The address bar is written a moment after the strip stops, not on every frame of a held
// arrow key: a browser rate-limits replaceState, and a visitor turning past a scene did
// not mean to link to it.
let writing = 0;
const mark = () => {
  clearTimeout(writing);
  writing = setTimeout(() => history.replaceState(null, '', `#${cast[current].id}`), 250);
};

/** Bring one scene to the middle. Everything here is synchronous: the strip has turned and
 * the caption has changed by the time this returns, whatever the machines are doing. */
function go(index, { quiet = false } = {}) {
  // Not while one machine has the whole screen. The arrow keys belong to that machine
  // then, and turning the strip under it would leave a visitor full screen on a machine
  // that is no longer the one in the middle.
  if (big) return;
  index = at(index);
  if (index === current) return;
  current = index;
  slides.forEach((slide, i) => {
    slide.classList.toggle('active', i === index);
    // Off the units, not off the slide's children: a slide has no children until `fit`
    // has cut it up, and the first `go` happens before that — asking the DOM here left
    // the opening slide's tiles inert, with nothing in them clickable, until the visitor
    // turned away from it and back.
    for (const unit of shapes[i].units) unit.el.inert = i !== index;
  });
  const scene = cast[index];
  captionTitle.textContent = scene.title;
  captionSummary.textContent = scene.summary ?? '';
  captionWhere.textContent = `${index + 1} / ${cast.length}`;
  arrange();
  moved = performance.now();
  // Nothing below is awaited: both hand out work and return.
  warmNear(index);
  if (!quiet) mark();
  wake(index);
}

slides.forEach((slide, index) => slide.addEventListener('click', () => go(index)));
document.getElementById('prev').addEventListener('click', () => go(current - 1));
document.getElementById('next').addEventListener('click', () => go(current + 1));
// The arrow keys turn the slideshow unless a machine has the keyboard.
document.addEventListener('keydown', event => {
  const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
  if (!step || event.target.closest?.('canvas, input, textarea')) return;
  event.preventDefault();
  go(current + step);
});

// A link to `#slack` opens on that machine — including on a page that is already open,
// so a fragment typed into the address bar or followed from elsewhere on the page turns
// the strip instead of doing nothing. `go` writes the fragment back with replaceState,
// which raises no event of its own, so this only ever hears about someone else's change.
window.addEventListener('hashchange', () => {
  const asked = cast.findIndex(scene => `#${scene.id}` === location.hash);
  if (asked >= 0) go(asked, { quiet: true });
});

// With nothing named, the cast's opening scene does, which is in the middle of the strip
// rather than at its end.
const named = cast.findIndex(scene => `#${scene.id}` === location.hash);
const first = Math.max(named >= 0 ? named : cast.findIndex(scene => scene.id === opening), 0);
track.classList.add('still');
go(first, { quiet: true });
requestAnimationFrame(() => requestAnimationFrame(() => track.classList.remove('still')));
// In order: the stills a keypress away, then the rest of the cast in the background, and
// only once those are asked for, the sixteen the machines need. A visitor who turns the
// strip in the meantime is served by `wake`, which waits for the same pictures first.
warmNear(first).then(() => {
  sweep(first);                                  // every still asked for, before the 16 MB
  return pictures();
}).then(() => idle()).then(() => download());

/** Copy some text and say so through `report`, which takes the word and then nothing. */
async function copy(text, report) {
  try {
    await navigator.clipboard.writeText(text);
    report('Copied');
  } catch {
    report('Press ⌘/Ctrl+C');
  }
  setTimeout(() => report(''), 1600);
}

// Everything below the slideshow. embed.html is the slideshow on its own — the same
// stage, the same machines, in an iframe — so none of it is there, and each piece
// asks for its own elements before wiring itself up.

// The install line and the snippet follow the language tabs.
const install = document.querySelector('#install code');
const button = document.getElementById('copy');
if (install && button) {
  const installs = { py: 'pip install computerworld', js: 'npm install computerworld', rs: 'cargo add computerworld --git https://github.com/JacobFV/computerworld --tag v0.2.0' };
  const panes = [...document.querySelectorAll('.codebox pre[data-lang]')];
  const langs = [...document.querySelectorAll('.tabs button')];
  langs.forEach(tab => tab.addEventListener('click', () => {
    const lang = tab.dataset.lang;
    langs.forEach(b => b.setAttribute('aria-selected', String(b === tab)));
    install.textContent = installs[lang];
    panes.forEach(p => (p.hidden = p.dataset.lang !== lang));
  }));

  button.addEventListener('click', () =>
    copy(panes.find(p => !p.hidden).innerText, said => (button.textContent = said || 'Copy')));
  // The install line is the thing most people came to copy, so it copies itself.
  install.parentElement.addEventListener('click', () =>
    copy(install.textContent, said => (install.dataset.said = said.toLowerCase())));
}

// The one request this page makes to anything outside itself. It is allowed to fail, and
// a repository with no stars yet says nothing rather than "0".
const badge = document.getElementById('stars');
if (badge) {
  fetch('https://api.github.com/repos/JacobFV/computerworld')
    .then(response => response.ok ? response.json() : Promise.reject(response.status))
    .then(({ stargazers_count: stars }) => {
      if (!stars) return;
      badge.textContent = stars >= 1000 ? `★ ${(stars / 1000).toFixed(1)}k` : `★ ${stars}`;
      badge.hidden = false;
    })
    .catch(() => {});
}
