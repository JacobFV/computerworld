// No framework: a slideshow of live machines, a snippet to copy, and one star count.
import { cast } from './cast.js';

const stage = document.getElementById('stage');
const track = document.getElementById('track');
const ticks = document.getElementById('ticks');

// One slide per scene, one tile per machine in it, each holding the still its machine
// replaces once it is running.
const slides = cast.map(scene => {
  const slide = document.createElement('article');
  slide.className = scene.machines.length > 1 ? 'slide duo' : 'slide';
  slide.id = `scene-${scene.id}`;   // not the bare id: the browser would scroll the strip to it
  slide.setAttribute('role', 'tabpanel');
  slide.setAttribute('aria-label', scene.title);
  for (const machine of scene.machines) {
    const [width, height] = machine.size;
    const tile = document.createElement('div');
    tile.className = height > width ? 'tile phone' : 'tile';
    tile.style.aspectRatio = `${width} / ${height}`;
    tile.dataset.machine = machine.id;
    tile.dataset.label = machine.label ?? scene.title;
    tile.inert = true;
    tile.innerHTML = `<img src="./media/scenes/${machine.id}.jpg" width="${width}" height="${height}" alt="" loading="lazy">` +
      '<p class="booting" data-boot-note></p>';
    tile.firstChild.alt = machine.label ?? scene.title;
    slide.append(tile);
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
  const count = slides.length;
  const gap = parseFloat(getComputedStyle(track).columnGap) || 0;
  const width = slides.map(slide => slide.offsetWidth);
  const at = index => (index % count + count) % count;
  // What one place out is worth in pixels here, so the same arc comes out of a phone's
  // narrow strip as out of a wide window. Machines differ in width, so take the middle
  // one and the average of the two beside it.
  const step = (width[current] + (width[at(current + 1)] + width[at(current - 1)]) / 2) / 2 + gap;
  slides.forEach((slide, index) => {
    let places = at(index - current);
    if (places > count / 2) places -= count;
    const side = Math.sign(places);
    let x = 0;
    for (let k = 0; k !== places; k += side) x += side * (width[at(current + k)] / 2 + gap + width[at(current + k + side)] / 2);
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

// A link to `#slack` opens on that machine.
const named = cast.findIndex(scene => `#${scene.id}` === location.hash);
track.classList.add('still');
go(Math.max(named, 0), { quiet: true });
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
