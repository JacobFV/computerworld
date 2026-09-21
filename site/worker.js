// A worker with machines in it. It holds an instance of the engine, the world definition
// every scene is cut from, and however many scenes the page has given it; the page keeps
// the canvases' placeholders and sends the events, and nothing that costs real time —
// building a world, running an opening, rendering a screen — happens on its thread.
//
// Scenes are addressed by id rather than passed over, because a scene is code: `open()`
// and `sync()` are written in site/scenes/<id>.js and cannot cross a postMessage. The
// worker imports the same cast the page does and looks them up.
import { cast } from './cast.js';
import { begin, run, installFontPack } from './engine.js';

let definition = null;
const running = new Map();   // scene id → the handle engine.js gave back
let fonts = null;            // the font pack, fetched once this worker has something to draw

const tell = (kind, rest) => self.postMessage({ kind, ...rest });

self.onmessage = ({ data }) => {
  const { kind, id } = data;
  if (kind === 'boot') {
    begin(data.module);
    // The definition arrives as the text it was downloaded as, not as an object: one
    // structured clone of 7.7 MB per worker is cheaper than the graph, and the parse
    // happens here rather than on the page's thread.
    definition = JSON.parse(data.world);
    tell('ready');
    return;
  }
  if (kind === 'start') {
    const scene = cast.find(s => s.id === id);
    if (!scene) return tell('failed', { id, why: `no scene ${id} in the cast` });
    try {
      running.set(id, run(definition, scene, data.screens));
    } catch (error) {
      return tell('failed', { id, why: String(error) });
    }
    tell('painted', { id });
    // Only now, with something on screen to improve: a worker holding no machines has no
    // reason to parse twenty megabytes of fonts.
    // Said either way. The pack failing is a page with boxes where some glyphs belong,
    // which is survivable; a page still waiting to be told is not, and the stills renderer
    // waits on exactly this before it saves a screen.
    fonts ??= installFontPack(file => tell('font', { file }))
      .catch(error => console.warn('font pack', error))
      .then(() => tell('fonts'));
    return;
  }
  const scene = running.get(id);
  if (!scene) {
    // A scene retired between the page asking and this arriving. Anything the page is
    // waiting on is still answered, or it waits for ever.
    if (kind === 'frame') tell('frame', { id, machine: data.machine, token: data.token });
    return;
  }
  if (kind === 'act') {
    const cursor = scene.act(data.machine, data.family, data.op, data.payload);
    if (cursor) tell('cursor', { id, machine: data.machine, cursor });
  } else if (kind === 'frame') {
    const frame = scene.frame(data.machine);
    if (!frame) return tell('frame', { id, machine: data.machine, token: data.token });
    self.postMessage({ kind: 'frame', id, machine: data.machine, token: data.token, ...frame }, [frame.rgba]);
  } else if (kind === 'redraw') {
    scene.redraw();
  } else if (kind === 'stop') {
    // The screens keep whatever they were last painted with: an OffscreenCanvas nobody
    // draws to any more goes on showing its last frame, which is the still the page wants
    // behind a machine it has stopped.
    scene.stop();
    running.delete(id);
  }
};
