# Real open-source web apps

Most sites on the world's internet are services written for this project: Rust
handlers that answer with HTML. This guide is about the other kind: real, published
open-source web applications, built by their own toolchains and run in the world
unmodified or nearly so, so that an agent in the world uses the products people
actually use. Six of them are live in [`worlds/oss-web`](../worlds/oss-web):

| Site | App | Upstream, pinned | Licence (as checked at that commit) | Kind |
|---|---|---|---|---|
| `todomvc.com` | TodoMVC, React example and Vue example | [tastejs/todomvc](https://github.com/tastejs/todomvc) `ff43b02e59dfa604386bb382034b2cd07c2bcd8a` | MIT (`license.md`) | frontend only |
| `conduit.realworld.show` | Conduit (RealWorld), React + Redux | [khaledosman/react-redux-realworld-example-app](https://github.com/khaledosman/react-redux-realworld-example-app) `53b0b4c0b8c371053a8d082ff9a42bfae68f3755` | MIT (`LICENSE.md`) | frontend of the API below |
| `vue.realworld.show` | Conduit (RealWorld), Vue 3 | [mutoe/vue3-realworld-example-app](https://github.com/mutoe/vue3-realworld-example-app) `741c215ef0f674f90fcb03c5493a1b3a3a7f1b03` | MIT (`LICENSE`) | frontend of the API below |
| `api.realworld.show` | The RealWorld API, Node/Express + Prisma | [gothinkster/node-express-realworld-example-app](https://github.com/gothinkster/node-express-realworld-example-app) `30b68e1e881462b2f4164ea09ab4c4f5699c7b0b` | MIT, **declared only in `package.json`**: the repository has never had a LICENSE file (checked over its whole history) | Node backend |
| `react-admin.marmelab.com` | react-admin's `examples/simple` | [marmelab/react-admin](https://github.com/marmelab/react-admin) `99e8c52b7db1712c0afa4eeed533e4a713dfc1ec` | MIT (`LICENSE.md`) | frontend with a fake REST provider |
| `json-server.typicode.com` | JSON Server 0.17.4 | [typicode/json-server](https://github.com/typicode/json-server) `78ea71375666d49145734689c097654c54f90686` (tag `v0.17.4`) | MIT (`LICENSE`) | Node backend with its own home page |

The dependencies bundled into the two backends are all permissive: 90 MIT, 5
Apache-2.0, 5 ISC, 2 BSD-3-Clause and one 0BSD package in the RealWorld API, 104
MIT, 8 ISC and 1 BSD-3-Clause in JSON Server (`license-checker --production` over
each checkout). The same entries are in [`research/sources.json`](../research/sources.json).

## What was changed, and why

Nothing in any app's logic. The changes are all at the edges:

- **RealWorld API, database.** Prisma's query engine is a native binary talking to
  Postgres. The app's `@prisma/client` import is bundled as
  [`shims/prisma-store.js`](../crates/services/oss-web/shims/prisma-store.js): a
  Prisma Client over one JSON file that reads the app's own `schema.prisma` and
  implements every query the app makes (the finds, `count`, `create`, `update`,
  `upsert`, `delete` and their `Many` forms, `$transaction`; `equals`/`in`/`lt`/
  `contains`… filters, `AND`/`OR`/`NOT`, `some`/`every`/`none`/`is`/`isNot`;
  `select`/`include` with nested `_count`; `orderBy` a field or a relation's
  `_count`; `connect`, `connectOrCreate`, `create`, `disconnect` and `set`; unique
  constraints and `onDelete: Cascade`). The app's own services and controllers run
  unchanged on top of it.
- **RealWorld API, bcrypt cost 10 → 4.** At cost 10 one `bcrypt.hash` is 1,024 key
  expansions in pure JavaScript, which runs past a request's step budget on the
  in-world VM. Cost 4 still produces real bcrypt hashes any bcrypt library verifies.
- **Conduit React, theme stylesheet.** Its page links the RealWorld theme from
  `demo.productionready.io`, which is not on this internet; the same `main.css`
  (from the Vue app) is served from the site itself.
- **react-admin, demo records.** The demo's fake REST data is compiled into its
  bundle; [`scripts/oss-web/react-admin-data.py`](../scripts/oss-web/react-admin-data.py)
  rewrites `data.tsx` before the build with the world's people and posts in place
  of lorem ipsum, keeping every id, date, tag and relation the screens depend on.
- **Configuration only:** the Conduit frontends are built with the API's in-world
  URL (`REACT_APP_BACKEND_URL`, `VITE_API_HOST`), and JSON Server is started as its
  CLI would be (`json-server /data/db.json --port 3000`).

## How they are hosted

### Packages: an app is code

[`scripts/oss-web/build.sh`](../scripts/oss-web/build.sh) fetches each upstream at its
pinned commit, installs with the upstream's own lockfile and package manager (npm,
yarn 1, yarn 4, pnpm) and runs its own build (webpack, Create React App, Vite,
esbuild for the backends), then writes the output to
`crates/services/oss-web/packages/<id>/{public,server}`. Source maps are left out.
Each package also has a checked-in `manifest.json`. The built output is checked in,
6.0 MB in all:

| Package | Bytes |
|---|---|
| json-server | 1,921,952 |
| react-admin | 1,656,767 (with the world's records, as rebuilt) |
| realworld-api | 1,494,109 |
| todomvc | 364,770 |
| conduit-react | 345,508 |
| conduit-vue | 223,815 |

`cw-oss-web` embeds every package (its `build.rs` includes each file), and
registering it registers the `node-app` kind with them. Packages are code, like a
Rust service's handler: they are never serialised, and a world names one by id. They
are **not** in the site's Wasm bundle: `cw-services` registers them only with its
`oss-web` feature (`computerworld`'s `oss-web` feature turns it on), which the Wasm
build does not enable.

### The `node-app` kind

[`crates/services/node-app`](../crates/services/node-app) serves a package:

- `GET`/`HEAD` of a file under the package's `public/` is answered from the package
  (a directory's `index.html`; a directory named without its slash redirects).
- A package with a `server` runs its own Node backend for every other request.
- A single-page app without a backend names a `spa_fallback`, served for any other
  `GET` that accepts HTML, so its client-side router sees deep links.

The backend runs on the in-house JavaScript VM through `cw_jsvm::serve`: a **fresh
VM per request** boots the server's entry file, waits for it to listen on its port,
hands it the request exactly as a loopback client's request reaches Node's
`http.Server` (so Express, its middleware and body parsers see an ordinary
`IncomingMessage` and `ServerResponse`), and runs the event loop until the response
has ended and the work due at that moment is done. What the VM sees:

- `/app` is the package's `server/` (read-only), with its `public/` at `/app/public`
  for a backend that serves its own files (`express.static`).
- The data directory (`/data` unless the manifest says otherwise) is the instance
  state's `files`: text, a JSON value (a seed, written out when first read) or
  `{"bytes": [...]}`. Whatever the app writes there is the new state, so the app
  keeps its database where it always did — a JSON file for JSON Server's lowdb and
  for the Prisma shim.
- The clock is the world's (`tick` past the world epoch, then advancing with the
  instructions executed); entropy is drawn from the instance seed and the number of
  requests served so far (`requests` in the state), so ids and salts differ between
  requests and are the same on every replay.
- There is no network: an outgoing request fails as if the machine were offline.
- Each request may execute 2,000,000,000 VM instructions.

Nothing else survives a request. The same state and the same request therefore
answer the same bytes however the world got there, and a snapshot, a fork or a
replay carries the app's data in the service state like any other service's
(`crates/services/node-app/tests/node_app.rs` checks that a copy of the state
answers byte for byte as the original did).

The state of a running app, for example the RealWorld API:

```json
{"package": "realworld-api", "env": {"JWT_SECRET": "..."}, "files": {"/data/db.json": {...}}, "requests": 42}
```

**Cost.** Booting per request is what keeps this deterministic, and it is affordable
because the VM caches compiled code across VMs on one thread. In a release test
build the RealWorld API answers most calls in 18-20 ms (the first, which compiles
the 1.5 MB bundle, in about 200 ms; a registration or login in 130-200 ms, which is
bcrypt), and JSON Server in 27-29 ms (its first in 135 ms).

### Seeds

The sites open with the world's own people (the internet's public, who have no
story elsewhere, and the workstation's user, Ada Okonkwo):

- **Conduit**: seven accounts with real bcrypt hashes, ten articles with their tags,
  nine comments, follows and favourites, written by
  [`scripts/oss-web/seed-realworld.mjs`](../scripts/oss-web/seed-realworld.mjs) into
  `worlds/oss-web/services/realworld-api/service.json` in the store's own format.
  The seeded accounts have `demo: true`: the API lists only demo authors' articles
  (and the reader's own) to everyone, its own rule against spam, which also means an
  account registered in the world sees its own articles and nobody else does.
- **react-admin**: the rewritten demo records (thirteen posts, eleven comments,
  three users).
- **JSON Server**: a reading list, two projects and their tasks, and a profile.
- **TodoMVC** starts empty, as it does everywhere.

The workstation's `~/accounts.txt` has Ada's Conduit password and the react-admin
demo's sign-in.

## The world

`worlds/oss-web/world.yml` is one Ubuntu workstation on the built-in internet with
the six sites hung off `pop-west`. It boots only in a build with `computerworld`'s
`oss-web` feature. The sites are not in the default internet (`worlds/internet`)
on purpose: every world that joins the internet would need the kind registered (so
the 6 MB of packages would be in the site's Wasm bundle), and every existing world's
state hashes would change. Adding one to the internet later is its `service.json` in
`worlds/internet/services/` and an include line, plus registering `cw-oss-web` by
default.

`crates/computerworld/tests/oss_webapps.rs` uses every app through the agent API:

    cargo test -p computerworld --features oss-web --test oss_webapps

- **TodoMVC**, React and Vue: add three todos, complete one, filter by the hash
  routes, clear the completed one.
- **Conduit React**: filter the feed by a tag, sign in, write an article, comment on
  it, edit it, check the API has the edit, delete it, favourite another from the feed.
- **Conduit Vue**: sign in, filter by a tag, write an article with tags, favourite
  and unfollow from an article, change the bio in Settings and see it on the profile.
- **react-admin**: search the posts, edit one, create a tag in both locales, delete a
  post through the undoable delete.
- **JSON Server**: its home lists the resources; writes from `http.v1` (create,
  patch, delete) are seen by the browser through a filter, an embed and a search.

Timers such as the fake REST provider's 300 ms latency and search debounces run on
world time: the tests let it pass between actions as the environment does
(`runtime.advance` and an empty step).

## Measured

Release test build, one fresh world per app, first visit and a second visit in the
same process (`measure` in `oss_webapps.rs`). The navigation time includes every
request the page makes to its backend; react-admin also needs 1.1 s of world time for
its fake provider and render.

| App | First load (ms) | Second load (ms) | Snapshot before → after (bytes) |
|---|---|---|---|
| TodoMVC React | 67 | 14 | 4,317,358 → 6,106,373 |
| TodoMVC Vue | 17 | 10 | 4,317,358 → 5,093,561 |
| Conduit React | 165 | 89 | 4,317,358 → 6,907,597 |
| Conduit Vue | 96 | 88 | 4,317,358 → 6,009,635 |
| react-admin | 2,466 (1,022 wall + waits) | 2,490 | 4,317,358 → 16,005,908 |
| JSON Server | 315 | 292 | 4,317,358 → 4,373,949 |

The snapshot grows by what the browser keeps to restore the tab's page (its realm's
inputs and heap image): 0.06 MB for JSON Server's plain page, 0.8-2.6 MB for the other
smaller apps and 11.7 MB for react-admin.
The process's resident memory went from 52 MiB to 411 MiB over the six visits in one
process (+46 MiB for TodoMVC React, the first; +171 MiB for react-admin).

**Fidelity against Chromium.** Framework-parity fixtures run TodoMVC's React and Vue
builds and JSON Server's home page (`oss-*` in
`crates/web/engine/tests/framework-parity`): TodoMVC passes 83.6-92.5 % of nodes per
state and JSON Server 5.9 %, with 1.07-2.93 % of pixels differing
(`node scripts/web-parity/gallery.mjs comparisons/oss-webapps oss-todomvc-react
oss-todomvc-vue oss-json-server`). The misses are in layout and style, not script:
TodoMVC's `appearance: none` checkbox between `top: 0` and `bottom: 0` is 13 px tall
where Chromium stretches it to 40; its `h1` keeps the old h1-in-section 0.83em
margins where Chromium uses 0.67em; the new-todo field computes font-weight 300 where
Chromium has 400; JSON Server's system font stack resolves to Ubuntu in Chromium on
the machine that dumped it. The Conduit frontends and react-admin have no fixture:
their bundles are ES modules (or load their route chunks from root-relative paths)
and need their API, which `dump.mjs`'s `file:` pages can load neither of.

**The compiled TSX path.** None of the React apps compiles with `cw-tsx`: TodoMVC's
and Conduit's modules are `.js`/`.jsx` files it does not resolve
(`cannot find module './todo/app'`), and react-admin imports MUI and react-admin
itself (`a compiled app imports only react and react-dom`) and CSS. They run on the
React fallback (the real React on the VM), which is what they are measured on above.

## What the engine needed

Each gap was found by an app failing and is covered by a test:

- **jsvm**: surrogate escapes in non-unicode regexes (Express's `encodeurl`),
  `Error.prepareStackTrace` with `CallSite` objects (`depd`), `tty.isatty`,
  `buffer.SlowBuffer` (`jwa`), an ES5-callable `StringDecoder` (`iconv-lite`),
  `crypto.KeyObject`/`createSecretKey` (`jsonwebtoken` 9), stream constructors
  callable the ES5 way and a flowing `Readable` that pulls again (`send`),
  `ServerResponse` writing its head through `writeHead`/`_implicitHeader`
  (`on-headers`, `compression`), `IncomingMessage.connection`, and Unicode property
  escapes for punctuation, symbols and emoji (react-admin); plus `cw_jsvm::serve`
  itself (`tests/serve.rs`, and the `web_app_deps` conformance program, which
  matches Node v24.21.0).
- **cw-web**: a fragment navigation fires `popstate` before `hashchange` (React
  Router's hash history), and Enter fires `keypress` (Vue's
  `@keypress.enter.prevent`).
- **The browser** allows a page 250 million VM steps per task (react-admin's first
  render is one task of 60-100 million).

Left for the form-controls work, with the flows that show them:

- Enter in a text field of a form with no submit button and more than one text field
  submits the form; browsers do not (Conduit React's editor, where the tests add no
  tag for that reason).
- A text field fires `change` on blur whenever it was typed in, even when script has
  since set its value back to what it was on focus; browsers compare values (Conduit
  Vue's tag field, where it adds an empty tag).

Behaviour of the apps themselves, the same in Chromium: Conduit React's profile pages
never render (its route is `/@:username`, which React Router 6 does not read as a
parameter) and it goes home after publishing; Conduit Vue fetches an article without
the reader's token, so the author shows as not followed until an action refreshes it.

Excalidraw was considered and not packaged: it draws its whole editor on a canvas and
subsets its fonts in a worker with a WebAssembly WOFF2 encoder
(`packages/excalidraw/workers.ts`, `subset/woff2`), none of which this round's apps
exercise.
