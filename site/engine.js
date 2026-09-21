// The machines themselves: one Wasm module, a world per scene, a session per machine and
// a canvas each. Nothing in this file touches the DOM, knows what a slide is or hears an
// event, so the same code runs in a worker against an OffscreenCanvas and in the tab
// against an ordinary one. `worker.js` is the first of those hosts and `live.js` the
// second; between them they decide which machines run and when, and this decides nothing.
import init, { initSync, World, installFont, fontPackStatus } from './pkg/computerworld.js';

const SEED = 2026;
const actions = ['terminal.v1', 'browser.v1', 'keyboard.v1', 'pointer.v1', 'application.v1', 'filesystem.v1', 'http.v1'];
const observations = ['terminal.v1', 'semantic.v1', 'browser.v1'];

/** Instantiate the engine from a module the host has already compiled. One compile serves
 * every worker: `WebAssembly.Module` survives `postMessage`, and instantiating a copy of it
 * is a fraction of what compiling the bundle again would be.
 *
 * A worker takes the short way and a page does not: `new WebAssembly.Instance` is refused
 * on a page's own thread for a module over 8 MB, and this one is thirty-five, so the tab
 * instantiates asynchronously. Await this either way. */
export function begin(module) {
  if (!(module instanceof WebAssembly.Module)) return init(module);
  if (typeof WorkerGlobalScope !== 'undefined' && self instanceof WorkerGlobalScope) {
    initSync({ module });
    return Promise.resolve();
  }
  return init({ module_or_path: module });
}

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

/** One machine's repaint, set up once. The screen is 1280x800 or thereabouts, so a frame
 * is four megabytes: the canvas keeps its backing store and the machine keeps its buffer,
 * and the engine renders straight into that buffer rather than handing back a fresh array
 * for each of `render`'s copies to walk over. */
function painter(env, screen, [width, height]) {
  screen.width = width;
  screen.height = height;
  const context = screen.getContext('2d');
  const image = new ImageData(width, height);
  return {
    image,
    draw() {
      env.renderInto(width, height, image.data);
      context.putImageData(image, 0, 0);
    },
  };
}

// Machines in one scene share a world: a text sent from one phone lands on the other. So
// once one of them has FINISHED a gesture or a keystroke, the scene may have the others
// catch up (`sync`), and every one of them is repainted.
//
// Only once it has finished, though. A pointer crossing a window, a button going down, a
// wheel turning: those change the machine under the hand and nothing else in the world, so
// that machine repaints alone. Repainting the whole scene for them is what made a slide of
// seven machines drag the page down — a hover is rendered every animation frame, and seven
// full screens do not fit in one.
const SETTLED = new Set(['up', 'key', 'type', 'double_click']);

/** Build `scene`'s world and paint every machine in it onto the screen given for that
 * machine's id. Returns the handle its host drives it through. Everything here is
 * synchronous and slow — a world is seconds of work — which is the reason a host would
 * rather be a worker than a tab. */
export function run(definition, scene, screens) {
  const world = new World(populate(definition, scene), SEED);
  const machines = scene.machines.map(({ id, size }) => {
    const { user } = world.definition().computers.find(c => c.id === id);
    const env = world.environment({ actor: user, machines: [id], actions, observations, action_budget: 1_000_000 });
    const { image, draw } = painter(env, screens[id], size);
    return { id, size, env, image, draw };
  });
  try { scene.open(...machines.map(m => hands(m.env, m.id, m.size))); } catch (error) { console.warn(scene.id, error); }
  const redraw = () => { for (const m of machines) m.draw(); };
  redraw();
  return {
    /** One action on one machine, and the repaint it earns. Gives back the cursor the
     * machine asked for, where it asked for one. */
    act(id, family, op, payload) {
      const acting = machines.find(m => m.id === id);
      if (!acting) return undefined;
      let cursor;
      try {
        const result = acting.env.step([{ family, op, machine: id, payload }]);
        cursor = result.outcomes[0]?.value?.cursor;
      } catch (error) {
        console.warn(id, error);
      }
      if (!SETTLED.has(op)) acting.draw();
      else {
        if (scene.sync) {
          const others = machines.filter(m => m !== acting).map(m => hands(m.env, m.id, m.size));
          try { scene.sync(others); } catch (error) { console.warn(scene.id, error); }
        }
        redraw();
      }
      return cursor;
    },
    redraw,
    /** The pixels a machine is showing, copied out of the buffer it was drawn from. The
     * canvas cannot answer this where it is an OffscreenCanvas: what a page gets back from
     * a placeholder is the frame it was first given, not the frame it is showing. */
    frame(id) {
      const m = machines.find(machine => machine.id === id);
      if (!m) return null;
      const [width, height] = m.size;
      return { rgba: m.image.data.slice().buffer, width, height };
    },
    stop() {
      for (const m of machines) m.env.free?.();
      world.free?.();
    },
  };
}

/** The CJK and emoji faces, which the Wasm build does not embed. Layout is final without
 * them but their glyphs draw as boxes until the file is installed, and emoji are everywhere
 * — Slack reactions, message tapbacks — so those come first and the rest follow behind
 * them. `landed` is called after each file, because whoever owns the screens is the one who
 * knows which of them a reader is looking at. */
export async function installFontPack(landed) {
  const pending = fontPackStatus().files.filter(f => !f.installed);
  const first = new Set(['noto-emoji.ttf', 'noto-color-emoji.ttf', 'noto-sans-sc.ttf', 'noto-sans-kr.ttf']);
  const fetchFonts = files => Promise.all(files.map(async f => {
    try {
      const r = await fetch(new URL(`./pkg/${f.path}`, import.meta.url));
      if (r.ok) { installFont(new Uint8Array(await r.arrayBuffer())); landed(f.file); }
    } catch (error) { console.warn('font pack', f.file, error); }
  }));
  await fetchFonts(pending.filter(f => first.has(f.file)));
  await fetchFonts(pending.filter(f => !first.has(f.file)));
}
