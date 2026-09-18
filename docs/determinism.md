# Determinism and checkpoints

Reproduction requires the same engine, registered module versions, world
definition, seed and ordered action sequence. Replay is a semantic guarantee for
supported operations, not a promise that arbitrary future engine versions will
interpret old checkpoints identically.

Time is unsigned logical microseconds. `Clock` rejects backward movement and
checked overflow. Scheduled work is ordered by due time, phase and insertion
sequence. Neither executing a command nor observing a frame consults wall time.

`Determinism` implements `sha256-named-splitmix64-v1`: each named random stream is
derived from the seed and stream name. Using an unrelated stream does not perturb
another subsystem. ID counters are namespaced, deterministic and checked for
overflow. Streams, IDs and queued work are serializable state. Rust extensions
must use the supplied deterministic context instead of host clocks or RNGs.

Snapshots retain simulation and environment state; the facade owns the complete
checkpoint boundary, including browser/application sessions. In-memory snapshots
and forks share immutable backing with copy-on-write roots where implemented.
Portable JSON export/import traverses state and is a different, more expensive
operation. Code registrations remain external and must match the checkpoint's
module identities. A checkpoint with unresolved live host effects is rejected on
restore; synthetic queued work is serializable. Inspecting or evaluating should not consume random values or
advance logical time.

Applications do work between actions only in steps: after the actions of every
`step()`, each native application with background work (a video export) advances by one
bounded unit (`NativeApp::background`), so how far it has got is a function of the steps
taken and replays exactly. Media playback reads the world clock: a playing timeline's
position is derived from the logical time playback started and the time now, never from
the host (see [video-editing.md](video-editing.md)).

Record trajectories for explanation and the input sequence for replay. Event
records include sequence, tick, kind, optional machine/actor and structured data.
Use state hashes to compare reconstructed suffixes. A hash mismatch is diagnostic
failure, not something to hide by dropping disagreeing event fields.

The renderer uses fixed scene geometry, bundled font data and explicit raster
inputs. Host font lookup, wall-time animations and browser layout are not part of
canonical output. Pixel parity tests cover supported scenes; this is not an
arbitrary HTML/CSS rendering guarantee.

## Rendered frames as labelled data

Because a frame is an exact function of (engine, world, seed, action sequence,
viewport), the text an agent types is also a *label* for the pixels that text
produces. Replay a recorded episode, render at whatever viewport you want, and you
have glyph-accurate ground truth for every string on screen — for free, at whatever
volume you are willing to render, with no annotation pass and no labelling error.

The scene is the index. `scene(width, height)` gives each text node's string,
bounds and transform before rasterization, so a `(crop, string)` pair falls out of
walking the scene and cropping the frame from `render(width, height)` at the same
viewport. Varying the theme, viewport and bundled typeface varies the rendering of
identical text, which is the axis that matters for OCR generalization.

One consumer used this to train an OCR model on its own agent's typed text and
measured held-out-font accuracy rising from 0.668 to 0.794.

This only holds within one engine version: pixel output is not stable across
releases (the alpha shell work changed it), so keep the version with the data.
