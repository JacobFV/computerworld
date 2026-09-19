# computerworld.dev — the project site

A single static page: no framework, no build step, no third-party requests. It is
deployed to GitHub Pages by `.github/workflows/pages.yml`, which also builds the Wasm
bundle and copies the browser console into `site/demo/` so the page can embed the real
simulator.

```sh
# Preview the page alone
python3 -m http.server 8000 --directory site

# Preview with the live demo embedded (needs the Wasm bundle built once)
bash scripts/build-wasm.sh && node examples/browser/build.mjs
mkdir -p site/demo/examples site/demo/pkg
cp -r examples/browser site/demo/examples/browser && cp -r pkg/web site/demo/pkg/web
python3 -m http.server 8000 --directory site
```

`site/demo/` is generated and git-ignored. The screenshots in `site/media/` are rendered
by the simulator itself; regenerate them by rendering the scenes you want with
`World::render` (the `render` feature) and saving the frames as JPEG.
