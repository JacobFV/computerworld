// No framework: a slideshow of live machines, a snippet to copy, and one star count.
import { cast, opening } from './cast.js';

const stage = document.getElementById('stage');
const track = document.getElementById('track');
const ticks = document.getElementById('ticks');

// How a slide holds more than one machine. The scene's lead machine stands at the slide's
// full height; the rest are grouped into rows stacked beside it that come to exactly the
// same height, so however many machines a scene has, its slide is one rectangle the same
// size as everybody else's, and the desktops in it are the big tiles with the phones
// smaller alongside. `BUDGET` is how wide that rectangle may be, measured in its own
// height: a group takes the fewest rows that keep it inside that, so two machines stand
// side by side, three desktops become one and a column of two, and a seven-machine team
// becomes one large screen and three short rows. `cast.js` documents the `layout` hint a
// scene can pass instead. Nothing is stretched: a tile keeps its machine's aspect ratio
// and every row shares one height.
const MAX_ROWS = 3;
const BUDGET = 2.45;

const width = row => row.reduce((total, unit) => total + unit.aspect, 0);

/** Cut `units` into `count` rows, in order, so the widest row is as narrow as it can be.
 * Ties go to the fuller row first, which puts the ragged row at the bottom. */
function split(units, count) {
  if (count === 1) return [units];
  let best = null;
  const consider = rows => {
    const widest = Math.max(...rows.map(width));
    const first = width(rows[0]);
    if (!best || widest < best.widest - 1e-9 || (widest < best.widest + 1e-9 && first > best.first)) {
      best = { rows, widest, first };
    }
  };
  const walk = (from, left, rows) => {
    if (left === 1) return consider([...rows, units.slice(from)]);
    for (let to = from + 1; to <= units.length - left + 1; to++) walk(to, left - 1, [...rows, units.slice(from, to)]);
  };
  walk(0, count, []);
  return best.rows;
}

/** Which machine leads a slide, and how the others are grouped behind it. */
function plan(scene, units) {
  const hint = scene.layout ?? 'auto';
  if (units.length === 1) return { hero: units[0], rows: [] };
  if (hint === 'row') return { hero: units[0], rows: [units.slice(1)] };
  // The lead is the first machine listed, or the first desktop when the slide mixes
  // shapes: a phone listed first should not be the one carrying the composition.
  const mixed = units.some(unit => unit.phone) && units.some(unit => !unit.phone);
  const lead = mixed ? units.findIndex(unit => !unit.phone) : 0;
  const hero = units[lead];
  const rest = units.filter((unit, index) => index !== lead);
  if (hint === 'stack') {
    return { hero, rows: [rest.filter(u => !u.phone), rest.filter(u => u.phone)].filter(row => row.length) };
  }
  if (Array.isArray(hint)) {
    const rows = [];
    let at = 0;
    for (const size of hint) {
      const row = rest.slice(at, at + size);
      at += row.length;
      if (row.length) rows.push(row);
    }
    if (at < rest.length) rows.push(rest.slice(at));
    return { hero, rows };
  }
  let best = null;
  for (let count = 1; count <= Math.min(MAX_ROWS, rest.length); count++) {
    const rows = split(rest, count);
    const spread = hero.aspect + Math.max(...rows.map(width)) / count;
    if (spread <= BUDGET) return { hero, rows };
    if (!best || spread < best.spread) best = { hero, rows, spread };
  }
  return best;
}

// One slide per scene, one tile per machine in it, each holding the still its machine
// replaces once it is running.
const plans = [];
const slides = cast.map(scene => {
  const slide = document.createElement('article');
  slide.className = 'slide';
  slide.id = `scene-${scene.id}`;   // not the bare id: the browser would scroll the strip to it
  slide.setAttribute('role', 'tabpanel');
  slide.setAttribute('aria-label', scene.title);
  const units = scene.machines.map(machine => {
    const [width, height] = machine.size;
    const tile = document.createElement('div');
    const phone = height > width;
    tile.className = phone ? 'tile phone' : 'tile';
    tile.style.aspectRatio = `${width} / ${height}`;
    tile.dataset.machine = machine.id;
    tile.dataset.label = machine.label ?? scene.title;
    tile.inert = true;
    const still = document.createElement('img');
    still.width = width;
    still.height = height;
    still.loading = 'lazy';
    still.alt = machine.label ?? scene.title;
    // A machine whose still has not been rendered yet keeps its shape and shows an empty
    // screen, not a broken image, so a scene can land before its pictures do.
    still.addEventListener('error', () => tile.classList.add('blank'), { once: true });
    still.src = `./media/scenes/${machine.id}.jpg`;
    const note = document.createElement('p');
    note.className = 'booting';
    note.dataset.bootNote = '';
    tile.append(still, note);
    return { el: tile, phone, aspect: width / height };
  });
  const shape = plan(scene, units);
  plans.push(shape);
  slide.append(shape.hero.el);
  if (shape.rows.length) {
    const rail = document.createElement('div');
    rail.className = 'rail';
    for (const row of shape.rows) {
      const line = document.createElement('div');
      line.className = 'row';
      line.append(...row.map(unit => unit.el));
      rail.append(line);
    }
    slide.append(rail);
  }
  track.append(slide);
  return slide;
});
const tabs = cast.map((scene, index) => {
  const tab = document.createElement('button');
  tab.setAttribute('role', 'tab');
  tab.setAttribute('aria-controls', `scene-${scene.id}`);
  tab.setAttribute('aria-label', scene.title);
  tab.title = scene.title;
  tab.addEventListener('click', () => go(index));
  ticks.append(tab);
  return tab;
});

// The room a slide has to live in stays in style.css, where the media queries and
// embed.css can reach it; these two read it back in pixels. Nothing is drawn in them.
const probe = document.createElement('div');
probe.style.cssText = 'position:absolute;top:0;left:0;visibility:hidden;pointer-events:none;width:var(--w-group);height:var(--h-desk)';
const probePhone = document.createElement('div');
probePhone.style.cssText = 'position:absolute;top:0;left:0;visibility:hidden;pointer-events:none;width:var(--gut);height:var(--h-phone)';
stage.append(probe, probePhone);

/** Size every slide's tiles for the room there is now: the lead machine as tall as the
 * slide, the rows beside it sharing that height between them, and the whole group inside
 * `--w-group`. A tile's width follows from its aspect ratio, and its corners are rounded
 * in proportion, so a phone shrinks to a phone rather than to a rounded stamp. */
function fit() {
  const room = probe.offsetWidth;
  const gut = probePhone.offsetWidth;
  const caps = { desk: probe.offsetHeight, phone: probePhone.offsetHeight };
  const spread = (shape, height) => {
    if (!shape.rows.length) return shape.hero.aspect * height;
    const each = (height - (shape.rows.length - 1) * gut) / shape.rows.length;
    const rail = Math.max(...shape.rows.map(row => width(row) * each + (row.length - 1) * gut));
    return shape.hero.aspect * height + gut + rail;
  };
  const put = (unit, height) => {
    unit.el.style.height = `${height.toFixed(1)}px`;
    const radius = unit.phone ? Math.max(4, height * 0.05) : Math.max(3, Math.min(10, height * 0.015));
    unit.el.style.borderRadius = `${radius.toFixed(1)}px`;
    // Too narrow for a sentence over it: style.css takes the boot note away there.
    unit.el.classList.toggle('tight', height * unit.aspect < 150);
  };
  for (const shape of plans) {
    const cap = caps[shape.hero.phone ? 'phone' : 'desk'];
    let height = cap;
    if (spread(shape, cap) > room) {
      // The spread grows with the height, so the tallest that still fits is one search away.
      let low = 0, high = cap;
      for (let step = 0; step < 24; step++) {
        const middle = (low + high) / 2;
        if (spread(shape, middle) > room) high = middle; else low = middle;
      }
      height = low;
    }
    put(shape.hero, height);
    if (!shape.rows.length) continue;
    const each = (height - (shape.rows.length - 1) * gut) / shape.rows.length;
    for (const row of shape.rows) for (const unit of row) put(unit, each);
  }
}

const notes = () => [...document.querySelectorAll('[data-boot-note]')];
const say = text => notes().forEach(n => { n.textContent = text; });

// The simulator downloads as soon as the page loads; machines come up as the slideshow
// reaches them, so the first screen is running before the rest cost anything.
const ready = (async () => {
  // A browser asking for Save-Data gets the stills and a way in, not a 10 MB download
  // it did not ask for. Everyone else gets a running machine.
  if (navigator.connection?.saveData) {
    say('Data saver is on. Tap to download the simulator (about 10 MB) and start the machines.');
    await new Promise(resolve => notes().forEach(n => n.addEventListener('click', resolve, { once: true })));
  }
  const live = await import('./live.js');
  return live.boot((text, machine) => {
    if (!machine) return say(text);
    const note = document.querySelector(`.tile[data-machine="${machine}"] [data-boot-note]`);
    if (note) note.textContent = text;
  });
})().catch(error => {
  say('This browser could not start the simulator. The screens here are real renders of it.');
  console.error(error);
});

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

/** Lay the strip out around the current machine. It is a ring: each slide sits as many
 * places to the left or right as is shortest, so there is always a neighbour on both sides,
 * and one that changes sides does so out of sight. */
function arrange() {
  fit();
  const count = slides.length;
  const gap = parseFloat(getComputedStyle(track).columnGap) || 0;
  // Slides differ in width now that a slide may hold a team: a seven-machine group is
  // half as wide again as one laptop. Every number below comes off the measured widths,
  // so the gutter between neighbours is the same wherever the strip is standing.
  const span = slides.map(slide => slide.offsetWidth);
  const at = index => (index % count + count) % count;
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
  track.style.height = `${slides[current].offsetHeight}px`;
}
new ResizeObserver(() => current >= 0 && arrange()).observe(stage);

/** Bring one scene to the middle and boot it, then its neighbours while nothing else is
 * happening, so the machines at the edges are already running when they are reached. */
async function go(index, { quiet = false } = {}) {
  index = (index + cast.length) % cast.length;
  if (index === current) return;
  current = index;
  slides.forEach((slide, i) => {
    slide.classList.toggle('active', i === index);
    slide.querySelectorAll('.tile').forEach(tile => (tile.inert = i !== index));
  });
  tabs.forEach((tab, i) => {
    tab.setAttribute('aria-selected', String(i === index));
    tab.tabIndex = i === index ? 0 : -1;
  });
  arrange();
  if (!quiet) history.replaceState(null, '', `#${cast[index].id}`);
  const start = await ready;
  if (!start) return;
  await start(cast[index], slides[index]);
  for (const near of [index + 1, index - 1]) {
    await new Promise(resolve => (window.requestIdleCallback ?? setTimeout)(resolve, { timeout: 600 }));
    if (current !== index) return;
    const i = (near + cast.length) % cast.length;
    await start(cast[i], slides[i]);
  }
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
  if (ticks.contains(document.activeElement)) tabs[current].focus();
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
const first = named >= 0 ? named : cast.findIndex(scene => scene.id === opening);
track.classList.add('still');
go(Math.max(first, 0), { quiet: true });
requestAnimationFrame(() => requestAnimationFrame(() => track.classList.remove('still')));

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
  const installs = { py: 'pip install computerworld', js: 'npm install computerworld', rs: 'cargo add computerworld --git https://github.com/JacobFV/computerworld --tag v0.1.2' };
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
