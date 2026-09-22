#!/usr/bin/env python3
"""Download the upstream masters that `build-fonts.py --noto` builds from.

Every file comes from a pinned revision and is checked against a pinned
SHA-256, so a rebuild starts from byte-identical inputs or fails:

  * google/fonts at one commit: the Noto Sans script and CJK faces, and the
    italic masters of the per-platform UI families (Inter, Open Sans, Roboto,
    Ubuntu Sans);
  * googlefonts/noto-emoji at its v2.051 tag: Noto Color Emoji in its COLRv1
    (vector) build;
  * the DejaVu 2.37 release tarball, from which the two oblique sans masters
    are extracted.

Nothing here runs at build or render time; the renderer only ever sees the
committed outputs under `fonts/`.

Usage: fetch-noto-sources.py <dest-dir>
"""
import hashlib
import sys
import tarfile
import urllib.parse
import urllib.request
from pathlib import Path

HOST = "https://raw.githubusercontent.com/"
COMMIT = "b346dc3e18bed8b4ca602e00537eb35d76ed5025"  # google/fonts main, 2026-09-18
BASE = f"{HOST}google/fonts/{COMMIT}/"
EMOJI_COMMIT = "8998f5dd683424a73e2314a8c1f1e359c19e8742"  # googlefonts/noto-emoji v2.051
EMOJI = f"{HOST}googlefonts/noto-emoji/{EMOJI_COMMIT}/"
DEJAVU = "https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/"
# (url, local name, sha256)
SOURCES = [(BASE + "ofl/" + path, name, sha) for path, name, sha in [
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
    # Locale forms of Han (Traditional Chinese, Japanese) for the font pack.
    ("notosanstc/NotoSansTC[wght].ttf", "NotoSansTC-var.ttf",
     "864727d210d54f2537bbe23b3a839436c3992af72de9322af5270897246bd44f"),
    ("notosansjp/NotoSansJP[wght].ttf", "NotoSansJP-var.ttf",
     "c2f3b4d463500a2ddcd3849cded1fceeb9fd6d1c32e6cbecd568453ba50fc68f"),
    ("notosanstc/OFL.txt", "NOTOSANSTC-OFL.txt",
     "1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9"),
    ("notosansjp/OFL.txt", "NOTOSANSJP-OFL.txt",
     "1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9"),
    # Further scripts; `build-fonts.py` documents which are embedded and which
    # are in the pack.
    ("notosansgeorgian/NotoSansGeorgian[wdth,wght].ttf", "NotoSansGeorgian-var.ttf",
     "dc591156f36842d38996c4a7a17fee9bb58e45da3e2cac7a31b7d33de700adb9"),
    ("notosansarmenian/NotoSansArmenian[wdth,wght].ttf", "NotoSansArmenian-var.ttf",
     "0870908d8318435a5daf1cd280ae15063f990cd9ad60f3e94c734ce9e1ffef71"),
    ("notosansbengali/NotoSansBengali[wdth,wght].ttf", "NotoSansBengali-var.ttf",
     "dcd42978094e584a849c84a51450eeac40c8826057d566ea6d4b9627a403a05a"),
    ("notosanstamil/NotoSansTamil[wdth,wght].ttf", "NotoSansTamil-var.ttf",
     "aa3a9b321f4b0bb2c40203ffbde9af89713227866e0e13f76e5b9eeea727cf88"),
    ("notosansgurmukhi/NotoSansGurmukhi[wdth,wght].ttf", "NotoSansGurmukhi-var.ttf",
     "1e6f728fa620e566f842d81e220265813faa12771214765d289c98e035adc5f2"),
    ("notosanslao/NotoSansLao[wdth,wght].ttf", "NotoSansLao-var.ttf",
     "9608b94603a82d09a8038946f9775242f99e3b3459b7f1e4d5b335b578cd7ab3"),
    ("notosanskhmer/NotoSansKhmer[wdth,wght].ttf", "NotoSansKhmer-var.ttf",
     "f37a8431a0c5d5ed2f81a767417546aca576a81fb7eff9c924d46aecf828f2ca"),
    ("notosansgujarati/NotoSansGujarati[wdth,wght].ttf", "NotoSansGujarati-var.ttf",
     "9901d8552f1dd5d2c50dbd4caa6f6e174e74e8264f06594ab259ae6e7b1ac428"),
    ("notosansethiopic/NotoSansEthiopic[wdth,wght].ttf", "NotoSansEthiopic-var.ttf",
     "0dbccc00b22d180ebd6a4bd8a733918a29e709fa6798023adc3e6cd40da65077"),
    ("notosansmyanmar/NotoSansMyanmar[wdth,wght].ttf", "NotoSansMyanmar-var.ttf",
     "7abbbfbe2514105d7ce94937aee3feb2ba89b73a256c8b77b5866bd9b83e32ec"),
    ("notosanssinhala/NotoSansSinhala[wdth,wght].ttf", "NotoSansSinhala-var.ttf",
     "9bd93e407a278075be403324063bc94a7e306c44de4df81214e932330c22eecf"),
    ("notosansgeorgian/OFL.txt", "NOTOSANSGEORGIAN-OFL.txt",
     "8c02263c5d73d40544f9ed91e30c4e947407057a3cc430d7b786189aeceff6df"),
    ("notosansarmenian/OFL.txt", "NOTOSANSARMENIAN-OFL.txt",
     "0468358b316f69f405b55cadf8a8314e16e3610b8feaad96772bd5d968112d02"),
    ("notosansbengali/OFL.txt", "NOTOSANSBENGALI-OFL.txt",
     "754f0e221aa7d5a915489f3bf1f20fe53ddc35ab2834a4d91656d78f9622de70"),
    ("notosanstamil/OFL.txt", "NOTOSANSTAMIL-OFL.txt",
     "f8ff8ce7d0a81bf8d5e121c635ef027250c531f2fd37d5988b8dd6e45f19d7f1"),
    ("notosansgurmukhi/OFL.txt", "NOTOSANSGURMUKHI-OFL.txt",
     "3f7451b7e2c8381be0c5712f7b0dd5c2d75fe787ae16a99f18b7fa45627a0fde"),
    ("notosanslao/OFL.txt", "NOTOSANSLAO-OFL.txt",
     "a42993999944845fb5af693ea678a372a053db1f0981d55e911dcfa9d330f279"),
    ("notosanskhmer/OFL.txt", "NOTOSANSKHMER-OFL.txt",
     "be0407f060aea48787ff9e75d8d3aedef70aef113b3ce9aca26fdaacd10b1870"),
    ("notosansgujarati/OFL.txt", "NOTOSANSGUJARATI-OFL.txt",
     "c0b88977aa18b5e4fd05d646d560da89fde61b6581fa4507cb00dd90bd1bf7d4"),
    ("notosansethiopic/OFL.txt", "NOTOSANSETHIOPIC-OFL.txt",
     "72606b23f312cb25973958f2892d4d2c2012deabadbf0f763232624a9649fc69"),
    ("notosansmyanmar/OFL.txt", "NOTOSANSMYANMAR-OFL.txt",
     "246a75859267af7da466823969d2e2b407ed8455ee5f74f4c8d63d8783be9b57"),
    ("notosanssinhala/OFL.txt", "NOTOSANSSINHALA-OFL.txt",
     "2d6f7c43bce61f4b1919379f901bc613484f5285f520b6d29bb7c1f31b17e841"),
    # Italic masters of the platform UI families. Their licences are the same
    # files as the upright faces' (`fonts/{INTER,OPENSANS,ROBOTO}-OFL.txt`,
    # `fonts/UBUNTU-UFL.txt`).
    ("inter/Inter-Italic[opsz,wght].ttf", "Inter-Italic-var.ttf",
     "acd98e64795781b2058f07b18475e0ecee2a0fe2b42a49e2f9e37d0d6bf66ce6"),
    ("opensans/OpenSans-Italic[wdth,wght].ttf", "OpenSans-Italic-var.ttf",
     "fe269381e992f32e135801740998544d6235061e37c93ec067ad2be3edd5b17b"),
    ("roboto/Roboto-Italic[wdth,wght].ttf", "Roboto-Italic-var.ttf",
     "9725a847af6b460ffca162ae66d20dad48b01876137947180b42d7dcd7887182"),
]] + [
    (BASE + "ufl/ubuntusans/UbuntuSans-Italic[wdth,wght].ttf", "UbuntuSans-Italic-var.ttf",
     "603f7a4b837143b742a8df3ad3ad80164a50e4529f02e3f6aa533b78e209d3df"),
    (EMOJI + "fonts/Noto-COLRv1.ttf", "Noto-COLRv1.ttf",
     "0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2"),
    (EMOJI + "fonts/LICENSE", "NOTOCOLOREMOJI-OFL.txt",
     "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2"),
    (DEJAVU + "dejavu-fonts-ttf-2.37.tar.bz2", "dejavu-fonts-ttf-2.37.tar.bz2",
     "fa9ca4d13871dd122f61258a80d01751d603b4d3ee14095d65453b4e846e17d7"),
]
# (archive, member, local name, sha256)
EXTRACT = [
    ("dejavu-fonts-ttf-2.37.tar.bz2", "dejavu-fonts-ttf-2.37/ttf/DejaVuSans-Oblique.ttf",
     "DejaVuSans-Oblique.ttf", "4af75fa16ee6d3ad43e1ecec41862c24954af26a55c6bb1ebb27bd486a50f5f4"),
    ("dejavu-fonts-ttf-2.37.tar.bz2", "dejavu-fonts-ttf-2.37/ttf/DejaVuSans-BoldOblique.ttf",
     "DejaVuSans-BoldOblique.ttf", "eb436dca0c2594b73d8b603b892e374fdfd8d885d25ffb4f18df4c4c0b49e50f"),
]


def check(path, expected):
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    ok = digest == expected
    print(f"{'ok ' if ok else 'BAD'} {digest} {path.stat().st_size:>10,} {path.name}")
    return ok


def main(argv):
    if len(argv) != 1:
        sys.exit(__doc__)
    dest = Path(argv[0])
    dest.mkdir(parents=True, exist_ok=True)
    failed = False
    for url, name, expected in SOURCES:
        out = dest / name
        if not out.exists():
            head, tail = url.rsplit("/", 1)
            urllib.request.urlretrieve(head + "/" + urllib.parse.quote(tail), out)
        failed |= not check(out, expected)
    for archive, member, name, expected in EXTRACT:
        out = dest / name
        if not out.exists():
            with tarfile.open(dest / archive) as tar:
                out.write_bytes(tar.extractfile(member).read())
        failed |= not check(out, expected)
    if failed:
        sys.exit("a source does not match its pinned SHA-256")


if __name__ == "__main__":
    main(sys.argv[1:])
