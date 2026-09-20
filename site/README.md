# computerworld.dev — the project site

A single static page: no framework, no build step, no third-party requests. Every screen
on it is a running machine — the simulator downloads when the page loads and each scene
boots into it as the slideshow reaches it, with no button to press.
`.github/workflows/pages.yml` deploys it, builds the Wasm bundle and the world
definition into `site/demo/` for the page to import, and builds the documentation into
`site/docs/`: `scripts/build-docs.mjs` turns `docs/*.md` into pages with the guides in
reading order (the order is in the script), and `cargo doc` supplies the Rust reference
under `site/docs/api/rust/`. Both directories are generated and git-ignored.

```sh
# Preview (the machines need the Wasm bundle built once)
bash scripts/build-wasm.sh && node examples/browser/build.mjs
mkdir -p site/demo/examples site/demo/pkg
cp -r examples/browser site/demo/examples/browser && cp -r pkg/web site/demo/pkg/web
node scripts/build-docs.mjs
cargo doc --no-deps -p computerworld --lib && cp -r target/doc site/docs/api/rust
node scripts/serve-site.mjs 8000
```

## The cast

`cast.js` is the slideshow: the scenes in `scenes/`, in the order they are shown. A scene
names its machines — each a copy of one of the five graphical computers in the reference
world, added under an id of its own — and an `open()` that drives them from a fresh boot to
something mid-work, using the same actions an agent would send. Each scene runs in a world
to itself, so what it shows never depends on which others the visitor passed first; the
machines within one share that world, which is how the two phones in `texting` are in one
conversation, and a scene's `sync()` is how the phone that did not just act catches up.
Only the few scenes nearest the visitor stay running; the rest keep their last frame.
`#<scene id>` links to a scene.

To add one, write `scenes/<id>.js`, import it in `cast.js`, and render its still:

```sh
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/usr/bin/google-chrome \
  node scripts/render-site-stills.mjs <scene id>      # or no id, for all of them
```

`site/media/scenes/<machine>.jpg` is what a machine shows until it is running, and all it
shows to a browser that cannot run the simulator. The script makes them by booting the
page itself, so a still is always a frame the machine really draws. `site/demo/` is
generated and git-ignored.

## The slideshow in someone else's page

`embed.html` is the slideshow alone — the same stage, the same `cast.js`, the same
`app.js` — with the title, the install box, the corner links and the footer taken
away, so a machine gets the whole frame. `embed.css` is only that reallocation; the
look comes from `style.css` either way. It is the hero in an iframe, not a copy of
one, so a scene added to the cast appears in both.

```html
<iframe src="https://jacobfv.github.io/computerworld/embed.html#github"
        loading="lazy" title="ComputerWorld"
        style="aspect-ratio: 16/10; width: 100%; border: 0"></iframe>
```

`#<scene id>` picks the machine it opens on, as on the front page. The simulator is
about 10 MB over the wire, so frame it lazily: a visitor who never scrolls to it
should never pay for it. Everything below the stage in `app.js` asks for its elements
before wiring itself up, which is what lets one script serve both pages — keep it
that way when adding to it.
