#!/usr/bin/env python3
"""Download the upstream masters that `build-fonts.py --web` builds from.

The web faces are the families pages ask for by name, or metric-compatible
stand-ins for the ones that cannot be bundled: Arimo for Arial and Helvetica,
Tinos for Times New Roman, Cousine for Courier New, Gelasio for Georgia, Carlito
for Calibri, Caladea for Cambria, and Lato, Source Sans 3, Source Serif 4,
Poppins, Montserrat, Playfair Display and JetBrains Mono as themselves. All are
SIL OFL 1.1; `cw_scene::fonts` maps `font-family` lists onto them.

Every file comes from a pinned revision and is checked against a pinned SHA-256,
so a rebuild starts from byte-identical inputs or fails — the same contract as
`fetch-noto-sources.py`, and the same google/fonts commit. Tinos's directory in
google/fonts carries no `OFL.txt` at that commit, so its licence text is taken
from the Tinos project's own repository at a pinned commit instead.

Nothing here runs at build or render time; the renderer only ever sees the
committed outputs under `fonts/`.

Usage: fetch-web-sources.py <dest-dir>
"""
import hashlib
import sys
import urllib.parse
import urllib.request
from pathlib import Path

HOST = "https://raw.githubusercontent.com/"
COMMIT = "b346dc3e18bed8b4ca602e00537eb35d76ed5025"  # google/fonts main, 2026-09-18
BASE = f"{HOST}google/fonts/{COMMIT}/ofl/"
TINOS_COMMIT = "3b4482a99b80ea5fc75f187b1be3120a3f5905b3"  # googlefonts/tinos main, 2026-04-27
# (url, local name, sha256)
SOURCES = [(BASE + path, name, sha) for path, name, sha in [
    # Arimo: Arial and Helvetica stand-in (variable, wght 400-700).
    ("arimo/Arimo[wght].ttf", "Arimo-var.ttf",
     "e43898b143ec826ac8cb4034816458a7047fbe0836558de2a1f8c6223ae3e0ca"),
    ("arimo/Arimo-Italic[wght].ttf", "Arimo-Italic-var.ttf",
     "a80fc54fd0233c1dfe298577c4d00f5ae81d5bb83510975e473c47e699b7f4ed"),
    ("arimo/OFL.txt", "ARIMO-OFL.txt",
     "11cce536cd2f3864d767003af5dcd739e2e15818cf2279b6175edeadd3960992"),
    # Tinos: Times New Roman stand-in (static).
    ("tinos/Tinos-Regular.ttf", "Tinos-Regular.ttf",
     "60a0e8ef0c04dd5dd69ffe91025fa2ae5836cbd35600a82ba031977557e2cb61"),
    ("tinos/Tinos-Bold.ttf", "Tinos-Bold.ttf",
     "393269dbab8899f938db19783eca5eac92eb431f7ae0ab45b8349ca895f1a06b"),
    ("tinos/Tinos-Italic.ttf", "Tinos-Italic.ttf",
     "5942266ed398b155d7dc23e36833e7ec6be988f2439bdbeb8ef1bede808eaa91"),
    ("tinos/Tinos-BoldItalic.ttf", "Tinos-BoldItalic.ttf",
     "a5de79f0fe863ea0954757acb3d47b3ccd0a930ce3dd5b97230cd3866790a06e"),
    # Cousine: Courier New stand-in (static).
    ("cousine/Cousine-Regular.ttf", "Cousine-Regular.ttf",
     "1da22250675fc4c42fcf3a9736c44bc0570516105331443b663fd5cfbd1412fe"),
    ("cousine/Cousine-Bold.ttf", "Cousine-Bold.ttf",
     "17c8a7245156d2253531c9e529474937b09d9f641c5ae7695c5e33f22822eef4"),
    ("cousine/Cousine-Italic.ttf", "Cousine-Italic.ttf",
     "ea2a76ae3d0ece9cd59f0d30fdc08dd70e8f5f457beee5b0852a7b50c2286c7c"),
    ("cousine/Cousine-BoldItalic.ttf", "Cousine-BoldItalic.ttf",
     "848e858726fee0ae27b754e4cd6a2755209bf1428a8c91f747696d58c33906c3"),
    ("cousine/OFL.txt", "COUSINE-OFL.txt",
     "b81c4d4dc0a9f72c9155e78187316e016e2012a8102468804173dc61468b906d"),
    # Gelasio: Georgia stand-in (variable, wght 400-700).
    ("gelasio/Gelasio[wght].ttf", "Gelasio-var.ttf",
     "4daecea457258c9ebeb8bc99ed3fd24353618bfad3ea4b93fa0b5d0468fc04e4"),
    ("gelasio/Gelasio-Italic[wght].ttf", "Gelasio-Italic-var.ttf",
     "52559e845a4d33514e5f93bb9ae7dbeae1894a53f2c565a15f18af40cd337c09"),
    ("gelasio/OFL.txt", "GELASIO-OFL.txt",
     "b393cb01867c919b44381512120dc3e4c954c7b47e2035c405f3a324799a4d29"),
    # Carlito: Calibri stand-in (static).
    ("carlito/Carlito-Regular.ttf", "Carlito-Regular.ttf",
     "f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595"),
    ("carlito/Carlito-Bold.ttf", "Carlito-Bold.ttf",
     "bb5d20f79b82599ec72983597437373a80f2d2085fa91fc144fd74e876a594db"),
    ("carlito/Carlito-Italic.ttf", "Carlito-Italic.ttf",
     "0b019225e58d702bfedcbd35c21696769f8ee115cb6343f84c2f240312450d1c"),
    ("carlito/Carlito-BoldItalic.ttf", "Carlito-BoldItalic.ttf",
     "b32928186c119599e03ca6a1ffc680fdcb7fac95772f4b95d989cf6cd3861517"),
    ("carlito/OFL.txt", "CARLITO-OFL.txt",
     "58402f82a7c332a700294988fe7554fbb0a63a8d27ccc1ee3bbc640311990a00"),
    # Caladea: Cambria stand-in (static).
    ("caladea/Caladea-Regular.ttf", "Caladea-Regular.ttf",
     "f1e899278b7b4491aba5b6a8253c4b04c050cc59b21865be5c37559a775153cd"),
    ("caladea/Caladea-Bold.ttf", "Caladea-Bold.ttf",
     "ae3cb2dcbc925809dd29d2a44e9802211cab66be541bacbfc9c08c74b27c3742"),
    ("caladea/Caladea-Italic.ttf", "Caladea-Italic.ttf",
     "4359a8e24f748b6447b1ff6d7a174febe70961d29f8bb8634b56dacd740a3deb"),
    ("caladea/Caladea-BoldItalic.ttf", "Caladea-BoldItalic.ttf",
     "ccabaa7b7e2fdf253d2b1a5fa699dd8a3df8d835a9eb285ad82631a677eb76c0"),
    ("caladea/OFL.txt", "CALADEA-OFL.txt",
     "ccdab61d371d8c8683a128a92cd7d498dbdb1d37689f7cb21f1bf6b16658d213"),
    # Lato (static; the family ships eighteen weights, four are used).
    ("lato/Lato-Regular.ttf", "Lato-Regular.ttf",
     "d636e4683231f931eda222d588e944d082bfd3bdba02f928bee461c0f185b251"),
    ("lato/Lato-Bold.ttf", "Lato-Bold.ttf",
     "8a0aace75d33794eece4b28187bfc1df0bbd2888b5d8a56e01788c8d65d16be1"),
    ("lato/Lato-Italic.ttf", "Lato-Italic.ttf",
     "e399c44efe1387100531d26c7e4800c5d12251b890d6654a3098c7c679cb1786"),
    ("lato/Lato-BoldItalic.ttf", "Lato-BoldItalic.ttf",
     "62c1b7f0d2e74b45960154c3520efc337b553db0961bfdc950d5618334596cc8"),
    ("lato/OFL.txt", "LATO-OFL.txt",
     "74ba064d03f1f1c4a952da936c3eb71866c34404916734de3cae73b34357e59e"),
    # Source Sans 3 (variable, wght 200-900).
    ("sourcesans3/SourceSans3[wght].ttf", "SourceSans3-var.ttf",
     "042fe2cc0b933e328410d7acbd0aa6a1873dca5aef81875f4bc214b08825c7b9"),
    ("sourcesans3/SourceSans3-Italic[wght].ttf", "SourceSans3-Italic-var.ttf",
     "39e3ab05ccd7cb94907c31005bb5bec1d5432f0b096a2b782976e217a540eb6c"),
    ("sourcesans3/OFL.txt", "SOURCESANS3-OFL.txt",
     "09746787287a289323b0ec3cff4d1a4a801331b82b7207c1e186f5d26619a392"),
    # Source Serif 4 (variable, wght 200-900, opsz 8-60).
    ("sourceserif4/SourceSerif4[opsz,wght].ttf", "SourceSerif4-var.ttf",
     "97b2d4da6e3cb494b5a1e66ae176914d852ccabef49e0c02c0df25f3e39aca0b"),
    ("sourceserif4/SourceSerif4-Italic[opsz,wght].ttf", "SourceSerif4-Italic-var.ttf",
     "15fbc7e4679489a501998c3669272637a6646388ef7e4bd77eebb5bf967a1f42"),
    ("sourceserif4/OFL.txt", "SOURCESERIF4-OFL.txt",
     "5f94c3fd3a23131a417ab5a0c8452de57e70c3cfb9f604d88241f7065ebf9fd9"),
    # Poppins (static; eighteen weights upstream, four used).
    ("poppins/Poppins-Regular.ttf", "Poppins-Regular.ttf",
     "7e65201e9b79159e2300267cc885e16c8dcef2424cdfa09a29bfb0980a94a7ba"),
    ("poppins/Poppins-Bold.ttf", "Poppins-Bold.ttf",
     "983676516167748b74de6f4771fb384c664fd913acb8b471122ecacf5da5ea6c"),
    ("poppins/Poppins-Italic.ttf", "Poppins-Italic.ttf",
     "4fa76ae75b40f926420514044722cb97f32186cafd3b38263cc34dad7174d46d"),
    ("poppins/Poppins-BoldItalic.ttf", "Poppins-BoldItalic.ttf",
     "3572ac8116a0ac7317d342262b29937bcbaf94d8f03f90df6fe666fa7e2fb43a"),
    ("poppins/OFL.txt", "POPPINS-OFL.txt",
     "6be04893d770899a015649c7aa3b582f871b272f8747a92b78b17c3e5c8b2573"),
    # Montserrat (variable, wght 100-900).
    ("montserrat/Montserrat[wght].ttf", "Montserrat-var.ttf",
     "0f7b311b2f3279e4eef9b2f968bcdbab6e28f4daeb1f049f4f278a902bcd82f7"),
    ("montserrat/Montserrat-Italic[wght].ttf", "Montserrat-Italic-var.ttf",
     "51607f316bc020e59f03cbf51543eecffbea501c0b31d73e5b82927c5cca442c"),
    ("montserrat/OFL.txt", "MONTSERRAT-OFL.txt",
     "8b7141c03fa4f8d44e6345d5d4931709290f0f67875e452e95ac1fd3a027802e"),
    # Playfair Display (variable, wght 400-900).
    ("playfairdisplay/PlayfairDisplay[wght].ttf", "PlayfairDisplay-var.ttf",
     "c40f2293766a503bc70cce9e512ef844a4ccb7cbcde792fe2ea31d191917d8d6"),
    ("playfairdisplay/PlayfairDisplay-Italic[wght].ttf", "PlayfairDisplay-Italic-var.ttf",
     "a5e26dc5e2e77fb2803a0bf02fd4f81ee136ec8dea863ccdb0c59a263b21378b"),
    ("playfairdisplay/OFL.txt", "PLAYFAIRDISPLAY-OFL.txt",
     "566be814f8e96e93dfa16101331557eb6b5467e9e03f627c0910fe93ca12300e"),
    # JetBrains Mono (variable, wght 100-800).
    ("jetbrainsmono/JetBrainsMono[wght].ttf", "JetBrainsMono-var.ttf",
     "48715a42ec242c21e9f02692891e147d022299a52e48d5e413e1a942193ffeda"),
    ("jetbrainsmono/JetBrainsMono-Italic[wght].ttf", "JetBrainsMono-Italic-var.ttf",
     "85ae2a5cd3f56baf1ce1c21a851322c58e3d8fbe8e8ad4a4d090a820dd7fe558"),
    ("jetbrainsmono/OFL.txt", "JETBRAINSMONO-OFL.txt",
     "b2fe5e8987594e9ffd1d2ca52a2f5d73eb8335243893c5d6254b5ad69269591d"),
]] + [
    (f"{HOST}googlefonts/tinos/{TINOS_COMMIT}/OFL.txt", "TINOS-OFL.txt",
     "cb3382d4643e8b02c12e322c220a3c76a5020d667e4fd4e7c75e744cca6caa6b"),
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
    if failed:
        sys.exit("a source does not match its pinned SHA-256")


if __name__ == "__main__":
    main(sys.argv[1:])
