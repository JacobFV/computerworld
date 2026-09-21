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

// THE NUMBER BELOW IS ONE HALF OF A MATCHED PAIR. live.js writes the same literal out
// again, and the two are bumped together whenever the shapes of the messages here change.
// Neither imports it from the other or from a third file: a shared module would be cached
// on its own timetable, and both sides would go on agreeing about a number neither of them
// had just fetched. This one is said in `ready`, and a page that hears a number other than
// its own runs the machines in the tab instead of talking to a worker it cannot understand.
const PROTOCOL = 1;

let definition = null;
const running = new Map();   // scene id → the handle engine.js gave back
let fonts = null;            // the font pack, fetched once this worker is told to

const tell = (kind, rest) => self.postMessage({ kind, ...rest });

/** The CJK and emoji faces, fetched and parsed once. Said either way. The pack failing is
 * a page with boxes where some glyphs belong, which is survivable; a page still waiting to
 * be told is not, and the stills renderer waits on exactly this before it saves a screen. */
const warm = () => {
  fonts ??= installFontPack(file => tell('font', { file }))
    .catch(error => console.warn('font pack', error))
    .then(() => tell('fonts'));
};

self.onmessage = ({ data }) => {
  const { kind, id } = data;
  if (kind === 'boot') {
    begin(data.module);
    // The definition arrives as the text it was downloaded as, not as an object: one
    // structured clone of 7.7 MB per worker is cheaper than the graph, and the parse
    // happens here rather than on the page's thread.
    definition = JSON.parse(data.world);
    tell('ready', { protocol: PROTOCOL });
    return;
  }
  // A worker does not go after the pack on its own account any more: the page says when,
  // because the page is the one that knows whether anybody is looking at what this worker
  // is drawing. It says so straight after `start` for a scene in the middle of the strip,
  // and once the page has gone quiet for a scene pre-booted at its edge — so that arrowing
  // onto a neighbour is not watching its emoji turn from boxes into glyphs.
  if (kind === 'warm') return warm();
  if (kind === 'start') {
    const scene = cast.find(s => s.id === id);
    if (!scene) return tell('failed', { id, why: `no scene ${id} in the cast` });
    try {
      running.set(id, run(definition, scene, data.screens));
    } catch (error) {
      return tell('failed', { id, why: String(error) });
    }
    tell('painted', { id });
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
