# computerworld.dev — the project site

A single static page: no framework, no build step, no third-party requests. Every screen
on it is a running machine — the simulator downloads when the page loads and each scene
boots into it as the slideshow reaches it, with no button to press. While it is coming
down, each tile darkens its still and shows a spinner and the download's own percentage;
where the bytes cannot honestly be counted — a host that gzips the module sends a
`Content-Length` counting the bytes on the wire, and a reader is handed back the decoded
ones — the number is dropped rather than guessed at, and the spinner stands alone.
`live.js` is the only thing that knows; it reports out through the callback `boot` takes.

The machines do not run on the page's own thread. `engine.js` is the whole of what touches
the simulator — a world per scene, a session per machine, a canvas each — and knows nothing
about slides or events, so it runs unchanged in either host: `worker.js`, which is where it
really runs, or the tab, for a browser without workers or `OffscreenCanvas`. A scene is
dealt to one of at most four workers and keeps it; the page hands over each canvas with
`transferControlToOffscreen` and thereafter sends events and receives a cursor, and no
pixels cross back. How many workers is bounded by `navigator.deviceMemory` as well as by
cores, because each one holds an engine of its own and a copy of whatever fonts it has
installed: on a device reporting 2 GB the pool of one costs 1,591 MB against the 1,808 MB
four cost, and the same walk across four scenes 1,610 MB against 1,958 MB.

`live.js` and `worker.js` each declare a protocol number as a literal, deliberately not
shared through a module they would both cache. A page served before a deploy can meet a
worker served after it — GitHub Pages gives these files ten minutes of cache — so the
worker reports its number before the pool is used, and a page that is answered in a
language it does not speak lets the workers go and runs the machines itself. The same
handshake catches a `worker.js` that is not there at all. Bump both literals together when
the messages change. Building a world is seconds of arithmetic and a screen is four megabytes
of rasterising, and neither is something a page can do while it is also expected to scroll:
the longest the main thread is unavailable while the seven-machine slide comes up is 112 ms,
against 2,340 ms when the same code ran in the tab.

One consequence, for anything reading a machine's pixels: a canvas whose control has gone
to a worker hands the page back the frame it was first given, not the frame it is showing.
`window.computerworldFrame(<machine id>)` asks the machine itself and is what
`scripts/render-site-stills.mjs` saves.

`.github/workflows/pages.yml` deploys it: it builds the Wasm bundle into `site/pkg/`
for `live.js` to import, and the documentation into `site/docs/`, where
`scripts/build-docs.mjs` turns `docs/*.md` into pages with the guides in reading order
(the order is in the script) and `cargo doc` supplies the Rust reference under
`site/docs/api/rust/`. Both directories are generated and git-ignored.
`world-definition.js` — the world the machines run in — is generated too, but checked in:
`scripts/build-live-world.mjs` writes it from `worlds/company-2026/world.json`, and
`scripts/build-content.sh` runs that alongside the other generators.

```sh
# Preview (the machines need the Wasm bundle built once)
bash scripts/build-wasm.sh && cp -r pkg/web site/pkg
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
page itself, so a still is always a frame the machine really draws. `site/pkg/` is
generated and git-ignored.

## The slideshow in someone else's page

`embed.html` is the slideshow alone — the same stage, the same `cast.js`, the same
`app.js` — with the title, the install box, the corner links and the footer taken
away, so a machine gets the whole frame. `embed.css` is only that reallocation; the
look comes from `style.css` either way. It is the hero in an iframe, not a copy of
one, so a scene added to the cast appears in both.

```html
<iframe src="https://jacobfv.github.io/computerworld/embed.html#github"
        loading="lazy" title="ComputerWorld" allow="fullscreen"
        style="aspect-ratio: 16/10; width: 100%; border: 0"></iframe>
```

`#<scene id>` picks the machine it opens on, as on the front page. `allow="fullscreen"`
is what lets a machine inside the frame take the whole screen; without it the control
is still there and still works, but it can only fill the frame it was given, which on a
16:10 embed is most of what fullscreen would have been anyway. The simulator is
about 16 MB gzipped — 14 MB of Wasm and 2 MB of world — so frame it lazily: a
visitor who never scrolls to it should never pay for it. Everything below the stage in `app.js` asks for its elements
before wiring itself up, which is what lets one script serve both pages — keep it
that way when adding to it.
