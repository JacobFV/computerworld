# Video editing

Every graphical machine has a video editor, each the platform's own and each an
interface over one deterministic engine, `crates/video` (`cw-video`):

| Platform | Application (kind) | Media folder |
|---|---|---|
| Windows 11 | Clipchamp (`clipchamp`) | `~/Videos` |
| macOS | iMovie (`imovie`) | `~/Movies` |
| Ubuntu | Kdenlive (`kdenlive`) | `~/Videos` |
| iOS | iMovie (`imovie`, phone layout) | `~/Movies` |
| Android | Video Editor (`videoeditor`) | `~/Movies` |

The interface code is `crates/applications/src/apps/video/`; its controls are listed in
[action-families.md](action-families.md#video-editor-controls).

## Media formats

Everything the editors read and write is a public format another program opens.

**Movies are Animated PNG** (APNG, the `acTL`/`fcTL`/`fdAT` extension registered in PNG
Third Edition). The decoder (`cw_video::apng`) walks the chunks itself, checks every CRC,
decodes each frame's compressed data with the `png` crate as a standalone image and
composes frames with the file's own `dispose_op` and `blend_op`, so any conforming APNG
plays as its author made it; frame durations accumulate as exact fractions, so a movie
at 12 frames a second lasts exactly its frame count divided by 12. The encoder writes
every frame at full size with `APNG_BLEND_OP_SOURCE` and a constant delay of
`1/fps`, 8-bit RGBA, looping forever. A browser plays an exported movie; a plain PNG
viewer shows its first frame.

**Sound is RIFF WAVE with linear PCM.** Reading accepts `WAVE_FORMAT_PCM` (or
`WAVE_FORMAT_EXTENSIBLE` carrying PCM) of 8, 16, 24 or 32 bits in any number of channels
and reduces it to mono 16-bit, averaging the channels; it is resampled to the project
rate (22 050 Hz) by exact integer linear interpolation. Writing produces canonical
16-bit mono PCM with a 44-byte header.

**Stills** are PNG (any colour type) and baseline or progressive JPEG (decoded with the
renderer's scalar `jpeg-decoder` configuration). A still lasts as long as its clip.

Imported media is kept in the editor's state, so a snapshot carries it and a restored
world shows the same frames: movie frames as QOI images (`cw_video::qoi`, the Quite OK
Image format 1.0 — lossless and decodable in one pass, so the monitor can pull any frame
at random), audio as little-endian PCM, both serialized as base64 behind shared
reference counts. Import also stores up to eight 36-pixel-high thumbnails and a waveform
of one peak per hundredth of a second, so a timeline draws without decoding anything.
Pictures larger than 1280 pixels on the long edge are reduced on import, as an editor's
optimized copy is; the file on disk is untouched.

## The sample media

Each machine's media folder is seeded (by `scripts/build-live-world.mjs`) with five files
the engine generates deterministically in `cw_video::samples` and checks in under
`worlds/company-2026/samples`: `Countdown.apng` (a 3 s film-leader countdown), `Color
Bars.apng` (2 s of 75% bars with a moving marker), `Sunset.apng` (3 s), `Countdown
Beeps.wav` (a beep each second) and `Music Bed.wav` (4 s of chords), all 320×180 at
12 fps and 11 025 Hz mono. `crates/video/tests/samples.rs` fails if a checked-in file is
not byte-for-byte what the generators write; regenerate with
`CW_UPDATE_SAMPLES=1 cargo test -p cw-video --test samples`.

## Project format

A project is saved as `<name>.cwvideo` in the media folder: UTF-8 JSON, the serialization
of `cw_video::Project`. Media is referenced by path, not embedded; opening a project
reads every referenced file again and marks the ones that are gone as missing (their
clips paint red and the monitor says "Missing media").

```json
{
  "format": "computerworld-video-project",
  "version": 1,
  "name": "Trailer",
  "width": 320, "height": 180, "fps": 24, "sample_rate": 22050,
  "background": "000000",
  "media": [{"id": 3, "path": "Movies/Countdown.apng", "kind": "video"}],
  "tracks": [{"id": 1, "kind": "video", "name": "V1", "muted": false, "hidden": false, "locked": false}],
  "clips": [{
    "id": 7, "track": 1, "source": {"type": "media", "media": 3},
    "start": 0, "length": 72, "offset": 0, "speed": 100, "reverse": false,
    "opacity": {"value": 1000, "keys": [{"frame": 0, "value": 0, "ease": "ease"}, {"frame": 23, "value": 1000}]},
    "x": {"value": 0}, "y": {"value": 0}, "scale": {"value": 1000}, "rotation": {"value": 0},
    "volume": {"value": 100},
    "crop": {"left": 0, "top": 0, "right": 0, "bottom": 0},
    "grade": {"brightness": 0, "contrast": 0, "saturation": 0, "temperature": 0},
    "fade_in": 0, "fade_out": 12, "name": "Countdown.apng"
  }],
  "transitions": [{"id": 9, "left": 7, "right": 8, "kind": "cross_dissolve", "frames": 24}],
  "next_id": 10
}
```

- **Time.** Timeline positions (`start`, `length`, keyframe `frame`s, fades, transition
  `frames`) are whole frames at `fps`. A clip's place in its source, `offset`, is in
  *centiframes* — hundredths of a timeline frame — so at any speed from 25% to 400% a
  clip advances a whole number of them per frame, and every trim, split and speed change
  is exact. The source shown at clip-local frame `l` is `offset + l × speed`, or
  `offset + (length − 1 − l) × speed` reversed.
- **Sources** are `{"type": "media", "media": <id>}`, `{"type": "color", "color":
  "rrggbb"}` or a title `{"type": "title", "text", "size", "bold", "color",
  "background", "position": "top|center|lower|bottom"}`.
- **Properties.** `opacity` and `scale` are thousandths (scale is of the size that fits
  the frame), `x`/`y` canvas pixels from the centre, `rotation` hundredths of a degree
  clockwise, `volume` percent (0–200). Each is a `Param`: a `value`, or `keys` with
  `linear` or `ease` (smoothstep) interpolation from each key to the next, in 16.16
  fixed point. Crop is thousandths from each edge; the grade values are −100…100.
- **Tracks** composite bottom-up: the first video track is the bottom layer. Clips on a
  track never overlap; the loader refuses a file where they do, and any file whose ids,
  ranges or keyframes are inconsistent.
- **Transitions** sit on a cut between two clips that touch, centred on it: the outgoing
  clip plays on past its end and the incoming one starts early, for half the duration
  each (the nearest frame holds where the media runs out).

## Rendering

`cw_video::Compositor::frame(f)` is the only way a picture is made: the program monitor
draws it and export encodes it, so the preview is the export. Per clip, the source frame
is cropped, colour-corrected with the raster engine's adjustments (brightness/contrast,
saturation via hue/saturation, temperature), fitted to the canvas, scaled, rotated and
positioned by `cw_raster::transform::affine` (bilinear on premultiplied colour, 16.16
fixed point; an unscaled upright clip is copied pixel for pixel), and its opacity and
fades applied. Tracks composite with source-over. Transitions mix the two clips'
layers: cross dissolve in premultiplied colour, dip to black or white through the solid
at the midpoint, a wipe revealing the incoming clip from the left, a slide bringing it
in from the right. Titles are drawn from glyph coverage the renderer rasterises in the
platform's own font (`AppEffect::RasterText`), so a title is exactly the renderer's text;
a build without the renderer says so on the monitor rather than showing a blank.

Audio (`cw_video::audio`) sums every unmuted audio track sample for sample: a clip's
samples are read at its speed (faster raises the pitch, as tape would), backwards when
reversed, scaled by its volume keyframes and its fades, and clipped to 16 bits.

All arithmetic is integer or the raster engine's correctly rounded IEEE basics, so a
native and a Wasm build produce identical frames, samples and files.

## Playback and export

Playback is driven by the world clock, never the host's: pressing play records the
clock and the frame; the playhead at any later moment is `from + elapsed × fps × rate`,
clamped to the programme, and it stops at either end. Time passes when the world's does
— a `sleep` in a shell, network activity — so `sleep 1.5` after Space plays 36 frames
at 24 fps. J/K/L change the rate in steps of 1×, 2× and 4× either way.

Export writes `<name>.apng` and `<name>.wav` to the media folder at 90p, 180p or 360p and
12, 24 or 30 fps. It is encoded between actions: after every environment step, each
application with background work (`NativeApp::busy`) gets one step
(`NativeApp::background`), in which an export encodes up to four 320×180 frames' worth
of pixels. Progress is the count of frames really encoded, so it is a function of steps
taken and replays exactly. When the last frame is done the files are written; a
re-imported export holds exactly the frames that were composited.

## Feature matrix

What each interface shows (the engine supports all of it; a product only offers what the
real application does, and refuses the rest):

| Feature | Clipchamp | iMovie (Mac) | Kdenlive | iMovie (iOS) | Android |
|---|---|---|---|---|---|
| Import from the machine, media bin | ✓ | ✓ | ✓ (Project Bin) | ✓ (+ adds at playhead) | ✓ (+ adds at playhead) |
| Drag clips onto a multi-track timeline | ✓ | ✓ | ✓ | ✓ | ✓ |
| Move, trim handles, split, delete | ✓ | ✓ | ✓ | ✓ | ✓ |
| Delete closes the gap (magnetic) | — | ✓ | `Shift+Delete` | ✓ | ✓ |
| Snap, zoom | ✓ | ✓ | ✓ | zoom | zoom |
| Track mute / hide / lock | mute, hide | — | all three | — | — |
| J/K/L shuttle, frame step | step | ✓ | ✓ | — | — |
| Opacity, position, scale, rotation | ✓ | PiP, crop modes | ✓ | — | scale, x, opacity |
| Keyframes (linear / ease) | — | Ken Burns (ease) | ✓ | — | ✓ |
| Crop | ✓ | ✓ | ✓ | — | — |
| Speed 0.25×–4× | ✓ | ✓ | ✓ | ✓ | ✓ |
| Reverse | — | ✓ | ✓ | — | ✓ |
| Fade in/out | ✓ | ✓ | ✓ | ✓ | ✓ |
| Transitions | 5 | 5 | 5 (Compositions) | 4 | 5 |
| Colour correction | ✓ (Adjust colors) | ✓ | ✓ | — | ✓ (Adjust) |
| Titles with style and position | ✓ | ✓ | ✓ | ✓ | ✓ |
| Picture in picture, background colours | ✓ | ✓ | ✓ | backgrounds | ✓ (Overlay) |
| Volume, audio fades, waveform, mixdown | ✓ | ✓ | ✓ | ✓ | ✓ |
| Export APNG + WAV (progress, cancel) | Export | Share | Render | Share | Export |
| Undo/redo, save/open project | ✓ | ✓ | ✓ | ✓ | ✓ |

## Known limits

- Clips carry either pictures or sound: an APNG has no audio track, so a movie's sound
  is a separate WAV clip, not linked audio.
- Audio is mono; there is no pan, and speed changes the pitch (no time-stretching).
- The canvas is 320×180 at 24 fps; exports scale it to the chosen size.
- iMovie's volume keyframes, Clipchamp's filters and effects, and Kdenlive's full effect
  catalogue are not reproduced; each product shows only what is listed above.
