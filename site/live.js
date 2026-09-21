// Live machines on the page: the download, somewhere to run the machines, and the events
// that reach them. The machines themselves are in engine.js, and where that engine runs is
// the point of this file — in a pool of workers, each holding a scene or two, drawing
// through an OffscreenCanvas the page has handed over. Building a world is seconds of
// arithmetic and a screen is four megabytes of rasterising, and none of it happens on the
// thread that has to answer the scroll.
//
// A browser without workers or OffscreenCanvas runs the same engine in the tab, as this
// file used to. It is the same code either side of that line; only the host differs.
import { begin, run, installFontPack } from './engine.js';

const WASM = new URL('./pkg/computerworld_bg.wasm', import.meta.url);
const WORLD = new URL('./world-definition.js', import.meta.url);

// How much stays running behind the visitor. Each scene is a world of its own, so a
// visitor who walks the whole slideshow would otherwise be holding dozens of them — and a
// scene is anything from one phone to a team of seven, so the count that matters is the
// machines, not the scenes. The machine in the middle and one neighbour stay whatever they
// cost; behind those two, either ceiling retires the rest.
const KEPT = 6;
const KEPT_MACHINES = 14;

// THE NUMBER BELOW IS ONE HALF OF A MATCHED PAIR. worker.js writes the same literal out
// again, and the two are bumped together whenever the shapes of the messages between these
// files change. It is deliberately not imported from a module both could share: a third
// file would be cached on its own timetable like these two are, and both sides would go on
// agreeing about a number neither of them had just fetched.
//
// The disagreement is real. GitHub Pages serves this site with `cache-control:
// max-age=600`, so for ten minutes after a deploy a returning visitor can hold this file
// from before it and worker.js from after, and a message the far side does not recognise
// is dropped without a word — a scene that never paints and a spinner that never stops.
// The worker says its number in `ready`; a pool that answers with anything else, or does
// not answer at all, is thrown away and the machines run in the tab, which is one file and
// cannot disagree with itself.
const PROTOCOL = 1;

// How long the first worker has to say `ready`. It instantiates the module and parses 7.7
// MB of world before it answers, which is 82 to 98 ms on this machine; two seconds leaves
// room for one a good deal slower. Nothing is waiting on this but the choice between the
// pool and the tab — the first machine is up in about 1.4 s either way, because the worker
// had to finish that same work before it could act on a `start` in any case — and a worker
// that has not spoken by then is not one to hand a scene to.
const GREETING = 2000;

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
 * over. Returns the module COMPILED but not instantiated, and the definition as the text
 * it came down as — both of which cross a `postMessage` intact, so however many workers
 * end up running machines, this is compiled and downloaded once for all of them.
 *
 * The font pack is not in here. It is fetched by each worker once it has a machine to draw
 * and is never waited on: layout is final without it and every running scene repaints as
 * each file lands, so a visitor is looking at a machine long before the faces are all in. */
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
  // Both at once, and the module compiles as it arrives: compilation streams off the same
  // bytes being counted.
  const source = new Response(world.body.pipeThrough(counting())).text();
  source.catch(() => {});        // if the module fails first, this must not go unhandled
  const module = await WebAssembly.compileStreaming(
    new Response(wasm.body.pipeThrough(counting()), { headers: { 'content-type': 'application/wasm' } }));
  const text = await source;
  report(false);
  // `export default <JSON>;`, written by scripts/build-live-world.mjs. The braces are cut
  // out here rather than in each worker, but the text is what travels: a worker parses it
  // into a definition of its own, since a world may not be shared between them.
  const start = text.indexOf('{'), end = text.lastIndexOf('}') + 1;
  return { module, world: start >= 0 ? text.slice(start, end) : text };
}

/** A pool of workers, or `null` where the browser will not give us one — or where the one
 * it gives us turns out to be speaking a different protocol from this file. A scene goes
 * to an idle worker where there is one and stays with it for as long as it runs, because
 * its world lives there; a worker is only made when a scene needs one, so a visitor who
 * looks at a single machine pays for a single worker.
 *
 * Four at most, and fewer on a small machine. Each one holds its own instance of the
 * engine, a world for every scene it is running and its own copy of whatever fonts have
 * been installed into it, which is worth a couple of hundred megabytes apiece: the front
 * page measures 1.81 GB resident across the browser with the two workers it fills against
 * 1.59 GB held to one, and a walk across four one-machine scenes 1.96 GB on four workers
 * against 1.61 GB on one.
 * So the ceiling is memory as much as it is cores, and `navigator.deviceMemory` —
 * gigabytes, rounded down to a power of two and capped at 8 by the browsers that offer it
 * — is the only thing a page is told about the first. Firefox and Safari say nothing,
 * which is what the `|| 0` is for: there the cores decide alone, as they did before. */
async function crew(seed, heard) {
  if (typeof Worker === 'undefined' || typeof OffscreenCanvas === 'undefined'
      || !HTMLCanvasElement.prototype.transferControlToOffscreen) return null;
  const gigabytes = navigator.deviceMemory || 0;
  const most = Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 4) - 1,
                                   gigabytes ? Math.floor(gigabytes / 2) : 4));
  const hands = [];
  const desk = new Map();          // scene id → the worker holding it
  const painting = new Map();      // scene id → { resolve, reject } while it comes up
  const frames = new Map();        // token → whoever asked for a machine's pixels
  let asked = 0;

  const holding = id => desk.get(id)?.worker;
  const held = hand => [...desk].filter(([, h]) => h === hand).map(([id]) => id);
  /** Which worker a scene goes to: an idle one if there is one, a new one while the pool
   * has room, and otherwise whichever is carrying least. A scene stays with the worker it
   * was dealt to for as long as it runs, because its world lives there. */
  const deal = id => {
    let hand = desk.get(id);
    if (hand) return hand;
    hand = hands.find(h => h.scenes === 0)
      ?? (hands.length < most ? hire() : null)
      ?? hands.reduce((fewest, h) => (h.scenes < fewest.scenes ? h : fewest));
    hand.scenes++;
    desk.set(id, hand);
    return hand;
  };

  /** Tell a worker to fetch the font pack, once. It does not fetch it off its own bat any
   * more: the pack is 20 MB of TTF and a worker parses its own copy, so one that is only
   * holding a neighbour warmed in the background — a scene nobody has looked at — is 20 MB
   * the visitor has not asked for. Until it is told, it has no fonts and nothing waits on
   * its `fonts` either, which is why `fonts` below reads `warmed` and not `scenes`. */
  const heat = hand => {
    if (!hand || hand.warmed) return;
    hand.warmed = true;
    hand.worker.postMessage({ kind: 'warm' });
  };

  const hire = () => {
    let worker;
    try {
      worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
    } catch (error) {
      console.warn('worker', error);
      return null;
    }
    const hand = { worker, scenes: 0, warmed: false };
    hand.fonts = new Promise(done => { hand.fontsIn = done; });
    hand.ready = new Promise(done => { hand.readyIn = done; });
    worker.onmessage = ({ data }) => {
      const waiting = painting.get(data.id);
      if (data.kind === 'ready') { hand.readyIn(data.protocol); return; }
      if (data.kind === 'painted') { painting.delete(data.id); waiting?.resolve(); }
      else if (data.kind === 'failed') { painting.delete(data.id); waiting?.reject(new Error(data.why)); }
      else if (data.kind === 'cursor') heard.cursor(data.id, data.machine, data.cursor);
      // Only this worker's own scenes are stale: a face installed in one worker changes
      // nothing about a machine running in another, and four workers each fetching a dozen
      // files would otherwise repaint the slideshow fifty times over.
      else if (data.kind === 'font') heard.font(held(hand));
      else if (data.kind === 'fonts') hand.fontsIn();
      else if (data.kind === 'frame') {
        frames.get(data.token)?.(data.rgba ? new ImageData(new Uint8ClampedArray(data.rgba), data.width, data.height) : null);
        frames.delete(data.token);
      }
    };
    // A worker that dies takes its scenes' worlds with it, and anything waiting on one of
    // them would otherwise wait for ever — the slideshow awaits a scene coming up, and the
    // stills renderer awaits a frame. Everything owed is answered before it is let go.
    worker.onerror = error => {
      console.warn('worker', error.message ?? error);
      for (const id of held(hand)) {
        painting.get(id)?.reject(new Error('the worker running this scene stopped'));
        painting.delete(id);
        desk.delete(id);
      }
      for (const [token, answer] of frames) { answer(null); frames.delete(token); }
      hand.fontsIn();
      hand.readyIn(null);            // a worker that died before it spoke never will
      hands.splice(hands.indexOf(hand), 1);
    };
    worker.postMessage({ kind: 'boot', module: seed.module, world: seed.world });
    hands.push(hand);
    return hand;
  };

  // One now, and a word out of it before the pool is used for anything. A browser that
  // refuses to make a module worker is found out here; so is a worker.js that is not the
  // one this file was written against, or that is not there at all. Either way the answer
  // is the tab rather than a dead slideshow. This costs the first machine nothing: the
  // worker has to finish this same `boot` before it could act on a `start` anyway, so the
  // wait below is one it was going to do inside `start`.
  const first = hire();
  if (!first) return null;
  const spoken = await Promise.race([first.ready, new Promise(done => setTimeout(done, GREETING, null))]);
  if (spoken !== PROTOCOL) {
    console.warn(`this page speaks protocol ${PROTOCOL} and its workers speak ${spoken ?? 'nothing at all'}; running the machines in the tab instead`);
    // `first` may already have taken itself out of `hands` on the way down; it is let go
    // either way, because a worker nobody can talk to is still holding an engine.
    for (const hand of new Set([first, ...hands])) hand.worker.terminate();
    return null;
  }

  return {
    start(scene, screens, spare) {
      const hand = deal(scene.id);
      const offscreen = {};
      const moving = [];
      for (const [machine, canvas] of Object.entries(screens)) {
        offscreen[machine] = canvas.transferControlToOffscreen();
        moving.push(offscreen[machine]);
      }
      const painted = new Promise((resolve, reject) => painting.set(scene.id, { resolve, reject }));
      hand.worker.postMessage({ kind: 'start', id: scene.id, screens: offscreen }, moving);
      if (!spare) heat(hand);
      return painted;
    },
    // A scene that was warmed as a neighbour is now the one being looked at, so the worker
    // holding it needs the faces after all.
    warm: id => heat(desk.get(id)),
    // Every worker that is holding anything, whether or not the visitor has reached it.
    // app.js asks for this once the strip has stood still, so that arrowing onto a
    // neighbour is not watching its emoji turn from boxes into glyphs.
    prewarm: () => { for (const hand of hands) if (hand.scenes) heat(hand); },
    act: (id, machine, family, op, payload) =>
      holding(id)?.postMessage({ kind: 'act', id, machine, family, op, payload }),
    frame(id, machine) {
      const worker = holding(id);
      if (!worker) return Promise.resolve(null);
      const token = ++asked;
      const answer = new Promise(resolve => frames.set(token, resolve));
      worker.postMessage({ kind: 'frame', id, machine, token });
      return answer;
    },
    redraw: id => holding(id)?.postMessage({ kind: 'redraw', id }),
    stop(id) {
      const hand = desk.get(id);
      if (!hand) return;
      hand.worker.postMessage({ kind: 'stop', id });
      hand.scenes--;
      desk.delete(id);
    },
    // Only the workers that have been asked for the pack: one that has never been dealt a
    // scene, or that is holding nothing but a neighbour nobody has looked at, has fetched
    // no fonts and never will unless it is told to, so waiting on it would be waiting for
    // ever. Read fresh each time, because which workers those are changes as the visitor
    // moves along the strip.
    fonts: () => Promise.all(hands.filter(hand => hand.warmed).map(hand => hand.fonts)),
  };
}

/** The same thing, in the tab, for a browser that will not run the other one. Every call
 * blocks the page exactly as this file used to. */
async function tab(seed, heard) {
  await begin(seed.module);
  const definition = JSON.parse(seed.world);
  const running = new Map();
  let fonts = null;
  return {
    async start(scene, screens, spare) {
      running.set(scene.id, run(definition, scene, screens));
      // Not for a neighbour nobody is looking at: here the pack is 6.1 s of parsing on the
      // page's own thread, which is the worst moment to spend on a scene off the strip.
      if (!spare) fonts ??= installFontPack(() => heard.font());
    },
    warm: () => { fonts ??= installFontPack(() => heard.font()); },
    prewarm: () => { if (running.size) fonts ??= installFontPack(() => heard.font()); },
    act(id, machine, family, op, payload) {
      const cursor = running.get(id)?.act(machine, family, op, payload);
      if (cursor) heard.cursor(id, machine, cursor);
    },
    redraw: id => running.get(id)?.redraw(),
    frame(id, machine) {
      const frame = running.get(id)?.frame(machine);
      return Promise.resolve(frame ? new ImageData(new Uint8ClampedArray(frame.rgba), frame.width, frame.height) : null);
    },
    stop(id) { running.get(id)?.stop(); running.delete(id); },
    fonts: () => fonts ?? Promise.resolve(),
  };
}

/** Wire one tile: its canvas takes the pointer, the wheel and the keyboard, and `send`
 * carries what they mean to whichever host is running that machine. Nothing here waits for
 * an answer — the machine draws itself, wherever it is — so a held arrow key or a dragged
 * pointer costs this thread a `postMessage` and no more. */
function wire(tile, machine, size, send) {
  const [width, height] = size;
  const canvas = document.createElement('canvas');
  canvas.className = 'live-canvas';
  canvas.tabIndex = 0;
  canvas.setAttribute('aria-label', `${tile.dataset.label}, running live`);
  // Every listener at once, when the machine is stopped: the canvas stays where it is,
  // showing the last frame it was painted with, and answers nothing.
  const listening = new AbortController();
  const on = (type, handler, options) =>
    canvas.addEventListener(type, handler, { signal: listening.signal, ...options });

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

  let gesture = null, queued = null, frame = 0;
  on('pointerenter', showCursor);
  on('pointerleave', e => { if (gesture === null) canvas.style.cursor = hint; else showCursor(e); });
  on('pointerdown', e => {
    e.preventDefault();
    canvas.focus();
    canvas.setPointerCapture(e.pointerId);
    gesture = e.pointerId;
    send('pointer.v1', 'down', at(e));
  });
  on('pointermove', e => {
    // A mouse moves the machine's pointer as it hovers; a finger only while it drags.
    if (gesture === null ? e.pointerType !== 'mouse' : gesture !== e.pointerId) return;
    showCursor(e);
    queued = at(e);
    if (!frame) frame = requestAnimationFrame(() => { frame = 0; if (queued) { send('pointer.v1', 'move', queued); queued = null; } });
  });
  on('pointerup', e => {
    if (gesture !== e.pointerId) return;
    if (frame) { cancelAnimationFrame(frame); frame = 0; queued = null; }
    send('pointer.v1', 'up', at(e));
    gesture = null;
    canvas.releasePointerCapture(e.pointerId);
  });
  on('dblclick', e => { e.preventDefault(); send('pointer.v1', 'double_click', at(e)); });
  on('wheel', e => {
    // Only claim the wheel once the machine is focused, so the page still scrolls past.
    if (document.activeElement !== canvas) return;
    e.preventDefault();
    const modifiers = ['shift', 'ctrl', 'alt', 'meta'].filter(m => e[`${m}Key`]);
    send('pointer.v1', 'wheel', { ...at(e), delta_y: Math.round(e.deltaY), delta_x: Math.round(e.deltaX), modifiers });
  }, { passive: false });
  on('keydown', e => {
    if (e.key === 'Tab') return;                       // leave tabbing out of the tile alone
    e.preventDefault();
    const prefix = (e.metaKey ? 'Meta+' : '') + (e.ctrlKey ? 'Ctrl+' : '') + (e.altKey ? 'Alt+' : '') +
      (e.shiftKey && e.key.length > 1 ? 'Shift+' : '');
    const typing = !prefix && e.key.length === 1;
    send('keyboard.v1', typing ? 'type' : 'key', typing ? { text: e.key } : { key: prefix + e.key });
  });

  return {
    canvas,
    /** The cursor the machine asked for, whenever its answer gets back here. */
    cursor(named) {
      hint = named;
      if (canvas.style.cursor !== 'none') canvas.style.cursor = named;
    },
    stop: () => listening.abort(),
  };
}

/** Download the simulator, then hand back a way to start one scene. Scenes come up when
 * the slideshow reaches them, so the first screen is live sooner.
 * `report` is the download talking, and nothing else is said over a still: see `arrive`. */
export async function boot(report) {
  const seed = await arrive(report);
  const running = new Map();   // scene id → { booting, count, ids, ... }, least recently shown first
  const wiring = new Map();    // scene id → machine id → what `wire` gave back

  const heard = {
    cursor: (id, machine, named) => wiring.get(id)?.get(machine)?.cursor(named),
    font: ids => restale(ids),
  };
  const host = await crew(seed, heard) ?? await tab(seed, heard);

  /** A font file landed, so every running scene's cached text is stale. Repaint the scene
   * the visitor is looking at, and no more than that: the pack is a dozen files and six
   * scenes may be running, so repainting all of them for every file is a few hundred full
   * screens in one burst, all but one of them behind the strip. The rest are marked here
   * and repainted when the slideshow next reaches them. */
  function restale(ids) {
    for (const [id, scene] of running) {
      if (ids && !ids.includes(id)) continue;
      if (scene.slide?.classList.contains('active')) host.redraw(id);
      else scene.stale = true;
    }
  }

  // What `scripts/render-site-stills.mjs` waits on before it saves a screen, and the one
  // thing about the fonts the page outside this file can see. A fresh promise each time it
  // is read, because which workers have machines — and so which have fetched fonts — is
  // settled by where the visitor has been.
  Object.defineProperty(window, 'computerworldFonts', { configurable: true, get: () => host.fonts() });

  // What a machine is showing, for `scripts/render-site-stills.mjs`, which saves these
  // frames to disk as the stills the page puts behind its machines. It cannot read them
  // off the canvas: a canvas whose control has gone to a worker hands a page back the
  // frame it was first given, however many have been drawn since.
  window.computerworldFrame = machine => {
    for (const [id, scene] of running) if (scene.ids.includes(machine)) return host.frame(id, machine);
    return Promise.resolve(null);
  };

  /** Stop the scenes shown longest ago. Their screens stay as they were left, as pictures:
   * a canvas nobody draws to any more goes on showing its last frame. */
  function retire() {
    let machines = 0;
    [...running].reverse().forEach(([id, scene], place) => {
      machines += scene.count;
      if (place < 2 || (place < KEPT && machines <= KEPT_MACHINES)) return;
      running.delete(id);
      scene.booting.then(() => {
        host.stop(id);
        for (const wired of wiring.get(id)?.values() ?? []) wired.stop();
        wiring.delete(id);
        for (const tile of scene.tiles ?? []) tile.classList.remove('live');
      });
    });
  }

  /** Bring up `scene` in `slide` if it is not already running. Resolves with whether it is
   * running: `true` once it is painted, `false` for one that would not come up and for a
   * spare that was declined. The slideshow shows its overlay while this is outstanding and
   * takes it away on the answer, so the answer has to come either way — a scene that fails
   * silently is a spinner that turns for ever over a still.
   *
   * `spare` marks a scene nobody is looking at — a neighbour warmed so that the edges of
   * the strip are already running when they are reached. That is worth a world when the
   * neighbour is a laptop and not when it is a team of seven: the slideshow opens on five
   * machines with four on one side and seven on the other, and building all sixteen before
   * the visitor has touched anything costs more than it saves. So a spare scene comes up
   * only while the machines already running leave room for it, and one that does not is
   * started the ordinary way when the strip reaches it. It is also what decides whether the
   * worker holding the scene fetches the font pack, so a scene that arrives as a spare and
   * is then looked at has to say so. */
  function start(scene, slide, { spare = false } = {}) {
    const known = running.get(scene.id);
    if (known) {
      running.delete(scene.id);
      running.set(scene.id, known);
      if (known.stale) { known.stale = false; host.redraw(scene.id); }
      // After it is up, not now: the scene is not on a worker's desk until `host.start` has
      // dealt it to one, and a spare can be looked at before it has finished coming up.
      if (known.spare && !spare) { known.spare = false; known.booting.then(() => host.warm(scene.id)); }
      return known.booting;
    }
    if (spare) {
      let machines = scene.machines.length;
      for (const live of running.values()) machines += live.count;
      if (machines > KEPT_MACHINES) return Promise.resolve(false);
    }
    const entry = { count: scene.machines.length, ids: scene.machines.map(m => m.id), slide, stale: false, spare, tiles: [] };
    entry.booting = (async () => {
      const tiles = new Map(scene.machines.map(({ id }) => [id, slide.querySelector(`[data-machine="${id}"]`)]));
      entry.tiles = [...tiles.values()];
      const wired = new Map();
      const screens = {};
      for (const { id, size } of scene.machines) {
        const tile = tiles.get(id);
        const hand = wire(tile, id, size, (family, op, payload) => host.act(scene.id, id, family, op, payload));
        wired.set(id, hand);
        screens[id] = hand.canvas;
      }
      wiring.set(scene.id, wired);
      // Let the browser paint the overlay's spinner before the world is built, which in
      // the tab holds the thread for a second or two and in a worker holds nothing.
      await new Promise(resolve => requestAnimationFrame(resolve));
      await host.start(scene, screens, entry.spare);
      // Not before it is painted: an empty canvas swapped in over the still is a machine
      // that blinks out and back, and the still is a real frame of the same machine.
      for (const { id } of scene.machines) {
        show(tiles.get(id), wired.get(id).canvas);
        tiles.get(id).classList.add('live');
      }
      return true;
    })().catch(error => {
      // The stills are real renders of the same machines, so a scene that will not come up
      // is left as the pictures it was, and the slideshow is not told to wait for it again.
      console.warn(scene.id, error);
      running.delete(scene.id);
      for (const wired of wiring.get(scene.id)?.values() ?? []) wired.stop();
      wiring.delete(scene.id);
      return false;
    });
    running.set(scene.id, entry);
    retire();
    return entry.booting;
  }

  /** The font pack into every worker that is holding a scene, spare or not. A worker only
   * fetches the pack for a machine somebody is looking at, which leaves a neighbour warmed
   * in the background drawing boxes where the emoji and the CJK belong; this is how those
   * get filled in before the visitor arrows onto them. app.js calls it once the page has
   * been quiet for a moment, because it is 20 MB a worker and none of it is urgent. */
  start.prewarm = () => host.prewarm();

  return start;
}
