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
*fallback* face, so a codepoint it lacks has nowhere else to go and renders as
`.notdef`. The set is therefore drawn to cover anything an actor can type or a
simulated service can emit, not merely what today's fixtures happen to show:

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

Outside that set a character renders as DejaVu's `.notdef` box (the subsetter
keeps its outline via `notdef_outline`), which is visible and self-explanatory;
`dejavu_subset_draws_notdef_for_uncovered_text` in `src/lib.rs` pins that
behaviour so an uncovered codepoint can never silently vanish. Subsetting does
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

It also writes `crates/scene/src/text/coverage.rs`, the cmap coverage of the
DejaVu faces as ranges, which is how layout decides where the fallback chain
leaves DejaVu without embedding DejaVu in the scene crate.

Usage:
  build-fonts.py <source-dir>   rebuild everything (requires the variable fonts)
  build-fonts.py --dejavu-only  rebuild just the DejaVu subsets and the metrics
                                table, using only files already in the tree
  build-fonts.py --noto <dir>   rebuild the Noto faces, the pack and its stubs
                                from the masters `fetch-noto-sources.py <dir>`
                                downloads (pinned commit, pinned SHA-256)
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
        # The mono face drives the fixed-cell `Text` primitive, which measures
        # by the font's own advances rather than the table.
        if weight != "mono":
            rows.append(("dejavu", weight, font["head"].unitsPerEm, table(font, WIDE)))


def write_metrics(rows):
    with open(METRICS, "w") as out:
        out.write("// Generated by crates/render/assets/build-fonts.py; do not edit.\n")
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
    font = TTFont(path)
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
        out.write("// Generated by crates/render/assets/build-fonts.py; do not edit.\n")
        out.write("// Inclusive codepoint ranges each embedded DejaVu face maps in its cmap.\n")
        for ident, name in [("DEJAVU_SANS", "dejavu-sans.ttf"), ("DEJAVU_SANS_BOLD", "dejavu-sans-bold.ttf"),
                            ("DEJAVU_MONO", "dejavu-mono.ttf")]:
            ranges = ranges_of(TTFont(OUT / name).getBestCmap())
            out.write(f"pub static {ident}: &[(u32, u32)] = &[\n")
            for i in range(0, len(ranges), 6):
                out.write("    " + " ".join(f"({a}, {b})," for a, b in ranges[i:i + 6]) + "\n")
            out.write("];\n")


def build_noto(src):
    src = Path(src)
    for name, master, ranges in NOTO_CORE:
        for weight, wght in NOTO_WEIGHTS:
            font = instancer.instantiateVariableFont(TTFont(src / master), {"wght": wght, "wdth": 100})
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
        font = instancer.instantiateVariableFont(TTFont(src / master), axes)
        noto_subset(font, unicodes, layout)
        path = OUT / "pack" / out_name
        font.save(path)
        stub(path, OUT / "stubs" / out_name)
        print(f"{'pack/' + out_name:30s} {path.stat().st_size:>10,}   stub "
              f"{(OUT / 'stubs' / out_name).stat().st_size:>8,}")
    for text in src.glob("NOTO*-OFL.txt"):
        (OUT / text.name).write_bytes(text.read_bytes())
    write_coverage()


def main(argv):
    OUT.mkdir(exist_ok=True)
    if argv[:1] == ["--noto"]:
        build_noto(argv[1])
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
