// Live machines on the page. One Wasm module, a world per scene, one session per machine
// and a canvas each. Nothing is fetched after boot: every frame is rendered here, in this tab.
import init, { World, installFont, fontPackStatus } from './pkg/computerworld.js';

// The world the machines run in is no longer a static import. It is 7.7 MB, and a static
// import is fetched before the first line of `boot` runs — which is exactly the wait the
// overlay over the stills exists to account for. Both files are fetched below instead,
// where their bytes can be counted as they land.
const WASM = new URL('./pkg/computerworld_bg.wasm', import.meta.url);
const WORLD = new URL('./world-definition.js', import.meta.url);

const SEED = 2026;
const actions = ['terminal.v1', 'browser.v1', 'keyboard.v1', 'pointer.v1', 'application.v1', 'filesystem.v1', 'http.v1'];
const observations = ['terminal.v1', 'semantic.v1', 'browser.v1'];

// How much stays running behind the visitor. Each scene is a world of its own, so a
// visitor who walks the whole slideshow would otherwise be holding dozens of them — and a
// scene is anything from one phone to a team of seven, so the count that matters is the
// machines, not the scenes. The machine in the middle and one neighbour stay whatever they
// cost; behind those two, either ceiling retires the rest.
const KEPT = 6;
const KEPT_MACHINES = 14;

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
 * is four megabytes: the canvas keeps its backing store and the scene keeps its buffer,
 * and the engine renders straight into that buffer rather than handing back a fresh
 * array for each of `render`'s three copies to walk over. */
function painter(env, canvas, [width, height]) {
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  const image = new ImageData(width, height);
  return () => {
    env.renderInto(width, height, image.data);
    context.putImageData(image, 0, 0);
  };
}

/** Wire one tile: its canvas takes the pointer, the wheel and the keyboard. `after` runs
 * once the machine has acted, to repaint whatever else in the scene that may have moved. */
function wire(tile, env, machine, size, after) {
  const [width, height] = size;
  const canvas = document.createElement('canvas');
  canvas.className = 'live-canvas';
  canvas.tabIndex = 0;
  canvas.setAttribute('aria-label', `${tile.dataset.label}, running live`);
  show(tile, canvas);

  const draw = painter(env, canvas, size);
  const at = e => {
    const r = canvas.getBoundingClientRect();
    return {
      x: Math.round((e.clientX - r.left) * width / r.width),
      y: Math.round((e.clientY - r.top) * height / r.height),
      width, height, button: e.button < 0 ? 0 : e.button,
      pointer_type: e.pointerType || 'mouse',
    };
  };
  // A desktop paints its own pointer into the frame, so a mouse over it hides the host's;
  // a phone paints none, and there the host cursor still follows the machine's hint.
  const phone = tile.classList.contains('phone');
  let hint = 'default';
  const showCursor = e => { canvas.style.cursor = !phone && e.pointerType === 'mouse' ? 'none' : hint; };
  const send = (family, op, payload) => {
    try {
      const result = env.step([{ family, op, machine, payload }]);
      const cursor = result.outcomes[0]?.value?.cursor;
      if (cursor && canvas.style.cursor !== 'none') canvas.style.cursor = cursor;
      if (cursor) hint = cursor;
    } catch (error) {
      console.warn(machine, error);
    }
    after(op);
  };

  let gesture = null, queued = null, frame = 0;
  canvas.addEventListener('pointerenter', showCursor);
  canvas.addEventListener('pointerleave', e => { if (gesture === null) canvas.style.cursor = hint; else showCursor(e); });
  canvas.addEventListener('pointerdown', e => {
    e.preventDefault();
    canvas.focus();
    canvas.setPointerCapture(e.pointerId);
    gesture = e.pointerId;
    send('pointer.v1', 'down', at(e));
  });
  canvas.addEventListener('pointermove', e => {
    // A mouse moves the machine's pointer as it hovers; a finger only while it drags.
    if (gesture === null ? e.pointerType !== 'mouse' : gesture !== e.pointerId) return;
    showCursor(e);
    queued = at(e);
    if (!frame) frame = requestAnimationFrame(() => { frame = 0; if (queued) { send('pointer.v1', 'move', queued); queued = null; } });
  });
  canvas.addEventListener('pointerup', e => {
    if (gesture !== e.pointerId) return;
    if (frame) { cancelAnimationFrame(frame); frame = 0; queued = null; }
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

/** What a tile shows: its screen, and whatever controls app.js hung on it (the fullscreen
 * button), which a swap of the screen must not throw away. */
const show = (tile, screen) => tile.replaceChildren(screen, ...tile.querySelectorAll('[data-keep]'));

/** How many bytes a reader will count out of a response, or `null` when that cannot be
 * known. `Content-Length` counts the bytes on the wire; a compressed body is handed back
 * decoded, so on a server that gzips these two files the header is not the number the
 * reader is counting up to, and there is no honest denominator. An invented one would be
 * a bar filling at a guessed rate, which is worse than no number at all — so `null`, and
 * the overlay drops its percentage and shows the spinner alone. */
function length(response) {
  const given = Number(response.headers.get('content-length'));
  return response.headers.get('content-encoding') || !(given > 0) ? null : given;
}

/** The whole of what a visitor waits for: the Wasm module and the world its machines run
 * in, fetched together and counted as they arrive. `report(percent)` gets a whole number
 * while the count is a real one, `null` while it is not, and `false` once the download is
 * over. Returns the world definition, with the module instantiated.
 *
 * The font pack is not in here. It is fetched after this returns and is never waited on:
 * layout is final without it and every running scene repaints as each file lands, so a
 * visitor is looking at a machine long before the faces are all in. */
async function arrive(report) {
  const [wasm, world] = await Promise.all([fetch(WASM), fetch(WORLD)]);
  if (!wasm.ok || !world.ok) throw new Error(`the simulator did not download: ${wasm.status}, ${world.status}`);
  const sizes = [length(wasm), length(world)];
  const total = sizes.includes(null) ? null : sizes[0] + sizes[1];
  let done = 0, shown = -1;
  const counting = () => new TransformStream({
    transform(chunk, out) {
      done += chunk.byteLength;
      out.enqueue(chunk);
      if (total === null) return;
      const percent = Math.min(100, Math.floor(done * 100 / total));
      if (percent !== shown) { shown = percent; report(percent); }
    },
  });
  report(total === null ? null : 0);
  // Both at once, and the module compiles as it arrives: `init` is handed a response
  // rather than a buffer, so instantiation streams off the same bytes being counted.
  const source = new Response(world.body.pipeThrough(counting())).text();
  source.catch(() => {});        // if the module fails first, this must not go unhandled
  await init({ module_or_path: new Response(wasm.body.pipeThrough(counting()), { headers: { 'content-type': 'application/wasm' } }) });
  const text = await source;
  report(false);
  // `export default <JSON>;`, written by scripts/build-live-world.mjs. If it ever stops
  // being plain JSON, the module itself still answers — out of the browser's cache.
  try { return JSON.parse(text.slice(text.indexOf('{'), text.lastIndexOf('}') + 1)); }
  catch { return (await import(WORLD.href)).default; }
}

/** Download the simulator, then hand back a way to start one scene. Scenes come up when
 * the slideshow reaches them, so the first screen is live sooner.
 * `report` is the download talking, and nothing else is said over a still: see `arrive`. */
export async function boot(report) {
  const definition = await arrive(report);
  const running = new Map();   // scene id → { booting, stop }, least recently shown first
  const painters = new Set();  // every running scene's repaint, for when a font lands

  /** A font file landed, so every running scene's cached text is stale. Repaint the scene
   * the visitor is looking at, and no more than that: the pack is a dozen files and six
   * scenes may be running, so repainting all of them for every file is a few hundred full
   * screens in one burst, all but one of them behind the strip. The rest are marked here
   * and repainted when the slideshow next reaches them. */
  const restale = () => {
    for (const painter of painters) {
      if (painter.slide.classList.contains('active')) painter.redraw();
      else painter.stale = true;
    }
  };

  // The Wasm build does not embed the CJK and emoji faces; layout is final without them,
  // but their glyphs draw as boxes until the file is installed. Emoji are everywhere
  // (Slack reactions, message tapbacks), so those two come first and the rest follow in
  // the background; the scene on screen is repainted as each file lands.
  const fontPack = (async () => {
    const pending = fontPackStatus().files.filter(f => !f.installed);
    const first = new Set(['noto-emoji.ttf', 'noto-color-emoji.ttf', 'noto-sans-sc.ttf', 'noto-sans-kr.ttf']);
    const fetchFonts = files => Promise.all(files.map(async f => {
      try {
        const r = await fetch(new URL(`./pkg/${f.path}`, import.meta.url));
        if (r.ok) { installFont(new Uint8Array(await r.arrayBuffer())); restale(); }
      } catch (error) { console.warn('font pack', f.file, error); }
    }));
    await fetchFonts(pending.filter(f => first.has(f.file)));
    await fetchFonts(pending.filter(f => !first.has(f.file)));
  })();
  window.computerworldFonts = fontPack;

  /** Stop the scenes shown longest ago. Their screens stay as they were left, as pictures. */
  function retire() {
    let machines = 0;
    [...running].reverse().forEach(([id, scene], place) => {
      machines += scene.machines;
      if (place < 2 || (place < KEPT && machines <= KEPT_MACHINES)) return;
      running.delete(id);
      scene.booting.then(() => scene.stop());
    });
  }

  /** Bring up `scene` in `slide` if it is not already running; resolves once it is painted.
   *
   * `spare` marks a scene nobody is looking at — a neighbour warmed so that the edges of
   * the strip are already running when they are reached. That is worth a world when the
   * neighbour is a laptop and not when it is a team of seven: the slideshow opens on five
   * machines with four on one side and seven on the other, and building all sixteen before
   * the visitor has touched anything costs more than it saves. So a spare scene comes up
   * only while the machines already running leave room for it, and one that does not is
   * started the ordinary way when the strip reaches it. */
  function start(scene, slide, { spare = false } = {}) {
    const known = running.get(scene.id);
    if (known) {
      running.delete(scene.id);
      running.set(scene.id, known);
      if (known.painter?.stale) { known.painter.stale = false; known.painter.redraw(); }
      return known.booting;
    }
    if (spare) {
      let machines = scene.machines.length;
      for (const live of running.values()) machines += live.machines;
      if (machines > KEPT_MACHINES) return Promise.resolve();
    }
    const entry = { machines: scene.machines.length, stop() {} };
    entry.booting = (async () => {
      const tiles = scene.machines.map(({ id }) => slide.querySelector(`[data-machine="${id}"]`));
      // Let the browser paint the overlay's spinner before the opening actions, which
      // build a world and hold the thread for a second or two, land on top of it.
      await new Promise(resolve => requestAnimationFrame(resolve));
      const world = new World(populate(definition, scene), SEED);
      const machines = scene.machines.map(({ id, size }, i) => {
        const { user } = world.definition().computers.find(c => c.id === id);
        const env = world.environment({ actor: user, machines: [id], actions, observations, action_budget: 1_000_000 });
        return { id, size, env, tile: tiles[i] };
      });
      try { scene.open(...machines.map(m => hands(m.env, m.id, m.size))); } catch (error) { console.warn(scene.id, error); }
      // Machines in one scene share a world: a text sent from one phone lands on the other.
      // So once one of them has FINISHED a gesture or a keystroke, the scene may have the
      // others catch up (`sync`), and every one of them is repainted.
      //
      // Only once it has finished, though. A pointer crossing a window, a button going
      // down, a wheel turning: those change the machine under the hand and nothing else in
      // the world, so that machine repaints alone. Repainting the whole scene for them is
      // what made a slide of seven machines drag the page down — a hover is rendered every
      // animation frame, and seven full screens do not fit in one.
      const settled = new Set(['up', 'key', 'type', 'double_click']);
      const draws = [];
      const redraw = () => { for (const draw of draws) draw(); };
      for (const m of machines) {
        const others = machines.filter(o => o !== m).map(o => hands(o.env, o.id, o.size));
        let mine;
        mine = wire(m.tile, m.env, m.id, m.size, op => {
          if (!settled.has(op)) { mine(); return; }
          if (scene.sync) try { scene.sync(others); } catch (error) { console.warn(scene.id, error); }
          redraw();
        });
        draws.push(mine);
      }
      redraw();
      entry.painter = { slide, redraw, stale: false };
      painters.add(entry.painter);
      for (const m of machines) m.tile.classList.add('live');
      entry.stop = () => {
        painters.delete(entry.painter);
        for (const m of machines) {
          // A copy of the last frame, with none of the listeners that drove the machine.
          const canvas = m.tile.querySelector('canvas');
          const picture = canvas.cloneNode();
          picture.removeAttribute('tabindex');
          picture.getContext('2d').drawImage(canvas, 0, 0);
          show(m.tile, picture);
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

  return start;
}
