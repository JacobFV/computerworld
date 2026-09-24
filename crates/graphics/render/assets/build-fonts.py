#!/usr/bin/env python3
"""Reproduce the bundled fonts and the scene text metrics table.

Two families of output:

  * `fonts/{inter,opensans,ubuntu,roboto}-{regular,bold}.ttf` — per-platform UI
    faces, instanced from upstream variable fonts and subset to `UI_RANGES`.
    Inputs (see README.md for sources and licenses):
      Inter[opsz,wght].ttf, OpenSans[wdth,wght].ttf, Roboto[wdth,wght].ttf
        (google/fonts, OFL-1.1)
      UbuntuSans[wdth,wght].ttf  (Ubuntu fonts-ubuntu package, UFL-1.0)
  * `fonts/dejavu-{sans,sans-bold,mono}.ttf` — the fallback and terminal faces,
    subset from the full DejaVu masters that sit beside this script.

Why DejaVu is subset: the three masters are 1,811,780 bytes of full-Unicode
coverage — Hebrew, Arabic, Armenian, Georgian, Thai, Lao, N'Ko, musical
notation and much more — and every byte ships in a Wasm bundle that renders a
simulated Latin-script desktop. `DEJAVU_RANGES` below is the coverage the world
can actually reach, and subsetting to it costs 713,820 bytes instead.

Choosing that set is a correctness decision, not a size one: DejaVu is the
first *fallback* face, and a codepoint neither it nor the Noto faces below map
renders as `.notdef`. The set is therefore drawn to cover anything an actor can
type or a simulated service can emit in the scripts DejaVu serves, not merely
what today's fixtures happen to show:

  Latin (Basic through Extended-B), IPA, spacing modifiers and combining marks
    — filenames, user-typed text, transliterated names.
  Greek and Cyrillic — the two non-Latin scripts the metrics table tabulates,
    and the source of the λ and Θ used as fallback probes in the tests.
  Punctuation, super/subscripts, currency, letterlike forms, number forms,
    arrows, mathematical operators — service page content and shell output.
  Miscellaneous Technical — ⌘ ⌥ ⌃ ⌫ ⏎, drawn in menus and key hints.
  Control Pictures, OCR, Enclosed Alphanumerics.
  Box Drawing, Block Elements, Geometric Shapes — terminal TUIs and progress
    bars; ▒ and ▧ are named in the glyph-fitting notes in README.md.
  Miscellaneous Symbols, Dingbats, Braille — ⚀ ⚙ ✓ and TUI spinners.
  U+FFFD, so decoders that replace bad bytes have a glyph to show.

Outside that set and the Noto coverage a character renders as DejaVu's `.notdef`
box (the subsetter keeps its outline via `notdef_outline`), which is visible and
self-explanatory; `uncovered_codepoints_draw_a_visible_notdef_box` in
`src/lib.rs` pins that behaviour so an uncovered codepoint can never silently
vanish. Subsetting does
not touch outlines, advances or `unitsPerEm`, so every retained glyph
rasterizes to exactly the pixels the full master produced.

Scripts beyond DejaVu's set come from Noto (all SIL OFL 1.1), in two tiers:

  * `fonts/noto-{hebrew,arabic,thai,devanagari}-{regular,bold}.ttf` — embedded
    in every build. They keep their OpenType layout tables (GSUB/GPOS/GDEF),
    because Arabic joining, lam-alef ligatures, Indic reordering and mark
    attachment are done by shaping with those tables.
  * `fonts/pack/noto-{sans-sc,sans-kr,emoji}.ttf` — the CJK/emoji font pack.
    Native builds embed it; the Wasm build does not, and a page fetches and
    installs it on demand (see README.md). Han is subset to the union of three
    national common sets, enumerated from Python's own codecs so the set is
    reproducible without a data file: GB 2312 (Simplified, 6,763), Big5 level 1
    (Traditional common, 5,401) and JIS X 0208 levels 1-2 (Japanese, 6,355),
    10,269 ideographs together. Kana, CJK punctuation and fullwidth forms come
    from the same face. Hangul is all 11,172 syllables plus compatibility jamo.
  * `fonts/stubs/*.ttf` — outline-free twins of the pack faces: same glyph
    order, cmap, advances and GSUB, every outline emptied. They are embedded
    everywhere, so text *layout* (measurement, wrapping, ellipsis, shaping) is
    identical whether or not the pack has been fetched; only the pixels of
    pack glyphs wait for it.

It also writes `crates/graphics/scene/src/text/coverage.rs`, the cmap coverage of the
DejaVu faces as ranges, which is how layout decides where the fallback chain
leaves DejaVu without embedding DejaVu in the scene crate.

`--noto` additionally builds, from the same pinned download:

  * Italics. `fonts/{inter,opensans,ubuntu,roboto}-{italic,bold-italic}.ttf`
    are instanced from the upstream italic masters at the upright faces' axes
    and subset to `UI_RANGES`; `fonts/dejavu-sans-{oblique,bold-oblique}.ttf`
    are DejaVu Sans Oblique/Bold Oblique 2.37 subset to `ITALIC_RANGES` (Latin,
    IPA, combining marks, Greek, Cyrillic, punctuation, currency). Symbols,
    arrows, box drawing and the rest of DejaVu's coverage stay upright: a
    slanted box-drawing line is a defect, not a style. Their advances go to
    `crates/graphics/scene/src/metrics_italic.rs`, the italic twin of the metrics table.
  * More scripts. Georgian, Armenian and Bengali are embedded in both weights
    (39 KB, 20 KB and 101 KB gzip -9 for the pair). The rest are candidates
    whose cost is only worth paying by pages that use them, so they are in the
    pack, regular weight only, each with an embedded stub for layout (gzip of
    both weights / of the stub): Tamil 35 KB / 4 KB, Gurmukhi 24 KB / 3 KB,
    Lao 21 KB / 3 KB, Khmer 47 KB / 7 KB, Gujarati 94 KB / 13 KB, Ethiopic
    195 KB / 2 KB, Myanmar 97 KB / 20 KB, Sinhala 92 KB / 8 KB. Embedding all
    eight would have added 605 KB gzip to the module; their stubs add 60 KB.
    Ethiopic is subset without layout tables: it needs no shaping, and its
    kerning alone made the always-embedded stub 172 KB.
  * CJK bold and locale forms. `pack/noto-sans-{sc,kr}-bold.ttf` are the SC
    and KR sets at weight 700. `pack/noto-sans-{tc,jp,kr-han}{,-bold}.ttf`
    hold only the Han (and CJK punctuation) glyphs whose outlines differ from
    Noto Sans SC's, within each locale's own common set: Big5 level 1 for
    Traditional Chinese, JIS X 0208 level 1 for Japanese and the KS X 1001
    hanja for Korean. A character the locale face lacks is the same drawing in
    SC, so the fallback to SC is exact. Bold twins share their regular face's
    glyph order, cmap and advances (checked here), so one stub lays out both.
  * `pack/noto-color-emoji.ttf`: Noto Color Emoji v2.051, COLRv1 build,
    copied byte for byte. Layout keeps shaping the monochrome Noto Emoji stub;
    the renderer draws a colour glyph in each emoji cluster's place when this
    file is installed.
  * `crates/graphics/scene/src/text/han.rs`: a bitset of the Han characters that are
    Traditional-only (Big5 level 1 but not GB 2312), the script heuristic that
    picks Traditional Chinese forms for text with no language tag.

`--web <dir>` builds the web faces, from the masters `fetch-web-sources.py <dir>`
downloads (google/fonts at the same pinned commit, pinned SHA-256):

  * `fonts/{arimo,tinos,cousine,gelasio,carlito,caladea,lato,sourcesans,
    sourceserif,poppins,montserrat,playfair,jetbrainsmono}-{regular,bold,italic,
    bold-italic}.ttf` — the families pages ask for by name, and metric-compatible
    stand-ins for the ones that cannot be bundled (Arimo for Arial/Helvetica,
    Tinos for Times New Roman, Cousine for Courier New, Gelasio for Georgia,
    Carlito for Calibri, Caladea for Cambria); `cw_scene::fonts` maps CSS
    `font-family` lists onto them. Variable masters are instanced at weights
    400 and 700 (other axes at their defaults); static families use their
    Regular, Bold, Italic and BoldItalic files. Every face is subset to
    `DEJAVU_RANGES` — the same Latin, Greek, Cyrillic and symbol coverage the
    DejaVu fallback has, so a page set in one of these faces is drawn in it for
    every character the family designs, and DejaVu only for what it does not —
    and, as with every other subset here, outlines, advances and unitsPerEm are
    untouched. Their advances go to `crates/graphics/scene/src/metrics_web.rs`, one table
    per face and one `[Option<..>; 4]` per family indexed by `bold + 2 * italic`,
    `None` where a family has no file for that style (the renderer then
    synthesises bold or oblique from the nearest face it does have).
  * `crates/graphics/scene/src/kerning_data.rs` — the `kern` feature of every web face,
    read from the same instanced master before it is subset (the subsets keep
    no layout tables, so the renderer positions by table and needs the pairs
    here). `extract_kerning` walks the feature's PairPos lookups the way
    HarfBuzz applies them — subtables in order, the first that covers a pair
    settles it, lookups accumulate — and expands class pairs (format 2) to the
    glyph pairs the subset covers, keyed by codepoint since the metrics tables
    are per codepoint. Pairs are kept for `KERN_RANGES` only — ASCII, Latin-1
    and the common punctuation of the General Punctuation block — because the
    class-kerned families (Lato, Montserrat, Source Sans/Serif, Carlito) pair
    nearly every glyph with every other: over the whole coverage set the table
    was 35 MB of source. The platform and DejaVu faces are deliberately not
    tabulated: the desktop scenes and their golden frames were laid out
    without kerning, and the web faces are the ones a page's widths are
    compared with Chromium's.
  * `crates/graphics/scene/src/kerning_dejavu.rs` (`--kern-dejavu`) — the same
    pairs for DejaVu Sans's four faces, read from their masters. Desktop scenes
    still lay DejaVu out unkerned; only web content (`Style::web`, which the web
    engine sets) applies them, as Chromium does with the system DejaVu Sans (the
    same 2.37 files): "Tracker" in 18 px DejaVu Sans Bold is 74.58 px there,
    77.03 unkerned. The platform faces are not tabulated: the files the engine
    draws them with keep no layout tables, and a page that serves those files
    (the analytics parity fixture's Inter) is measured unkerned by Chromium, so
    kerning them by their masters' pairs lost 9 of its 448 nodes.
  * `crates/graphics/scene/src/metrics_dejavu.rs` (`--dejavu-web`) — the advances
    of every codepoint the DejaVu subsets draw beyond `WIDE`. Native text keeps
    measuring those at 0.6 em; web content measures them by the font, as
    Chromium does when it falls back to DejaVu Sans for a symbol (a 13 px
    "☎" is 16.19 px there, and was 7.8 here).

Usage:
  build-fonts.py <source-dir>   rebuild everything (requires the variable fonts)
  build-fonts.py --dejavu-only  rebuild just the DejaVu subsets and the metrics
                                table, using only files already in the tree
  build-fonts.py --noto <dir>   rebuild the Noto faces, the pack and its stubs,
                                the italics and the colour emoji from the masters
                                `fetch-noto-sources.py <dir>` downloads (pinned
                                commits, pinned SHA-256)
  build-fonts.py --web <dir>    rebuild the web faces, `metrics_web.rs` and
                                `kerning_data.rs` from the masters
                                `fetch-web-sources.py <dir>` downloads
  build-fonts.py --kern <dir>   rewrite just `kerning_data.rs` from those masters
  build-fonts.py --dejavu-web   write `metrics_dejavu.rs` from the committed
                                DejaVu subsets
  build-fonts.py --kern-dejavu <dir>
                                write `kerning_dejavu.rs` from the DejaVu masters
                                here and the obliques `fetch-noto-sources.py <dir>`
                                downloads
Requires fontTools (4.55.3 was used); brotli not needed.
"""
import sys
from pathlib import Path
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
from fontTools import subset

HERE = Path(__file__).resolve().parent
OUT = HERE / "fonts"
METRICS = HERE.parents[1] / "scene" / "src" / "metrics_data.rs"

# Coverage of the per-platform UI faces. These are never a fallback: anything
# they lack falls through to DejaVu, so they stay a tight Latin set.
UI_RANGES = [(0x20, 0x7E), (0xA0, 0x17F), (0x2010, 0x2027), (0x2030, 0x203A), (0x20AC, 0x20AC),
             (0x2122, 0x2122), (0x2190, 0x2193), (0x21E7, 0x21E7), (0x2212, 0x2212), (0x2303, 0x2303),
             (0x2318, 0x2318), (0x2325, 0x2325), (0x232B, 0x232B), (0x23CE, 0x23CE), (0x2713, 0x2713)]
# Codepoints the metrics table tabulates, so layout measures them exactly.
WIDE = UI_RANGES + [(0x370, 0x3FF), (0x400, 0x4FF), (0x25A0, 0x25FF)]
# Coverage of the DejaVu fallback and terminal faces; see the module docstring.
DEJAVU_RANGES = WIDE + [
    (0x0180, 0x024F),  # Latin Extended-B
    (0x0250, 0x02AF),  # IPA Extensions
    (0x02B0, 0x02FF),  # Spacing Modifier Letters
    (0x0300, 0x036F),  # Combining Diacritical Marks
    (0x2000, 0x206F),  # General Punctuation
    (0x2070, 0x209F),  # Superscripts and Subscripts
    (0x20A0, 0x20CF),  # Currency Symbols
    (0x2100, 0x214F),  # Letterlike Symbols
    (0x2150, 0x218F),  # Number Forms
    (0x2190, 0x21FF),  # Arrows
    (0x2200, 0x22FF),  # Mathematical Operators
    (0x2300, 0x23FF),  # Miscellaneous Technical
    (0x2400, 0x243F),  # Control Pictures
    (0x2440, 0x245F),  # Optical Character Recognition
    (0x2460, 0x24FF),  # Enclosed Alphanumerics
    (0x2500, 0x257F),  # Box Drawing
    (0x2580, 0x259F),  # Block Elements
    (0x2600, 0x26FF),  # Miscellaneous Symbols
    (0x2700, 0x27BF),  # Dingbats
    (0x2800, 0x28FF),  # Braille Patterns
    (0xFFFD, 0xFFFD),  # Replacement character
]
FACES = [  # typeface, weight name, source, axes
    ("inter", "regular", "Inter-var.ttf", {"wght": 400, "opsz": 14}),
    ("inter", "bold", "Inter-var.ttf", {"wght": 600, "opsz": 14}),
    ("opensans", "regular", "OpenSans-var.ttf", {"wght": 400, "wdth": 100}),
    ("opensans", "bold", "OpenSans-var.ttf", {"wght": 600, "wdth": 100}),
    ("ubuntu", "regular", "UbuntuSans-var.ttf", {"wght": 400, "wdth": 100}),
    ("ubuntu", "bold", "UbuntuSans-var.ttf", {"wght": 700, "wdth": 100}),
    ("roboto", "regular", "Roboto-var.ttf", {"wght": 400, "wdth": 100}),
    ("roboto", "bold", "Roboto-var.ttf", {"wght": 500, "wdth": 100}),
]
# Full-Unicode masters kept in the tree so the subsets can be regenerated
# offline. They are not embedded; only the `fonts/dejavu-*.ttf` outputs are.
DEJAVU = [  # weight name, master, emitted subset
    ("regular", "DejaVuSans.ttf", "dejavu-sans.ttf"),
    ("bold", "DejaVuSans-Bold.ttf", "dejavu-sans-bold.ttf"),
    ("mono", "DejaVuSansMono.ttf", "dejavu-mono.ttf"),
]


def codepoints(ranges):
    return sorted({c for a, b in ranges for c in range(a, b + 1)})


def table(font, ranges):
    cmap, hmtx = font.getBestCmap(), font["hmtx"]
    return [(c, hmtx[cmap[c]][0]) for c in codepoints(ranges) if c in cmap]


def shrink(font, ranges):
    """Subset in place. Outlines, advances and unitsPerEm are untouched, so a
    retained glyph rasterizes identically to the same glyph in the master."""
    options = subset.Options()
    options.hinting = False          # fontdue is an unhinted rasterizer
    options.layout_features = []     # no shaping; the renderer positions by table
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]  # keeps licence and family names
    options.notdef_outline = True    # an uncovered codepoint must draw a visible box
    options.glyph_names = False
    sub = subset.Subsetter(options)
    sub.populate(unicodes=codepoints(ranges))
    sub.subset(font)
    return font


def build_dejavu(rows):
    for weight, master, out_name in DEJAVU:
        font = shrink(TTFont(HERE / master), DEJAVU_RANGES)
        path = OUT / out_name
        font.save(path)
        before = (HERE / master).stat().st_size
        print(f"{path.name:22s} {before:>8,} -> {path.stat().st_size:>8,}")
        # The mono face drives the fixed-cell `Text` primitive; its row tabulates
        # coverage for `Typeface::Mono`, which measures on the terminal grid rather
        # than by these advances (they are all the same anyway).
        rows.append(("dejavu", weight, font["head"].unitsPerEm, table(font, WIDE)))


def write_metrics(rows):
    with open(METRICS, "w") as out:
        out.write("// Generated by crates/graphics/render/assets/build-fonts.py; do not edit.\n")
        out.write("// (codepoint, advance in font units), sorted by codepoint.\n")
        for face, weight, upem, advances in rows:
            ident = f"{face}_{weight}".upper()
            out.write(f"pub const {ident}_UPEM: u32 = {upem};\n")
            out.write(f"pub static {ident}: &[(u32, u16)] = &[\n")
            for i in range(0, len(advances), 8):
                out.write("    " + " ".join(f"({c}, {a})," for c, a in advances[i:i + 8]) + "\n")
            out.write("];\n")


COVERAGE = HERE.parents[1] / "scene" / "src" / "text" / "coverage.rs"
SHAPING = [0x200C, 0x200D, 0x25CC]  # joiners, and the dotted circle a shaper inserts
NOTO_CORE = [  # name, master, axes per weight, coverage
    ("hebrew", "NotoSansHebrew-var.ttf", [(0x0590, 0x05FF), (0xFB1D, 0xFB4F)]),
    ("arabic", "NotoSansArabic-var.ttf", [(0x0600, 0x06FF), (0x0750, 0x077F), (0xFE70, 0xFEFF)]),
    ("thai", "NotoSansThai-var.ttf", [(0x0E00, 0x0E7F)]),
    ("devanagari", "NotoSansDevanagari-var.ttf", [(0x0900, 0x097F), (0xA8E0, 0xA8FF)]),
]
NOTO_WEIGHTS = [("regular", 400), ("bold", 700)]
CJK_BASE = [
    (0x3000, 0x303F),  # CJK Symbols and Punctuation
    (0x3040, 0x309F),  # Hiragana
    (0x30A0, 0x30FF),  # Katakana
    (0x31F0, 0x31FF),  # Katakana Phonetic Extensions
    (0xFF00, 0xFFEF),  # Halfwidth and Fullwidth Forms
]
HANGUL = [(0xAC00, 0xD7A3), (0x3130, 0x318F)]


def codec_set(codec, lead, trail):
    """Every character a double-byte codec maps, straight from Python's tables."""
    out = set()
    for a in lead:
        for b in trail:
            try:
                s = bytes([a, b]).decode(codec)
            except UnicodeDecodeError:
                continue
            if len(s) == 1:
                out.add(ord(s))
    return out


def common_han():
    han = lambda s: {c for c in s if 0x3400 <= c <= 0x9FFF or 0xF900 <= c <= 0xFAFF}
    gb2312 = han(codec_set("gb2312", range(0xA1, 0xF8), range(0xA1, 0xFF)))
    big5_level1 = han(codec_set("big5", range(0xA4, 0xC7), [*range(0x40, 0x7F), *range(0xA1, 0xFF)]))
    jisx0208 = han(codec_set("euc_jp", range(0xA1, 0xFF), range(0xA1, 0xFF)))
    union = gb2312 | big5_level1 | jisx0208
    print(f"han: GB 2312 {len(gb2312)}, Big5 level 1 {len(big5_level1)}, "
          f"JIS X 0208 {len(jisx0208)}, union {len(union)}")
    return union


def noto_subset(font, unicodes, layout):
    options = subset.Options()
    options.hinting = False
    options.layout_features = ["*"] if layout else []
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
    options.notdef_outline = True
    options.glyph_names = False
    sub = subset.Subsetter(options)
    sub.populate(unicodes=sorted(unicodes))
    sub.subset(font)
    return font


def stub(path, out):
    """Outline-free twin: identical glyph order, cmap, hmtx advances and layout
    tables, so shaping it yields exactly the glyph ids and positions shaping the
    full face would. Only outlines (and side bearings) are dropped."""
    from fontTools.ttLib.tables._g_l_y_f import Glyph
    font = TTFont(path, recalcTimestamp=False)
    order = font.getGlyphOrder()
    for name in order:
        font["glyf"][name] = Glyph()
        font["hmtx"][name] = (font["hmtx"][name][0], 0)
    for tag in ["vhea", "vmtx", "BASE", "STAT", "gasp", "prep", "fpgm", "cvt ", "DSIG"]:
        if tag in font:
            del font[tag]
    font.save(out)


def ranges_of(cmap):
    out = []
    for c in sorted(cmap):
        if out and out[-1][1] + 1 == c:
            out[-1][1] = c
        else:
            out.append([c, c])
    return out


def write_coverage():
    with open(COVERAGE, "w") as out:
        out.write("// Generated by crates/graphics/render/assets/build-fonts.py; do not edit.\n")
        out.write("// Inclusive codepoint ranges each embedded DejaVu face maps in its cmap.\n")
        for ident, name in [("DEJAVU_SANS", "dejavu-sans.ttf"), ("DEJAVU_SANS_BOLD", "dejavu-sans-bold.ttf"),
                            ("DEJAVU_MONO", "dejavu-mono.ttf"),
                            ("DEJAVU_SANS_OBLIQUE", "dejavu-sans-oblique.ttf"),
                            ("DEJAVU_SANS_BOLD_OBLIQUE", "dejavu-sans-bold-oblique.ttf")]:
            ranges = ranges_of(TTFont(OUT / name).getBestCmap())
            out.write(f"pub static {ident}: &[(u32, u32)] = &[\n")
            for i in range(0, len(ranges), 6):
                out.write("    " + " ".join(f"({a}, {b})," for a, b in ranges[i:i + 6]) + "\n")
            out.write("];\n")


def build_noto(src):
    src = Path(src)
    for name, master, ranges in NOTO_CORE:
        for weight, wght in NOTO_WEIGHTS:
            font = instancer.instantiateVariableFont(TTFont(src / master, recalcTimestamp=False), {"wght": wght, "wdth": 100})
            noto_subset(font, codepoints(ranges) + SHAPING, layout=True)
            path = OUT / f"noto-{name}-{weight}.ttf"
            font.save(path)
            print(f"{path.name:30s} {path.stat().st_size:>10,}")
    (OUT / "pack").mkdir(exist_ok=True)
    (OUT / "stubs").mkdir(exist_ok=True)
    emoji = TTFont(src / "NotoEmoji-var.ttf")
    pack = [
        ("noto-sans-sc.ttf", "NotoSansSC-var.ttf", {"wght": 400}, set(codepoints(CJK_BASE)) | common_han(), False),
        ("noto-sans-kr.ttf", "NotoSansKR-var.ttf", {"wght": 400}, set(codepoints(HANGUL)), False),
        ("noto-emoji.ttf", "NotoEmoji-var.ttf", {"wght": 400}, set(emoji.getBestCmap()), True),
    ]
    for out_name, master, axes, unicodes, layout in pack:
        font = instancer.instantiateVariableFont(TTFont(src / master, recalcTimestamp=False), axes)
        noto_subset(font, unicodes, layout)
        path = OUT / "pack" / out_name
        font.save(path)
        stub(path, OUT / "stubs" / out_name)
        print(f"{'pack/' + out_name:30s} {path.stat().st_size:>10,}   stub "
              f"{(OUT / 'stubs' / out_name).stat().st_size:>8,}")
    build_more_scripts(src)
    build_cjk_extras(src)
    (OUT / "pack" / "noto-color-emoji.ttf").write_bytes((src / "Noto-COLRv1.ttf").read_bytes())
    print(f"{'pack/noto-color-emoji.ttf':30s} {(OUT / 'pack' / 'noto-color-emoji.ttf').stat().st_size:>10,}")
    build_italics(src)
    write_han_data()
    write_wide_data()
    for text in src.glob("NOTO*-OFL.txt"):
        (OUT / text.name).write_bytes(text.read_bytes())
    write_coverage()


# Further scripts: (name, master, coverage, keep layout tables). The danda and
# double danda (U+0964/0965) are shared by the Indic scripts.
NOTO_EMBEDDED = [
    ("bengali", "NotoSansBengali-var.ttf", [(0x0980, 0x09FF), (0x0964, 0x0965)], True),
    ("georgian", "NotoSansGeorgian-var.ttf", [(0x10A0, 0x10FF), (0x1C90, 0x1CBF), (0x2D00, 0x2D2F)], True),
    ("armenian", "NotoSansArmenian-var.ttf", [(0x0530, 0x058F), (0xFB13, 0xFB17)], True),
]
NOTO_PACKED = [
    ("tamil", "NotoSansTamil-var.ttf", [(0x0B80, 0x0BFF), (0x0964, 0x0965)], True),
    ("gurmukhi", "NotoSansGurmukhi-var.ttf", [(0x0A00, 0x0A7F), (0x0964, 0x0965)], True),
    ("lao", "NotoSansLao-var.ttf", [(0x0E80, 0x0EFF)], True),
    ("khmer", "NotoSansKhmer-var.ttf", [(0x1780, 0x17FF), (0x19E0, 0x19FF)], True),
    ("gujarati", "NotoSansGujarati-var.ttf", [(0x0A80, 0x0AFF), (0x0964, 0x0965)], True),
    ("ethiopic", "NotoSansEthiopic-var.ttf", [(0x1200, 0x139F), (0x2D80, 0x2DDF)], False),
    ("myanmar", "NotoSansMyanmar-var.ttf", [(0x1000, 0x109F), (0xA9E0, 0xA9FF), (0xAA60, 0xAA7F)], True),
    ("sinhala", "NotoSansSinhala-var.ttf", [(0x0D80, 0x0DFF), (0x0964, 0x0965)], True),
]


def build_more_scripts(src):
    for name, master, ranges, layout in NOTO_EMBEDDED:
        for weight, wght in NOTO_WEIGHTS:
            font = instancer.instantiateVariableFont(TTFont(src / master, recalcTimestamp=False), {"wght": wght, "wdth": 100})
            noto_subset(font, codepoints(ranges) + SHAPING, layout=layout)
            path = OUT / f"noto-{name}-{weight}.ttf"
            font.save(path)
            print(f"{path.name:30s} {path.stat().st_size:>10,}")
    for name, master, ranges, layout in NOTO_PACKED:
        font = instancer.instantiateVariableFont(TTFont(src / master, recalcTimestamp=False), {"wght": 400, "wdth": 100})
        noto_subset(font, codepoints(ranges) + SHAPING, layout=layout)
        path = OUT / "pack" / f"noto-{name}.ttf"
        font.save(path)
        stub(path, OUT / "stubs" / path.name)
        print(f"{'pack/' + path.name:30s} {path.stat().st_size:>10,}   stub "
              f"{(OUT / 'stubs' / path.name).stat().st_size:>8,}")


def outlines(font, unicodes):
    """Codepoint -> (points, contour ends, on-curve flags) of a static face."""
    cmap, glyf = font.getBestCmap(), font["glyf"]
    out = {}
    for c in unicodes:
        if c in cmap:
            coords, ends, flags = glyf[cmap[c]].getCoordinates(glyf)
            out[c] = (tuple(coords), tuple(ends), tuple(int(f) & 1 for f in flags))
    return out


def same_layout(a, b):
    """Two builds lay out identically: glyph order, cmap and advances agree."""
    return (a.getGlyphOrder() == b.getGlyphOrder() and a.getBestCmap() == b.getBestCmap()
            and all(a["hmtx"][g][0] == b["hmtx"][g][0] for g in a.getGlyphOrder()))


def han_sets():
    han = lambda s: {c for c in s if 0x3400 <= c <= 0x9FFF or 0xF900 <= c <= 0xFAFF}
    gb2312 = han(codec_set("gb2312", range(0xA1, 0xF8), range(0xA1, 0xFF)))
    big5_level1 = han(codec_set("big5", range(0xA4, 0xC7), [*range(0x40, 0x7F), *range(0xA1, 0xFF)]))
    jis_level1 = han(codec_set("euc_jp", range(0xB0, 0xD0), range(0xA1, 0xFF)))
    ksx1001 = han(codec_set("euc_kr", range(0xCA, 0xFE), range(0xA1, 0xFF)))
    return gb2312, big5_level1, jis_level1, ksx1001


def build_cjk_extras(src):
    def instance(master, wght):
        return instancer.instantiateVariableFont(TTFont(src / master, recalcTimestamp=False), {"wght": wght})

    # Bold twins of the SC and KR pack faces: same subset, weight 700.
    for out_name, master, unicodes in [
        ("noto-sans-sc", "NotoSansSC-var.ttf", set(codepoints(CJK_BASE)) | common_han()),
        ("noto-sans-kr", "NotoSansKR-var.ttf", set(codepoints(HANGUL))),
    ]:
        font = noto_subset(instance(master, 700), unicodes, False)
        path = OUT / "pack" / f"{out_name}-bold.ttf"
        font.save(path)
        assert same_layout(TTFont(path), TTFont(OUT / "pack" / f"{out_name}.ttf")), out_name
        print(f"{'pack/' + path.name:30s} {path.stat().st_size:>10,}   (shares the regular stub)")

    # Locale forms: only the glyphs that differ from SC, within each locale's set.
    _, big5_level1, jis_level1, ksx1001 = han_sets()
    base = set(codepoints(CJK_BASE))
    sc = outlines(instance("NotoSansSC-var.ttf", 400), base | common_han())
    for out_name, master, national in [
        ("noto-sans-tc", "NotoSansTC-var.ttf", big5_level1),
        ("noto-sans-jp", "NotoSansJP-var.ttf", jis_level1),
        ("noto-sans-kr-han", "NotoSansKR-var.ttf", ksx1001),
    ]:
        regular = instance(master, 400)
        mine = outlines(regular, (base | national) & set(sc))
        differ = {c for c, o in mine.items() if o != sc[c]}
        print(f"{out_name}: {len(differ)} of {len(mine)} glyphs differ from SC "
              f"({len([c for c in differ if c < 0x3400])} outside Han)")
        noto_subset(regular, differ, False)
        path = OUT / "pack" / f"{out_name}.ttf"
        regular.save(path)
        stub(path, OUT / "stubs" / path.name)
        bold = noto_subset(instance(master, 700), differ, False)
        bold_path = OUT / "pack" / f"{out_name}-bold.ttf"
        bold.save(bold_path)
        assert same_layout(TTFont(path), TTFont(bold_path)), out_name
        print(f"{'pack/' + path.name:30s} {path.stat().st_size:>10,}   stub "
              f"{(OUT / 'stubs' / path.name).stat().st_size:>8,}   bold {bold_path.stat().st_size:>10,}")


HAN_DATA = HERE.parents[1] / "scene" / "src" / "text" / "han.rs"
WIDE_DATA = HERE.parents[1] / "scene" / "src" / "text" / "wide.rs"


def write_wide_data():
    """East Asian Wide and Fullwidth ranges (UAX #11), from Python's unicodedata:
    the characters a terminal gives two cells."""
    import unicodedata
    ranges = []
    for c in range(0x110000):
        if 0xD800 <= c <= 0xDFFF or unicodedata.east_asian_width(chr(c)) not in ("W", "F"):
            continue
        if ranges and ranges[-1][1] + 1 == c:
            ranges[-1][1] = c
        else:
            ranges.append([c, c])
    with open(WIDE_DATA, "w") as out:
        out.write("// Generated by crates/graphics/render/assets/build-fonts.py; do not edit.\n")
        out.write(f"// East Asian Wide/Fullwidth ranges, Unicode {unicodedata.unidata_version}.\n")
        out.write("pub static WIDE: &[(u32, u32)] = &[\n")
        for i in range(0, len(ranges), 6):
            out.write("    " + " ".join(f"({a}, {b})," for a, b in ranges[i:i + 6]) + "\n")
        out.write("];\n")


def write_han_data():
    """Traditional-only Han as a bitset over U+4E00..U+9FFF."""
    gb2312, big5_level1, _, _ = han_sets()
    traditional = sorted(c for c in big5_level1 - gb2312 if 0x4E00 <= c <= 0x9FFF)
    words = [0] * ((0x9FFF - 0x4E00 + 64) // 64)
    for c in traditional:
        words[(c - 0x4E00) // 64] |= 1 << ((c - 0x4E00) % 64)
    with open(HAN_DATA, "w") as out:
        out.write("// Generated by crates/graphics/render/assets/build-fonts.py; do not edit.\n")
        out.write(f"// {len(traditional)} Han characters of Big5 level 1 that GB 2312 lacks, as bits\n")
        out.write("// over U+4E00..=U+9FFF (bit c - 0x4E00): text containing them is Traditional.\n")
        out.write(f"pub static TRADITIONAL_ONLY: [u64; {len(words)}] = [\n")
        for i in range(0, len(words), 4):
            out.write("    " + " ".join(f"0x{w:016x}," for w in words[i:i + 4]) + "\n")
        out.write("];\n")


# Italic coverage of DejaVu: text scripts only; see the module docstring.
ITALIC_RANGES = [
    (0x0020, 0x007E), (0x00A0, 0x024F),  # Latin through Extended-B
    (0x0250, 0x02FF),  # IPA, spacing modifier letters
    (0x0300, 0x036F),  # combining diacritical marks
    (0x0370, 0x03FF), (0x0400, 0x04FF),  # Greek, Cyrillic
    (0x2000, 0x206F),  # general punctuation
    (0x20A0, 0x20CF),  # currency
    (0x2122, 0x2122),
]
ITALIC_FACES = [  # typeface, weight name, master, axes (the upright faces' axes)
    ("inter", "italic", "Inter-Italic-var.ttf", {"wght": 400, "opsz": 14}),
    ("inter", "bold_italic", "Inter-Italic-var.ttf", {"wght": 600, "opsz": 14}),
    ("opensans", "italic", "OpenSans-Italic-var.ttf", {"wght": 400, "wdth": 100}),
    ("opensans", "bold_italic", "OpenSans-Italic-var.ttf", {"wght": 600, "wdth": 100}),
    ("ubuntu", "italic", "UbuntuSans-Italic-var.ttf", {"wght": 400, "wdth": 100}),
    ("ubuntu", "bold_italic", "UbuntuSans-Italic-var.ttf", {"wght": 700, "wdth": 100}),
    ("roboto", "italic", "Roboto-Italic-var.ttf", {"wght": 400, "wdth": 100}),
    ("roboto", "bold_italic", "Roboto-Italic-var.ttf", {"wght": 500, "wdth": 100}),
]
METRICS_ITALIC = HERE.parents[1] / "scene" / "src" / "metrics_italic.rs"


def build_italics(src):
    rows = []
    for face, weight, master, axes in ITALIC_FACES:
        font = instancer.instantiateVariableFont(TTFont(src / master, recalcTimestamp=False), axes)
        shrink(font, UI_RANGES)
        path = OUT / f"{face}-{weight.replace('_', '-')}.ttf"
        font.save(path)
        rows.append((face, weight, font["head"].unitsPerEm, table(font, UI_RANGES)))
        print(f"{path.name:30s} {path.stat().st_size:>10,}")
    for weight, master, out_name in [("italic", "DejaVuSans-Oblique.ttf", "dejavu-sans-oblique.ttf"),
                                     ("bold_italic", "DejaVuSans-BoldOblique.ttf", "dejavu-sans-bold-oblique.ttf")]:
        font = shrink(TTFont(src / master, recalcTimestamp=False), ITALIC_RANGES)
        path = OUT / out_name
        font.save(path)
        rows.append(("dejavu", weight, font["head"].unitsPerEm, table(font, ITALIC_RANGES)))
        print(f"{path.name:30s} {path.stat().st_size:>10,}")
    global METRICS
    saved, METRICS = METRICS, METRICS_ITALIC
    try:
        write_metrics(rows)
    finally:
        METRICS = saved


# Web faces: typeface, licence file, and per style (regular, bold, italic,
# bold_italic) the master and the axes to instance it at, or None where the
# family has no such face. Variable masters name only the axes that matter;
# any other axis stays at its fvar default (Source Serif 4's opsz at 20, its
# text size).
WEB_FACES = [
    ("arimo", "ARIMO-OFL.txt", {
        "regular": ("Arimo-var.ttf", {"wght": 400}),
        "bold": ("Arimo-var.ttf", {"wght": 700}),
        "italic": ("Arimo-Italic-var.ttf", {"wght": 400}),
        "bold_italic": ("Arimo-Italic-var.ttf", {"wght": 700}),
    }),
    ("tinos", "TINOS-OFL.txt", {
        "regular": ("Tinos-Regular.ttf", None),
        "bold": ("Tinos-Bold.ttf", None),
        "italic": ("Tinos-Italic.ttf", None),
        "bold_italic": ("Tinos-BoldItalic.ttf", None),
    }),
    ("cousine", "COUSINE-OFL.txt", {
        "regular": ("Cousine-Regular.ttf", None),
        "bold": ("Cousine-Bold.ttf", None),
        "italic": ("Cousine-Italic.ttf", None),
        "bold_italic": ("Cousine-BoldItalic.ttf", None),
    }),
    ("gelasio", "GELASIO-OFL.txt", {
        "regular": ("Gelasio-var.ttf", {"wght": 400}),
        "bold": ("Gelasio-var.ttf", {"wght": 700}),
        "italic": ("Gelasio-Italic-var.ttf", {"wght": 400}),
        "bold_italic": ("Gelasio-Italic-var.ttf", {"wght": 700}),
    }),
    ("carlito", "CARLITO-OFL.txt", {
        "regular": ("Carlito-Regular.ttf", None),
        "bold": ("Carlito-Bold.ttf", None),
        "italic": ("Carlito-Italic.ttf", None),
        "bold_italic": ("Carlito-BoldItalic.ttf", None),
    }),
    ("caladea", "CALADEA-OFL.txt", {
        "regular": ("Caladea-Regular.ttf", None),
        "bold": ("Caladea-Bold.ttf", None),
        "italic": ("Caladea-Italic.ttf", None),
        "bold_italic": ("Caladea-BoldItalic.ttf", None),
    }),
    ("lato", "LATO-OFL.txt", {
        "regular": ("Lato-Regular.ttf", None),
        "bold": ("Lato-Bold.ttf", None),
        "italic": ("Lato-Italic.ttf", None),
        "bold_italic": ("Lato-BoldItalic.ttf", None),
    }),
    ("sourcesans", "SOURCESANS3-OFL.txt", {
        "regular": ("SourceSans3-var.ttf", {"wght": 400}),
        "bold": ("SourceSans3-var.ttf", {"wght": 700}),
        "italic": ("SourceSans3-Italic-var.ttf", {"wght": 400}),
        "bold_italic": ("SourceSans3-Italic-var.ttf", {"wght": 700}),
    }),
    ("sourceserif", "SOURCESERIF4-OFL.txt", {
        "regular": ("SourceSerif4-var.ttf", {"wght": 400}),
        "bold": ("SourceSerif4-var.ttf", {"wght": 700}),
        "italic": ("SourceSerif4-Italic-var.ttf", {"wght": 400}),
        "bold_italic": ("SourceSerif4-Italic-var.ttf", {"wght": 700}),
    }),
    ("poppins", "POPPINS-OFL.txt", {
        "regular": ("Poppins-Regular.ttf", None),
        "bold": ("Poppins-Bold.ttf", None),
        "italic": ("Poppins-Italic.ttf", None),
        "bold_italic": ("Poppins-BoldItalic.ttf", None),
    }),
    ("montserrat", "MONTSERRAT-OFL.txt", {
        "regular": ("Montserrat-var.ttf", {"wght": 400}),
        "bold": ("Montserrat-var.ttf", {"wght": 700}),
        "italic": ("Montserrat-Italic-var.ttf", {"wght": 400}),
        "bold_italic": ("Montserrat-Italic-var.ttf", {"wght": 700}),
    }),
    ("playfair", "PLAYFAIRDISPLAY-OFL.txt", {
        "regular": ("PlayfairDisplay-var.ttf", {"wght": 400}),
        "bold": ("PlayfairDisplay-var.ttf", {"wght": 700}),
        "italic": ("PlayfairDisplay-Italic-var.ttf", {"wght": 400}),
        "bold_italic": ("PlayfairDisplay-Italic-var.ttf", {"wght": 700}),
    }),
    ("jetbrainsmono", "JETBRAINSMONO-OFL.txt", {
        "regular": ("JetBrainsMono-var.ttf", {"wght": 400}),
        "bold": ("JetBrainsMono-var.ttf", {"wght": 700}),
        "italic": ("JetBrainsMono-Italic-var.ttf", {"wght": 400}),
        "bold_italic": ("JetBrainsMono-Italic-var.ttf", {"wght": 700}),
    }),
]
WEB_STYLES = ["regular", "bold", "italic", "bold_italic"]
METRICS_WEB = HERE.parents[1] / "scene" / "src" / "metrics_web.rs"


def instance(path, axes):
    """A static face of `path`: the named axes pinned, every other axis at its
    fvar default. A static master is returned as is."""
    font = TTFont(path, recalcTimestamp=False)
    if axes is None:
        return font
    location = {a.axisTag: a.defaultValue for a in font["fvar"].axes}
    location.update(axes)
    return instancer.instantiateVariableFont(font, location)

KERNING = HERE.parents[1] / "scene" / "src" / "kerning_data.rs"
# The codepoints kerning is tabulated between: ASCII, Latin-1 and common
# punctuation (dashes, quotes, bullet, ellipsis, per mille, guillemets).
KERN_RANGES = [(0x20, 0x7E), (0xA0, 0xFF), (0x2010, 0x2027), (0x2030, 0x203A)]


def kern_lookups(font):
    """The PairPos subtables of the `kern` feature, grouped by lookup, in the
    order HarfBuzz applies them (lookup index order)."""
    if "GPOS" not in font:
        return []
    gpos = font["GPOS"].table
    indices = set()
    for record in gpos.FeatureList.FeatureRecord:
        if record.FeatureTag == "kern":
            indices.update(record.Feature.LookupListIndex)
    lookups = []
    for index in sorted(indices):
        lookup = gpos.LookupList.Lookup[index]
        subtables = []
        for sub in lookup.SubTable:
            if sub.LookupType == 9:  # extension: unwrap
                sub = sub.ExtSubTable
            if sub.LookupType == 2:
                subtables.append(sub)
        if subtables:
            lookups.append(subtables)
    return lookups


def extract_kerning(font, unicodes):
    """`{(left codepoint, right codepoint): x-advance adjustment in font units}`
    for every pair of `unicodes` the face's `kern` feature adjusts. Within one
    lookup the first subtable whose coverage holds the left glyph (and, for a
    glyph-pair subtable, whose pair set holds the right glyph) settles the
    pair, as in HarfBuzz; separate lookups add up."""
    cmap = font.getBestCmap()
    glyphs = {}  # glyph name -> codepoints it serves
    for c in unicodes:
        name = cmap.get(c)
        if name is not None:
            glyphs.setdefault(name, []).append(c)
    total = {}
    for subtables in kern_lookups(font):
        settled = {}
        for sub in subtables:
            coverage = sub.Coverage.glyphs
            if sub.Format == 1:
                for first, pair_set in zip(coverage, sub.PairSet):
                    if first not in glyphs:
                        continue
                    for record in pair_set.PairValueRecord:
                        second = record.SecondGlyph
                        if second not in glyphs or (first, second) in settled:
                            continue
                        value = record.Value1
                        settled[(first, second)] = getattr(value, "XAdvance", 0) if value else 0
            elif sub.Format == 2:
                class1 = sub.ClassDef1.classDefs
                class2 = sub.ClassDef2.classDefs
                covered = [g for g in coverage if g in glyphs]
                for first in covered:
                    c1 = class1.get(first, 0)
                    if c1 >= sub.Class1Count:
                        continue
                    row = sub.Class1Record[c1].Class2Record
                    for second in glyphs:
                        if (first, second) in settled:
                            continue
                        c2 = class2.get(second, 0)
                        if c2 >= sub.Class2Count:
                            continue
                        value = row[c2].Value1
                        settled[(first, second)] = getattr(value, "XAdvance", 0) if value else 0
        for pair, value in settled.items():
            total[pair] = total.get(pair, 0) + value
    pairs = {}
    for (first, second), value in total.items():
        if value == 0:
            continue
        for a in glyphs[first]:
            for b in glyphs[second]:
                pairs[(a, b)] = value
    return pairs


def write_kerning(rows, families, path=KERNING, flag="--web", faces="web faces'"):
    """`rows`: (face, style, pairs); `families`: (FACE, [ident or None; 4])."""
    with open(path, "w") as out:
        out.write(f"// Generated by crates/graphics/render/assets/build-fonts.py {flag}; do not edit.\n")
        out.write(f"// Pair kerning of the {faces} `kern` feature, for the codepoints of\n")
        out.write("// KERN_RANGES. Per face: `_LEFTS` is (left codepoint, start index), sorted by\n")
        out.write("// codepoint; that left's pairs are `_RIGHTS[start..next start]` (sorted right\n")
        out.write("// codepoints) and `_VALUES` at the same indices (x-advance adjustment in font\n")
        out.write("// units). One `[Option<..>; 4]` per family indexed by `bold + 2 * italic`.\n")
        out.write("pub type Pairs = Option<(&'static [(u16, u32)], &'static [u16], &'static [i16])>;\n")

        def rows_of(out, items, per):
            for i in range(0, len(items), per):
                out.write("    " + "".join(f"{x}," for x in items[i:i + per]) + "\n")

        for face, style, pairs in rows:
            if not pairs:
                continue
            ident = f"{face}_{style}".upper()
            ordered = sorted(pairs.items())
            lefts, last = [], None
            for i, ((a, _b), _v) in enumerate(ordered):
                if a != last:
                    lefts.append(f"({a},{i})")
                    last = a
            out.write(f"pub static {ident}_LEFTS: &[(u16, u32)] = &[\n")
            rows_of(out, lefts, 12)
            out.write("];\n")
            out.write(f"pub static {ident}_RIGHTS: &[u16] = &[\n")
            rows_of(out, [b for (_a, b), _v in ordered], 24)
            out.write("];\n")
            out.write(f"pub static {ident}_VALUES: &[i16] = &[\n")
            rows_of(out, [v for _k, v in ordered], 24)
            out.write("];\n")
        kerned = {f"{face}_{style}".upper() for face, style, pairs in rows if pairs}
        for ident, present in families:
            cells = ", ".join(f"Some(({p}_LEFTS, {p}_RIGHTS, {p}_VALUES))" if p in kerned else "None"
                              for p in present)
            out.write(f"pub static {ident}: [Pairs; 4] = [{cells}];\n")


def build_kerning(src):
    """Just `kerning_data.rs`, from the same instanced masters `build_web` uses."""
    src = Path(src)
    rows, families = [], []
    unicodes = codepoints(KERN_RANGES)
    for face, _licence, styles in WEB_FACES:
        present = []
        for style in WEB_STYLES:
            source = styles.get(style)
            if source is None:
                present.append(None)
                continue
            master, axes = source
            pairs = extract_kerning(instance(src / master, axes), unicodes)
            rows.append((face, style, pairs))
            present.append(f"{face}_{style}".upper())
            print(f"{face}-{style.replace('_', '-'):24s} {len(pairs):>6} kern pairs")
        families.append((face.upper(), present))
    write_kerning(rows, families)
    print(f"{KERNING.name}: {KERNING.stat().st_size:,} bytes")


KERNING_DEJAVU = HERE.parents[1] / "scene" / "src" / "kerning_dejavu.rs"
# DejaVu Sans's four faces: the masters beside this script for the uprights, the
# obliques from what `fetch-noto-sources.py <dir>` downloads.
DEJAVU_KERN_FACES = {
    "regular": HERE / "DejaVuSans.ttf",
    "bold": HERE / "DejaVuSans-Bold.ttf",
    "italic": "DejaVuSans-Oblique.ttf",
    "bold_italic": "DejaVuSans-BoldOblique.ttf",
}


METRICS_DEJAVU = HERE.parents[1] / "scene" / "src" / "metrics_dejavu.rs"


def build_dejavu_web_advances():
    """`metrics_dejavu.rs`: the advances of every codepoint the committed DejaVu
    subsets cover beyond `WIDE` (symbols, arrows, maths, box drawing, dingbats,
    ...). `metrics_data.rs` tabulates `WIDE` only, and native text measures the
    rest at 0.6 em; web content measures them by these, as Chromium does."""
    rows = []
    wide = set(codepoints(WIDE))
    for weight, _master, out_name in DEJAVU:
        font = TTFont(OUT / out_name)
        extra = [(c, a) for c, a in table(font, DEJAVU_RANGES) if c not in wide]
        rows.append(("dejavu", f"{weight}_web", font["head"].unitsPerEm, extra))
        print(f"{out_name:22s} {len(extra):>6} advances beyond WIDE")
    global METRICS
    saved, METRICS = METRICS, METRICS_DEJAVU
    try:
        write_metrics(rows)
    finally:
        METRICS = saved


def build_dejavu_kerning(src):
    """`kerning_dejavu.rs`: the `kern` feature of DejaVu Sans's four faces, read
    from the masters (the committed subsets keep no layout tables)."""
    src = Path(src)
    rows, present = [], []
    unicodes = codepoints(KERN_RANGES)
    for style in WEB_STYLES:
        pairs = extract_kerning(TTFont(src / DEJAVU_KERN_FACES[style]), unicodes)
        rows.append(("dejavu", style, pairs))
        present.append(f"DEJAVU_{style}".upper())
        print(f"dejavu-{style.replace('_', '-'):24s} {len(pairs):>6} kern pairs")
    write_kerning(rows, [("DEJAVU", present)], KERNING_DEJAVU, "--kern-dejavu", "DejaVu Sans faces'")
    print(f"{KERNING_DEJAVU.name}: {KERNING_DEJAVU.stat().st_size:,} bytes")


def build_web(src):
    src = Path(src)
    rows, families, kern_rows = [], [], []
    for face, licence, styles in WEB_FACES:
        present = []
        for style in WEB_STYLES:
            source = styles.get(style)
            if source is None:
                present.append(None)
                continue
            master, axes = source
            font = instance(src / master, axes)
            # Read the kern pairs before subsetting: `shrink` keeps no layout tables.
            kern_rows.append((face, style, extract_kerning(font, codepoints(KERN_RANGES))))
            font = shrink(font, DEJAVU_RANGES)
            path = OUT / f"{face}-{style.replace('_', '-')}.ttf"
            font.save(path)
            advances = table(font, DEJAVU_RANGES)
            rows.append((face, style, font["head"].unitsPerEm, advances))
            present.append(f"{face}_{style}".upper())
            print(f"{path.name:30s} {(src / master).stat().st_size:>10,} -> {path.stat().st_size:>8,}"
                  f"   {len(advances):>5} codepoints")
        families.append((face.upper(), present))
        (OUT / licence).write_bytes((src / licence).read_bytes())
    with open(METRICS_WEB, "w") as out:
        out.write("// Generated by crates/graphics/render/assets/build-fonts.py --web; do not edit.\n")
        out.write("// (codepoint, advance in font units), sorted by codepoint.\n")
        out.write("pub type Face = Option<(&'static [(u32, u16)], u32)>;\n")
        for face, weight, upem, advances in rows:
            ident = f"{face}_{weight}".upper()
            out.write(f"pub const {ident}_UPEM: u32 = {upem};\n")
            out.write(f"pub static {ident}: &[(u32, u16)] = &[\n")
            for i in range(0, len(advances), 8):
                out.write("    " + " ".join(f"({c}, {a})," for c, a in advances[i:i + 8]) + "\n")
            out.write("];\n")
        out.write("// A family's faces by `bold + 2 * italic`; `None` where it has no such file.\n")
        for ident, present in families:
            cells = ", ".join("None" if p is None else f"Some(({p}, {p}_UPEM))" for p in present)
            out.write(f"pub static {ident}: [Face; 4] = [{cells}];\n")
    write_kerning(kern_rows, families)


def main(argv):
    OUT.mkdir(exist_ok=True)
    if argv[:1] == ["--noto"]:
        build_noto(argv[1])
        return
    if argv[:1] == ["--web"]:
        build_web(argv[1])
        return
    if argv[:1] == ["--kern"]:
        build_kerning(argv[1])
        return
    if argv[:1] == ["--dejavu-web"]:
        build_dejavu_web_advances()
        return
    if argv[:1] == ["--kern-dejavu"]:
        build_dejavu_kerning(argv[1])
        return
    dejavu_only = "--dejavu-only" in argv
    rows = []
    if not dejavu_only:
        src = Path(argv[0])
        for face, weight, source, axes in FACES:
            font = instancer.instantiateVariableFont(TTFont(src / source), axes, inplace=False)
            shrink(font, UI_RANGES)
            path = OUT / f"{face}-{weight}.ttf"
            font.save(path)
            rows.append((face, weight, font["head"].unitsPerEm, table(font, UI_RANGES)))
            print(f"{path.name:22s} {'':>8}    {path.stat().st_size:>8,}")
    build_dejavu(rows)
    if dejavu_only:
        # The UI-face rows are unchanged; keep the existing table as authored.
        print("--dejavu-only: metrics_data.rs left untouched")
        return
    write_metrics(rows)


if __name__ == "__main__":
    main(sys.argv[1:])
