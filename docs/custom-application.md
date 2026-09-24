# Create an application

Start from [`examples/native/counter.rs`](../examples/native/counter.rs). Implement `Application`
and register it independently of the kernel. Put persistent semantic state in its
instance value; derive a native `Page` from immutable state.

Define event names and stable target IDs. Return explicit effects for filesystem
or network work, and update the visible page from completed results. Test state
transitions separately from layout, then test hit testing and actor interaction.
The same app can be observed structurally without rasterization or drawn to RGBA
for a vision agent.

Keep OS-specific mechanics in the computer substrate and transport mechanics in
networking. The application should not directly inspect other machines or service
stores. See [application-sdk.md](application-sdk.md).

## Web applications

An application can instead be written as a web app: TSX (or any script) that renders
a document, hosted in a desktop window whose frame stays the platform's own. The web
engine lays the document out and paints it in the window; the platform keeps what it
keeps for every native application — its palette and UI font, scrolling, the phone's
keyboard and Back. The built-in Notes is one (`crates/applications/web/notes/`).

Register it with `World::register_web_application` (or
`Environment::register_web_application`) and install its kind on a machine
(`installed_apps`); `application.v1` `launch` then opens it like any application.

```rust
world.register_web_application(cw_sdk::WebApplication {
    kind: "counter".into(),
    version: 1,
    titles: [("*".into(), "Counter".into())].into(),
    source: cw_sdk::WebSource::Script {
        script: include_str!("counter.bundle.js").into(),
        style: include_str!("counter.css").into(),
        react: true, // React 18's production build is loaded first
    },
})?;
```

An app inside `cw-tsx`'s compiled subset ([TSX apps](tsx-apps.md)) is registered with
`WebSource::Compiled { ir, script, style }` instead — the `<app>.ui.json` and
`<app>.js` that `cw-tsx build` writes — and runs on `cw-ui`, with no VM and no React;
its snapshot is `cw-ui`'s own state (document, components, hook values), so it declares
none. The `script` is its React fallback, run when this build cannot mount the IR. The
compiled subset has no `cw` global yet, so an app that reaches the machine is a
`Script` app for now.

The script renders into `#root`. It reaches the machine only through the `cw` global,
typed in `crates/applications/web/types/cw.d.ts`; each call becomes an application
effect the environment mediates exactly as it mediates a native application's:

| `cw.` | does |
| --- | --- |
| `fs.readFile(path)`, `fs.writeFile(path, text)`, `fs.list(folder)`, `fs.mkdir(folder)` | the machine's files, with the user's permissions |
| `fetch(url, { method, body })` | a request to a world service; a transport failure rejects |
| `launch(kind, argument)` | opens another application's window |
| `emit(name, data)` | records named data in the world's event log |
| `state.get()`, `state.set(value)` | the declared state (below) |
| `now()`, `env`, `onEnv(f)` | the world clock of the input being handled; platform, size and palette |
| `refuse(message)` | fails the action that delivered the input, as a native refusal does |
| `window.set({ document, caption, modified })` | what the frame shows about what is open |

`Date.now()` and `Math.random()` are deterministic too (the world clock, a seed fixed
by the kind), but `Date.now()` may run a few milliseconds ahead of `cw.now()`.

**Agents act by element id.** Every element with an `id` is addressed by it: a click
on a control is a click on that element, and the scene names each interactive element
by its id, as a native application names its controls. Typed text goes to the focused
text control, and that control's id is the window's text focus. The semantic page
lists every element with an id in document order — headings, buttons, links, text
controls (with their live values), and `p`/`span`/`label` or `role="status"|"alert"`
text; `data-page-id` gives an element a page id different from its control id, and an
`aria-hidden="true"` subtree is left out.

**Platform integration by attribute.** A scroll container with an id (or
`data-cw-pane="<name>"`) is a pane of the window: its offset is window state, the
wheel and a phone's swipe move it, and the platform paints its scroll bar.
`data-cw-large-title="<title>"` on a pane gives it iOS's collapsing large title;
`data-cw-back` marks where a phone's Back goes.

**Snapshots.** A web application is its declared state. A snapshot keeps what the
application last passed to `cw.state.set`; a restored window boots the same code with
`cw.state.get()` returning it, and must show the same document — so everything the
document depends on belongs in that state. A restore boot must not request anything
(requests it makes are dropped), and neither may a handler of `cw.onEnv`. Focus and
caret are the application's to derive from state; scroll offsets are the window's.
The code itself is never serialised: a snapshot names the kind and version, and the
world restoring it must have registered the same application.

**Building TSX.** `node crates/applications/web/build.mjs` type-checks the sources
against `cw.d.ts` and React's types, then compiles each app with `cw-tsx`
([TSX apps](tsx-apps.md)): `<app>.js` is the script React 18's production build runs,
and `<app>.diagnostics.json` records why the app is not (yet) inside the compiled
subset, whose IR would run on `cw-ui` without a VM. The outputs are checked in beside
the source (`--check` fails when one is stale).
`crates/applications/web/sdk/cw.ts` has a store for declared state
(`declaredStore`, `useStore`) and `useEnv`.
