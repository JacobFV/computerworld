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

/** Lay the strip out around the current machine. It is a ring: each slide sits as many
 * places to the left or right as is shortest, so there is always a neighbour on both sides,
 * and one that changes sides does so out of sight. */
function arrange() {
  const count = slides.length;
  const gap = parseFloat(getComputedStyle(track).columnGap) || 0;
  const width = slides.map(slide => slide.offsetWidth);
  const at = index => (index % count + count) % count;
  slides.forEach((slide, index) => {
    let places = at(index - current);
    if (places > count / 2) places -= count;
    const side = Math.sign(places);
    let x = 0;
    for (let k = 0; k !== places; k += side) x += side * (width[at(current + k)] / 2 + gap + width[at(current + k + side)] / 2);
    const far = Math.abs(places) > 3;
    if (far !== slide.classList.contains('far')) slide.classList.toggle('far', far);
    slide.style.setProperty('--x', `${Math.round(x)}px`);
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

// The install line and the snippet follow the language tabs.
const installs = { py: 'pip install computerworld', js: 'npm install computerworld', rs: 'cargo add computerworld --git https://github.com/JacobFV/computerworld --tag v0.1.0' };
const install = document.querySelector('#install code');
const panes = [...document.querySelectorAll('.codebox pre[data-lang]')];
const langs = [...document.querySelectorAll('.tabs button')];
langs.forEach(tab => tab.addEventListener('click', () => {
  const lang = tab.dataset.lang;
  langs.forEach(b => b.setAttribute('aria-selected', String(b === tab)));
  install.textContent = installs[lang];
  panes.forEach(p => (p.hidden = p.dataset.lang !== lang));
}));

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

const button = document.getElementById('copy');
button.addEventListener('click', () =>
  copy(panes.find(p => !p.hidden).innerText, said => (button.textContent = said || 'Copy')));
// The install line is the thing most people came to copy, so it copies itself.
install.parentElement.addEventListener('click', () =>
  copy(install.textContent, said => (install.dataset.said = said.toLowerCase())));

// The one request this page makes to anything outside itself. It is allowed to fail, and
// a repository with no stars yet says nothing rather than "0".
fetch('https://api.github.com/repos/JacobFV/computerworld')
  .then(response => response.ok ? response.json() : Promise.reject(response.status))
  .then(({ stargazers_count: stars }) => {
    if (!stars) return;
    const badge = document.getElementById('stars');
    badge.textContent = stars >= 1000 ? `★ ${(stars / 1000).toFixed(1)}k` : `★ ${stars}`;
    badge.hidden = false;
  })
  .catch(() => {});
