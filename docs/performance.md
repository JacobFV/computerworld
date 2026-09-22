# Performance

> **Partly re-measured.** The native world table below was re-run on the current tree on
> a quiet host and is current. **Every other section is still stale**: the raster,
> binding, browser and predecessor numbers were captured before the desktop shell and
> renderer overhaul, and before the reference world grew from 16.7 KB to 432 KB of
> definition. Re-run `benchmarks/run-all.sh` on a quiet host before quoting those. The
> methodology and the negative results are accurate throughout and are the reason to
> keep the document.
>
> What the growth cost, measured rather than guessed: portable snapshot decode went from
> 0.142 ms to 4.90 ms and fork from 16.5 µs to 265 µs, both roughly tracking the 26x
> larger world definition. Everything that does not touch the whole world — a terminal
> step, a file write, an editor keystroke, a reset — is unchanged.
>
> **The two fork rows below are now stale in the other direction, and by more.** Fork
> cost tracked the definition because it validated it — twice, once in `Runtime::fork`
> and once in the `restore` the fork performed on itself. It does not any more: a
> checkpoint whose definition is one the runtime already holds is not re-walked, and the
> environment's fork validates once. Measured on the current tree on the 3.7 MB
> company-2026 world (which is 8.7x the 432 KB world the table below was taken on), a
> fork of a freshly booted world is **15.9 µs**, and of one driven through 4,000 actor
> steps **116.4 µs**. What a populated step costs fell with it, for an unrelated reason
> in the same measurement pass: **176.2 ms → 219.2 µs** at 16,013 accumulated events.
> [`benchmarks/fork-throughput.md`](../benchmarks/fork-throughput.md) has the method, the
> three-run spreads, the cause of each and the evidence that no hash, event log,
> `inspect()` or exported snapshot byte moved. The two binding fork rows further down
> were measured on the old engine and the small world and have not been re-run at all.

Measured on September 17, 2026, Linux ARM64, Cortex-X925 CPU 19 (3.9 GHz
maximum), Rust 1.97.1 release builds and Chrome 151. Native actor terminal steps
cost **3.84 µs p50**, dirty resets **1.52 µs**, and warm 1280×720 frames **0.522 ms**.

The renderer optimization improved native full-frame p50 **3.11×** and
100% patch rendering **7.21×**. Against a Chromium DOM, the format-matched custom
PNG capture pipeline achieved **2.74× p50 / 2.29× p95** improvement. The proposed
3× predecessor capture target was **not met**. The same Chromium screenshot API
on both representations was slower for canvas in this run. We have not established
a general renderer-only speedup over Chromium.

[Raw samples and summary](../benchmarks/results/summary.json),
[final machine and artifact hashes](../benchmarks/results/machine.json),
[initial renderer/native baseline hashes](../benchmarks/results/machine-baseline.json), and
[commands](../benchmarks/README.md) accompany the report. Final refreshed bindings
are measured, including the cached reset and exact integer conversion changes.
The final device-lifecycle runtime is included; no viewer or page around the engine is
a measured workload. These are results on one host, not universal
hardware guarantees.

## Method

Persistent release runtimes, five runs, 100 warmups and 1,000 individual samples
per short workload. Raster loops use 200 samples/run, portable snapshot encoding
and decoding 200, and browser PNG captures 30 samples/run with three warmups.
Tables use the median of the five per-run p50 and p95 values. Raw samples and
per-run dispersion remain in `benchmarks/results/`; throughput uses the arithmetic
mean latency within a run. This does not treat batch averages as operation p95.

Native runs are pinned to CPU 19 on this ARM64 host. Machine/compiler/browser,
world, lockfile and font/bundle hashes are recorded in `results/machine.json`.
`machine-baseline.json` retains the earlier measurement metadata for the native
renderer before/after runs; the final lifecycle-runtime runs refresh native world,
capacity, Node, Python and browser results. `native-dirty-reset.json` is the initial
standalone reset experiment; the report uses the newer `native-world.json` row.
Initialization, boundary conversion, scene construction, rasterization and PNG
capture are distinct workloads. The native world runner measures the full actor
boundary including outcome construction, observation projection and journaling.
Neither structured observations nor scene generation requests rasterization.

## Native world results

All values are **microseconds**, and are the median across five independent runs of
each workload's own p50 and p95. Each short row has 5,000 measured operations;
portable snapshot encode/decode have 1,000. These use the rich company world and
full journaling. The initial snapshot is held during each workload. File, pipe
and HTTP results are asserted to prevent no-op success from counting as work.

The first run of each workload is excluded, because it pays one-time cache
population the steady state does not. That only matters in one row: virtual HTTP
costs **148 µs** on its first request against 83 µs afterwards, which is the DNS
resolver cache filling. A consumer measuring a single cold request will see the
larger figure, and that is the honest number for one-shot work.

| Workload | p50 | p95 |
|---|---:|---:|
| Terminal `pwd`, full actor step | 4.776 | 5.128 |
| Terminal parse + pipe | 5.224 | 5.568 |
| File write + read, two actor actions | 7.536 | 7.904 |
| Virtual HTTP, full actor step | 82.856 | 87.441 |
| Synthetic browser navigation through network | 159.672 | 168.657 |
| Editor type + backspace, two actions | 6.536 | 7.056 |
| Structured actor observation | 34.368 | 35.112 |
| Desktop scene, no raster | 93.192 | 95.184 |
| Already-clean same-seed reset | 0.560 | 0.584 |
| Dirty same-seed reset | 4.544 | 4.728 |
| Snapshot handle | 0.976 | 1.040 |
| Fork from initial snapshot (**superseded**, see the note at the top) | 264.921 | 271.161 |
| Fork + first file mutation (**superseded**) | 290.457 | 296.185 |
| Portable initial snapshot encode | 962.083 | 1005.259 |
| Portable initial snapshot decode | 4895.680 | 5464.466 |

A dirty same-seed reset is **1.520 µs p50 / 1.568 µs p95**. Every iteration mutates a file before the timer,
then resets inside the timer and verifies the file disappeared after timing.
The 0.144 µs clean-reset row intentionally measures an already-reset world;
it should not be presented as the cost of discarding a populated episode.
Snapshot encode/decode rows use the initialized snapshot; populated trajectory
size naturally increases portable serialization cost. Fork + first write includes
both operations rather than claiming an isolated mutation latency. The two fork rows
predate the fix described at the top of this document and are kept only as the before
column; `benchmarks/fork-throughput.md` is the current measurement, and it is on a
larger world than this table.

## Native renderer results

A 100-text-node fixture, integer layout and the bundled DejaVuSansMono font.
Full render includes returning/dropping an owned RGBA copy. Incremental render
returns the retained framebuffer; each before/after pair uses the identical API.
Values are **microseconds**, with 1,000 samples per warmed raster row.

| Workload | Before p50 | After p50 | Before p95 | After p95 | Median improvement |
|---|---:|---:|---:|---:|---:|
| Full 640×480 | 772.59 | 354.51 | 781.36 | 368.02 | 2.18× |
| Full 1280×720 | 1623.70 | 522.06 | 1715.22 | 559.39 | 3.11× |
| Full 1920×1080 | 2586.03 | 792.43 | 2781.28 | 897.39 | 3.26× |
| Patch + raster 1% | 40.59 | 18.16 | 42.26 | 18.91 | 2.24× |
| Patch + raster 10% | 310.70 | 53.65 | 319.49 | 56.18 | 5.79× |
| Patch + raster 100% | 3075.46 | 426.45 | 3124.79 | 444.91 | 7.21× |

Cold renderer construction plus a 1280×720 frame costs **10.389 ms p50 /
10.739 ms p95** (150 samples). This includes parsing the bundled font; reuse a
persistent renderer for repeated frames. Scene construction is 10.560 µs,
hit testing is 1.120 µs, and scene-only 1%/10%/100% patches are
8.320 / 10.624 / 20.096 µs p50. None of those structured workloads rasterizes.

## Renderer profiling and optimization

Linux `perf` is blocked by `perf_event_paranoid=4`; we did not change host policy.
Temporary instrumentation on a disposable renderer copy measured sorting,
clearing, text preparation and node raster stages. The source of that instrumentation
and its results are included. Before optimization, node raster consumed about 95%
of full-frame wall time. Identical old/new patch damage also painted the same area
twice, making a 100% incremental update slower than full rendering.

The resulting optimization caches nonzero text spans, specializes identity text
and opaque rectangle drawing, and coalesces redundant damage. Renderer semantic
contracts and golden frames remain unchanged. Cache limits account for retained
span storage. Authoritative before/after p50/p95 use uninstrumented binaries.

## Chromium comparisons

Three distinct measures avoid misleading ratios:

- DOM mutation plus forced layout: no claim that this includes rasterization.
- Wasm patch, software raster and RGBA copy: no ratio against DOM layout alone.
- PNG delivered to the automation caller: both paths produce PNG, including
  encoding and transfer. We measure both the same Chrome screenshot API on DOM
  and canvas and the custom canvas export path (`toDataURL`) versus DOM screenshot.
  The latter is capture-pipeline savings, not isolated renderer speedup.

The primitive fixture uses identical content, dimensions and bundled font source;
Chrome and fontdue differ in glyph antialiasing. The real predecessor mail fixture
is `synthux-mail-mock`'s inbox, normalized to that font. An additional migrated
scene captures its per-character text geometry and background boxes. It omits CSS
rounded corners, shadows and emoji fallback, so it is an approximate workload,
not a pixel-identical HTML compatibility or matched-render speedup result. Review
images are included beside the raw results.

## Browser output and predecessor baseline

The next table is **milliseconds**, with 150 captures per row. Inputs and outputs
are the same 1280×720 primitive/text fixture; both final outputs are PNG returned
to the Node caller. The custom incremental path combines update, raster, canvas
upload, PNG encoding and transfer into one RPC. The DOM path needs the browser's
screenshot API. These are end-to-end capture costs, including their required
transport, not isolated compositor benchmarks.

| Workload | p50 | p95 |
|---|---:|---:|
| DOM update + Chrome PNG capture | 35.004 | 52.579 |
| Wasm full update + same Chrome PNG capture | 44.671 | 53.737 |
| Wasm full update + canvas PNG export, two RPCs | 15.195 | 26.865 |
| Wasm incremental update + PNG export, one RPC | 12.786 | 22.980 |

DOM mutation plus forced layout costs 5 / 40 / 355 µs p50 for 1% / 10% / 100%
changes. Wasm patch + raster + full RGBA boundary copy costs 1.675 / 1.800 / 2.560 ms.
Those are different operations and are **not divided into a speedup ratio**.
Browser timers are quantized; the smallest DOM result is near timer resolution.
The RGBA-copy and canvas/export boundary is a remaining optimization target.

The real predecessor mail visual fixture captures in 46.434 / 63.092 ms p50/p95.
Its migrated geometry reconstruction captures in 50.915 / 74.317 ms. The latter
uses per-character nodes to preserve predecessor positions, omits decorations and
emoji fallback, and is not a faithful same-pixel screen. It does **not** establish
a renderer speedup. Reviewed [DOM](../benchmarks/results/predecessor-mail.png) and
[reconstructed scene](../benchmarks/results/migrated-mail.png) images are retained.
These are visual-state fixtures, not a service mutation end-to-end benchmark.

Persistent SCE runtime operational baselines (microseconds):

| Workload | p50 | p95 |
|---|---:|---:|
| Terminal pwd | 9.408 | 11.216 |
| Terminal pipe | 18.816 | 23.008 |
| Shell file write + read | 150.337 | 212.337 |
| Virtual HTTP | 45.280 | 52.064 |

SCE's host-backed VFS, reference topology, shell projection and tracing differ from
Computerworld. These are useful operational reference points, not isolated Rust
versus JavaScript language comparisons. SCE initialization is recorded separately
in its raw result. No per-step subprocess is used in either benchmark.

## Bindings and memory

Binding values are **microseconds**. Node executes the generated Wasm module;
Python executes the installed PyO3 wheel. These terminal-only configurations have
a smaller observation projection than the native desktop fixture, so cross-table
ratios would not isolate binding overhead.

| Workload | p50 | p95 |
|---|---:|---:|
| Node Wasm terminal step | 13.408 | 42.817 |
| Node Wasm ten-action batch | 67.425 | 77.184 |
| Node Wasm observe | 1.472 | 3.696 |
| Node Wasm already-clean reset | 0.352 | 0.384 |
| Node Wasm snapshot handle | 0.960 | 1.233 |
| Node Wasm fork | 26.352 | 28.752 |

| Workload | p50 | p95 |
|---|---:|---:|
| Python terminal step | 7.936 | 8.480 |
| Python ten-action batch | 42.849 | 59.136 |
| Python observe | 1.280 | 1.360 |
| Python already-clean reset | 0.256 | 0.288 |
| Python snapshot handle | 0.672 | 0.720 |
| Python fork | 16.896 | 17.296 |

Actual Chrome Wasm terminal execution is **15 µs p50 / 35 µs p95** over 5,000
steps. One cold Node module load/instantiation took **16.600 ms**; this is one
observation, not a p50. Browser world construction warmed from 25.705 ms on the
first run to 1.020–3.330 ms on later runs; five points do not establish a robust
cold-start percentile.

| Packaged artifact | Raw bytes | Gzip bytes |
|---|---:|---:|
| Browser Wasm, standard services + renderer/fonts | 19,727,850 | 8,378,969 |
| Browser JS glue | 44,560 | 7,051 |
| Font pack, 20 files (fetched on demand) | 23,363,740 | 12,961,365 |

Measured with `gzip -9` on a `pkg/web` built by `scripts/build-wasm.sh` from the current
source. The fonts, wallpapers, icons and symbol masks are embedded in the Wasm, except
the font pack in `pkg/web/fonts/`, whose files a page fetches only if it wants their
glyphs: Han, kana and Hangul (and their bold and regional forms), emoji (monochrome
and colour) and eight further scripts (see "Complex scripts" and "Italic, colour
emoji, CJK forms and more scripts" below).

**Complex scripts, before and after.** Hebrew, Arabic, Thai, Devanagari, CJK and emoji
support (fallback faces, bidi, `rustybuzz` shaping, CJK line breaking) grew the module
by 9.2% gzipped. Both builds below are `scripts/build-wasm.sh` with the same toolchain
(Rust 1.97.1, wasm-bindgen 0.2.128), the "before" from the release commit `6adc175`:

| Module | Raw bytes | Gzip bytes |
|---|---:|---:|
| Before (`6adc175`) | 10,129,130 | 4,949,209 |
| After | 11,355,891 | 5,403,366 |
| Change | +1,226,761 (+12.1%) | +454,157 (+9.2%) |

Where the growth went: 717,496 raw / about 259,000 gzip is font data — Noto Sans
Hebrew, Arabic, Thai and Devanagari in two weights (512,860 raw; Devanagari alone is
328,276, its conjunct forms) and the outline-free *stubs* of the three pack faces
(204,636 raw, 44,055 gzip) that let layout shape CJK and emoji without the pack. The rest,
about 508,000 raw / 194,000 gzip, is code and Unicode tables: `rustybuzz` (179 KB of
code), the extra `ttf-parser` tables it reads (68 KB), `unicode-bidi` (28 KB) and
their property data. The pack itself — Noto Sans SC subset to 10,269 common Han
(2,190,576 gzip), all 11,172 Hangul syllables in Noto Sans KR (855,949) and Noto Emoji
(566,769) — would have taken the module to about 9 MB gzipped; kept separate, it costs
nothing unless used. Native builds embed it.

**Italic, colour emoji, CJK forms and more scripts, before and after.** Italic and
bold-italic faces, the COLRv1 colour emoji renderer, CJK bold and Traditional
Chinese/Japanese/Korean glyph forms, terminal shaping and bidi, and Georgian, Armenian,
Bengali (embedded) plus Tamil, Gurmukhi, Lao, Khmer, Gujarati, Ethiopic, Myanmar and
Sinhala (pack) grew the module by 6.9% gzipped. Both builds are `scripts/build-wasm.sh`
with the same toolchain (Rust 1.97.1, wasm-bindgen 0.2.128), the "before" from
`7aeec57`:

| Module | Raw bytes | Gzip bytes |
|---|---:|---:|
| Before (`7aeec57`) | 18,634,805 | 7,835,005 |
| After | 19,727,850 | 8,378,969 |
| Change | +1,093,045 (+5.9%) | +543,964 (+6.9%) |

Where it went (standalone `gzip -9` of each file): the eight platform italics
(210,644 raw / 138,207 gzip) and DejaVu's two obliques (209,004 / 127,429); Noto Sans
Georgian, Armenian and Bengali in two weights (297,068 / 159,808); and eleven new
layout stubs (238,476 / 90,320) — for the locale Han faces, whose bold twins share
them, and for the eight pack scripts. That is 955,192 raw of font data; the other
~138,000 raw is code and tables: the COLR painter (`src/colr.rs`), the terminal cell
layout, the italic advance table, a 2.6 KB Traditional-only Han bitset and the East
Asian Width ranges. Tamil, Gurmukhi, Lao and Khmer were first embedded too, which cost
128 KB gzip of module; they moved to the pack, where their stubs cost 17 KB.

The pack grew from 3 files (6,751,696 raw / 3,613,294 gzip) to 20 (23,363,740 /
12,961,365). The largest additions are Noto Color Emoji (4,991,984 / 2,825,172; its
CBDT bitmap build would be 10.7 MB), Noto Sans SC bold (3,518,020 / 2,219,356) and KR
bold (2,365,908 / 872,952). The locale faces hold only glyphs drawn differently from
Noto Sans SC within each locale's common set: 2,573 for Traditional Chinese (624,023
gzip), 1,772 for Japanese (355,927) and 2,832 for Korean hanja (608,669), each again in
bold. None of it is fetched unless a page draws those glyphs; the project site waits
at boot only for regular SC and KR and the two emoji faces (6.4 MB gzip, of which the
colour emoji are 2.8 MB) and fetches the rest behind them.

The web faces (Milestone 0a of `docs/contracts/web-engine-plan.md`) — Arimo, Tinos, Cousine,
Gelasio, Carlito, Caladea, Lato, Source Sans 3, Source Serif 4, Poppins, Montserrat,
Playfair Display and JetBrains Mono, four faces each, subset to DejaVu's coverage —
are embedded in the module, since a page's body text cannot wait for a fetch the way
a CJK glyph can. `scripts/build-wasm.sh` before and after, same toolchain:

| Module | Raw bytes | Gzip bytes |
|---|---:|---:|
| Before (`cea51ef`) | 26,185,463 | 10,397,794 |
| After | 31,146,890 | 13,282,437 |
| Change | +4,961,427 (+18.9%) | +2,884,643 (+27.7%) |

The fifty-two font files are 4,583,364 raw / 2,762,660 gzip of that (per family in
`crates/graphics/render/assets/README.md`, "Web faces"); the rest is their advance table
(`crates/graphics/scene/src/metrics_web.rs`, 6,700 lines of `(codepoint, advance)` pairs) and
the alias resolver. A text-scripts-only subset would have saved under 8%, so the symbol
blocks were kept. Moving the web faces to the on-demand pack (drawing in DejaVu at the
face's own advances until each file arrives) is the lever if the module must shrink.

Runtime, measured in Node on this build with the pack installed: painting eight colour
emoji in a fresh renderer (so every glyph is rasterized from its COLR paint graph)
takes 2.7, 4.1 and 7.2 ms p50 at 16, 24 and 48 px, and the colour face costs nothing
to load (it is parsed lazily, not prepared by `fontdue`). The first frame needing a
bold CJK face pays the same `fontdue` preparation as the regular one (~350 ms for SC
bold, first use only). Latin rows and all other pinned frames are unchanged.

A first build embedded each Noto face twice (1.23 MB of fonts for 0.72 MB of files):
`include_bytes!` behind a `const` is re-materialized at every inlined use site. The
faces are now in `static`s, which have one address; `cw-render` puts the native pack
in statics for the same reason.

Loading a pack face is the one runtime cost: `fontdue` prepares every outline of a face
when it is first used. Measured in Node on this build, the first frame that needs emoji,
Hangul or Han takes about 34, 106 and 174 ms and grows memory by about 26, 46 and 60 MiB;
the embedded scripts take 3–11 ms the first time, and every later frame is
sub-millisecond. Latin text pays nothing: it takes the original table path, and no pinned
frame hash moved.

**Bundle size, before and after.** The Wasm had grown to 22,136,038 raw / 13,813,875
gzip (the previous revision of this table recorded 22,104,977 / 13,804,622) once eleven
font faces, symbol sheets and icon/wallpaper assets were embedded — 8.3x the 1,664,137
gzip it was before the shell work. It is now 2.8x smaller gzipped. Where the bytes went,
as raw / standalone `gzip -9`:

| Component | Before | After | Change |
|---|---:|---:|---|
| Wallpapers (5) | 9,293,680 / 9,240,819 | 1,765,706 / 1,751,728 | PNG → baseline JPEG q92 4:4:4, same dimensions |
| DejaVu faces (3) | 1,811,780 / 940,395 | 717,820 / 404,395 | subset to the reachable coverage set |
| Platform UI fonts (8) | 203,612 / 127,998 | 203,612 / 127,998 | unchanged; already subset |
| Icons | 570,094 / 567,725 (90) | 686,517 / 684,153 (105) | +15 new `music`/`maps`/`weather` icons |
| Symbol masks (98) | 88,604 / 91,324 | 88,604 / 91,324 | unchanged |
| Wasm code section | 7,979,322 / 2,472,878 | 5,810,481 / 1,625,158 | `opt-level="z"` + fat LTO, raster path kept at 3 |
| Wasm `name` section | 1,410,209 / 141,903 | 0 | `strip="symbols"` |
| **Whole module** | **22,136,038 / 13,813,875** | **10,083,313 / 4,932,659** | |

Wallpapers were the headline, not fonts: they were photographic images stored as PNG,
which is already DEFLATE, so `gzip` could not compress them at all. They made up two
thirds of the download. JPEG keeps them at 35–54 dB PSNR. Font subsetting and the
compiler flags each saved roughly 0.5–0.85 MB gzip.

What moved, and what did not:

- **Pixels.** Every frame that shows a wallpaper changed, because the decoded wallpaper
  pixels changed. Native and Wasm still agree bit for bit: `scripts/checks/smoke-desktop-pixels.cjs`
  followed by `examples/python/desktop_pixels.py` re-renders all five OS themes natively
  from the Wasm checkpoint and matches every hash. The decoder is `jpeg-decoder` with its
  `platform_independent` feature, which compiles out the SSSE3/NEON/`simd128` kernels
  that are not bit-identical to the scalar ones, and the wallpapers are 4:4:4 so its
  floating-point chroma upsampler never runs. `wallpapers_decode_to_pinned_pixels` pins
  each decoded wallpaper's SHA-256. No text glyph moved: subsetting keeps outlines and
  advances, the renderer's pinned frame hashes and `smoke-node.cjs`'s fixture hash are
  unchanged, and `metrics_data.rs` needed no regeneration.
- **Unsupported scripts.** Text in a script outside the DejaVu coverage set (Hebrew,
  Arabic, Thai, CJK and others) rendered as the `.notdef` box after this change. The
  complex-script work above restored Hebrew, Arabic and Thai and added Devanagari, CJK
  and emoji through Noto faces; scripts no bundled face covers (Georgian, Armenian,
  Ethiopic, Bengali, …) still draw the box, and a test pins that.
- **Speed.** Compiling for size costs Wasm speed. The raster path (`cw-render` and its
  PNG/JPEG/font decoders) is kept at opt-level 3, but scene construction, services and
  the kernel are now at `z`. A warm 960×640 desktop `render()` in Node, including scene
  construction, takes about 70 ms p50, against about 50 ms at opt-level 3 throughout
  and about 88 ms at `z` throughout. The browser timings earlier in this document were
  measured on the opt-level-3 bundle. Raster-only rows should still hold, but anything
  that includes world stepping or scene construction should be re-measured.
  `scripts/build-wasm.sh` records the options:

  | Wasm build | Gzip bytes | Desktop `render()` p50 |
  |---|---:|---:|
  | opt-level 3 everywhere | 6,004,786 | ~50 ms |
  | opt-level `z` everywhere | 4,906,012 | ~88 ms |
  | `z`, raster path at 3 (shipped) | 4,931,475 | ~70 ms |
  | `z`, raster and scene building at 3 | 5,369,981 | ~50 ms |

- **`wasm-opt` is not used.** With binaryen 119, `-Oz`, `-Os` and `-O2` all shrank the raw
  module by 0.7–1.0 MB but made the gzipped download 150–160 KB *larger*. The build does
  not need binaryen installed.

Levers left, with their cost: the Ubuntu icons are byte-for-byte 256×256 Yaru copies,
46% of icon bytes for a fifth of the icons (about 200 KB would come back at 128×128, at
the cost of no longer being unmodified upstream artwork). A zopfli repack of the icon and
symbol PNGs would save about 15% of their bytes with identical pixels, but committed PNGs
would then no longer be `generate-icons.py`'s output byte for byte. Wallpapers are stored
at 1586×992 and 853×1844. That is about 1.2x the largest desktop viewport rendered
in-tree (1280×800) and over 2x the phone viewports. Downscaling would save roughly
0.6 MB, but a large viewport would lose sharpness.

Browser linear memory grew from **19 MiB to 20.56 MiB** across five
create/1,100-step/free cycles; Wasm memory retains its high-water allocation after
handles are freed. That is a bounded sample, not proof of zero growth over all
workloads. Native 1 / 100 / 1,000 independent, unrendered company worlds occupied
3.42 / 27.34 / 244.68 MiB RSS and took 2.73 / 27.79 / 272.25 ms total to create in
one serial trial each. RSS after dropping them retained allocator pages; no claim
of OS page reclamation is made.

## Structured observation versus a pixel pipeline

The shape that matters for episode throughput: `scene()` is cheaper than `render()`
by orders of magnitude, and both are cheaper than a screenshot pipeline by more
still. On the 100-text-node fixture above, scene construction is **10.560 µs** and a
warm full 1280×720 raster is **522 µs** — roughly 50×, on a fixture chosen to stress
text, not a full desktop. Against a browser, the end-to-end capture rows show the
larger gap: 12.8–15.2 ms for the Wasm paths versus 35.0 ms for DOM update plus a
Chrome PNG capture.

An agent that acts on `scene()` and never rasterizes pays neither the raster nor the
capture cost. `EnvironmentConfig` makes that a grant decision, not a convention:
withhold `pixels.v1`, keep `semantic.v1`, and rasterization is unreachable. See
[action families](action-families.md).

Reported by a consumer porting an existing agent stack onto this library, on their
own hardware and their own workload: per-episode wall time 7.07 s → 0.0158 s
(60.2 episodes/s), `env.scene(w, h)` at 0.41 ms against 223 ms for the pixel
pipeline they replaced, a 1280×800 desktop render at 4.1 ms, and two external
processes per episode down to zero. **These are not measurements from this repo.**
They are a different host, a different world, and a full desktop scene rather than
the text fixture above, so they are not comparable row-for-row with the tables here
and are not reproduced by `benchmarks/run-all.sh`. There is no episode-throughput
benchmark in `benchmarks/`; the closest supported statement this repo can make is
the per-operation actor-step cost in the native world table. The process count is
structural rather than measured: no binding spawns a subprocess or requires a
simulation server, which `scripts/checks/check-boundaries.py` enforces.

## Limits and next measurements

This is a single ARM64 host and single pinned performance core. No release target
or claim applies universally to x86, mobile or other browser engines. Quiet
windows remove project builds/tests, not every unrelated host process. Per-run
min/max p50 and all raw samples are in the summary; median tables do not hide
run-zero warmup variability in the source data. No allocator event count, hardware
counter profile, multithread throughput or full CSS/browser fidelity is claimed.

The next useful work is reducing full-frame Wasm boundary copies, sharing parsed
immutable font data across renderers, measuring large populated snapshots and
traces, and extending the fair migrated-screen corpus. The 3× predecessor capture
goal remains open; it must not be substituted with the separate 3.11× native
before/after improvement.

## Reproduction

See [benchmark commands](../benchmarks/README.md). Do not run performance captures
concurrently with builds, tests or other benchmark processes. All data is local;
Chrome only serves static package assets, and the simulator uses its synthetic
network. Python and Node bindings use the same Rust runtime, without a simulation
subprocess per step.
