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

Record trajectories for explanation and the input sequence for replay. Event
records include sequence, tick, kind, optional machine/actor and structured data.
Use state hashes to compare reconstructed suffixes. A hash mismatch is diagnostic
failure, not something to hide by dropping disagreeing event fields.

The renderer uses fixed scene geometry, bundled font data and explicit raster
inputs. Host font lookup, wall-time animations and browser layout are not part of
canonical output. Pixel parity tests cover supported scenes; this is not an
arbitrary HTML/CSS rendering guarantee.
