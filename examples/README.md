# Examples

One directory per language binding, each holding runnable programs. Every one of
them drives the engine through the same public surface an agent would: no
privileged state inspection supplies an answer a program prints.

## Rust — `native/`

Registered as cargo examples of the `computerworld` crate, so the name you run is
not always the file name.

| Run | File | What it shows |
|---|---|---|
| `cargo run -p computerworld --example company` | [`company.rs`](native/company.rs) | An actor-only episode in the [reference world](../worlds/company-2026): information moves between machines through mail, documents and the browser, never through the fixture. |
| `cargo run -p computerworld --example custom-app` | [`counter.rs`](native/counter.rs) | Adding a native application. The counter keeps semantic state in a serializable value; an `activate` event changes it and a pure `page` method projects a label and a button through the native page contract — no HTML, CSS, DOM, host timer or browser process. The program registers the module, launches it through `application.v1`, takes the button's geometry from the scene and clicks it; the actor's observation then reads `Count: 1`. See [custom application](../docs/custom-application.md). |
| `cargo run -p computerworld --example custom-service` | [`echo.rs`](native/echo.rs) | Adding a synthetic-internet service. It registers `example.counter`, gives it a node and an explicit network link in the [unrelated lab](../worlds/unrelated-lab), and reaches it with an actor HTTP action. `GET` reads the counter, `POST` increments it; the service sees the source computer, actor and logical tick, and has no browser or OS dependency. Its state is checkpointed by the runtime like any other. See [custom service](../docs/custom-service.md). |
| `cargo run -p computerworld --example os-gallery` | [`os_gallery.rs`](native/os_gallery.rs) | Every native OS shell rendered in representative states to PNG. Takes an output directory. |

The registry these two register into holds trusted Rust code. It is an extension
point, not an untrusted plugin sandbox.

## JavaScript — `javascript/`

    node examples/javascript/computer-interaction.mjs --output target/javascript-demo

[`computer-interaction.mjs`](javascript/computer-interaction.mjs) is a persistent
JavaScript → Wasm session: keyboard and pointer input, rendering and replay. It is
also the demo shipped inside the npm package, so its path is part of a published
layout — `scripts/package-release.py` copies this directory verbatim.

## Python — `python/`

    python examples/python/computer_interaction.py --output target/python-demo

| File | What it shows |
|---|---|
| [`computer_interaction.py`](python/computer_interaction.py) | The same session as the JavaScript demo, against the native extension. `--compare <dir>` checks it against the JavaScript run's output, which is what makes the two bindings' agreement a test rather than a claim; change one and change the other. |
| [`desktop_pixels.py`](python/desktop_pixels.py) | Cross-binding parity: imports a Node/Wasm desktop checkpoint and re-renders it. |
| [`smoke.py`](python/smoke.py) | The wheel works at all. Run against an installed package; `scripts/package-release-smoke.py` uses it on release artifacts. |

`scripts/smoke-bindings.sh` runs the JavaScript and Python programs together and
diffs their outputs; CI runs the same pair on every push.
