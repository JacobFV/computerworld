# Bundled fonts

`fonts/dejavu-mono.ttf` supplies the original fixed-cell `Text` primitive.
`fonts/dejavu-sans.ttf` supplies proportional `UiText` for desktop/mobile interfaces,
and `fonts/dejavu-sans-bold.ttf` supplies `UiTextBold`. All three are DejaVu fonts,
derived from Bitstream Vera, distributed under the permissive font license reproduced
in `FONT-LICENSE.txt`. DejaVu additions are public domain. Font bytes are embedded at
compilation; no runtime host fonts or network access are consulted. SHA-256
fingerprints are checked by renderer tests.

Upstream: https://dejavu-fonts.github.io/

## DejaVu is subset, and what that costs

The three embedded faces are **subsets**, not upstream DejaVu. The full-Unicode
masters — `DejaVuSans.ttf`, `DejaVuSans-Bold.ttf`, `DejaVuSansMono.ttf` — stay in this
directory so the subsets can be regenerated offline, but they are not embedded:
1,811,780 bytes of Hebrew, Arabic, Armenian, Georgian, Thai, Lao, N'Ko and musical
notation were shipping in a Wasm bundle that renders a Latin-script desktop.
`build-fonts.py --dejavu-only` rebuilds them; the coverage set (`DEJAVU_RANGES`) and the
reasoning behind each block are documented at the top of that script. In short it is
Latin (Basic through Extended-B), IPA, spacing modifiers and combining marks, Greek,
Cyrillic, punctuation, super/subscripts, currency, letterlike forms, number forms,
arrows, mathematical operators, Miscellaneous Technical, Control Pictures, OCR,
Enclosed Alphanumerics, Box Drawing, Block Elements, Geometric Shapes, Miscellaneous
Symbols, Dingbats, Braille and U+FFFD.

The Bitstream Vera license permits modification provided the result is not named
"Bitstream" or "Vera"; "DejaVu Sans" and "DejaVu Sans Mono" satisfy that, and the
subsetter retains name IDs 0, 13 and 14, so each file still carries its own copyright
and license text. DejaVu declares no OFL-style reserved font name.

Subsetting touches neither outlines, advances nor `unitsPerEm`, so **every retained
glyph rasterizes to exactly the pixels the master produced** — the renderer's golden
frame hashes are unchanged by it, and `crates/scene/src/metrics_data.rs` needed no
regeneration because every codepoint it tabulates is inside the coverage set.

DejaVu is no longer the last fallback: the Noto faces below cover the scripts the
subset dropped. A codepoint that no bundled face maps (Georgian, Armenian, Ethiopic,
Bengali, Tamil, …) still renders as `.notdef`, a visible hollow box (`notdef_outline`
keeps its outline). `uncovered_codepoints_draw_a_visible_notdef_box` in `src/lib.rs`
pins that: an uncovered character must draw a mark, and must draw the *same* mark as
every other uncovered character. It can never silently vanish, which would be
indistinguishable from a rendering bug and unreadable to an OCR consumer.

The renderer uses fontdue antialiasing, integer raster origins, 1/64-pixel
quantized UI advances, and integer line heights. `Text` retains its original
fixed-cell pixel output.

## Scripts beyond Latin: Noto faces, shaping and the font pack

Hebrew, Arabic, Thai, Devanagari, Chinese, Japanese, Korean and emoji are drawn from
Noto fonts (SIL OFL 1.1, `fonts/NOTO*-OFL.txt`), built by
`build-fonts.py --noto <dir>` from masters that `fetch-noto-sources.py <dir>`
downloads from google/fonts at a pinned commit and checks against pinned SHA-256s.
The build is byte-for-byte reproducible (fontTools 4.55.3; timestamps are kept from
the masters), and each output keeps its name table, so every file still carries its
copyright and license. Only Noto Sans SC/KR carry a Reserved Font Name, "Source",
which the modified files do not use.

| Face | Files | Coverage | Tier |
| --- | --- | --- | --- |
| Noto Sans Hebrew | `fonts/noto-hebrew-{regular,bold}.ttf` | U+0590–05FF, FB1D–FB4F | embedded |
| Noto Sans Arabic | `fonts/noto-arabic-{regular,bold}.ttf` | U+0600–06FF, 0750–077F, FE70–FEFF | embedded |
| Noto Sans Thai | `fonts/noto-thai-{regular,bold}.ttf` | U+0E00–0E7F | embedded |
| Noto Sans Devanagari | `fonts/noto-devanagari-{regular,bold}.ttf` | U+0900–097F, A8E0–A8FF | embedded |
| Noto Sans SC | `fonts/pack/noto-sans-sc.ttf` | 10,269 common Han, kana, CJK punctuation, fullwidth forms | pack |
| Noto Sans KR | `fonts/pack/noto-sans-kr.ttf` | all 11,172 Hangul syllables, compatibility jamo | pack |
| Noto Emoji | `fonts/pack/noto-emoji.ttf` | all of Noto Emoji, with its sequence ligatures | pack |

The four embedded scripts keep their OpenType layout tables and are instanced at
weights 400 and 700 (bold `UiTextBold` gets real bold). The Han set is the union of
GB 2312 (Simplified, 6,763), Big5 level 1 (Traditional common, 5,401) and JIS X 0208
levels 1–2 (Japanese, 6,355), enumerated from Python's own codecs so no data file is
needed. Pack faces are regular weight only.

**Fallback chain.** Per character, in order: the scene's platform face (Inter, Open
Sans, Ubuntu, Roboto), DejaVu Sans (regular or bold), the matching-weight Noto
Hebrew/Arabic/Thai/Devanagari face, Noto Sans SC, Noto Sans KR, Noto Emoji, and
finally DejaVu's `.notdef`. Combining marks and joiners stay in their base's face.
Emoji sequences — VS16, ZWJ sequences, skin-tone modifiers, keycaps, regional-indicator
flags and tag flags — go to Noto Emoji as one cluster even when the base is a symbol
DejaVu draws as text (`❤` stays DejaVu's text heart; `❤️` is the emoji); VS15 keeps
the text glyph. The chain lives in `cw_scene::text`, and scene metrics use it too.

**Layout.** `cw_scene::text::layout` wraps, reorders and shapes; `metrics::wrap`,
`text_width` and `ellipsize` and the renderer all go through it, so what layout
measures is what is drawn (`measurement_agrees_with_placement`,
`raster_stays_inside_the_measured_width_and_wraps_where_metrics_do`).

* Bidi: each `\n`-paragraph takes its direction from its first strong character
  (UAX #9 P2/P3), and every wrapped line is reordered with that base direction by the
  `unicode-bidi` crate, including number runs and bracket mirroring. Lines are
  left-aligned in the node; trailing spaces of a right-to-left line are kept out of
  the visual left edge.
* Shaping: `rustybuzz` (a pure-Rust HarfBuzz port, pinned) shapes each run with the
  face's GSUB/GPOS: Arabic initial/medial/final/isolated forms and the mandatory
  lam-alef forms, Devanagari reordering, reph and conjuncts, Thai and Hebrew mark
  attachment, emoji ligatures. Positions are font units scaled to 1/64 pixel with the
  advance table's rounding — integer arithmetic throughout.
* Line breaking: after spaces, as before, and additionally between Han, kana and
  Hangul characters, never before closing punctuation or small kana (`。」ッー`…) or
  after opening brackets. A word wider than the line breaks between grapheme clusters,
  never inside a base-plus-marks cluster, virama conjunct, flag or ZWJ sequence.
* Latin is untouched: a paragraph with nothing beyond the platform/DejaVu tables and
  no right-to-left, joiner or emoji-sequence character takes the original per-character
  path, so every existing pixel hash is unchanged.

**The font pack, and how Wasm gets it.** CJK and emoji outlines are 6.75 MB, so
`pkg/{web,node}/fonts/` ships them beside the module instead of in it. The module
exports `fontPackStatus()` (each file's name, path, SHA-256 and size, which are
installed, and which a renderer has needed but lacked) and `installFont(bytes)`,
which accepts only a file whose SHA-256 matches this build's pack. Native builds
(Rust, Python) embed the pack and need neither. Layout never waits for it:
layout shapes `fonts/stubs/*.ttf`, outline-free twins of the pack faces with the same
glyph order, cmap, advances and GSUB (`stubs_match_their_pack_faces`), which are
always embedded. Before a pack file is installed its glyphs draw as boxes at their
final positions; after, every renderer drops its cached text and repaints fully on
its next frame. With the pack installed, Wasm renders exactly the native pixels:
`scripts/smoke-node.cjs` checks the multi-script frame of `tests/scripts-scene.json`
against the hash `multi_script_scene_is_pinned` pins natively. The browser demo
fetches the pack during boot, alongside the first paint, so an episode makes no
network requests.

Sizes (bytes; gzip is `gzip -9` of each file):

| Component | Raw | Gzip | Where |
| --- | ---: | ---: | --- |
| Noto Hebrew/Arabic/Thai/Devanagari, 2 weights each | 512,860 | 221,486 | Wasm module |
| Pack stubs (SC, KR, emoji) | 204,636 | 44,055 | Wasm module |
| `pack/noto-sans-sc.ttf` | 3,522,836 | 2,190,576 | fetched on demand |
| `pack/noto-sans-kr.ttf` | 2,366,072 | 855,949 | fetched on demand |
| `pack/noto-emoji.ttf` | 862,788 | 566,769 | fetched on demand |

Alternatives measured while choosing the sets: GB 2312 alone would be 1,402,476 gzip
for Han (but no Traditional or Japanese-only kanji); the 2,350 KS X 1001 syllables
would be 194,982 gzip for Hangul (but 8,822 valid syllables would draw as boxes).
Neither saving touches the default download, which excludes the pack either way.
`docs/performance.md` has the module before/after.

**Limits.**

* Emoji are monochrome (Noto Emoji), tinted with the text colour like any glyph;
  flags are Noto Emoji's monochrome flag glyphs. Colour emoji would need an RGBA text
  path and a colour font several times the size.
* One CJK face: Han is drawn with Simplified-Chinese glyph forms whatever the
  language, since text carries no language tag. Han outside the common set, and CJK
  bold, fall back to a box and to the regular weight respectively.
* No dictionary word breaking: Thai breaks at spaces (and between clusters when a
  word is wider than the line). No hyphenation, justification or vertical text.
* Bidi explicit embeddings and isolates apply within a line; right-to-left paragraphs
  are left-aligned rather than right-aligned.
* `Text` (terminals) gets glyph fallback only: no shaping or reordering, and a
  CJK or emoji glyph is drawn at the largest whole pixel size that fits one cell.
* In Wasm, fontdue prepares every outline of a face when it is first used: measured
  in Node, the first frame needing emoji, Hangul or Han takes about 34, 106 and
  174 ms and grows memory by about 26, 46 and 60 MiB; later frames are sub-millisecond.

## Platform UI fonts

`fonts/{inter,opensans,ubuntu,roboto}-{regular,bold}.ttf` give each OS shell a system-like
typeface: Inter for macOS/iOS, Open Sans for Windows, Ubuntu Sans for Ubuntu and Roboto
for Android ("bold" is the platform's emphasis weight: 600, 600, 700 and 500). They are
unhinted Latin subsets (Basic Latin to Latin Extended-A plus common punctuation, arrows
and key symbols), instanced from the upstream variable fonts by `build-fonts.py`
(fontTools). Glyph outlines and family names are unchanged. A scene selects a family with
`Scene::typeface`; the default remains DejaVu, and any character a subset lacks falls
back to DejaVu and then the Noto faces above. Sources: <https://github.com/google/fonts> (`ofl/inter`, `ofl/opensans`,
`ofl/roboto`) and Ubuntu's `fonts-ubuntu` package. Inter, Open Sans and Roboto are under
the SIL Open Font License 1.1 (`fonts/*-OFL.txt`); Ubuntu Sans is under the Ubuntu Font
Licence 1.0 (`fonts/UBUNTU-UFL.txt`). These notices must accompany redistributed bundles.

`build-fonts.py` also writes `crates/scene/src/metrics_data.rs`, the advance table that
lets layout code measure, centre, wrap and ellipsize text exactly as the renderer will
draw it. The renderer takes its advances and word wrapping from the same table.

## Symbols

`symbols/*.svg` are original monochrome glyphs (Wi-Fi, battery, chevrons, …) under the
repository MIT license; PNGs are 96 px alpha masks built by `generate-symbols.py`
(CairoSVG + Pillow), which also writes `src/symbols.rs`. `Primitive::Symbol` tints a
mask with any colour. Runtime identifiers are `symbol/{name}`.

## Desktop resources

`wallpapers/{macos,windows,ios,android}.png` are original AI-generated raster
artwork created for Computerworld using the built-in imagegen tool. They are
OS-inspired artwork, not Apple/Microsoft/Google supplied wallpapers or real
photographs. Prompts are recorded in `WALLPAPER-PROMPTS.md`. The original generated
Ubuntu alternative is retained as `wallpapers/ubuntu-original-generated.png`.
These original project assets and the original vector icon sources are offered
under the repository MIT license to the extent applicable.

**The `.png` wallpapers are masters, not what ships.** `wallpapers/*.jpg`, built by
`build-wallpapers.py`, are what `src/assets.rs` embeds. These are the only photographic
assets in the bundle, and PNG stored them at roughly 8 bits per pixel that the outer
`gzip` of the Wasm module could not compress at all: 9,293,680 bytes raw, 9,240,819
gzipped, two thirds of the entire download. Baseline JPEG at quality 92 carries the same
artwork in 1,765,706 bytes at 35-54 dB PSNR. Source dimensions are unchanged; only the
container is. They are encoded 4:4:4, deliberately: chroma subsampling would save
another fifth but would route decoding through `jpeg-decoder`'s chroma upsampler, which
is the one part of that crate that uses floating point, and rendering here must be
bit-identical between native and Wasm. `wallpapers_decode_to_pinned_pixels` in
`src/assets.rs` hashes the decoded pixels on every target to hold that line.

`wallpapers/ubuntu.png` is the official Noble Numbat dimmed wallpaper from the
Ubuntu `ubuntu-wallpapers` package, resized from 3480×2160 to 1600×993 using
ImageMagick. Source: `/usr/share/backgrounds/Numbat_wallpaper_dimmed_3480x2160.png`,
upstream <https://launchpad.net/ubuntu-wallpapers>. It remains **CC-BY-SA-3.0** under
the package's default asset license, credited to the Ubuntu community
contributors. Full package copyright and license: `UBUNTU-WALLPAPER-COPYRIGHT.txt`.

`icons/ubuntu-*.png` are **CC-BY-SA-4.0** Yaru icons, credited to Sam Hewitt and
Yaru contributors, copied without artistic modifications from the Ubuntu
`yaru-theme-icon` package's `256x256/apps` (or `48x48` fallback) and `places`
directories. Upstream: <https://github.com/ubuntu/yaru>. Full attribution and
license: `YARU-COPYRIGHT.txt`. These asset licenses are separate from the Rust code
license, and must accompany redistributed bundles. `ubuntu-notes.png`,
`ubuntu-contacts.png`, `ubuntu-clock.png`, `ubuntu-calculator.png`, `ubuntu-music.png`,
`ubuntu-maps.png` and `ubuntu-weather.png` are byte-for-byte copies of that package's
`apps/{notes-app,address-book-app,clock-app,calculator-app,music-app,maps-app,weather-app}.png`.
Yaru draws Contacts and Phone with the same address-book artwork, so `icon/ubuntu/phone`
and `icon/ubuntu/contacts` carry identical pixels; the other shells draw them apart.

`icons/{macos,windows,ios,android}-*.svg` are original vector artwork; PNGs are
128×128 offline rasterizations, reproducible with `generate-icons.py` (CairoSVG).
Nothing here is sourced from a vendor or an icon set: every path, gradient and colour
in these files was written by hand in `generate-icons.py` and rasterized once, and the
committed PNG bytes are what the renderer embeds. Regenerating on the same CairoSVG
reproduces them byte for byte, so the checked-in bytes — not the toolchain — are the
determinism guarantee. These icon approximations are not vendor-distributed artwork.
Platform-specific silhouettes, backgrounds and colors are intentional: a macOS
soft-shadowed squircle, a bare Windows silhouette, an iOS rounded-square gradient tile
and an Android adaptive circle, all from one shared drawing per application.
`notes` (ruled pad), `contacts` (address book with a portrait), `clock` (dial at three o'clock)
and `calculator` (keypad with a lit display) were drawn the same way, for the same four
namespaces, and are likewise generated rather than sourced. `music` (two beamed eighth
notes), `maps` (a folded map with a location pin) and `weather` (a sun behind a cloud)
are the most recent additions and were drawn the same way again: every path and colour
is written by hand in `generate-icons.py`, rasterized once at 128×128 by CairoSVG, and
committed. `code` (Visual Studio Code: a ribbon folded into a chevron, in three blues) is
an original approximation of that mark drawn the same way; it has no tile on macOS and
Windows, where the product ships its mark bare, and a tile on iOS and Android. Yaru has no
Visual Studio Code icon — the editor installs its own mark on Ubuntu — so
`ubuntu-code.png` is the same original artwork rasterized at Yaru's 256×256 rather than a
copy from the package. Re-running the generator reproduces all 89 generated PNGs byte for
byte on CairoSVG 2.9.1, which is how each batch is checked before it lands. DejaVuSans-Bold.ttf uses
the same bundled DejaVu font license as the other fonts (`FONT-LICENSE.txt`).

The image editors each exist on one platform, so each has one icon, in that
platform's idiom: `windows-paint` (a palette and brush), `macos-preview` (photo prints
under a loupe), `macos-pixelmator` (a spectrum swirl on a white squircle),
`ubuntu-gimp` (a grey-brown creature holding a brush) and `ubuntu-pinta` (a palette
and brush) on 256 px Yaru-like rounded squares, and `android-sketchbook` (a pencil
swoosh on an orange disc). They are original artwork under the repository MIT
license — renditions in the spirit of each product, not vendor logos, and the two
Ubuntu ones are not Yaru icons. `generate-icons.py --editors` rebuilds just these six
without touching the others. The editors' tool glyphs (`pencil`, `brush`, `bucket`,
`eraser`, `eyedropper`, `select-rect`, `lasso`, `wand`, `crop`, `layers`, `undo`, …)
are original symbols in `generate-symbols.py`, and regenerating the symbol set leaves
every earlier PNG byte-identical.

FreeCAD exists on the three desktops only, so it has three icons: one original drawing
(an isometric machined block with a bored hole and a red sketch profile on its top face;
not FreeCAD's logo) on a macOS squircle (`macos-freecad`), bare as Windows draws app
marks (`windows-freecad`), and on a 256 px Yaru-like rounded square (`ubuntu-freecad`,
original artwork, not a Yaru icon). `generate-icons.py --freecad` rebuilds just these
three. The toolbar and tree pictograms inside the application are vector drawings made
at runtime (`crates/applications/src/apps/freecad/icons.rs`), not bundled images.

The spreadsheets and SQLite clients are each platform's own product too, so each has
its platform's icon: `windows-spreadsheet` (Excel: a green ruled sheet behind a tile
with a white X), `macos-excel` (the same mark over a soft shadow), `macos-spreadsheet`
and `ios-spreadsheet` (Numbers: white bars on a green tile), `android-spreadsheet`
(Sheets: a folded green page with a white table on the adaptive circle),
`windows-database` and `ubuntu-database` (DB Browser for SQLite: a stacked blue
cylinder with a small table, bare on Windows and on a Yaru-like rounded square on
Ubuntu) and `macos-database` (TablePlus: a ruled table window on an amber squircle).
They are original artwork under the repository MIT license, drawn in
`generate-icons.py` (`--office` rebuilds just these), not vendor logos.
`ubuntu-spreadsheet.png` is the exception: it is Yaru's own LibreOffice Calc icon,
`256x256/apps/libreoffice-calc.png` from the `yaru-theme-icon` package, copied without
modification under the Yaru terms above.

Runtime identifiers: `wallpaper/{macos,windows,ubuntu,ios,android}` and
`icon/{platform}/{files,browser,terminal,docs,mail,calendar,chat,settings,camera,photos,phone,store,launcher,trash,notes,contacts,clock,calculator,music,maps,weather,code}`,
for all five platforms, plus `icon/windows/paint`, `icon/macos/{preview,pixelmator}`,
`icon/ubuntu/{gimp,pinta}`, `icon/android/sketchbook`, `icon/{macos,windows,ubuntu}/freecad`,
`icon/{windows,macos,ubuntu,ios,android}/spreadsheet`, `icon/macos/excel` and
`icon/{windows,ubuntu,macos}/database`. `editor` aliases `docs`, `messages` aliases `chat`, `notepad`
aliases `notes`, `addressbook` aliases `contacts`, `clocks` aliases `clock` and `calc`
aliases `calculator`; unqualified `icon/{app}` and `icon/common/{app}` use macOS
artwork. Unknown names paint nothing and never access a host path or
URL. Because that is silent — an unknown id leaves an empty dock slot rather than
failing — `crates/render/tests/icon_assets.rs` renders every advertised id and asserts
it covers the tile with more than one colour, that every platform carries every
application, and that each alias paints exactly its canonical artwork; the
`icon_table_tests` in `src/assets.rs` additionally pin each icon's decoded size
(128×128 original artwork, 256×256 for every Ubuntu icon). All PNGs decode lazily once per process/Wasm instance into immutable shared
`Arc<Frame>` resources. Scenes contain identifiers, never repeated image bytes.

Icons are reduced with a coverage-weighted box filter, wallpapers fill their bounds with a
centred aspect-preserving crop and bilinear filtering, and vector paths are antialiased
on a fixed 4x4 grid. `Primitive::Backdrop` blurs what is already composited beneath it
(three clamped integer box passes) for frosted-glass materials; incremental repaints that
touch a backdrop's source region repaint that whole region so they equal a full repaint.
`Node::rounded_clip` antialiases window corners. Compositing is integer arithmetic; the
binding checks verify that native and Wasm frames hash identically on every platform.

`Primitive::Shadow` uses a rounded mask inset by `blur` pixels inside its bounds,
then three separable integer box-filter passes. Bounds therefore include the
padding. The cached shadow mask is renderer-local and color-independent; cache
retention is bounded. `UiTextBold` uses a separately bundled bold font and has
separate glyph/text cache keys. Neither feature consults host fonts, clocks,
networking or browser rendering.

## Glyph legibility for OCR consumers

Frames are exactly reproducible, so rendered terminal text doubles as labelled OCR
training data. The rasterizer is unhinted, which at terminal sizes leaves some glyphs
ambiguous. `crates/render/src/glyph_fit.rs` grid-fits the fixed-pitch `Text` face to
remove the three worst cases. Every step is integer arithmetic over the coverage bytes,
so a fitted glyph is bit-identical on every target; no output depends on float rounding.
Proportional `UiText`/`UiTextBold` are **not** fitted — chrome labels keep the
rasterizer's own output, and only `Text` carries the guarantees below.

Fitted, for `-` `_` `‐` `‑` `–` `—` `−` `¯` `=` `~` `0`:

* **Horizontal bars snap onto one whole pixel row.** Unfitted, `-` landed on a half-pixel
  row at most sizes and split across two rows: at 11 px it was two rows of 45% grey, which
  OCR drops (`atlas-sync-31288` read as `atlas sync-31288`). Ink is conserved, so a
  downsampling consumer sees the same signal and a native-resolution one sees twice the
  contrast. Peak coverage of `-` at 10/11/12/13 px: 169/116/196/245 before, 204/225/245/255
  after, always in exactly one row. `=` keeps two rows, separated by at least two pixels.
* **`~` is rasterized at double size and box-filtered back to one cell width**, doubling
  the wave's vertical amplitude while keeping the tabulated advance. Below 18 px the
  outline spans under 2 px, so the wave survived a 2:1 downsample as a flat bar that reads
  as `"` or `#`; at 10 px a halved `~` and a halved `-` differed in 2 cells, now 5. The
  wave covers 3–6 rows at 25% coverage or more, against 2–3 before, and is drawn bolder
  than the font designs it.
* **The dotted zero's centre dot is snapped to full opacity.** It is the only feature
  separating `0` from `8` and `O`, and it rasterized mid-grey (211 of 255 at 13 px, 163 at
  11 px). The rule is deliberately narrow — a single interior mark, enclosed by a counter,
  no larger than a third of the glyph box — so shading blocks (`▒`, `▧`) and multi-dot
  symbols keep their designed density. It also crisps the bar of `Ø`/`ø` and the marks in
  `ʘ`, `Θ`, `⚀` and `⚙`.

`crates/render/tests/terminal_legibility.rs` asserts each of these at 10–20 px.

### Where to still expect trouble

Measured on black-on-white `Text` after fitting, as differing pixels (coverage apart by
more than 16) within one cell:

| sizes | pair or glyph | what remains |
| --- | --- | --- |
| ≤10 px | `0` vs `8` | The zero's dot does not resolve as a separate mark at all — it merges into the bowl, so nothing can snap it. 20 differing pixels at 10 px, against 32 at 11 px and 45 at 13 px. Render terminals at 11 px or more. |
| all | `O` vs `Q` | 3–5 pixels. The tail is a few subpixels of ink below the bowl; the closest pair in the font. |
| all | `,` vs `.`, `:` vs `;` | 3–7 pixels. The comma's tail is one partial pixel below the baseline. |
| all | `h` vs `n` | 3–6 pixels. Only the ascender differs, and it is one column wide. |
| all | `'` vs `` ` ``, `b` vs `p`, `I` vs `T` | 8–12 pixels. |
| ≤13 px | `_` | The faintest glyph in the face: one row peaking at 99/109/129 of 255 at 10/11/13 px. Snapping collapsed it onto a whole row but the outline itself is thin; it is a weight property of the font, not a grid-fit one. Distinguish `_` from a blank cell by position, not by contrast. |
| ≤10 px | `-` vs `.` vs `` ` `` | 8 pixels. All three are small marks that differ mainly in vertical position. |

`UiText` is unfitted, so hyphens and tildes in window titles, menus and page text keep the
two-row, low-contrast form described above. OCR chrome labels with that in mind, or read
them from the scene graph's semantic labels instead of the pixels.
