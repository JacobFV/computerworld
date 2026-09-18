#!/usr/bin/env python3
"""Download the upstream Noto masters that `build-fonts.py --noto` subsets.

Every file comes from the google/fonts repository at one pinned commit and is
checked against a pinned SHA-256, so a rebuild starts from byte-identical
inputs or fails. Nothing here runs at build or render time; the renderer only
ever sees the committed outputs under `fonts/`.

Usage: fetch-noto-sources.py <dest-dir>
"""
import hashlib
import sys
import urllib.parse
import urllib.request
from pathlib import Path

COMMIT = "b346dc3e18bed8b4ca602e00537eb35d76ed5025"  # google/fonts main, 2026-09-18
BASE = f"https://raw.githubusercontent.com/google/fonts/{COMMIT}/ofl/"
# (path under ofl/, local name, sha256)
SOURCES = [
    ("notosanshebrew/NotoSansHebrew[wdth,wght].ttf", "NotoSansHebrew-var.ttf",
     "7ef36a2c3593758cdb622e1bdef4f84523e92fbc3ccc667438dd80ff54c2de88"),
    ("notosansarabic/NotoSansArabic[wdth,wght].ttf", "NotoSansArabic-var.ttf",
     "63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e"),
    ("notosansthai/NotoSansThai[wdth,wght].ttf", "NotoSansThai-var.ttf",
     "5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8"),
    ("notosansdevanagari/NotoSansDevanagari[wdth,wght].ttf", "NotoSansDevanagari-var.ttf",
     "14ec4af41f27482216d1c2229f417ff9b1425e1babb014e57d1d40d03229853e"),
    ("notosanssc/NotoSansSC[wght].ttf", "NotoSansSC-var.ttf",
     "a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da"),
    ("notosanskr/NotoSansKR[wght].ttf", "NotoSansKR-var.ttf",
     "194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252"),
    ("notoemoji/NotoEmoji[wght].ttf", "NotoEmoji-var.ttf",
     "de6c18832938afc99caf132b39d6a30a19bac7f2e812e28db2535b4608d27551"),
    ("notosanshebrew/OFL.txt", "NOTOSANSHEBREW-OFL.txt",
     "9b9fe028b5ba74d231659a1bbaf0ed09b11e759d1ca6a070999e16d151616b47"),
    ("notosansarabic/OFL.txt", "NOTOSANSARABIC-OFL.txt",
     "07fc70bfeb985cc1a87a8587d0a0c80bab11c86c9dc3fd95b6f0cb332f983e96"),
    ("notosansthai/OFL.txt", "NOTOSANSTHAI-OFL.txt",
     "2e98fd23a52d253db8612cd5942c8f2ff4111b21d2367050fdca91d8ccc374a0"),
    ("notosansdevanagari/OFL.txt", "NOTOSANSDEVANAGARI-OFL.txt",
     "a216f6f8d85c7228093e0ee5e258d9d377e6671f68acb4db1930b29583d0f331"),
    ("notosanssc/OFL.txt", "NOTOSANSSC-OFL.txt",
     "1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9"),
    ("notosanskr/OFL.txt", "NOTOSANSKR-OFL.txt",
     "1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9"),
    ("notoemoji/OFL.txt", "NOTOEMOJI-OFL.txt",
     "500bb1ccf43df7bbb522112f9133a52b16e1c35e809632f5d8609b179152de5b"),
]


def main(argv):
    if len(argv) != 1:
        sys.exit(__doc__)
    dest = Path(argv[0])
    dest.mkdir(parents=True, exist_ok=True)
    failed = False
    for path, name, expected in SOURCES:
        out = dest / name
        if not out.exists():
            urllib.request.urlretrieve(BASE + urllib.parse.quote(path), out)
        digest = hashlib.sha256(out.read_bytes()).hexdigest()
        ok = digest == expected
        failed |= not ok
        print(f"{'ok ' if ok else 'BAD'} {digest} {out.stat().st_size:>10,} {name}")
    if failed:
        sys.exit("a source does not match its pinned SHA-256")


if __name__ == "__main__":
    main(sys.argv[1:])
