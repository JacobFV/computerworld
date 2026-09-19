# computerworld.dev — the project site

A single static page: no framework, no build step, no third-party requests. Every screen
on it is a running machine — the simulator downloads when the page loads and each panel
boots into it, with no button to press. `.github/workflows/pages.yml` deploys it, and
builds the Wasm bundle and the world definition into `site/demo/` for the page to import.

```sh
# Preview (the machines need the Wasm bundle built once)
bash scripts/build-wasm.sh && node examples/browser/build.mjs
mkdir -p site/demo/examples site/demo/pkg
cp -r examples/browser site/demo/examples/browser && cp -r pkg/web site/demo/pkg/web
python3 -m http.server 8000 --directory site
```

`site/demo/` is generated and git-ignored. The screenshots in `site/media/` are rendered
by the simulator itself; regenerate them by rendering the scenes you want with
`World::render` (the `render` feature) and saving the frames as JPEG.
