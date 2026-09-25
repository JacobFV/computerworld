# TSX apps

Apps and sites in the world can be written as React components in TypeScript
(`.tsx`), the way an agent would write them for Chrome. One source runs two ways:

* **Compiled.** `cw-tsx` (crates/web/tsx) type-checks the module against a subset of
  React and TypeScript and lowers it to a UI IR; `cw-ui` (crates/web/ui) runs the IR
  natively on the engine's document: no JS VM and no React library. The document,
  cascade, layout, paint, hit testing, focus and form state are the engine's own;
  only the script layer is replaced.
* **Fallback.** From the same source `cw-tsx` emits a plain script (types stripped,
  JSX lowered to `React.createElement`) that React 18's UMD build runs: in Chrome,
  and in the world's browser on the JS `Realm` whenever the compiled path is not
  available. A module outside the subset still gets its fallback.
* **Islands, between the two.** What the compiled path cannot compile, such as npm
  packages and modules of the app outside the subset, runs on a jsvm VM inside
  cw-ui behind a React shim, beside the compiled code, in the same document and
  snapshot (see [Islands](#islands-packages-and-code-outside-the-subset)). An app
  goes to its React fallback only when even the island cannot run it.

The two are checked against each other and against Chromium (see
[Verification](#verification)): after every state of a scripted session the compiled
document is identical, node for node, to the one React builds.

## Writing an app

A compiled app is a set of modules that import `react`, `react-dom`
(`react-dom/client`) and each other (`./App`, `../shared/icons`), with one render
call at the top level of one of them:

```tsx
import { useState } from 'react';
import { createRoot } from 'react-dom/client';

interface Task { id: number; title: string; done: boolean }

function Row({ task, onToggle }: { task: Task; onToggle: (id: number) => void }) {
  return (
    <li className={task.done ? 'done' : ''}>
      <button onClick={() => onToggle(task.id)}>{task.title}</button>
    </li>
  );
}

function App() {
  const [tasks, setTasks] = useState<Task[]>([{ id: 1, title: 'Ship it', done: false }]);
  const toggle = (id: number) =>
    setTasks((ts) => ts.map((t) => (t.id === id ? { ...t, done: !t.done } : t)));
  return <ul>{tasks.map((t) => <Row key={t.id} task={t} onToggle={toggle} />)}</ul>;
}

createRoot(document.getElementById('root')!).render(<App />);
```

`crates/web/engine/tests/framework-parity/tsx-tasks.tsx` is a one-module example;
`framework-parity/app-src/<app>/` holds nine apps written by a coding agent (an
analytics dashboard, a calendar, a chat, a data table, an inbox, a kanban board, a
music player, a settings form, a shop:
Tailwind classes, inline SVG icons, several modules each) that compile unchanged.

### The compiled subset

**Components and hooks.** Function components (declarations, `const X = (...) =>`,
`memo(...)`), custom hooks (`useX`), `useState`, `useReducer`, `useMemo`,
`useCallback`, `useRef`, `useEffect`, `useLayoutEffect`, `useContext` with
`createContext` and `<Ctx.Provider value>`, `useId`, `useSyncExternalStore`
(subscribed after commit, resubscribed when `subscribe` changes, unsubscribed on
unmount), `Fragment`/`<>`, `StrictMode`
(renders its children), `key`, `ref` on host elements (object or callback refs).
Hooks must be called at a component's or hook's top level, not inside conditions,
loops or closures.

**JSX and the DOM.** Host elements with any attributes; `className`, `style`
objects (numbers get `px` as React adds it), boolean and numeric attributes,
`data-*`/`aria-*`, spread props, SVG. Event props `onClick`, `onDoubleClick`,
`onChange`, `onInput`, `onSubmit`, `onReset`, `onKeyDown`/`Up`/`Press`,
`onFocus`/`onBlur`, `onMouse*`, `onPointer*`, `onWheel`, `onScroll`,
`onContextMenu`, and their `...Capture` forms. Controlled inputs, textareas and
selects (`value`, `checked`, `defaultValue`, `defaultChecked`), `autoFocus`.
`window.addEventListener`/`removeEventListener` and the same on `document` (with
`capture` as a boolean or `{ capture }`; capture listeners run before React's
handlers, bubbling ones after, and `stopPropagation` in either stops the rest),
`resize` on `window`, `document.getElementById`, `document.querySelector`,
`document.activeElement`, `document.body`, `document.title`,
`window.innerWidth`/`innerHeight`, `el.focus()`, `blur()`, `select()`,
`setSelectionRange(start, end)`. An element leaving the document takes focus and
hover with it, with no events, as the document's removal steps do.

**Host globals.** A module may reference declaration files (`/// <reference
path="../types/cw.d.ts" />`); their types are visible to every module and they
emit no code. The one host global the subset knows is computerworld's `cw` (the
desktop web-app host's, typed by `crates/applications/web/types/cw.d.ts`):
`cw.kind`, `argument`, `env`, `onEnv`, `now()`, `state.get<T>()`/`set`,
`fs.readFile`/`writeFile`/`list`/`mkdir`, `fetch`, `launch`, `emit`, `refuse`,
`window.set`. cw-ui implements it over the channel the host's JS bridge uses (see
Embedding).

**Modules.** Relative imports resolve as a bundler resolves them (`./x`, `x.tsx`,
`x.ts`, `x/index.tsx`, `x/index.ts`, `x.d.ts`, `./x.js` naming `x.ts`, path
aliases); named, default, namespace (`import * as`) and `type` imports; `export`
declarations, `export default`, `export { a as b }`, re-exports (`export … from`,
`export *`), `enum`, JSON modules, import cycles (live bindings, as ES modules
have them), and module-level code with effects, run in module order. A stylesheet
import (`import './App.css'`) is the page's, and a media import (`import logo from
'./logo.svg'`) is the file's URL, as Vite serves them. Bare specifiers resolve in
`node_modules` as Node's ESM resolution does (`exports` with the browser, import,
module and default conditions, subpath patterns, `module`, `main`); a package runs
on the island.

**Types.** `interface` (with `extends`), `type` aliases, unions of literals,
intersections, arrays and tuples, object types (optional members, index
signatures), `Record<K, V>`, `Partial`, `Pick`, `Omit`, `Exclude`, `Extract`,
`keyof`, `typeof x` in a type, indexed access (`T['k']`, `T[number]`), `as const`,
generic functions and generic `type`/`interface` declarations (a call binds the
type parameters from its explicit type arguments, then from its arguments' types;
inside the body a parameter stands for its constraint), function types,
`Set<T>`, `Map<K, V>`, `RegExp`, React's types (`ReactNode`, `FormEvent`, `ChangeEvent<…>`, `Dispatch<…>`,
`RefObject<…>`, `CSSProperties`, …), DOM element types. Unannotated callback
parameters take their type from context (an array method's element, a setter's
state, an event handler's event, a component prop's declared type).

**Expressions and statements.** Numbers, strings, template literals, booleans,
`null`/`undefined`, arrays and objects (spread, computed keys), destructuring with
defaults and rest, optional chaining, `??`, ternaries, all arithmetic, comparison
and bitwise operators, `typeof`, `instanceof` of `Error`, `TypeError`,
`RangeError` or `SyntaxError`, `Object.is`, assignment and update; `if`, `for`, `for...of`,
`while`, `switch`, `break`/`continue`, `return`, `throw`, `try`/`catch`/`finally`
(thrown values are real `Error`s: `message`, `name`), `async` functions and
`await` (in a `const`/`let` initialiser, an expression statement, an assignment's
right side or a `return`; inside `if`, loops, `switch` and `try` as well). A
variable a closure captures and someone reassigns (`let cancelled = false` flipped
by an effect's cleanup) is shared between them, as in JavaScript. Array methods (`map`,
`filter`, `find`, `findIndex`, `findLast`, `some`, `every`, `reduce`, `forEach`,
`slice`, `concat`, `includes`, `indexOf`, `join`, `sort`/`toSorted`,
`reverse`/`toReversed`, `push`, `pop`, `shift`, `unshift`, `splice`, `flat`,
`flatMap`, `fill`, `at`, `keys`, `entries`, `with`), string methods (`trim*`,
case, `includes`, `startsWith`, `endsWith`, `indexOf`, `slice`, `substring`,
`split`, `replace`/`replaceAll` with strings, `repeat`, `padStart`/`padEnd`,
`charAt`, `charCodeAt`, `codePointAt`, `at`, `localeCompare`, `concat`), `toFixed`, `toString`,
`Math.*`, `Number(…)`, `String(…)`, `parseInt`/`parseFloat`, `Object.keys`/
`values`/`entries`/`assign`/`fromEntries`, `Array.from`/`of`/`isArray`,
`JSON.stringify`, `Set` and `Map` (`new`, `has`, `add`, `get`, `set`, `delete`,
`clear`, `size`, `forEach`, `keys`/`values`/`entries`, spread, `for...of`),
regular expressions (literals; `test`, `exec`; `match`, `search`, `replace`,
`replaceAll`, `split` with a regex; JavaScript syntax and flags on cw-regex, the JS
VM's engine), `console.*`, `Date.now()` on the world clock, timers
(`setTimeout`, `setInterval`) on the world clock, `fetch` with `.then`/`.catch`/
`.finally`, a response's `ok`, `status`, `statusText`, `url`,
`headers.get(name)`/`has(name)`, `text()` and `json()` (`json<T>()` promises a
`T`; unannotated it is `unknown`, so give the result a type), and on a `cw.fetch`
reply also `body`, its text (a `fetch` response's `body` is a stream and outside
the subset; `r.headers` evaluates to the response itself, so only `get` and `has`
read it), `Promise.resolve`, `Promise.reject`,
`Promise.all`, `new Promise((resolve, reject) => …)`.

**Types are hints.** A value whose type says nothing (`any`, `unknown`, a
package's export, `JSON.parse`) compiles: its property reads, calls and method
calls are looked up when they run, by the value's kind, as JavaScript looks
them up. Only an unmodelled host object's property or method (a `Response`'s
`body`, an element's `attachShadow`) is refused, since there is nothing to look
it up in.

**Also compiled:** `Date` (on jsvm's date arithmetic, UTC as on the Realm),
function hoisting and use before declaration, per-iteration `let` in loops, rest
parameters, logical and destructuring assignment, `delete`, `for...in`,
`instanceof` of the built-in classes, `forwardRef`, `useImperativeHandle`,
`useTransition`, `useDeferredValue`, `startTransition` (synchronous: there is no
concurrent rendering), `useInsertionEffect` (as a layout effect), `Suspense`
(renders its children), member component names (`<Menu.Item>`), element geometry
(`getBoundingClientRect`, `offset*`, `client*`, `scroll*`, `scrollIntoView`, the
same numbers as the Realm's layout bindings), `localStorage`/`sessionStorage`,
`location`'s parts, `alert`/`confirm`/`prompt`. With an island: `Intl` and every
`toLocale…` method (the island's jsvm Intl, the one the Realm runs), and `new` on
a value of unknown type (a package's class), constructed on the island.

**Outside the subset** (the module runs on the island if the island can run it,
else the app runs its React fallback): `await` inside a larger expression
(`f(await g())`: await into a variable first), `for await`, generators, classes
and class components, `this`, labelled statements, tagged templates, BigInt,
`dangerouslySetInnerHTML`, portals, `React.lazy`, and host APIs cw-ui does not
model (`history`, `matchMedia`, `requestAnimationFrame`, `navigator` beyond its
constants, assigning `location`).

Every refusal names its place:

```
$ cw-tsx check app.tsx
app.tsx:line 40:21: value of type any: the compiled subset needs a concrete type
app.tsx:line 52:3: `useEffect` must be called at the top level of a component or hook
```

## Building

```
cw-tsx build main.tsx -o out/ [--name app]   # out/app.ui.json, out/app.js, out/app.diagnostics.json
cw-tsx build main.tsx -o out/ --emit rust [--mod app]
                                             # also out/app.ui.rs: the IR as Rust (see below)
cw-tsx check main.tsx                        # diagnostics only; exit 1 if any
```

The argument is the app's entry; the modules it imports are compiled with it into one
IR and one script (each module in its own scope in the script, as a bundler's output
has them).

`build` exits 0 when both outputs were written, 3 when only the fallback was (the
app is outside what the compiled path and its island can run, and a stale
`app.ui.json` is removed), 1 when neither could be. When modules of the app run on
the island, `build` and `check` list them with what put each outside the subset. Compiling the 6,833-byte tracker takes 2.35 ms per run, process included.

## Serving a page

A page loads React's UMD build and the fallback script, and names the compiled IR
on that script with `data-cw-ui`:

```html
<div id="root"></div>
<script src="/vendor/react-18.3.1.production.min.js"></script>
<script src="/vendor/react-dom-18.3.1.production.min.js"></script>
<script src="app.js" data-cw-ui="app.ui.json"></script>
```

Chrome ignores the attribute and runs React. The world's browser
(`crates/web/browser/src/page_script.rs`) fetches the IR the attribute names
(resolved against the page); if it parses, is this runtime's `IR_VERSION` and the
container it renders into is in the page, the app runs on `cw-ui` and none of the
page's scripts run. Otherwise — no attribute, a 404, another version, more than one
declared app — the page is an ordinary scripted page on the Realm and React runs
it. Stylesheets, pictures and everything else load as for any page.

## Runtime semantics

`cw-ui` follows React 18 (`createRoot`, no StrictMode double-rendering):

* Updates inside one native event are batched and rendered when its dispatch ends;
  so are updates in a timer callback or a promise reaction. A `useState` update that
  computes the current value renders nothing (React's eager bailout), and a render
  whose state did not change keeps its output.
* Layout effects run after the DOM is updated, children before parents; then
  passive effects. Cleanups run before creates; a removed subtree's cleanups run
  parent first. A layout effect's update renders synchronously, after the passive
  effects of the commit that scheduled it.
* Keyed children keep their component state and DOM through reorders; unkeyed ones
  match by position.
* A controlled input is restored to its `value` after an event whose handler did not
  update it; `onChange` on a text control fires on `input`, on a checkbox on
  `click`.
* A provider's new value re-renders the consumers below it.
* Attributes come out as React DOM writes them (`className` → `class`, style objects
  through the engine's CSSOM setter, an input's `value` attribute kept in step).
* An exception in render unmounts the app, as React does without an error boundary;
  in a handler, effect or timer it is logged to the console.

Updates are fine-grained: a template is instantiated once and afterwards only holes
whose values changed touch the DOM. In a component's own render a hole whose inputs
are unchanged by identity is not evaluated at all, and an unchanged child element is
not re-rendered. That shortcut is taken only when it cannot be observed: when no code
mutates values in place, no effect lacks a dependency list, and no function reads a
ref's `.current`, the clock, randomness or the console.

Differences from React that a program can observe: `React.memo` compiles to the
component itself (renders are pure, so only render counts differ); `useId` gives
React 18's client ids (`:r0:`, `:r1:`, …) in render order; there is no concurrent
rendering (`startTransition` runs its callback at once, `useTransition` is never
pending, `Suspense` shows its children).

## Islands: packages and code outside the subset

An island is a jsvm VM that cw-ui owns and runs beside the compiled program
(`crates/web/ui/src/island`). cw-tsx gives it one script
(`emit_js::emit_island`): the app's packages, each run as it loads; the app's
modules outside the subset, each wrapped to run at its place in the module order;
and getters over the compiled globals those modules import. Its React is a shim
(`island/shim.js`) over cw-ui's renderer, so there is one React:

* **Elements** are React's own shape in the VM (`$$typeof`, `type`, `key`, `ref`,
  `props`), so a package can build, inspect, clone and map them. They convert to
  cw-ui elements when rendered: a VM component is a component cw-ui renders, a
  host element a template made once per tag. Compiled templates convert back.
* **Hooks** a VM component calls are cw-ui's hooks of the component cw-ui is
  rendering, so state, effects, refs, context, `useSyncExternalStore` and
  `useImperativeHandle` work across the boundary in both directions.
* **Values** cross as handles. A compiled value in the VM is a Proxy (or a native
  function) reading and writing it where it lives, and inheriting the VM's
  Array.prototype and Object.prototype, so VM code uses compiled arrays with its
  own methods. A VM value in compiled code is a `Value::Foreign`, one per VM
  object, whose properties, calls, `new`, iteration, operators and `String()` are
  the VM's. Dates cross as dates (copies); element trees cross as elements.
* **Time and I/O** are cw-ui's: VM timers are cw-ui timers on the world clock, VM
  microtasks drain when cw-ui settles, and the shim's `document`, `window`
  listeners, storage, `location`, window size, scrolling and dialogs call the same
  builtins compiled code does.
* **Snapshots** carry the island: the VM's heap image (jsvm's heap snapshot), the
  tables pairing VM objects with cw-ui values, contexts, exports and entropy
  state. A snapshot restores interpreted or generated, like any other.

**What goes there.** `build_modules` lowers the app, moves every module with a
diagnostic onto the island (and, across an import cycle, the module at the other
end), and lowers again until the rest compiles. An entry on the island renders
through the shim's `createRoot`, `hydrateRoot` or `ReactDOM.render`.
`build_modules_with_island(sources, files)` puts chosen modules there from the
start.

**What does not.** The island is not a browser page. `cw_tsx::island_check` reads
every module bound for it and refuses:

* a free global it lacks (`fetch`, `history`, `matchMedia`,
  `requestAnimationFrame`, `navigator`, …);
* a `window.x` or `document.x` member the shim lacks;
* assigning `location`;
* `dangerouslySetInnerHTML`, `createPortal`, `React.lazy`, and class components.

Then the app is React's, as before islands. jsvm's Node globals are removed so
feature tests answer as a page's would, and `process.env.NODE_ENV` is
`"production"` in both bundles, as a bundler defines it.

**Measured.** In `tsx_parity`, every agent app with its components' module
(`App.tsx`) forced onto the island matches React in every state. The kanban
board in release, medians of 15 (`perf.rs`, `island_against_compiled_and_fallback`):

| | boot | click | key | live after boot | snapshot | restore |
|---|---|---|---|---|---|---|
| compiled | 0.63 ms | 1.03 ms | 0.033 ms | 3,319 KB | 286 KB | 5.9 ms |
| data.ts on the island | 1.53 ms | 1.14 ms | 0.034 ms | 4,069 KB | 437 KB | 8.1 ms |
| App.tsx on the island | 2.46 ms | 2.32 ms | 0.168 ms | 4,258 KB | 564 KB | 9.5 ms |
| React (fallback) | 13.8 ms | 7.92 ms | 1.04 ms | 6,866 KB | 656 KB | 45.2 ms |

## The held-out corpus

`crates/web/tsx/corpus` holds 1,356 files of permissively licensed React and
TypeScript: 287 projects from 15 repositories. `manifest.json` records each
project's source, commit and license. `assemble.mjs` pins them and splits them 138
DEV / 149 TEST with a fixed seed before any compiler change; work on the
subset looks at DEV only. `cw-tsx corpus crates/web/tsx/corpus --split dev|test
[--no-islands] [--json out.json] [--top n] [--grep text]` reports modules,
functions and components inside the subset, whole apps, and the functions blocked
and diagnostics by cause. With islands, a function that uses a package counts as
inside the subset. `--no-islands` counts code compiled alone. The apps line under
"packages stubbed" builds each app with empty modules for its packages (the
corpus has none) and its modules outside the subset on the island. The results
are in `corpus/results` (`baseline.*` before this work, `islands.*` and
`islands-off.*` now):

| | DEV baseline | DEV now | TEST baseline | TEST now |
|---|---|---|---|---|
| functions, compiled alone | 141 / 985 | 302 / 985 | 77 / 770 | 177 / 770 |
| modules, compiled alone | 38 / 532 | 101 / 532 | 18 / 530 | 84 / 530 |
| components, compiled alone | 42 / 647 | 97 / 647 | 34 / 594 | 85 / 594 |
| functions, with islands | — | 891 / 985 | — | 690 / 770 |
| modules, with islands | — | 448 / 532 | — | 452 / 530 |
| apps compiled whole | 0 / 29 | 1 / 29 | 0 / 39 | 0 / 39 |
| apps that build with islands (packages stubbed) | — | 23 / 29 | — | 25 / 39 |
| modules of those apps compiled | — | 90 / 101 | — | 85 / 92 |

The largest cause left is packages: 638 DEV functions use one, and they compile
only with an island.

## Embedding: the `cw_ui::UiApp` API

For a host that is not the web browser (the desktop web-app host, a test):

```rust
use cw_ui::UiApp;
use cw_web::script::{ScriptHostDocument, UiEvent};

let module = UiApp::parse_ir(&ir_json)?;                 // IR version checked
let mut app = UiApp::new(module, &html_shell, url, Box::new(host))?;
// or UiApp::with_document(module, document, url, host) to render into a Document,
// or UiApp::generated(&app_rs::PROGRAM, &html_shell, url, host) for generated Rust
app.boot();                                               // globals, first render, effects
let action = app.dispatch(UiEvent::Click { x, y, button: 0, modifiers, detail: 1 });
app.run_until_idle(16);                                   // timers due on the world clock
let next = app.next_timer_micros();                       // when to call it again
```

* The host is the Realm's `ScriptHostDocument` (fetch, world clock, entropy,
  storage, cookies, console, relayout requests): anything that hosts a Realm hosts an
  app.
* `dispatch` takes the browser's `UiEvent`s (pointer, click, keys, typing,
  `SetValue`, scroll, wheel, focus, resize) and returns the `DefaultAction` left to
  the host (a form submission, a navigation, a focus change). `request_submit(form)`
  is `form.requestSubmit()`.
* To paint and hit-test: `document()`, `styles()`, `fragment_tree()`, and `inner()`
  (the engine state: scroll offsets, focus, `form.values`, `element_from_point`);
  `cw_web::paint::paint(doc, styles, tree, viewport, &ctx)` with `ctx.values =
  app.form_values()`.
* `focused()`, `hovered()`, `title()`, `url()`, `logs()`, `crashed()`,
  `query_selector(css)`, `centre_of(node)`, `stats()` (renders, holes evaluated and
  skipped, script time).
* `snapshot()` returns a `UiState` (serde; `to_json`/`from_json`): the document, the
  component tree with every hook's value, handlers, timers and form state, with
  shared values kept shared. `UiApp::restore(&state, host)` decodes it; nothing
  replays. Work in flight is in it: pending promises with their reactions,
  `Promise.all`s part way, and async functions suspended at an `await` (their
  frames, and their place as positions in the function's statements), so a
  restored app carries on as the original does. As on the Realm, whose snapshot
  replays its journal to the same point, a `cw` request awaiting its reply is
  answered after a restore.
* The `cw` global (`cw_ui`'s `cw` module) speaks the protocol of
  `crates/applications/src/web_app/bridge.js` over the same transport, the host's
  synchronous call (`ScriptHostDocument::host_call`, which the bridge reaches as
  `__cw_host(name, payload)`): `boot` answers the boot facts (`{kind, argument,
  state, env}`), `now` the world clock in microseconds, and `out` takes each
  message (`{op: "request", id, kind, …}`, `{op: "state", value}`, `{op: "refuse",
  message}`, `{op: "chrome", chrome}`), so one host serves an app on either
  backend, interpreted or generated.
  `cw_deliver(replies_json)` settles requests (`[{"id", "value"} | {"id", "error"}]`,
  bridge.js's `__cw_deliver`) and `cw_env(env_json)` updates `cw.env` and runs the
  `onEnv` listeners (`__cw_env`; the host applies the theme itself); both settle the
  app. `declares_state()` says whether the app called `cw.state.set`: such an app's
  declared state is what a host keeps and boots it again from, as on the JS
  backend, while an app that declares none is kept as its `snapshot()`, which
  keeps the requests awaiting replies: `cw_deliver` answers them after a restore
  (`crates/web/ui/tests/cw_bridge.rs`). A host must keep its own record of those
  requests across the snapshot for the replies to come.

## Verification

* `crates/web/ui/tests/tsx_parity.rs`: every `framework-parity/tsx-*.tsx` fixture,
  compiled on `cw-ui` and as its fallback on React on the Realm, through every state
  of its `steps.json`; the documents (with form values, checkedness and focus) must
  be identical, the compiled layout must reach the state's `thresholds.json` entry
  against Chromium's dump of the fallback, and the checked-in `<name>.js` and
  `<name>.ui.json` must be what `cw-tsx` builds (`CW_TSX_BLESS=1` rewrites them).
* The same test compiles each agent-written app in `framework-parity/app-src/` and
  runs it three ways through its `app-<name>.steps.json`: compiled, the agent's own
  esbuild bundle on React on the Realm, and cw-tsx's bundle on React on the Realm. In
  all 39 states of the nine apps the compiled document is identical to React's,
  cw-tsx's bundle is identical to esbuild's, and the compiled layout reaches the
  React fixture's threshold against Chromium.
* `crates/web/engine/tests/framework_parity.rs`: the fallback on the Realm against
  the same Chromium dumps.
* `crates/web/ui/tests/react_semantics.rs`: small apps run both ways, logs and
  documents compared (batching, bailouts, effect order, unmount cleanups, keyed
  reorders with state, controlled inputs and carets, context, reducers, memos, refs,
  intervals, forms, attributes, snapshot/restore).
* `crates/web/tsx/tests/diagnostics.rs`: what is refused, and where, including
  what the island refuses.
* Islands: `react_semantics` runs packages (vendored clsx and zustand, and
  `tests/islands/packages/island-kit`), modules and an entry outside the subset,
  Intl, and the page APIs through the shim, each interpreted, generated, on React
  and restored across forms. `tsx_parity` runs every agent app with `App.tsx` on
  the island against React.
* Generated Rust: `crates/web/ui/fixtures` (`cw-ui-fixtures`) generates, at build
  time, every app the tests above run (31 programs). `react_semantics` runs each app
  interpreted, generated and on React; `tsx_parity` runs tsx-tasks and the six
  agent apps (31 sessions) interpreted and generated in lockstep. After boot and
  every step the documents (with form values, checkedness, focus), logs and render
  counters must be identical, the final states equal, and each form's snapshot must
  restore on the other. `cw_bridge` runs on both forms and restores a snapshot with
  suspended async functions and requests in flight on the other form.
* `crates/web/browser/tests/compiled_app.rs`: a served page runs compiled when its IR
  is there and on React when not, showing the same thing through the same actions,
  and snapshots and restores.

## Performance

Release builds; engine sides median of 15 fresh apps (`cargo test --release -p cw-ui
--test perf -- --ignored --nocapture`), Chromium median of 15 fresh pages with
DevTools' script time (`node crates/web/ui/bench/chromium-perf.mjs
…/tsx-tasks.html`). Boot is to the rendered DOM; the first style and layout pass,
which both engine sides do next, is not in it.

The task tracker (`tsx-tasks`):

| | compiled (cw-ui) | fallback (React 18 on the Realm) | Chromium (script) |
|---|---|---|---|
| boot | 0.31 ms (0.13 of it parsing the IR) | 53.0 ms first load, 9.8 ms with the code cache warm | 10.9 ms |
| first click that re-renders | 2.13 ms, of which 0.057 ms script | 6.47 ms | 3.51 ms |
| second click | 0.46 ms, of which 0.040 ms script | 3.78 ms | 0.75 ms |
| keystroke, controlled input | 0.024 ms | 2.65 ms | 1.10 ms |
| memory held by the mounted app | 729 KB | 5,162 KB | — |
| snapshot | 82 KB (32 KB of it the IR) | 520 KB (journal) | — |
| restore, first layout included | 2.8 ms | 31.3 ms (replay) | — |

The agent-written kanban board (`app-src/kanban`, Tailwind, 92 KB of IR), the
fallback being the agent's esbuild bundle:

| | compiled (cw-ui) | fallback (React 18 on the Realm) |
|---|---|---|
| boot | 0.75 ms (0.31 of it parsing the IR) | 76.4 ms first load, 30.7 ms warm |
| click that moves a card | 23.2 ms, of which 0.21 ms script | 41.4 ms |
| keystroke, controlled input | 0.037 ms | 3.6 ms |
| memory held by the mounted app | 4,753 KB | 9,548 KB |
| snapshot | 288 KB | 656 KB |
| restore, first layout included | 21.1 ms | 208.6 ms |

A compiled click is almost all the engine's style and layout pass that hit testing
forces once the hover state changes (a full restyle and layout is 1.65 ms for the
tracker, 17.9 ms for the kanban page's Tailwind sheet); handlers, render and commit
take 57 µs and 209 µs. `cw-ui` added 799 KB raw, 207 KB gzipped, to the site's
Wasm module when the browser first linked it (fef4e40).

Notes, the desktop's notes app (`crates/applications/web/notes`, 51 KB of IR), in
its host, through the host's own harness (`cargo test --release -p cw-applications
--test web_notes_cost -- --ignored --nocapture web_notes_costs`), medians, the
fallback being the same source's React bundle on the Realm (measured in the same
session):

| | compiled (cw-ui) | fallback (React 18 on the Realm) |
|---|---|---|
| launch, listing delivered | 0.24 ms | 9.8 ms |
| open a note, type, save | 4.0 ms | 12.2 ms |
| memory held by the window | 908 KB | 5,511 KB |
| restore and first paint | 2.5 ms | 14.2 ms |
| paint of a fresh clone | 1.26 ms | 0.25 ms |

The paint row is not like for like: the Realm restyles and lays out while it
settles an input, so its paint only builds the scene, while cw-ui leaves style and
layout to the first read. Per entry cw-ui's script (handlers, render, commit, the
`cw` bridge) takes 20–210 µs; the rest is the engine's style flush, about 0.9 ms
for Notes' 24 elements, of which building the cascade engine from the sheets is
360 µs before anything is restyled (`crates/web/ui/tests/perf.rs`, `notes_phases`).
The Realm pays the same flush on every input.

## Ahead-of-time Rust for built-in apps

An app built into the binary need not ship IR for an interpreter: `cw-tsx build
--emit rust` (`cw_tsx::emit_rust`) translates the IR into a Rust module defining
`pub static PROGRAM: cw_ui::GenProgram`, which `UiApp::generated` runs. Apps loaded
at run time (a served page, a world's own application) keep the IR: there is no
rustc in the world or in Wasm.

**One runtime, two forms.** `cw_ui::Program` is what a runtime runs; `IrProgram`
(the interpreter) and `GenProgram` (generated code) both implement it and keep the
IR's numbering (function `n`, template `t`, global `g`, hook order), so the
reconciler, hooks, events, commit, `cw` bridge and snapshots are shared and a
document, a log or a snapshot does not depend on the form. Generated code calls
the same helpers the interpreter does (`cw_ui::gen`), in the same order.

**What the generator does.** Each function is a Rust `fn`; frame slots are Rust
locals; expressions are flattened into temporaries, so evaluation order is the
source's; `try`, optional chains, `switch` and loops are labelled blocks. Templates
are `static` data built by the runtime's one template builder (static code per
template would add Wasm size for no measurable gain: instantiating is DOM work).
Hole dependencies are named in the code, and a template all of whose holes read
the same slots is decided with one comparison. Calls to module functions nobody
reassigns are direct calls. String literals and property keys are made once per
runtime; comparisons feed conditions as Rust `bool`s; `===` against a string
literal, `+` with one, and arithmetic or comparison with a number literal need no
temporary value. Async function bodies stay IR (JSON in the module, parsed on
first call) and run on the interpreter's suspendable walk, whose continuations are
positions in those bodies; everything they call is generated.

**What it does not do.** State is not typed Rust structs: hook state, props and
every object stay the runtime's dynamic `Value`s. The reconciler's bailouts compare
by identity, a spread copies keys in order, closures see mutations, and the
snapshot format and cross-form restore need one representation; TypeScript's types
are also unsound where it matters (an index past the end is `undefined` typed as the
element), so a typed field would still need checks. The measurements below show why
it would not pay: script is a small part of an interaction, and the interpreter's
walk was a fraction of that.

**Snapshots.** A generated app's `UiState` names its program (`program:
ProgramId`, the name and FNV-1a 64 of the IR's canonical JSON) instead of carrying
the IR in `module`. `UiApp::restore` finds it among programs registered with
`cw_ui::program::register`; `UiApp::restore_with(state, program, host)` restores any
snapshot on either form of the same IR (checked by that hash): an interpreted
app's snapshot on its generated program, a generated app's on the interpreter
given the IR, suspended async functions and requests in flight included. A
snapshot of different IR is refused.

**Notes.** `crates/applications/web/build.mjs` writes `notes/notes.ui.rs` beside
`notes.ui.json` and `--check` fails when it is stale. It is checked in rather than
generated by a build script so building the world never needs cw-tsx (oxc) and the
code is reviewable. The catalog pairs the program with the IR it came from and
the web-app host runs it when booting exactly that IR (a unit test checks the
pairing); Notes snapshots taken before carry their IR and restore interpreted.

**Measured** (release, median of 15, `cargo test --release -p cw-ui --test perf --
--ignored --nocapture`; Notes on a stubbed `cw` channel, its React side from
`web_notes_cost`'s `where_a_web_entry_spends_its_time`; one session, machine shared
with other builds):

| | interpreted IR | generated Rust | React 18 on the Realm |
|---|---|---|---|
| tsx-tasks boot | 0.278 ms (0.109 IR parse) | 0.155 ms | 8.36 ms (37.1 first load) |
| tsx-tasks click: total / script | 0.229 / 0.0536 ms | 0.213 / 0.0427 ms | 3.06 ms |
| tsx-tasks keystroke: total / script | 0.0171 / 0.0144 ms | 0.0132 / 0.0111 ms | 1.90 ms |
| tsx-tasks heap after boot | 1,009 KB | 871 KB | 5,446 KB |
| tsx-tasks snapshot / restore | 82 KB / 1.82 ms | 50 KB / 1.60 ms | 520 KB / 20.7 ms |
| kanban boot | 0.700 ms (0.318 IR parse) | 0.359 ms | 17.5 ms (44.2 first load) |
| kanban click: total / script | 0.853 / 0.155 ms | 0.842 / 0.145 ms | 11.6 ms |
| kanban keystroke: total / script | 0.0191 / 0.0157 ms | 0.0185 / 0.0152 ms | 2.36 ms |
| kanban heap after boot | 3,351 KB | 2,928 KB | 8,145 KB |
| kanban snapshot / restore | 288 KB / 8.67 ms | 196 KB / 8.27 ms | 656 KB / 62.1 ms |
| Notes launch, listing delivered | 0.459 ms | 0.325 ms | 32.5 ms boot |
| Notes session script (open, read, type, save, list) | 0.164 ms | 0.123 ms | — |
| Notes keystroke script | 0.0237 ms | 0.0169 ms | 0.66–0.74 ms type+settle |
| Notes heap after launch | 758 KB | 534 KB | — |
| Notes snapshot / restore+layout | 71.6 KB / 1.02 ms | 21.7 KB / 0.80 ms | — |

Plainly: generated code wins where the IR itself cost something — boot (no parse:
44–49% less), heap (the IR is gone: 138–423 KB), snapshots (no IR: 32–70% smaller)
and restore (5–21%). On interactions it barely matters: script falls 3–29% (the
kanban click by 7%, 10 µs), because most script time is the shared runtime
(reconciling, committing, DOM writes), not the walk over the IR, and a click's total
is the engine's style and layout, which neither form touches. Generated Notes adds
95,854 bytes raw, 26,041 bytes gzipped, to the Wasm module (40,560,922 → 40,656,776,
cargo's `cw_wasm.wasm` with `scripts/build-wasm.sh`'s settings), 0.24% and 0.17%.
