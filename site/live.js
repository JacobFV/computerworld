// Live machines on the page. One Wasm module, one world, one session per machine, and a
// canvas each. Nothing is fetched after boot: every frame is rendered here, in this tab.
import init, { World } from './demo/pkg/web/computerworld.js';
import definition from './demo/examples/browser/world-definition.js';

const SEED = 2026;
const actions = ['terminal.v1', 'browser.v1', 'keyboard.v1', 'pointer.v1', 'application.v1', 'filesystem.v1', 'http.v1'];
const observations = ['terminal.v1', 'semantic.v1', 'browser.v1'];

// What each machine opens with, so a visitor lands on something worth touching rather
// than an empty desktop.
const openings = {
  'carol-ubuntu': env => {
    step(env, 'carol-ubuntu', 'terminal.v1', 'execute', {
      command: "mkdir -p /home/carol/project && printf 'import math\\n\\n" +
        'def primes(n):\\n    sieve = [True] * n\\n    for p in range(2, int(math.sqrt(n)) + 1):\\n' +
        '        if sieve[p]:\\n            for q in range(p * p, n, p):\\n                sieve[q] = False\\n' +
        "    return [i for i in range(2, n) if sieve[i]]\\n\\nprint(primes(60))\\n' > /home/carol/project/primes.py",
    });
    step(env, 'carol-ubuntu', 'application.v1', 'launch', { kind: 'code', argument: '/home/carol/project' });
    click(env, 'carol-ubuntu', 'code:tree:primes.py', [1100, 700]);
    click(env, 'carol-ubuntu', 'code:cmd:python.execInTerminal', [1100, 700]);
  },
  'alice-mac': env => step(env, 'alice-mac', 'application.v1', 'launch', { kind: 'music' }),
  'bob-windows': env => {
    step(env, 'bob-windows', 'terminal.v1', 'execute', {
      command: "printf 'Region,Q1,Q2,Q3\\nNorth,1200,1450,1610\\nSouth,980,1120,1290\\n" +
        "East,1440,1380,1520\\nWest,860,940,1080\\n' > sales.csv",
    });
    step(env, 'bob-windows', 'application.v1', 'launch', { kind: 'spreadsheet', argument: 'C:/Users/bob/sales.csv' });
  },
  'alice-phone': env => step(env, 'alice-phone', 'application.v1', 'launch', { kind: 'music' }),
  'bob-android': env => step(env, 'bob-android', 'application.v1', 'launch', { kind: 'music' }),
};

const step = (env, machine, family, op, payload) => env.step([{ family, op, machine, payload }]);

/** Click the control carrying `target`, wherever the shell painted it. */
function click(env, machine, target, [width, height]) {
  const scene = env.scene(width, height);
  const suffix = `:content:${target}`;
  const hit = scene.nodes
    .filter(n => n.interaction === target || n.interaction?.endsWith(suffix))
    .sort((a, b) => (a.z ?? 0) - (b.z ?? 0))
    .pop();
  if (!hit) return false;
  const t = hit.transform ?? { a: 1024, b: 0, c: 0, d: 1024, tx: 0, ty: 0 };
  const cx = hit.bounds.x + hit.bounds.width / 2;
  const cy = hit.bounds.y + hit.bounds.height / 2;
  step(env, machine, 'pointer.v1', 'click', {
    x: Math.round((t.a * cx + t.c * cy) / 1024 + t.tx),
    y: Math.round((t.b * cx + t.d * cy) / 1024 + t.ty),
    width, height,
  });
  return true;
}

function paint(env, canvas, [width, height]) {
  const frame = env.render(width, height);
  canvas.width = width;
  canvas.height = height;
  canvas.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(frame.rgba), width, height), 0, 0);
  frame.free();
}

/** Wire one tile: its canvas takes the pointer, the wheel and the keyboard. */
function wire(tile, env, machine, size) {
  const [width, height] = size;
  const canvas = document.createElement('canvas');
  canvas.className = 'live-canvas';
  canvas.tabIndex = 0;
  canvas.setAttribute('aria-label', `${tile.dataset.label}, running live`);
  tile.querySelector('.screen').replaceChildren(canvas);

  const draw = () => paint(env, canvas, size);
  const at = e => {
    const r = canvas.getBoundingClientRect();
    return {
      x: Math.round((e.clientX - r.left) * width / r.width),
      y: Math.round((e.clientY - r.top) * height / r.height),
      width, height, button: e.button < 0 ? 0 : e.button,
      pointer_type: e.pointerType || 'mouse',
    };
  };
  const send = (family, op, payload) => {
    try {
      const result = step(env, machine, family, op, payload);
      const cursor = result.outcomes[0]?.value?.cursor;
      if (cursor) canvas.style.cursor = cursor;
    } catch (error) {
      console.warn(machine, error);
    }
    draw();
  };

  let gesture = null, queued = null, frame = 0;
  canvas.addEventListener('pointerdown', e => {
    e.preventDefault();
    canvas.focus();
    canvas.setPointerCapture(e.pointerId);
    gesture = e.pointerId;
    send('pointer.v1', 'down', at(e));
  });
  canvas.addEventListener('pointermove', e => {
    if (gesture !== e.pointerId) return;
    queued = at(e);
    if (!frame) frame = requestAnimationFrame(() => { frame = 0; if (queued) { send('pointer.v1', 'move', queued); queued = null; } });
  });
  canvas.addEventListener('pointerup', e => {
    if (gesture !== e.pointerId) return;
    if (frame) { cancelAnimationFrame(frame); frame = 0; }
    send('pointer.v1', 'up', at(e));
    gesture = null;
    canvas.releasePointerCapture(e.pointerId);
  });
  canvas.addEventListener('dblclick', e => { e.preventDefault(); send('pointer.v1', 'double_click', at(e)); });
  canvas.addEventListener('wheel', e => {
    // Only claim the wheel once the machine is focused, so the page still scrolls past.
    if (document.activeElement !== canvas) return;
    e.preventDefault();
    const modifiers = ['shift', 'ctrl', 'alt', 'meta'].filter(m => e[`${m}Key`]);
    send('pointer.v1', 'wheel', { ...at(e), delta_y: Math.round(e.deltaY), delta_x: Math.round(e.deltaX), modifiers });
  }, { passive: false });
  canvas.addEventListener('keydown', e => {
    if (e.key === 'Tab') return;                       // leave tabbing out of the tile alone
    e.preventDefault();
    const prefix = (e.metaKey ? 'Meta+' : '') + (e.ctrlKey ? 'Ctrl+' : '') + (e.altKey ? 'Alt+' : '') +
      (e.shiftKey && e.key.length > 1 ? 'Shift+' : '');
    const typing = !prefix && e.key.length === 1;
    send('keyboard.v1', typing ? 'type' : 'key', typing ? { text: e.key } : { key: prefix + e.key });
  });

  const chips = tile.querySelector('.chips');
  chips.hidden = false;
  chips.querySelectorAll('button').forEach(chip => {
    chip.addEventListener('click', () => {
      chips.querySelectorAll('button').forEach(b => b.classList.toggle('on', b === chip));
      if (chip.dataset.home) send('application.v1', 'home', {});
      else send('application.v1', 'launch', { kind: chip.dataset.app });
      canvas.focus();
    });
  });
  return draw;
}

export async function boot(note) {
  note(`Downloading the simulator…`);
  await init();
  note('Starting the world…');
  const world = new World(definition, SEED);
  const tiles = [...document.querySelectorAll('.tile[data-machine]')];
  for (const tile of tiles) {
    const machine = tile.dataset.machine;
    const size = tile.dataset.size.split('x').map(Number);
    const computer = world.definition().computers.find(c => c.id === machine);
    const env = world.environment({ actor: computer.user, machines: [machine], actions, observations, action_budget: 1_000_000 });
    note(`Starting ${tile.dataset.label}…`);
    try { openings[machine]?.(env); } catch (error) { console.warn(machine, error); }
    const draw = wire(tile, env, machine, size);
    draw();
    tile.classList.add('live');
  }
  note('');
  return world;
}
