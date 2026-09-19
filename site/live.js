// Live machines on the page. One Wasm module, a world per scene, one session per machine
// and a canvas each. Nothing is fetched after boot: every frame is rendered here, in this tab.
import init, { World } from './demo/pkg/web/computerworld.js';
import definition from './demo/examples/browser/world-definition.js';

const SEED = 2026;
const actions = ['terminal.v1', 'browser.v1', 'keyboard.v1', 'pointer.v1', 'application.v1', 'filesystem.v1', 'http.v1'];
const observations = ['terminal.v1', 'semantic.v1', 'browser.v1'];

// How many scenes stay running. Each is a world of its own, so a visitor who walks the
// whole slideshow would otherwise be holding dozens of them.
const KEPT = 6;

/** The world one scene runs in: the reference company, plus that scene's machines. Each is
 * a copy of the Mac, the PC, the ThinkPad or one of the two phones the reference world
 * ships, on an address of its own. A scene gets a world to itself because openings write to
 * its services (a text, a calendar event, an issue), and what one scene shows should not
 * depend on which others the visitor happened to pass first. */
function populate(definition, scene) {
  definition = structuredClone(definition);
  let host = 10;
  for (const { id, like } of scene.machines) {
    const computer = structuredClone(definition.computers.find(c => c.id === like));
    Object.assign(computer, { id, address: `10.0.7.${host++}` });
    definition.computers.push(computer);
    definition.network.nodes.push({ id, address: computer.address, zone: 'local' });
    definition.network.links.push({ from: 'app-server', to: id, bidirectional: true, latency_us: 10, loss_per_million: 0 });
    definition.metadata.device_presentations[id] = definition.metadata.device_presentations[like];
  }
  return definition;
}

/** What a scene's opening is written against: one machine, and the few verbs that stage it. */
function hands(env, machine, size) {
  const step = (family, op, payload) => env.step([{ family, op, machine, payload }]);
  const [width, height] = size;
  const find = target => env.scene(width, height).nodes
    .filter(n => n.interaction === target || n.interaction?.endsWith(`:content:${target}`))
    .sort((a, b) => (a.z ?? 0) - (b.z ?? 0))
    .pop();
  const clickAt = (x, y) => step('pointer.v1', 'click', { x, y, width, height });
  return {
    step, clickAt,
    launch: (kind, argument) => step('application.v1', 'launch', argument ? { kind, argument } : { kind }),
    sh: command => step('terminal.v1', 'execute', { command }),
    visit: url => step('browser.v1', 'navigate', { url }),
    type: text => step('keyboard.v1', 'type', { text }),
    key: key => step('keyboard.v1', 'key', { key }),
    /** Click the control carrying `target`, wherever the shell painted it. */
    click(target) {
      const hit = find(target);
      if (!hit) return false;
      const t = hit.transform ?? { a: 1024, b: 0, c: 0, d: 1024, tx: 0, ty: 0 };
      const cx = hit.bounds.x + hit.bounds.width / 2;
      const cy = hit.bounds.y + hit.bounds.height / 2;
      clickAt(Math.round((t.a * cx + t.c * cy) / 1024 + t.tx), Math.round((t.b * cx + t.d * cy) / 1024 + t.ty));
      return true;
    },
  };
}

function paint(env, canvas, [width, height]) {
  const frame = env.render(width, height);
  canvas.width = width;
  canvas.height = height;
  canvas.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(frame.rgba), width, height), 0, 0);
  frame.free();
}

/** Wire one tile: its canvas takes the pointer, the wheel and the keyboard. `after` runs
 * once the machine has acted, to repaint whatever else in the scene that may have moved. */
function wire(tile, env, machine, size, after) {
  const [width, height] = size;
  const canvas = document.createElement('canvas');
  canvas.className = 'live-canvas';
  canvas.tabIndex = 0;
  canvas.setAttribute('aria-label', `${tile.dataset.label}, running live`);
  tile.replaceChildren(canvas);

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
      const result = env.step([{ family, op, machine, payload }]);
      const cursor = result.outcomes[0]?.value?.cursor;
      if (cursor) canvas.style.cursor = cursor;
    } catch (error) {
      console.warn(machine, error);
    }
    after(op);
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

  return draw;
}

/** Download the simulator, then hand back a way to start one scene. Scenes come up when
 * the slideshow reaches them, so the first screen is live sooner.
 * `note(text, machine)` writes to one panel, or to all of them when no machine is named. */
export async function boot(note) {
  note('Downloading the simulator, about 10 MB…');
  await init();
  const running = new Map();   // scene id → { booting, stop }, least recently shown first

  /** Stop the scenes shown longest ago. Their screens stay as they were left, as pictures. */
  function retire() {
    for (const [id, scene] of [...running].slice(0, -KEPT)) {
      running.delete(id);
      scene.booting.then(() => scene.stop());
    }
  }

  /** Bring up `scene` in `slide` if it is not already running; resolves once it is painted. */
  function start(scene, slide) {
    const known = running.get(scene.id);
    if (known) {
      running.delete(scene.id);
      running.set(scene.id, known);
      return known.booting;
    }
    const entry = { stop() {} };
    entry.booting = (async () => {
      const tiles = scene.machines.map(({ id }) => slide.querySelector(`[data-machine="${id}"]`));
      scene.machines.forEach(({ id }, i) => note(`Starting ${tiles[i].dataset.label}…`, id));
      // Let the browser paint that note before the opening actions block the thread.
      await new Promise(resolve => requestAnimationFrame(resolve));
      const world = new World(populate(definition, scene), SEED);
      const machines = scene.machines.map(({ id, size }, i) => {
        const { user } = world.definition().computers.find(c => c.id === id);
        const env = world.environment({ actor: user, machines: [id], actions, observations, action_budget: 1_000_000 });
        return { id, size, env, tile: tiles[i] };
      });
      try { scene.open(...machines.map(m => hands(m.env, m.id, m.size))); } catch (error) { console.warn(scene.id, error); }
      // Machines in one scene share a world: a text sent from one phone lands on the other.
      // So once one of them has finished a gesture or a keystroke, the scene may have the
      // others catch up (`sync`), and every one of them is repainted.
      const settled = new Set(['up', 'key', 'type', 'double_click']);
      const draws = [];
      const redraw = () => draws.forEach(draw => draw());
      for (const m of machines) {
        const others = machines.filter(o => o !== m).map(o => hands(o.env, o.id, o.size));
        draws.push(wire(m.tile, m.env, m.id, m.size, op => {
          if (scene.sync && settled.has(op)) try { scene.sync(others); } catch (error) { console.warn(scene.id, error); }
          redraw();
        }));
      }
      redraw();
      for (const m of machines) { m.tile.classList.add('live'); note('', m.id); }
      entry.stop = () => {
        for (const m of machines) {
          // A copy of the last frame, with none of the listeners that drove the machine.
          const canvas = m.tile.querySelector('canvas');
          const picture = canvas.cloneNode();
          picture.removeAttribute('tabindex');
          picture.getContext('2d').drawImage(canvas, 0, 0);
          m.tile.replaceChildren(picture);
          m.tile.classList.remove('live');
          m.env.free?.();
        }
        world.free?.();
      };
    })();
    running.set(scene.id, entry);
    retire();
    return entry.booting;
  }

  note('');
  return start;
}
