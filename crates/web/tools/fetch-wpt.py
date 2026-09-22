#!/usr/bin/env python3
"""Fetches web-platform-tests at a pinned commit and regenerates `crates/web/tests/wpt/`.

The tarball of the pinned commit is downloaded from GitHub, verified against a pinned
SHA-256, and read sparsely: only the CSS directories listed in `AREAS`, the shared
`css/reference` and `css/support` trees, `common/`, `fonts/ahem.css` and `LICENSE.md`
are unpacked (into memory, never onto disk in full). From those, the reftests are
selected:

- a test is a `.html`/`.htm`/`.xht`/`.xhtml` file with a `<link rel="match">` or
  `<link rel="mismatch">` whose target exists;
- it is excluded, together with its reference, when either document has a `<script>`
  element, is or contains SVG, uses vertical writing modes (`writing-mode`,
  `text-orientation`, `vertical-rl`...), needs `reftest-wait`, targets print media
  (`@page`, `media="print"`), nests another document (`iframe`, `object`, `embed`,
  `video`, `canvas`), or declares `@font-face` in the document or a linked stylesheet
  other than `fonts/ahem.css` (the runner maps Ahem to a bundled monospace face);
- whole leaf directories are taken in the plan's order (`docs/contracts/web-engine-plan.md`,
  Verification) while the running total stays under `CAP` pairs; a directory that does
  not fit is skipped and listed in the README as the next step. Directories named in
  `OUT_OF_SCOPE` (tentative specs, animation and invalidation tests, hidpi, multicol,
  URL fetching) are never taken.

What is written under `crates/web/tests/wpt/`: every selected test and reference plus
the stylesheets, images and other files they reference (at their original paths, so
`/css/support/...` and `../reference/...` resolve inside the tree), `css/support/`
whole, `LICENSE.md`, `manifest.json` (the pairs, grouped by directory) and `README.md`
(the directory list and the exclusion counts). `expectations.json` is created with 0
per directory when absent and otherwise left alone, since the integration step raises
it as the engine improves.

Run from anywhere: `python3 crates/web/tools/fetch-wpt.py [--tarball path] [--check]`.
Requires only the standard library. `--tarball` reuses a downloaded archive (still
verified); `--check` reports whether the checked-in manifest matches without writing.
"""

import collections
import hashlib
import io
import json
import os
import re
import shutil
import sys
import tarfile
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.dirname(HERE)
OUT = os.path.join(CRATE, "tests", "wpt")

COMMIT = "269bca0dd35c303639f3c9cf1d8bcb3d911bdb60"
URL = f"https://codeload.github.com/web-platform-tests/wpt/tar.gz/{COMMIT}"
SHA256 = "0cc977e9e5e0c249bb11deb9ea86b7d01e7efa8a8d3118039e5b7c2421bf43fa"

# How many pairs to take, at most: whole directories in order until the next one would
# go over.
CAP = 1600

# The search space, in the plan's order, each area with its own budget so that every
# area is represented under the global cap (a single budget would let the 700-pair
# `css/css-flexbox` root swallow everything after it). `css/CSS2` is restricted to its
# block, inline, float and positioning directories ("start with css/CSS2 block and
# inline"); the others are taken whole, leaf directory by leaf directory, in sorted
# order, skipping a directory that does not fit the area's budget.
AREAS = [
    ("css/CSS2", ["linebox", "floats", "floats-clear", "box", "box-display", "abspos"], 340),
    ("css/css-flexbox", None, 720),
    ("css/css-grid", None, 110),
    ("css/css-tables", None, 110),
    ("css/css-position", None, 60),
    ("css/selectors", None, 100),
    ("css/css-cascade", None, 40),
    ("css/css-variables", None, 0),
    ("css/css-values", None, 110),
    ("css/css-backgrounds", None, 0),
    ("css/css-text", None, 0),
    ("css/css-box", None, 0),
    ("css/css-display", None, 50),
    ("css/css-overflow", None, 0),
]
# Also unpacked, for references and support files (`css/css-writing-modes` is skipped
# on purpose).
SHARED = ["css/reference", "css/support", "common", "fonts/ahem.css", "LICENSE.md"]
# Leaf directory names that are never taken, whatever the budget.
OUT_OF_SCOPE = {
    "tentative", "animation", "animations", "invalidation", "hidpi", "multicol", "overlay", "urls", "grid-lanes",
    "scroll-markers", "line-clamp", "run-in", "vector", "background-attachment-local", "i18n", "shaping",
    "text-encoding", "text-spacing-trim", "text-group-align", "text-fit", "text-autospace", "math", "calc-size",
}
# The listed exclusions from the task: script, SVG, vertical writing, reftest-wait,
# print, nested documents and real web fonts.
EXCLUSIONS = ["script", "svg", "writing-mode", "reftest-wait", "print", "nested-document", "font-face", "missing-reference", "missing-stylesheet"]

TEST_EXT = (".html", ".htm", ".xht", ".xhtml")
LINK = re.compile(r"<link\b[^>]*>", re.I)
ATTR = re.compile(r"""([a-zA-Z:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))""")
IMPORT = re.compile(r"""@import\s+(?:url\(\s*)?["']?([^"')\s;]+)""", re.I)
URLREF = re.compile(r"""url\(\s*["']?([^"')\s]+)["']?\s*\)""", re.I)
SRC = re.compile(r"""\bsrc\s*=\s*(?:"([^"]*)"|'([^']*)')""", re.I)


def fetch(url, sha256, cached=None):
    if cached:
        with open(cached, "rb") as f:
            data = f.read()
    else:
        print(f"fetching {url}", file=sys.stderr)
        with urllib.request.urlopen(url, timeout=600) as r:
            data = r.read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        sys.exit(f"{url}: SHA-256 {got} does not match the pinned {sha256}")
    return data


def wanted(path):
    """Whether a path inside the repository is in the sparse set."""
    for area, _, _ in AREAS:
        if path == area or path.startswith(area + "/"):
            return True
    for s in SHARED:
        if path == s or path.startswith(s + "/"):
            return True
    return False


def unpack(data):
    """Reads the sparse set out of the tarball into a dict of repo path to bytes."""
    files = {}
    prefix = f"wpt-{COMMIT}/"
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        for member in tar:
            if not member.isfile() or not member.name.startswith(prefix):
                continue
            path = member.name[len(prefix):]
            if wanted(path):
                files[path] = tar.extractfile(member).read()
    return files


class Tree:
    def __init__(self, files):
        self.files = files
        self.text_cache = {}

    def text(self, path):
        if path not in self.text_cache:
            data = self.files.get(path)
            self.text_cache[path] = None if data is None else data.decode("utf-8", "replace")
        return self.text_cache[path]

    def resolve(self, base, href):
        href = href.split("#")[0].split("?")[0]
        if not href or "://" in href or href.startswith("data:"):
            return None
        if href.startswith("/"):
            return os.path.normpath(href.lstrip("/"))
        return os.path.normpath(os.path.join(os.path.dirname(base), href))


def attrs(tag):
    return {m.group(1).lower(): (m.group(2) or m.group(3) or m.group(4) or "") for m in ATTR.finditer(tag)}


def stylesheets(tree, path, text):
    out = []
    for tag in LINK.findall(text):
        a = attrs(tag)
        if "stylesheet" in a.get("rel", "").lower().split() and a.get("href"):
            p = tree.resolve(path, a["href"])
            if p:
                out.append(p)
    for m in IMPORT.finditer(text):
        p = tree.resolve(path, m.group(1))
        if p:
            out.append(p)
    return out


def references(tree, path, text):
    out = []
    for tag in LINK.findall(text):
        a = attrs(tag)
        rel = a.get("rel", "").lower()
        if rel in ("match", "mismatch") and a.get("href"):
            p = tree.resolve(path, a["href"])
            if p:
                out.append((rel, p))
    return out


def reasons(tree, path, text):
    """Why a document is out of the runner's reach, as a list of exclusion names."""
    r = []
    if re.search(r"<script\b", text, re.I):
        r.append("script")
    if path.endswith(".svg") or re.search(r"<svg\b", text, re.I):
        r.append("svg")
    if re.search(r"writing-mode\s*:|text-orientation|vertical-(rl|lr)|sideways-(rl|lr)", text, re.I):
        r.append("writing-mode")
    if "reftest-wait" in text:
        r.append("reftest-wait")
    if re.search(r"@page\b|media\s*=\s*[\"']?print|@media[^{]*\bprint\b", text, re.I):
        r.append("print")
    if re.search(r"<(iframe|object|embed|video|canvas)\b", text, re.I):
        r.append("nested-document")
    if re.search(r"@font-face", text, re.I):
        r.append("font-face")
    for css in stylesheets(tree, path, text):
        t = tree.text(css)
        if t is None:
            r.append("missing-stylesheet")
            continue
        if os.path.basename(css) == "ahem.css":
            continue
        if re.search(r"@font-face", t, re.I):
            r.append("font-face")
        if re.search(r"writing-mode\s*:", t, re.I):
            r.append("writing-mode")
    return r


def dependencies(tree, path, text):
    """Files the document loads besides its reference: stylesheets (recursively),
    images and other `url()` / `src` targets that exist in the tree."""
    out = set()
    todo = [(path, text)]
    while todo:
        p, t = todo.pop()
        for css in stylesheets(tree, p, t):
            if css in out:
                continue
            ct = tree.text(css)
            if ct is not None:
                out.add(css)
                todo.append((css, ct))
        for m in list(URLREF.finditer(t)) + list(SRC.finditer(t)):
            href = m.group(1) if m.lastindex == 1 else (m.group(1) or m.group(2))
            q = tree.resolve(p, href)
            if q and q in tree.files:
                out.add(q)
    return out


def leaf_dirs(tree, area, subdirs):
    """The leaf directories of an area holding candidate tests, in walk order."""
    dirs = collections.OrderedDict()
    for path in sorted(tree.files):
        if not (path.startswith(area + "/")):
            continue
        d = os.path.dirname(path)
        rel = os.path.relpath(d, area)
        parts = [] if rel == "." else rel.split("/")
        if subdirs is not None and (not parts or parts[0] not in subdirs):
            continue
        if any(p in ("support", "reference", "resources") for p in parts):
            continue
        if any(p in OUT_OF_SCOPE for p in parts):
            continue
        dirs.setdefault(d, []).append(path)
    if subdirs is not None:
        order = {name: i for i, name in enumerate(subdirs)}
        return sorted(dirs.items(), key=lambda kv: (order[os.path.relpath(kv[0], area).split("/")[0]], kv[0]))
    return list(dirs.items())


def select(tree):
    """Returns (directories, skipped) where directories is a list of
    {dir, pairs, counts} in order and skipped the directories over budget."""
    total = 0
    taken = []
    skipped = []
    for area, subdirs, budget in AREAS:
        area_total = 0
        for d, files in leaf_dirs(tree, area, subdirs):
            counts = collections.Counter()
            pairs = []
            deps = set()
            for path in files:
                if not path.endswith(TEST_EXT):
                    continue
                text = tree.text(path)
                refs = references(tree, path, text)
                if not refs:
                    continue
                counts["candidates"] += 1
                why = reasons(tree, path, text)
                chosen = None
                for kind, refp in refs:
                    rt = tree.text(refp)
                    if rt is None:
                        why.append("missing-reference")
                        continue
                    why += reasons(tree, refp, rt)
                    if chosen is None:
                        chosen = (kind, refp, rt)
                why = sorted(set(why))
                if why or chosen is None:
                    counts["excluded"] += 1
                    for w in why:
                        counts[w] += 1
                    continue
                kind, refp, rt = chosen
                pairs.append({"test": path, "kind": kind, "ref": refp})
                deps |= dependencies(tree, path, text) | dependencies(tree, refp, rt)
            if not pairs:
                continue
            entry = {"dir": d, "pairs": pairs, "counts": dict(counts), "deps": sorted(deps)}
            if total + len(pairs) > CAP or area_total + len(pairs) > budget:
                skipped.append(entry)
                continue
            total += len(pairs)
            area_total += len(pairs)
            taken.append(entry)
    return taken, skipped


def write_tree(tree, taken):
    if os.path.isdir(OUT):
        for name in os.listdir(OUT):
            if name == "expectations.json":
                continue
            p = os.path.join(OUT, name)
            shutil.rmtree(p) if os.path.isdir(p) else os.remove(p)
    os.makedirs(OUT, exist_ok=True)
    paths = set()
    for entry in taken:
        for pair in entry["pairs"]:
            paths.add(pair["test"])
            paths.add(pair["ref"])
        paths.update(entry["deps"])
    for path in tree.files:
        if path.startswith("css/support/") or path == "LICENSE.md" or path == "fonts/ahem.css":
            paths.add(path)
    for path in sorted(paths):
        dst = os.path.join(OUT, path)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        with open(dst, "wb") as f:
            f.write(tree.files[path])
    return len(paths)


def manifest(taken):
    return {
        "commit": COMMIT,
        "viewport": {"width": 800, "height": 600},
        "directories": [
            {
                "dir": e["dir"],
                "pairs": [{"test": p["test"], "kind": p["kind"], "ref": p["ref"]} for p in e["pairs"]],
            }
            for e in taken
        ],
    }


def readme(taken, skipped, file_count):
    total = sum(len(e["pairs"]) for e in taken)
    lines = [
        "# Web Platform Tests reftest corpus",
        "",
        f"A sparse copy of [web-platform-tests](https://github.com/web-platform-tests/wpt) at commit "
        f"`{COMMIT}`, produced by `crates/web/tools/fetch-wpt.py` (pinned tarball SHA-256 `{SHA256}`). "
        "The tests are copyright the W3C and the WPT contributors and are used under the 3-Clause BSD "
        "License in `LICENSE.md`.",
        "",
        "The runner is `crates/web/tests/wpt.rs`: for every pair in `manifest.json` it parses the test "
        "and its reference, resolves `<link rel=stylesheet>` and `@import` from this tree "
        "(`/css/support/...` and relative paths), maps `font-family: Ahem` to the bundled JetBrains Mono "
        "on both sides, runs the pipeline at 800x600 (the WPT default), rasterises both and compares the "
        "pixels; a `rel=match` pair passes when the rasters are identical and a `rel=mismatch` pair when "
        "they differ. The report lands in `crates/web/target-parity/wpt-report.md`, grouped by directory; "
        "the test fails only when a directory's pass count drops below `expectations.json`, which starts "
        "at 0 and which the integration step raises as the engine improves.",
        "",
        "## Selection",
        "",
        "Reftests only (`<link rel=match>` or `rel=mismatch` whose target exists). A test is excluded, "
        "with its reference, when either document has a `<script>`, is or contains SVG, uses vertical "
        "writing modes, needs `reftest-wait`, targets print media, nests another document (`iframe`, "
        "`object`, `embed`, `video`, `canvas`), or declares `@font-face` (other than `fonts/ahem.css`). "
        f"Whole leaf directories are taken in the plan's order while the total stays under {CAP} pairs "
        "and each area under its own budget (the `AREAS` table in `fetch-wpt.py`, so that one large "
        "directory cannot crowd out the areas after it); "
        "directories named `tentative`, `animation(s)`, `invalidation`, `hidpi`, `multicol`, `overlay`, "
        "`urls`, `grid-lanes`, `scroll-markers`, `line-clamp`, `run-in` and the font-dependent text "
        "directories are out of scope. `css/CSS2` contributes its block, inline, float and positioning "
        "directories only (`linebox`, `floats`, `floats-clear`, `box`, `box-display`, `abspos`); "
        "`css/css-writing-modes` is skipped on purpose.",
        "",
        f"**{total} pairs in {len(taken)} directories, {file_count} files.**",
        "",
        "## Included directories",
        "",
        "| Directory | Pairs | Candidates | " + " | ".join(EXCLUSIONS) + " |",
        "|---|---|---|" + "---|" * len(EXCLUSIONS),
    ]
    for e in taken:
        c = e["counts"]
        lines.append(f"| `{e['dir']}` | {len(e['pairs'])} | {c.get('candidates', 0)} | " + " | ".join(str(c.get(x, 0)) for x in EXCLUSIONS) + " |")
    lines += [
        "",
        "## Skipped for budget (next in line)",
        "",
        "These directories passed the same filter but did not fit under the cap; raising `CAP` in "
        "`fetch-wpt.py` takes them in this order.",
        "",
        "| Directory | Pairs | Candidates |",
        "|---|---|---|",
    ]
    for e in skipped:
        lines.append(f"| `{e['dir']}` | {len(e['pairs'])} | {e['counts'].get('candidates', 0)} |")
    lines += [
        "",
        "## Exclusion totals",
        "",
        "| Reason | Tests (included directories) | Tests (skipped directories) |",
        "|---|---|---|",
    ]
    for x in EXCLUSIONS:
        a = sum(e["counts"].get(x, 0) for e in taken)
        b = sum(e["counts"].get(x, 0) for e in skipped)
        lines.append(f"| {x} | {a} | {b} |")
    lines.append("")
    return "\n".join(lines)


def main():
    args = sys.argv[1:]
    cached = None
    if "--tarball" in args:
        cached = args[args.index("--tarball") + 1]
    check = "--check" in args
    data = fetch(URL, SHA256, cached)
    tree = Tree(unpack(data))
    print(f"unpacked {len(tree.files)} files from the sparse set", file=sys.stderr)
    taken, skipped = select(tree)
    m = manifest(taken)
    total = sum(len(e["pairs"]) for e in taken)
    if check:
        with open(os.path.join(OUT, "manifest.json")) as f:
            current = json.load(f)
        if current != m:
            sys.exit("manifest.json differs from what fetch-wpt.py would write")
        print(f"manifest.json is up to date ({total} pairs)")
        return
    file_count = write_tree(tree, taken)
    with open(os.path.join(OUT, "manifest.json"), "w") as f:
        json.dump(m, f, indent=1)
        f.write("\n")
    with open(os.path.join(OUT, "README.md"), "w") as f:
        f.write(readme(taken, skipped, file_count))
    expectations_path = os.path.join(OUT, "expectations.json")
    expectations = {}
    if os.path.exists(expectations_path):
        with open(expectations_path) as f:
            expectations = json.load(f)
    for e in taken:
        expectations.setdefault(e["dir"], 0)
    for d in list(expectations):
        if d not in {e["dir"] for e in taken}:
            del expectations[d]
    with open(expectations_path, "w") as f:
        json.dump(dict(sorted(expectations.items())), f, indent=1)
        f.write("\n")
    print(f"wrote {total} pairs in {len(taken)} directories ({file_count} files) to {OUT}")
    for e in taken:
        print(f"  {e['dir']}: {len(e['pairs'])}")
    print(f"skipped for budget: {len(skipped)} directories, {sum(len(e['pairs']) for e in skipped)} pairs")


if __name__ == "__main__":
    main()
