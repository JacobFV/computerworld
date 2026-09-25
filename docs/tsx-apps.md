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
`framework-parity/app-src/<app>/` holds six apps written by a coding agent (an
analytics dashboard, a chat, a data table, a kanban board, a settings form, a shop:
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
`x.ts`, `x/index.tsx`, `x/index.ts`); named, default and `type` imports; `export`
declarations, `export default function`, `export { a as b }`.

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
`.finally`, `response.json()`/`text()`, `Promise.resolve`, `Promise.reject`,
`Promise.all`, `new Promise((resolve, reject) => …)`.

**Outside the subset** (the page runs its React fallback): `any` and values of
unknown type (`JSON.parse`, an unannotated `response.json()` result), `await`
inside a larger expression (`f(await g())`: await into a variable first), `for
await`, generators, classes and class components, packages other than React
(an app bundles its own modules only), re-exports (`export … from`), `import * as`
of a module of the app, `enum`, `delete`, `this`, `new` (except `Set`, `Map`,
`Error`, `Promise`), `instanceof` of anything but an error class, ambient
declarations other than `declare const` and types, host globals other than `cw`,
`dangerouslySetInnerHTML`,
portals and the React APIs not listed above (`forwardRef`, `useTransition`, …).

Every refusal names its place:

```
$ cw-tsx check app.tsx
app.tsx:line 40:21: value of type any: the compiled subset needs a concrete type
app.tsx:line 52:3: `useEffect` must be called at the top level of a component or hook
```

## Building

```
cw-tsx build main.tsx -o out/ [--name app]   # out/app.ui.json, out/app.js, out/app.diagnostics.json
cw-tsx check main.tsx                        # diagnostics only; exit 1 if any
```

The argument is the app's entry; the modules it imports are compiled with it into one
IR and one script (each module in its own scope in the script, as a bundler's output
has them).

`build` exits 0 when both outputs were written, 3 when only the fallback was (the
module is outside the subset, and a stale `app.ui.json` is removed), 1 when neither
could be. Compiling the 6,833-byte tracker takes 2.35 ms per run, process included.

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
rendering, `startTransition` or Suspense.

## Embedding: the `cw_ui::UiApp` API

For a host that is not the web browser (the desktop web-app host, a test):

```rust
use cw_ui::UiApp;
use cw_web::script::{ScriptHostDocument, UiEvent};

let module = UiApp::parse_ir(&ir_json)?;                 // IR version checked
let mut app = UiApp::new(module, &html_shell, url, Box::new(host))?;
// or UiApp::with_document(module, document, url, host) to render into a Document
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
  `crates/applications/src/web_app/bridge.js` over the same channel, the host's
  `localStorage`: it reads `"\u{1}cw:boot"` (`{kind, argument, state, env}`) and
  `"\u{1}cw:now"`, and writes each message (`{op: "request", id, kind, …}`,
  `{op: "state", value}`, `{op: "refuse", message}`, `{op: "chrome", chrome}`) to
  `"\u{1}cw:out"`, so one host serves an app on either backend.
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
  all 27 states the compiled document is identical to React's, cw-tsx's bundle is
  identical to esbuild's, and the compiled layout reaches the React fixture's
  threshold against Chromium (18 states at 100% of nodes, the others at
  98.8–99.7%, as React's are).
* `crates/web/engine/tests/framework_parity.rs`: the fallback on the Realm against
  the same Chromium dumps.
* `crates/web/ui/tests/react_semantics.rs`: small apps run both ways, logs and
  documents compared (batching, bailouts, effect order, unmount cleanups, keyed
  reorders with state, controlled inputs and carets, context, reducers, memos, refs,
  intervals, forms, attributes, snapshot/restore).
* `crates/web/tsx/tests/diagnostics.rs`: what is refused, and where.
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

## Later: Rust code generation

The IR carries the static type of every function parameter, frame slot and template
hole, and addresses variables by slot, so a later stage can translate a module into
Rust source for built-in apps: each slot a typed local, each template a function that
builds its DOM, each hole an update guarded by its dependencies.
