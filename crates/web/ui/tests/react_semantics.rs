//! React 18 semantics, checked against React itself: each test is a small TSX app
//! that logs what it observes (renders, effects, cleanups, values). It is compiled
//! by `cw-tsx` and run twice through the same interaction: natively on `cw-ui`, and
//! as the emitted fallback on React 18's production build on the engine's JS
//! `Realm`. The console logs and the documents must be identical. The compiled side
//! runs twice more: as the Rust `cw-tsx` generates from the same IR
//! (`cw-ui-fixtures`), which must match the interpreter after every step (document,
//! logs, render counters) and end in the same state (their snapshots equal but for
//! the program's name), and each side's snapshot restored on the other form.

use cw_ui::UiApp;
use cw_web::dom::{Document, NodeId, NodeKind};
use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};

const BASE: &str = "https://example.test/";

fn vendor(h: MemoryHost) -> MemoryHost {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/vendor");
    let mut h = h
        .with_response(
            &format!("{BASE}api/items"),
            "application/json",
            r#"[{"id": 2, "name": "beta", "tags": ["x"]}, {"id": 1, "name": "alpha", "tags": []}]"#,
        )
        .with_response(
            &format!("{BASE}api/user/1"),
            "application/json",
            r#"{"id": 1, "name": "Ada"}"#,
        )
        .with_response(
            &format!("{BASE}api/user/2"),
            "application/json",
            r#"{"id": 2, "name": "Bo"}"#,
        )
        .with_response(
            &format!("{BASE}api/broken"),
            "application/json",
            "{not json",
        );
    for name in [
        "react-18.3.1.production.min.js",
        "react-dom-18.3.1.production.min.js",
    ] {
        let body = std::fs::read_to_string(dir.join(name)).expect("vendor");
        h = h.with_response(&format!("{BASE}vendor/{name}"), "text/javascript", &body);
    }
    h
}

const SHELL: &str = r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<style>body { margin: 0; font: 14px/20px sans-serif; } button { display: block; width: 200px; height: 30px; }
input { display: block; width: 200px; height: 30px; }</style>
<script src="/vendor/react-18.3.1.production.min.js"></script>
<script src="/vendor/react-dom-18.3.1.production.min.js"></script>
</head><body><div id="root"></div><script src="app.js"></script></body></html>"#;

#[derive(Clone, Debug)]
enum Step {
    Click(&'static str),
    Type(&'static str),
    Key(&'static str),
    /// Advances the clock (timers).
    Wait(u32),
    /// The window is resized to this width and height.
    Resize(u32, u32),
}

struct Outcome {
    logs: Vec<String>,
    dom: String,
}

fn dom_text(
    doc: &Document,
    values: &std::collections::BTreeMap<NodeId, String>,
    sel: &dyn Fn(NodeId) -> Option<(usize, usize)>,
) -> String {
    fn walk(
        doc: &Document,
        n: NodeId,
        values: &std::collections::BTreeMap<NodeId, String>,
        sel: &dyn Fn(NodeId) -> Option<(usize, usize)>,
        out: &mut String,
    ) {
        match doc.kind(n) {
            NodeKind::Element { tag, attrs, ns } => {
                if tag == "script" {
                    return;
                }
                out.push('<');
                if *ns != cw_web::dom::Namespace::Html {
                    out.push_str(&format!("{ns:?}:"));
                }
                out.push_str(tag);
                for a in attrs {
                    out.push_str(&format!(" {}={:?}", a.name, a.value));
                }
                if tag == "input" {
                    out.push_str(&format!(
                        " [value={:?} sel={:?}]",
                        values.get(&n).cloned().unwrap_or_default(),
                        sel(n)
                    ));
                }
                out.push('>');
                for c in doc.children(n) {
                    walk(doc, c, values, sel, out);
                }
                out.push_str(&format!("</{tag}>"));
            }
            NodeKind::Text(t) => out.push_str(t),
            _ => {}
        }
    }
    let mut out = String::new();
    if let Some(root) = doc.by_id("root").first() {
        walk(doc, *root, values, sel, &mut out);
        // Whatever else the app put in the body (a portal's content).
        if let Some(body) = doc.parent(*root) {
            for c in doc.children(body) {
                if c != *root && doc.is_element(c) && !doc.is(c, "script") {
                    walk(doc, c, values, sel, &mut out);
                }
            }
        }
    }
    out
}

fn compile(tsx: &str) -> (cw_ui::ir::Module, String) {
    // Several modules (`// @file <path>` lines), the first the entry; or one.
    let b = match cw_tsx::virtual_files(tsx) {
        Some(files) => {
            let packages =
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/islands/packages");
            cw_tsx::build_virtual_with(&files, Some(&packages)).unwrap_or_else(|d| panic!("{d:?}"))
        }
        None => cw_tsx::build(tsx, "app.tsx"),
    };
    assert!(
        b.diagnostics.is_empty(),
        "outside the subset:\n{}",
        b.diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
    (b.ir.unwrap(), b.js.unwrap())
}

fn event(step: &Step, at: Option<(i32, i32)>) -> Vec<UiEvent> {
    let modifiers = Modifiers::default();
    match step {
        Step::Click(_) => {
            let (x, y) = at.unwrap();
            vec![
                UiEvent::PointerMove { x, y, modifiers },
                UiEvent::Click {
                    x,
                    y,
                    button: 0,
                    modifiers,
                    detail: 1,
                },
            ]
        }
        Step::Type(t) => vec![UiEvent::TypeText { text: (*t).into() }],
        Step::Key(k) => vec![UiEvent::Key {
            key: (*k).into(),
            code: String::new(),
            modifiers,
            repeat: false,
        }],
        Step::Wait(_) => vec![],
        Step::Resize(w, h) => vec![UiEvent::Resize {
            width: *w,
            height: *h,
        }],
    }
}

/// A compiled app on the interpreter, or on the Rust generated from its IR.
fn app_on(module: &cw_ui::ir::Module, generated: bool) -> UiApp {
    let host = Box::new(vendor(MemoryHost::new()));
    let url = format!("{BASE}app.html");
    if generated {
        let p = cw_ui_fixtures::for_module(module).expect(
            "no generated program for this IR: cw-ui-fixtures' build.rs did not see its source",
        );
        cw_ui::program::register(p);
        UiApp::generated(p, SHELL, &url, host).unwrap()
    } else {
        UiApp::new(module.clone(), SHELL, &url, host).unwrap()
    }
}

/// A snapshot as JSON without the program's identity (the IR or its name), which is
/// all an interpreted and a generated app's states may differ in.
fn state_json(app: &UiApp) -> String {
    let mut s = app.snapshot();
    s.module = None;
    s.program = None;
    s.to_json()
}

struct Compiled {
    outcome: Outcome,
    /// The document after each step.
    steps: Vec<String>,
    stats: String,
    state: String,
    app: UiApp,
}

fn run_compiled_on(module: &cw_ui::ir::Module, steps: &[Step], generated: bool) -> Compiled {
    let mut app = app_on(module, generated);
    app.boot();
    let mut per_step = Vec::new();
    for s in steps {
        let at = match s {
            Step::Click(sel) => {
                let n = app.query_selector(sel).unwrap_or_else(|| {
                    panic!(
                        "compiled: no {sel}; logs: {:?}",
                        app.logs().iter().map(|l| &l.text).collect::<Vec<_>>()
                    )
                });
                app.centre_of(n)
            }
            _ => None,
        };
        for ev in event(s, at) {
            app.dispatch(ev);
        }
        app.run_until_idle(match s {
            Step::Wait(ms) => *ms,
            _ => 20,
        });
        if std::env::var_os("CW_UI_TRACE").is_some() {
            let d = app.document().clone();
            eprintln!("{s:?}: {}", dom_text(&d, &Default::default(), &|_| None));
        }
        let values = app.form_values();
        let doc = app.document().clone();
        let selections = app.inner().form.selection.clone();
        per_step.push(dom_text(&doc, &values, &|n| selections.get(&n).copied()));
    }
    let values = app.form_values();
    let doc = app.document().clone();
    let selections = app.inner().form.selection.clone();
    let st = app.stats();
    Compiled {
        outcome: Outcome {
            logs: app
                .logs()
                .into_iter()
                .map(|l| format!("{:?}: {}", l.level, l.text))
                .collect(),
            dom: dom_text(&doc, &values, &|n| selections.get(&n).copied()),
        },
        steps: per_step,
        stats: format!(
            "renders {} holes {} skipped {} reused {}",
            st.renders, st.holes_evaluated, st.holes_skipped, st.elements_reused
        ),
        state: state_json(&app),
        app,
    }
}

/// The interpreter and the generated program through `steps`: identical after
/// every step and at the end, and each one's snapshot restores on the other.
fn same_as_generated(module: &cw_ui::ir::Module, steps: &[Step]) -> Outcome {
    let interp = run_compiled_on(module, steps, false);
    let generated = run_compiled_on(module, steps, true);
    assert!(generated.app.is_generated() && !interp.app.is_generated());
    for (i, (a, b)) in interp.steps.iter().zip(&generated.steps).enumerate() {
        assert_eq!(
            a, b,
            "documents differ after step {i} (left: interpreted, right: generated)"
        );
    }
    assert_eq!(
        interp.outcome.logs, generated.outcome.logs,
        "console logs differ (left: interpreted, right: generated)"
    );
    assert_eq!(interp.stats, generated.stats, "render counters differ");
    assert_eq!(
        interp.state, generated.state,
        "states differ (left: interpreted, right: generated)"
    );
    // Cross restores: the interpreter's snapshot on the generated program, and the
    // generated program's on the interpreter.
    let program = cw_ui_fixtures::for_module(module).unwrap();
    let from_interp = cw_ui::UiState::from_json(&interp.app.snapshot().to_json()).unwrap();
    let on_gen = UiApp::restore_with(
        &from_interp,
        std::rc::Rc::new(cw_ui::program::StaticProgram(program)),
        Box::new(vendor(MemoryHost::new())),
    )
    .unwrap();
    assert!(on_gen.is_generated());
    assert_eq!(
        state_json(&on_gen),
        interp.state,
        "interpreted state restored on generated code"
    );
    let from_gen = cw_ui::UiState::from_json(&generated.app.snapshot().to_json()).unwrap();
    assert!(
        from_gen.module.is_none(),
        "a generated app's snapshot names its program"
    );
    let on_interp = UiApp::restore_with(
        &from_gen,
        std::rc::Rc::new(cw_ui::IrProgram::new(module.clone())),
        Box::new(vendor(MemoryHost::new())),
    )
    .unwrap();
    assert!(!on_interp.is_generated());
    assert_eq!(
        state_json(&on_interp),
        generated.state,
        "generated state restored on the interpreter"
    );
    interp.outcome
}

fn run_fallback(js: &str, steps: &[Step]) -> Outcome {
    let host =
        vendor(MemoryHost::new()).with_response(&format!("{BASE}app.js"), "text/javascript", js);
    let mut r = Realm::new(SHELL, &format!("{BASE}app.html"), Box::new(host));
    r.run_document();
    r.run_until_idle(50);
    for s in steps {
        let at = match s {
            Step::Click(sel) => {
                let src = format!(
                    "(() => {{ const q = document.querySelector({sel:?}).getBoundingClientRect(); return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()"
                );
                let v = r.eval(&src).unwrap();
                let mut it = v.split(',').map(|n| n.parse::<f64>().unwrap() as i32);
                Some((it.next().unwrap(), it.next().unwrap()))
            }
            _ => None,
        };
        for ev in event(s, at) {
            r.dispatch(ev);
        }
        r.run_until_idle(match s {
            Step::Wait(ms) => *ms,
            _ => 20,
        });
    }
    let values = r.form_values();
    let doc = r.document().clone();
    let selections = r.layout().form.selection.clone();
    Outcome {
        logs: r
            .logs()
            .into_iter()
            .map(|l| format!("{:?}: {}", l.level, l.text))
            .collect(),
        dom: dom_text(&doc, &values, &|n| selections.get(&n).copied()),
    }
}

/// Runs `tsx` both ways through `steps` and requires identical logs and documents.
fn same_as_react(tsx: &str, steps: &[Step]) -> Vec<String> {
    let (module, js) = compile(tsx);
    let a = same_as_generated(&module, steps);
    let b = run_fallback(&js, steps);
    if std::env::var_os("CW_UI_SHOW").is_some() {
        eprintln!("logs: {:?}\ndom: {}", a.logs, a.dom);
    }
    assert_eq!(
        a.logs, b.logs,
        "console logs differ (left: compiled, right: React)"
    );
    assert_eq!(
        a.dom, b.dom,
        "documents differ (left: compiled, right: React)"
    );
    a.logs
}

#[test]
fn updates_in_one_event_render_once() {
    let logs = same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
function App() {
  const [a, setA] = useState(0);
  const [b, setB] = useState(0);
  console.log('render', a, b);
  return <button id="go" onClick={() => { setA(a + 1); setB((x) => x + 10); setA((x) => x + 1); }}>{a}/{b}</button>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
    assert_eq!(
        logs.iter().filter(|l| l.contains("render")).count(),
        3,
        "{logs:?}"
    );
}

#[test]
fn setting_the_same_state_bails_out() {
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
function App() {
  const [n, setN] = useState(1);
  console.log('render', n);
  return <button id="go" onClick={() => setN(1)}>{n}</button>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn layout_effects_run_before_effects_and_cleanups_before_creates() {
    same_as_react(
        r#"
import { useEffect, useLayoutEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
function Child({ n }: { n: number }) {
  useLayoutEffect(() => { console.log('child layout', n); return () => console.log('child layout cleanup', n); }, [n]);
  useEffect(() => { console.log('child effect', n); return () => console.log('child effect cleanup', n); }, [n]);
  return <span>{n}</span>;
}
function App() {
  const [n, setN] = useState(0);
  useLayoutEffect(() => { console.log('app layout', n); return () => console.log('app layout cleanup', n); }, [n]);
  useEffect(() => { console.log('app effect', n); return () => console.log('app effect cleanup', n); }, [n]);
  useEffect(() => { console.log('app mount only'); }, []);
  return <div><button id="go" onClick={() => setN(n + 1)}>more</button><Child n={n} /><Child n={n * 10} /></div>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn unmounting_runs_cleanups_parent_first() {
    same_as_react(
        r#"
import { useEffect, useLayoutEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
function Leaf({ name }: { name: string }) {
  useEffect(() => { console.log('mount', name); return () => console.log('unmount', name); }, []);
  useLayoutEffect(() => () => console.log('layout unmount', name), []);
  return <i>{name}</i>;
}
function Panel() {
  useEffect(() => () => console.log('unmount panel'), []);
  return <section><Leaf name="a" /><Leaf name="b" /></section>;
}
function App() {
  const [open, setOpen] = useState(true);
  return <div><button id="toggle" onClick={() => setOpen(!open)}>toggle</button>{open && <Panel />}{open ? null : <p>closed</p>}</div>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#toggle"), Step::Click("#toggle")],
    );
}

#[test]
fn keyed_items_keep_their_state_through_reorders() {
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
function Counter({ label }: { label: string }) {
  const [n, setN] = useState(0);
  return <li><button id={'inc-' + label} onClick={() => setN(n + 1)}>{label}: {n}</button></li>;
}
function App() {
  const [items, setItems] = useState(['a', 'b', 'c', 'd']);
  return (
    <div>
      <button id="reverse" onClick={() => setItems([...items].reverse())}>reverse</button>
      <button id="rotate" onClick={() => setItems([...items.slice(1), items[0]])}>rotate</button>
      <button id="drop" onClick={() => setItems(items.filter((x) => x !== 'b'))}>drop b</button>
      <ul>{items.map((x) => <Counter key={x} label={x} />)}</ul>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#inc-b"),
            Step::Click("#inc-b"),
            Step::Click("#inc-d"),
            Step::Click("#reverse"),
            Step::Click("#inc-a"),
            Step::Click("#rotate"),
            Step::Click("#drop"),
            Step::Click("#reverse"),
        ],
    );
}

#[test]
fn controlled_inputs_keep_value_and_caret() {
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
function App() {
  const [text, setText] = useState('');
  const [short, setShort] = useState('ab');
  const [free, setFree] = useState('x');
  return (
    <div>
      <input id="upper" value={text} onChange={(e) => setText(e.target.value.toUpperCase())} />
      <input id="short" value={short} onChange={(e) => { if (e.target.value.length <= 4) setShort(e.target.value); }} />
      <input id="stuck" value="fixed" onChange={(e) => console.log('stuck saw', e.target.value)} />
      <input id="free" defaultValue={free} onInput={(e) => setFree(e.target.value)} />
      <p>{text}|{short}|{free}</p>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#upper"),
            Step::Type("hello"),
            Step::Key("Backspace"),
            Step::Click("#short"),
            Step::Type("cdef"),
            Step::Click("#stuck"),
            Step::Type("zz"),
            Step::Click("#free"),
            Step::Type("yz"),
        ],
    );
}

#[test]
fn context_updates_reach_consumers() {
    same_as_react(
        r#"
import { createContext, useContext, useState } from 'react';
import { createRoot } from 'react-dom/client';
const Theme = createContext('light');
function Label() {
  const theme = useContext(Theme);
  return <b className={theme}>{theme}</b>;
}
function Middle() {
  return <div><Label /><Theme.Provider value="nested"><Label /></Theme.Provider></div>;
}
function App() {
  const [dark, setDark] = useState(false);
  return (
    <div>
      <button id="flip" onClick={() => setDark(!dark)}>flip</button>
      <Label />
      <Theme.Provider value={dark ? 'dark' : 'light'}><Middle /></Theme.Provider>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#flip"), Step::Click("#flip")],
    );
}

#[test]
fn reducers_memos_refs_and_timers() {
    same_as_react(
        r#"
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
type Action = { type: 'add'; by: number } | { type: 'reset' };
function reducer(state: number, action: Action): number {
  switch (action.type) {
    case 'add': return state + action.by;
    case 'reset': return 0;
    default: return state;
  }
}
function App() {
  const [count, dispatch] = useReducer(reducer, 0);
  const [ticks, setTicks] = useState(0);
  const renders = useRef(0);
  renders.current += 1;
  const doubled = useMemo(() => { console.log('memo', count); return count * 2; }, [count]);
  const add = useCallback(() => dispatch({ type: 'add', by: 3 }), []);
  useEffect(() => {
    const id = setInterval(() => setTicks((t) => t + 1), 100);
    return () => clearInterval(id);
  }, []);
  useEffect(() => { if (ticks === 3) console.log('three ticks'); }, [ticks]);
  return (
    <div>
      <button id="add" onClick={add}>add</button>
      <button id="reset" onClick={() => dispatch({ type: 'reset' })}>reset</button>
      <p>{count} {doubled} {ticks}</p>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#add"),
            Step::Click("#add"),
            Step::Wait(350),
            Step::Click("#reset"),
        ],
    );
}

#[test]
fn forms_submit_through_react_handlers() {
    same_as_react(
        r#"
import { useState } from 'react';
import type { FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
function App() {
  const [items, setItems] = useState<string[]>([]);
  const [draft, setDraft] = useState('');
  const [checked, setChecked] = useState(false);
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!draft.trim()) return;
    setItems([...items, draft.trim()]);
    setDraft('');
  };
  return (
    <form onSubmit={submit}>
      <input id="draft" value={draft} onChange={(e) => setDraft(e.target.value)} />
      <input id="check" type="checkbox" checked={checked} onChange={(e) => setChecked(e.target.checked)} />
      <button id="add" type="submit">add</button>
      <ol>{items.map((it, i) => <li key={i}>{it}{checked ? '!' : ''}</li>)}</ol>
    </form>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#draft"),
            Step::Type("one"),
            Step::Key("Enter"),
            Step::Type("  two "),
            Step::Click("#add"),
            Step::Click("#check"),
            Step::Click("#draft"),
            Step::Key("Enter"),
        ],
    );
}

#[test]
fn styles_classes_and_attributes_follow_react_dom() {
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
function App() {
  const [on, setOn] = useState(false);
  const style = on ? { width: 120, opacity: 0.5, backgroundColor: 'red', zIndex: 3 } : { width: '50%', marginTop: 4 };
  return (
    <div>
      <button id="go" onClick={() => setOn(!on)}>go</button>
      <div id="box" className={on ? 'on big' : undefined} style={style} data-on={on} aria-hidden={on} hidden={!on} tabIndex={on ? 0 : -1} title={on ? 'yes' : undefined}>
        {on ? 'on' : null}{0}{''}{false}{[1, 'two', null]}
      </div>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn snapshot_and_restore_continue_where_they_left_off() {
    let (module, _) = compile(
        r#"
import { useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
function App() {
  const [items, setItems] = useState<number[]>([1, 2]);
  const clicks = useRef(0);
  const add = () => { clicks.current += 1; setItems([...items, items.length + 1]); };
  return <div><button id="add" onClick={add}>add</button><span id="n">{items.join(',')}</span><i id="c">{clicks.current}</i></div>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
    );
    for generated in [false, true] {
        let mut app = app_on(&module, generated);
        app.boot();
        let click = |app: &mut UiApp| {
            let n = app.query_selector("#add").unwrap();
            let (x, y) = app.centre_of(n).unwrap();
            app.dispatch(UiEvent::Click {
                x,
                y,
                button: 0,
                modifiers: Modifiers::default(),
                detail: 1,
            });
        };
        click(&mut app);
        let state = app.snapshot();
        let json = state.to_json();
        let back = cw_ui::UiState::from_json(&json).unwrap();
        assert_eq!(back, state);
        let mut restored = UiApp::restore(&back, Box::new(vendor(MemoryHost::new()))).unwrap();
        click(&mut app);
        click(&mut restored);
        let text = |a: &UiApp| {
            a.document()
                .text_content(a.query_selector("#root").unwrap())
        };
        assert_eq!(text(&app), text(&restored));
        assert_eq!(text(&restored), "add1,2,3,42");
        assert_eq!(app.snapshot().to_json(), restored.snapshot().to_json());
        assert_eq!(restored.is_generated(), generated);
    }
}

#[test]
fn svg_spread_props_ids_and_autofocus() {
    same_as_react(
        r#"
import { useId, useState } from 'react';
import { createRoot } from 'react-dom/client';
function Field({ label }: { label: string }) {
  const id = useId();
  return <p><label htmlFor={id}>{label}</label><input id={id} autoFocus={label === 'b'} /></p>;
}
function App() {
  const [n, setN] = useState(1);
  const extra = { title: 'spread', 'data-n': n, className: 'x' + n };
  return (
    <div>
      <button id="go" {...extra} onClick={() => setN(n + 1)}>go</button>
      <svg width="40" height="20" viewBox="0 0 40 20"><rect x={n} y="2" width="10" height="10" strokeWidth={2} fill="red" /></svg>
      <Field label="a" /><Field label="b" />
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Type("typed")],
    );
}

#[test]
fn capture_focus_and_pointer_handlers_fire_in_order() {
    same_as_react(
        r#"
import { createRoot } from 'react-dom/client';
function App() {
  return (
    <div id="outer" onClickCapture={() => console.log('outer capture')} onClick={() => console.log('outer bubble')}
         onFocus={() => console.log('focus within')} onBlur={() => console.log('blur within')}>
      <button id="inner" onClickCapture={() => console.log('inner capture')}
              onClick={(e) => { console.log('inner bubble', e.target === e.currentTarget); }}
              onMouseDown={() => console.log('down')} onMouseUp={() => console.log('up')}
              onMouseEnter={() => console.log('enter')}>inner</button>
      <button id="stop" onClick={(e) => { e.stopPropagation(); console.log('stopped'); }}>stop</button>
      <input id="field" onKeyDown={(e) => console.log('key', e.key)} onFocus={() => console.log('field focus')} />
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#inner"),
            Step::Click("#stop"),
            Step::Click("#field"),
            Step::Type("ab"),
            Step::Click("#inner"),
        ],
    );
}

#[test]
fn controlled_selects_and_checkboxes() {
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
function App() {
  const [size, setSize] = useState('m');
  const [on, setOn] = useState(true);
  const [stuck] = useState(false);
  return (
    <div>
      <select id="size" value={size} onChange={(e) => setSize(e.target.value)}>
        <option value="s">small</option><option value="m">medium</option><option value="l">large</option>
      </select>
      <input id="on" type="checkbox" checked={on} onChange={(e) => setOn(e.target.checked)} />
      <input id="stuck" type="checkbox" checked={stuck} onChange={() => console.log('stuck clicked')} />
      <button id="big" onClick={() => setSize('l')}>big</button>
      <p>{size} {on ? 'on' : 'off'}</p>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#on"),
            Step::Click("#stuck"),
            Step::Click("#big"),
            Step::Click("#on"),
        ],
    );
}

#[test]
fn fetched_data_renders() {
    same_as_react(
        r#"
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
interface Item { id: number; name: string; tags: string[] }
function App() {
  const [items, setItems] = useState<Item[]>([]);
  const [status, setStatus] = useState('loading');
  useEffect(() => {
    fetch('/api/items')
      .then((r) => { console.log('status', r.status, r.ok); return r.json(); })
      .then((data: Item[]) => { setItems(data); setStatus('done'); })
      .catch((e: string) => setStatus('failed ' + e));
    fetch('/api/missing').then((r) => console.log('missing', r.status));
  }, []);
  const sorted = [...items].sort((a, b) => a.name.localeCompare(b.name));
  return <div><p>{status}</p><ul>{sorted.map((it) => <li key={it.id}>{it.name} ({it.tags.length})</li>)}</ul></div>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Wait(50)],
    );
}

#[test]
fn sets_maps_regexes_and_type_level_code() {
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
const TABS = [{ id: 'a', label: 'Alpha' }, { id: 'b', label: 'Beta' }] as const;
type TabId = (typeof TABS)[number]['id'];
interface Profile { name: string; email: string }
type Errors = Partial<Record<keyof Profile, string>>;
function check(p: Profile): Errors {
  const e: Errors = {};
  if (!/^[a-z ]+$/i.test(p.name)) e.name = 'letters only';
  if (!/^[^\s@]+@[^\s@]+$/.test(p.email)) e.email = 'bad email';
  return e;
}
function App() {
  const [picked, setPicked] = useState<Set<number>>(new Set([2]));
  const [tab, setTab] = useState<TabId>('a');
  const [counts] = useState(() => new Map<string, number>([['x', 1], ['y', 2]]));
  const toggle = (n: number) => setPicked((s) => { const next = new Set(s); if (next.has(n)) next.delete(n); else next.add(n); return next; });
  function update<K extends keyof Profile>(key: K, value: Profile[K]): Profile { return { name: 'Ann', email: 'a@b', [key]: value } as Profile; }
  const errors = check(update('email', 'nope'));
  const words = 'one, two;three'.split(/[,;]\s*/);
  const shout = 'a-b-c'.replace(/-/g, (m) => m + m);
  return (
    <div>
      {[1, 2, 3].map((n) => <button key={n} id={'n' + n} className={picked.has(n) ? 'on' : 'off'} onClick={() => toggle(n)}>{n}</button>)}
      {TABS.map((t) => <a key={t.id} id={'tab-' + t.id} className={tab === t.id ? 'active' : ''} onClick={() => setTab(t.id)}>{t.label}</a>)}
      <p>{picked.size} {[...picked].join('+')} {counts.get('y')} {Array.from(counts.keys()).join('')}</p>
      <p>{errors.email ?? 'ok'} {errors.name ?? 'ok'} {words.join('|')} {shout} {'x1y22'.match(/\d+/g)?.join(',')}</p>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#n1"),
            Step::Click("#n2"),
            Step::Click("#tab-b"),
        ],
    );
}

#[test]
fn captured_variables_are_shared_and_errors_are_caught() {
    same_as_react(
        r#"
import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
function counter() {
  let n = 0;
  return { next: () => { n += 1; return n; }, peek: () => n };
}
function parse(text: string): number {
  const v = Number(text);
  if (Number.isNaN(v)) throw new Error('not a number: ' + text);
  return v;
}
function App() {
  const [log, setLog] = useState<string[]>([]);
  const c = useRef(counter());
  useEffect(() => {
    let cancelled = false;
    const id = setTimeout(() => { if (!cancelled) setLog((l) => [...l, 'timer fired']); }, 50);
    return () => { cancelled = true; clearTimeout(id); console.log('cleanup saw', cancelled); };
  }, []);
  const tryIt = (text: string) => {
    const out: string[] = [];
    try {
      out.push('parsed ' + parse(text));
    } catch (e) {
      out.push('caught ' + String(e) + ' / ' + (e as Error).message);
    } finally {
      out.push('finally ' + c.current.next());
    }
    setLog((l) => [...l, ...out]);
  };
  return (
    <div>
      <button id="good" onClick={() => tryIt('42')}>good</button>
      <button id="bad" onClick={() => tryIt('4x2')}>bad</button>
      <ul>{log.map((l, i) => <li key={i}>{l}</li>)}</ul>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#good"),
            Step::Click("#bad"),
            Step::Wait(100),
            Step::Click("#good"),
        ],
    );
}

#[test]
fn async_functions_await_fetches_timers_and_each_other() {
    same_as_react(
        r#"
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
interface User { id: number; name: string }
const delay = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
async function getUser(id: number): Promise<User> {
  const res = await fetch('/api/user/' + id);
  if (!res.ok) throw new Error('HTTP ' + res.status);
  const user: User = await res.json();
  return user;
}
function App() {
  const [users, setUsers] = useState<User[]>([]);
  const [status, setStatus] = useState('idle');
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    let cancelled = false;
    async function load() {
      setLoading(true);
      try {
        const both = await Promise.all([getUser(1), getUser(2)]);
        await delay(30);
        if (!cancelled) setUsers(both);
        const missing = await getUser(3);
        console.log('never', missing.name);
      } catch (e) {
        setStatus('failed: ' + (e as Error).message);
      } finally {
        setLoading(false);
      }
    }
    load();
    return () => { cancelled = true; };
  }, []);
  async function sequence() {
    const names: string[] = [];
    for (const id of [2, 1]) {
      const u = await getUser(id);
      names.push(u.name);
      if (names.length > 5) break;
    }
    let i = 0;
    while (i < 2) {
      await delay(10);
      i++;
    }
    try {
      const r = await fetch('/api/broken');
      await r.json();
    } catch (e) {
      names.push('bad json');
    }
    setStatus(names.join(',') + ' after ' + i);
  }
  return (
    <div>
      <button id="seq" onClick={() => { sequence(); }}>seq</button>
      <p>{loading ? 'loading' : 'done'} {status}</p>
      <ul>{users.map((u) => <li key={u.id}>{u.name}</li>)}</ul>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Wait(100), Step::Click("#seq"), Step::Wait(100)],
    );
}

#[test]
fn window_listeners_external_stores_and_dom_globals() {
    same_as_react(
        r#"
import { useEffect, useState, useSyncExternalStore } from 'react';
import { createRoot } from 'react-dom/client';
let count = 0;
const subscribers = new Set<() => void>();
const store = {
  subscribe(cb: () => void) {
    subscribers.add(cb);
    console.log('subscribe', subscribers.size);
    return () => { subscribers.delete(cb); console.log('unsubscribe', subscribers.size); };
  },
  get() { return count; },
  bump() { count++; subscribers.forEach((s) => s()); },
};
function Counter({ label }: { label: string }) {
  const n = useSyncExternalStore(store.subscribe, store.get);
  console.log('render', label, n);
  return <span className="count">{label}={n}</span>;
}
function App() {
  const [keys, setKeys] = useState<string[]>([]);
  const [shown, setShown] = useState(true);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      console.log('window key', e.key, document.activeElement === document.body);
      if (e.key === 'b') store.bump();
      else setKeys((k) => [...k, e.key]);
    };
    const onCapture = (e: KeyboardEvent) => console.log('document capture', e.key);
    window.addEventListener('keydown', onKey);
    document.addEventListener('keydown', onCapture, true);
    document.addEventListener('keydown', onCapture, { capture: true });
    return () => {
      window.removeEventListener('keydown', onKey);
      document.removeEventListener('keydown', onCapture, true);
      console.log('removed');
    };
  }, []);
  useEffect(() => {
    const el = document.getElementById('status');
    const first = document.querySelector('ul > li:last-child');
    console.log('effect', el ? el.textContent : 'none', first ? first.textContent : 'no items', Object.is(NaN, NaN), Object.is(0, -0));
    console.log('size', window.innerWidth > 0, window.innerHeight > 0, 'a😀'.codePointAt(1), 'x'.codePointAt(3));
  });
  function fail() {
    try {
      throw new TypeError('bad');
    } catch (e) {
      console.log('caught', e instanceof Error, e instanceof TypeError, e instanceof RangeError, e instanceof Error ? e.message : String(e));
    }
    console.log('plain', ('s' as unknown) instanceof Error);
    setShown((s) => !s);
  }
  return (
    <div>
      <p id="status">{keys.join(',')}</p>
      <ul>{keys.map((k, i) => <li key={i}>{k}</li>)}</ul>
      {shown ? <Counter label="a" /> : null}
      <Counter label="b" />
      <button id="toggle" onClick={fail}>toggle</button>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Key("x"),
            Step::Key("b"),
            Step::Click("#toggle"),
            Step::Key("b"),
            Step::Key("y"),
            Step::Click("#toggle"),
            Step::Key("b"),
        ],
    );
}

#[test]
fn listeners_and_store_subscriptions_survive_restore() {
    let (module, _) = compile(
        r#"
import { useEffect, useState, useSyncExternalStore } from 'react';
import { createRoot } from 'react-dom/client';
let count = 0;
const subs = new Set<() => void>();
const subscribe = (cb: () => void) => { subs.add(cb); return () => { subs.delete(cb); }; };
const get = () => count;
function App() {
  const n = useSyncExternalStore(subscribe, get);
  const [keys, setKeys] = useState('');
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === '+') { count++; subs.forEach((s) => s()); } else setKeys((k) => k + e.key);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
  return <p id="out">{n}:{keys}:{subs.size}</p>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
    );
    for generated in [false, true] {
        let mut app = app_on(&module, generated);
        app.boot();
        let key = |app: &mut UiApp, k: &str| {
            app.dispatch(UiEvent::Key {
                key: k.into(),
                code: String::new(),
                modifiers: Modifiers::default(),
                repeat: false,
            });
            app.run_until_idle(20);
        };
        key(&mut app, "a");
        key(&mut app, "+");
        let state = cw_ui::UiState::from_json(&app.snapshot().to_json()).unwrap();
        let mut restored = UiApp::restore(&state, Box::new(vendor(MemoryHost::new()))).unwrap();
        for a in [&mut app, &mut restored] {
            key(a, "b");
            key(a, "+");
        }
        let text = |a: &UiApp| a.document().text_content(a.query_selector("#out").unwrap());
        assert_eq!(text(&app), "2:ab:1");
        assert_eq!(text(&restored), "2:ab:1");
        assert_eq!(app.snapshot().to_json(), restored.snapshot().to_json());
        assert_eq!(restored.is_generated(), generated);
    }
}

#[test]
fn generics_selection_ranges_and_focus_leaving_with_its_element() {
    same_as_react(
        r#"
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
interface Box<T> { value: T; label: string; map<U>(f: (v: T) => U): U }
type Pair<A, B = string> = { first: A; second: B };
function box<T>(value: T, label: string): Box<T> {
  return { value, label, map: <U,>(f: (v: T) => U) => f(value) };
}
function firstOf<T>(items: T[], fallback: T): T {
  return items.length > 0 ? items[0] : fallback;
}
function pair<A>(first: A): Pair<A> {
  return { first, second: 'two' };
}
function App() {
  const [editing, setEditing] = useState(true);
  const [text, setText] = useState('hello world');
  const field = useRef<HTMLInputElement | null>(null);
  const numbers = box<number[]>([3, 1, 2], 'nums');
  const word = firstOf(['alpha', 'beta'], 'none');
  const p = pair(42);
  useLayoutEffect(() => {
    const el = field.current;
    if (el) {
      el.focus();
      el.setSelectionRange(2, 5);
      console.log('selected', el.selectionStart, el.selectionEnd);
    }
  }, [editing]);
  useEffect(() => {
    console.log('active is body', document.activeElement === document.body, editing);
  });
  return (
    <div>
      <p id="info">{numbers.label}:{numbers.value.join('+')}={numbers.map((v) => v.reduce((a, b) => a + b, 0))} {word.toUpperCase()} {p.first + 1} {p.second.length}</p>
      {editing ? <input id="field" ref={field} value={text} onChange={(e) => setText(e.target.value)} /> : <span>{text}</span>}
      <button id="toggle" onMouseDown={(e) => e.preventDefault()} onClick={() => setEditing((v) => !v)}>toggle</button>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Type("X"),
            Step::Click("#toggle"),
            Step::Type("Y"),
            Step::Click("#toggle"),
            Step::Type("Z"),
        ],
    );
}

#[test]
fn responses_have_status_headers_text_and_typed_json() {
    same_as_react(
        r#"
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
interface Item { id: number; name: string; tags: string[] }
function App() {
  const [out, setOut] = useState<string[]>([]);
  useEffect(() => {
    async function run() {
      const lines: string[] = [];
      const r = await fetch('/api/items');
      lines.push(r.ok + ' ' + r.status + ' ' + r.statusText.length + ' ' + (r.url.length > 0));
      lines.push(String(r.headers.get('Content-Type')) + ' ' + r.headers.has('content-type') + ' ' + r.headers.get('x-missing'));
      const items = await r.json<Item[]>();
      lines.push(items.map((i) => i.name + i.tags.length).join(','));
      const again = await fetch('/api/user/1');
      const text = await again.text();
      lines.push(text.length > 0 ? 'text ' + text.includes('Ada') : 'empty');
      const missing = await fetch('/api/nothing-here');
      lines.push('missing ' + missing.ok + ' ' + missing.status);
      setOut(lines);
    }
    run();
  }, []);
  return <ul>{out.map((l, i) => <li key={i}>{l}</li>)}</ul>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Wait(50)],
    );
}

#[test]
fn untyped_and_any_typed_code_runs_as_javascript_does() {
    // Types are hints: `any`, unannotated parameters, unions the compiler does not
    // narrow and types it does not model all resolve when the code runs.
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
type Draft = Record<string, string> | {};
function label(item) {
  return item.name.trim().toUpperCase() + ':' + item.tags.map((t) => t.toLowerCase()).join('|');
}
function total(rows: any) {
  return rows.filter((r) => r.n > 1).reduce((a, r) => a + r.n, 0).toFixed(1);
}
const api: any = {
  count: 0,
  bump(by) { return by * 2; },
};
function App() {
  const [n, setN] = useState(0);
  const [draft, setDraft] = useState<Draft>({});
  const data: any = JSON.parse('[{"name":" Ada ","tags":["X","Y"],"n":1},{"name":"bo","tags":[],"n":2.5}]');
  const errs: Draft = n > 1 ? { to: 'required' } : {};
  const owned = (errs as any).hasOwnProperty('to');
  const called = api.bump.call(null, n) + api.bump.apply(null, [n + 1]);
  console.log('render', n, data.length, label(data[0]), total(data), owned, called);
  return (
    <div>
      <p id="out">{data.map((d) => label(d)).join(' / ')} {(errs as Record<string, string>).to ?? 'ok'} {draft.subject ?? '-'}</p>
      <button id="go" onClick={() => { setN(n + 1); setDraft({ subject: 'S' + n }); }}>more {String(n).padStart(3, '0')}</button>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn hover_follows_content_that_moves_under_a_still_pointer() {
    // A click that inserts content above the button moves the button out from
    // under the pointer: hover leaves it, with the boundary events and no moves,
    // as Chromium and the Realm do after a layout.
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
function App() {
  const [shown, setShown] = useState(false);
  return (
    <div>
      {shown && <p id="msg" style={{ height: 80 }}>An error appeared</p>}
      <button id="go" onMouseEnter={() => console.log('enter go')} onMouseLeave={() => console.log('leave go')} onMouseMove={() => console.log('move go')} onClick={() => setShown(true)}>show</button>
      <div id="below" onMouseEnter={() => console.log('enter below')} style={{ height: 200 }}>below</div>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Wait(20)],
    );
}

#[test]
fn bindings_hoist_as_javascript_hoists_them() {
    // A closure may use a `const` declared below it (recursion included), a
    // function declaration is callable from the top of its block, and each loop
    // iteration's bindings are its own.
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
interface Node { name: string; kids?: Node[] }
function App() {
  const [n, setN] = useState(1);
  const total = sum(n, 2, 3);
  const walk = (node: Node, depth: number): string =>
    node.name + depth + (node.kids ?? []).map((k) => walk(k, depth + 1)).join('');
  const later = () => label + '!';
  const label = 'L' + n;
  const fns: (() => number)[] = [];
  for (let i = 0; i < 3; i++) {
    const k = i * n;
    fns.push(() => k + i);
  }
  function sum(...xs: number[]) {
    return xs.reduce((a, b) => a + b, 0) + offset();
  }
  function offset() { return 100; }
  const tree: Node = { name: 'a', kids: [{ name: 'b', kids: [{ name: 'c' }] }, { name: 'd' }] };
  console.log('render', total, walk(tree, 0), later(), fns.map((f) => f()).join(','));
  return <button id="go" onClick={() => setN(n + 1)}>{total} {later()}</button>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn render_props_rest_parameters_and_builtins_as_values() {
    same_as_react(
        r#"
import { useState, type ReactNode } from 'react';
import { createRoot } from 'react-dom/client';
function Toggle({ children }: { children: (on: boolean, flip: () => void) => ReactNode }) {
  const [on, setOn] = useState(false);
  return <div className="toggle">{children(on, () => setOn(!on))}</div>;
}
function join(sep: string, ...parts: (string | number)[]) {
  return parts.filter(Boolean).map(String).join(sep);
}
function App() {
  const nums = ['3', '10', 'x', '7'].map(Number).filter((v) => !Number.isNaN(v));
  const max = nums.reduce((a, b) => Math.max(a, b), 0);
  const parsed = ['1', '2', '3'].map(parseInt);
  const blanks = Array(3).fill('-').join('') + new Array(2, 4).join('/') + Array.from({ length: 2 }, (_, i) => i).join('');
  console.log('values', nums.join(','), max, parsed.join(','), blanks, [0, 1, '', 'a', null].filter(Boolean).length, [1.4, 2.6].map(Math.round).join(','));
  return (
    <Toggle>
      {(on, flip) => <button id="go" onClick={flip}>{on ? 'on' : 'off'} {join('-', 'a', 0, 'b', '', 2)}</button>}
    </Toggle>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn modules_re_export_import_cycles_and_run_code_when_they_load() {
    // Re-exports and `export *`, `import * as`, an anonymous default export, an
    // enum, module-level statements (a loop, a guard that throws, a log) that run
    // in order when the module loads, and an import cycle (the library uses the
    // app's constant when called, after both modules loaded).
    same_as_react(
        r#"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import App, { total } from './App';
import * as util from './util';
const rootElement = document.getElementById('root')!;
if (!rootElement) throw new Error('no root element');
console.log('boot', util.twice(2), total, util.Color.Green, util.Color[6], util.Color.Blue);
const root = createRoot(rootElement);
root.render(<App />);
// @file App.tsx
import { useState } from 'react';
import { twice } from './util';
import { label, isEven, shout } from './lib';
export const total = [1, 2, 3].reduce((a, b) => a + b, 0);
let calls = 0;
for (let i = 0; i < 4; i++) {
  calls += i;
}
console.log('App module loaded', calls, isEven(calls));
export default function (): JSX.Element {
  const [n, setN] = useState(calls);
  return <button id="go" onClick={() => setN(twice(n) + 1)}>{label(n)} {shout('x')}</button>;
}
// @file util.ts
export enum Color { Red, Green = 5, Blue }
export const twice = (n: number) => n * 2;
// @file lib/index.ts
export * from './impl';
export { yell as shout } from './impl';
// @file lib/impl.ts
import { total } from '../App';
export function label(n: number) { return 'n=' + n + ' ' + (isEven(n) ? 'even' : 'odd') + ' of ' + total; }
export const isEven = (n: number) => n % 2 === 0;
export const yell = (s: string) => s.toUpperCase() + '!';
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn dom_refs_measure_query_and_scroll_as_the_page_does() {
    // Geometry reads flush layout and give the Realm's numbers; element queries,
    // traversal and scrolling act on the engine's document.
    same_as_react(
        r#"
import { useLayoutEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
function App() {
  const box = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const [out, setOut] = useState('');
  const [tall, setTall] = useState(false);
  useLayoutEffect(() => {
    const el = box.current!;
    const r = el.getBoundingClientRect();
    const ul = list.current!;
    const items = ul.querySelectorAll('li.item');
    const second = ul.querySelector('li:nth-child(2)')!;
    console.log('rect', r.x, r.y, r.width, r.height, r.top, r.right, r.bottom, r.left);
    console.log('offset', el.offsetLeft, el.offsetTop, el.offsetWidth, el.offsetHeight, el.clientWidth, el.clientHeight, el.scrollHeight);
    console.log('query', items.length, second.textContent, second.closest('ul') === ul, ul.contains(second), second.matches('.item'), second.getAttribute('data-k'), second.hasAttribute('title'));
    console.log('tree', ul.children.length, ul.firstElementChild!.textContent, second.nextElementSibling!.textContent, second.parentElement === ul, document.querySelectorAll('li').length);
    setOut(`${Math.round(r.width)}x${Math.round(r.height)}`);
  }, [tall]);
  const scroll = () => {
    const el = box.current!;
    el.scrollTop = 30;
    const before = el.scrollTop;
    el.scrollBy(0, 15);
    list.current!.lastElementChild!.scrollIntoView();
    window.scrollTo(0, 40);
    console.log('scrolled', before, el.scrollTop, window.scrollY, document.documentElement.scrollTop);
    setTall(!tall);
  };
  return (
    <div>
      <div id="box" ref={box} style={{ width: 150, height: 60, overflow: 'auto', border: '3px solid black', padding: 4 }}>
        <div style={{ height: tall ? 400 : 200 }}>content</div>
      </div>
      <ul ref={list}>
        <li className="item" data-k="a">one</li>
        <li className="item" data-k="b">two</li>
        <li>three</li>
      </ul>
      <p id="out">{out}</p>
      <button id="go" onClick={scroll}>scroll</button>
      <div style={{ height: 1500 }}>spacer</div>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn dates_compute_and_print_as_the_page_does() {
    // `Date` on the world clock, the local time zone being the VM's (UTC).
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
const DAY = 24 * 60 * 60 * 1000;
function addDays(d: Date, n: number) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
}
function isoWeek(d: Date) {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = t.getUTCDay() || 7;
  t.setUTCDate(t.getUTCDate() + 4 - day);
  const yearStart = Date.UTC(t.getUTCFullYear(), 0, 1);
  return Math.ceil(((t.getTime() - yearStart) / DAY + 1) / 7);
}
function App() {
  const [anchor, setAnchor] = useState(new Date(2026, 8, 24, 9, 30));
  const end = addDays(anchor, 10);
  const parsed = new Date('2026-02-28T12:00:00Z');
  const copy = new Date(anchor);
  copy.setHours(23, 59);
  console.log(anchor.toISOString(), end.toDateString(), isoWeek(anchor), end > anchor, end.getTime() - anchor.getTime(),
    parsed.getMonth(), parsed.getDay(), copy.toString(), JSON.stringify({ at: anchor }), anchor instanceof Date,
    new Date(NaN).getTime(), String(new Date(0)), Date.parse('2026-01-02'), new Date(2026, 0, 31).getDate(),
    new Date(99, 1, 1).getFullYear(), anchor.valueOf() === +anchor, new Date(anchor.getTime() + DAY).getDate(),
    typeof Date.now(), new Date(0).toUTCString(), anchor.getTimezoneOffset());
  return (
    <div>
      <p id="out">{anchor.toDateString()} → {end.getMonth() + 1}/{end.getDate()} week {isoWeek(end)}</p>
      <button id="go" onClick={() => setAnchor((a) => addDays(a, 40))}>next</button>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn forward_refs_imperative_handles_and_component_namespaces() {
    same_as_react(
        r#"
import { forwardRef, memo, useEffect, useImperativeHandle, useRef, useState, useTransition, useDeferredValue, startTransition, Suspense } from 'react';
import { createRoot } from 'react-dom/client';
interface FieldHandle { focus: () => void; clear: () => void }
const Field = forwardRef<FieldHandle, { label: string }>((props, ref) => {
  const input = useRef<HTMLInputElement>(null);
  const [v, setV] = useState('');
  useImperativeHandle(ref, () => ({
    focus: () => input.current!.focus(),
    clear: () => setV(''),
  }), []);
  // Render counts differ with a transition or a deferred value (React renders
  // again for them; compiled, they are synchronous), so log once.
  useEffect(() => console.log('field mounted', props.label, 'ref' in props), []);
  return <label>{props.label}<input id="field" ref={input} value={v} onChange={(e) => setV(e.target.value)} /></label>;
});
const Plain = memo(forwardRef<HTMLButtonElement, { children: string; onClick: () => void }>((p, ref) => <button ref={ref} id="plain" onClick={p.onClick}>{p.children}</button>));
const Tabs = {
  List: ({ children }: { children: React.ReactNode }) => <ul className="tabs">{children}</ul>,
  Tab: ({ label }: { label: string }) => <li>{label}</li>,
};
function App() {
  const field = useRef<FieldHandle>(null);
  const button = useRef<HTMLButtonElement>(null);
  const [count, setCount] = useState(0);
  const [pending, start] = useTransition();
  const deferred = useDeferredValue(count);
  const Wrapped = memo(Tabs.Tab);
  return (
    <div>
      <Field ref={field} label="Name" />
      <Plain ref={button} onClick={() => { field.current!.focus(); start(() => setCount((c) => c + 1)); console.log('button is', button.current!.id, pending); }}>focus</Plain>
      <button id="clear" onClick={() => { field.current!.clear(); startTransition(() => setCount(0)); }}>clear</button>
      <Suspense fallback={<p>loading</p>}>
        <Tabs.List><Tabs.Tab label={`count ${count}`} /><Wrapped label={`deferred ${deferred}`} /></Tabs.List>
      </Suspense>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#plain"),
            Step::Type("Ada"),
            Step::Click("#plain"),
            Step::Click("#clear"),
        ],
    );
}

#[test]
fn logical_and_destructuring_assignments_delete_and_host_globals() {
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
interface Cfg { name?: string; count?: number; tags?: string[] | null; extra?: string }
function App() {
  const [out, setOut] = useState('');
  const run = () => {
    const cfg: Cfg = { count: 0, tags: null, extra: 'x' };
    cfg.name ??= 'anon';
    cfg.count ||= 5;
    cfg.tags ??= [];
    cfg.tags.push('t');
    let flag = true;
    flag &&= cfg.count > 3;
    delete cfg.extra;
    let a = 1, b = 2;
    [a, b] = [b, a];
    const pair: [number, number] = [3, 4];
    let first = 0, rest: number[] = [];
    [first, ...rest] = [9, 8, 7];
    let n: string | undefined, c: number;
    ({ name: n, count: c = 1 } = cfg);
    localStorage.setItem('k', 'v' + a);
    window.localStorage.setItem('other', '1');
    const stored = localStorage.getItem('k');
    const len = localStorage.length;
    localStorage.removeItem('other');
    const missing = sessionStorage.getItem('nope');
    alert('saved ' + stored);
    const ok = confirm('sure?');
    const line = [JSON.stringify(cfg), 'extra' in cfg, flag, a, b, pair[0], first, rest.join('+'), n, c, stored, len, localStorage.length, missing, ok,
      location.pathname, location.origin, location.hash === '', navigator.language, navigator.userAgent.includes('Chrome')].join(' ');
    console.log(line);
    setOut(line);
  };
  return <div><button id="go" onClick={run}>run</button><p>{out}</p></div>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go")],
    );
}

#[test]
fn packages_run_on_the_island_beside_compiled_code() {
    // clsx and zustand (real npm packages, tests/islands/packages) run on the app's
    // island; the compiled components call them, pass them closures, and render
    // with zustand's hook, which is cw-ui's useSyncExternalStore.
    same_as_react(
        r#"
// @file main.tsx
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import clsx from 'clsx';
import { create } from 'zustand';
interface Counter { count: number; label: string; inc: () => void; reset: () => void }
const useCounter = create<Counter>((set) => ({
  count: 0,
  label: 'none',
  inc: () => set((s) => ({ count: s.count + 1, label: 'n' + (s.count + 1) })),
  reset: () => set({ count: 0, label: 'reset' }),
}));
function Badge({ n }: { n: number }) {
  const count = useCounter((s) => s.count);
  return <b className={clsx('badge', { hot: count > 1, cold: count === 0 }, n > 1 && 'big')}>{n}:{count}</b>;
}
function App() {
  const count = useCounter((s) => s.count);
  const label = useCounter((s) => s.label);
  const inc = useCounter((s) => s.inc);
  const [n, setN] = useState(1);
  console.log('render', count, label, clsx(['a', null, 'b'], { c: n > 1 }), typeof inc);
  return (
    <div>
      <button id="go" onClick={() => { inc(); setN(n + 1); }}>go</button>
      <button id="reset" onClick={() => useCounter.getState().reset()}>reset</button>
      <Badge n={n} />
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#go"),
            Step::Click("#go"),
            Step::Click("#reset"),
        ],
    );
}

#[test]
fn package_components_render_host_elements_and_take_events_refs_and_context() {
    // island-kit (tests/islands/packages) renders host elements with its own
    // state, effects and handlers; compiled code gives it children, callbacks,
    // a ref through forwardRef, and a context value, and renders its render props.
    same_as_react(
        r#"
// @file main.tsx
import { useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Card, Counter, Emphasize, FancyInput, ThemeProvider, useTheme } from 'island-kit';
function Themed() {
  const theme = useTheme();
  return <i className="themed">{theme}</i>;
}
function App() {
  const [theme, setTheme] = useState('light');
  const [log, setLog] = useState<string[]>([]);
  const input = useRef<HTMLInputElement>(null);
  return (
    <ThemeProvider theme={theme}>
      <button id="theme" onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')}>theme</button>
      <button id="focus" onClick={() => { input.current?.focus(); setLog([...log, 'focused ' + (document.activeElement === input.current)]); }}>focus</button>
      <Card title="first" onToggle={(open: boolean, type: string) => setLog([...log, type + ' ' + open])}>
        <p id="inside">{log.join(', ')}</p>
        <Themed />
      </Card>
      <FancyInput id="name" label="Name" ref={input} placeholder="who" />
      <Emphasize>
        <span>one</span>
        {'two'}
        <em>three</em>
      </Emphasize>
      <Counter start={5} render={(n: number, inc: () => void) => <button id="count" onClick={inc}>{n}</button>} />
    </ThemeProvider>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#theme"),
            Step::Click(".toggle"),
            Step::Click("#count"),
            Step::Click(".toggle"),
            Step::Click("#focus"),
            Step::Click("#count"),
        ],
    );
}

#[test]
fn modules_outside_the_subset_run_on_the_island_between_compiled_ones() {
    // fancy.tsx uses a generator, a class with a private field and a labelled
    // loop, none of which cw-tsx compiles: the module runs on the island, reading
    // util.ts's compiled exports, while main.tsx and util.ts are compiled and
    // render its component, call its functions and pass it callbacks.
    let logs = same_as_react(
        r#"
// @file main.tsx
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Fancy, firstPair, summary } from './fancy';
import { calls } from './util';
function App() {
  const [n, setN] = useState(4);
  const [last, setLast] = useState<number[]>([]);
  return (
    <div>
      <button id="more" onClick={() => setN(n + 2)}>more</button>
      <Fancy n={n} onPick={(v) => setLast([...last, v])} />
      <p id="out">{last.join(',')} | {firstPair(last)} | {summary(last)} | {calls > 0 ? 'called' : 'not'}</p>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
// @file util.ts
export const scale = 3;
export let calls = 0;
export function double(n: number): number {
  calls = calls + 1;
  return n * 2;
}
// @file fancy.tsx
import { useState } from 'react';
import { double, scale } from './util';
function* evens(n: number) {
  for (let i = 0; i < n; i++) if (i % 2 === 0) yield double(i) * scale;
}
class Acc {
  #total = 0;
  add(n: number) { this.#total += n; return this; }
  get total() { return this.#total; }
}
let made = 0;
console.log('fancy loads', scale);
export function firstPair(xs: number[]): string {
  outer: for (const a of xs) {
    for (const b of xs) {
      if (a !== b && a + b === 36) { return a + '+' + b; }
      if (b > 100) continue outer;
    }
  }
  return 'none';
}
export const summary = (xs: number[]) => new Acc().add(xs.length).add(made).total;
export function Fancy({ n, onPick }: { n: number; onPick: (v: number) => void }) {
  const [picked, setPicked] = useState(-1);
  made++;
  const items = [...evens(n)];
  return (
    <ul>
      {items.map((v) => (
        <li key={v} className={v === picked ? 'on' : 'off'} onClick={() => { setPicked(v); onPick(v); }}>{v}</li>
      ))}
    </ul>
  );
}
"#,
        &[
            Step::Click("#more"),
            Step::Click("li:nth-child(2)"),
            Step::Click("li:nth-child(3)"),
            Step::Click("#more"),
            Step::Click("li:nth-child(4)"),
        ],
    );
    assert_eq!(logs, vec!["Log: fancy loads 3".to_owned()]);
}

#[test]
fn an_entry_outside_the_subset_renders_compiled_components_from_the_island() {
    // main.tsx keeps its state in a class and renders through React 18's
    // createRoot: it runs on the island, and the compiled App it renders reads
    // and changes that store through the functions it is given.
    same_as_react(
        r#"
// @file main.tsx
import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './app';
class Store {
  #items: string[] = ['a'];
  #subs = new Set<() => void>();
  get items() { return this.#items; }
  add(x: string) { this.#items = [...this.#items, x]; this.#subs.forEach((f) => f()); }
  subscribe(f: () => void) { this.#subs.add(f); return () => { this.#subs.delete(f); }; }
}
const store = new Store();
const container = document.getElementById('root')!;
console.log('container', container.id);
ReactDOM.createRoot(container).render(
  <React.StrictMode>
    <App subscribe={(f) => store.subscribe(f)} items={() => store.items} add={(x) => store.add(x)} />
  </React.StrictMode>,
);
// @file app.tsx
import { useSyncExternalStore } from 'react';
export function App({ subscribe, items, add }: { subscribe: (f: () => void) => () => void; items: () => string[]; add: (x: string) => void }) {
  const list = useSyncExternalStore(subscribe, items);
  return (
    <div>
      <button id="add" onClick={() => add('n' + list.length)}>add</button>
      <ul>{list.map((x) => <li key={x}>{x}</li>)}</ul>
    </div>
  );
}
"#,
        &[Step::Click("#add"), Step::Click("#add")],
    );
}

#[test]
fn island_code_uses_compiled_arrays_and_objects_as_its_own() {
    // list.tsx runs on the island (its generator is outside the subset) and works
    // over data.ts's compiled arrays and objects with the array methods,
    // spreads, destructuring, iteration, JSON and Object's statics.
    same_as_react(
        r#"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import { List } from './list';
createRoot(document.getElementById('root')!).render(<List />);
// @file data.ts
export interface Row { id: string; n: number; tags: string[]; meta: { a: number } }
export const rows: Row[] = [
  { id: 'a', n: 2, tags: ['x'], meta: { a: 1 } },
  { id: 'b', n: 5, tags: [], meta: { a: 2 } },
  { id: 'c', n: 1, tags: ['y', 'z'], meta: { a: 3 } },
];
// @file list.tsx
import { useState } from 'react';
import { rows } from './data';
function* ids() { for (const r of rows) yield r.id; }
export function List() {
  const [items, setItems] = useState(rows);
  const total = items.reduce((n, c) => n + c.n, 0);
  const right = items.reduceRight((s, c) => s + c.id, '');
  const big = items.filter((r) => r.n > 1).map((r) => r.id).join('');
  const found = items.find((r) => r.tags.includes('z'))?.id;
  const idx = items.findIndex((r) => r.id === 'b');
  const some = items.some((r) => r.n > 4) && items.every((r) => r.n > 0);
  const sorted = [...items].sort((a, b) => a.n - b.n).map((r) => r.id).join('');
  const flat = items.flatMap((r) => r.tags).join('');
  const [first, ...rest] = items;
  const { meta: { a } } = first;
  const keys = Object.keys(first).join(',');
  const entries = Object.entries(first.meta).map(([k, v]) => k + v).join('');
  const copy = { ...first, n: 10 };
  const all = [...ids()].join('');
  const json = JSON.stringify(items[2]);
  const slice = items.slice(1).map((r) => r.id).join('') + items.indexOf(items[1]) + items.length;
  return (
    <div>
      <p id="out">{total} {right} {big} {found} {idx} {String(some)} {sorted} {flat} {rest.length} {a} {keys} {entries} {copy.n} {first.n} {all} {slice}</p>
      <p id="json">{json}</p>
      <button id="bump" onClick={() => setItems(items.map((r) => (r.id === 'c' ? { ...r, n: r.n + 10 } : r)))}>bump</button>
      <ul>{items.map((r) => <li key={r.id}>{r.id}:{r.n}</li>)}</ul>
    </div>
  );
}
"#,
        &[Step::Click("#bump")],
    );
}

#[test]
fn island_elements_update_in_place() {
    // Children the island builds as `[header, rows.map(…)]` nest a VM array in a
    // list; a re-render must update those rows, not mount them again (a mount
    // would lose focus and set `checked` as an attribute). A style object from
    // the VM is applied like a compiled one. The callback ref logs whether the
    // button stayed the same node.
    same_as_react(
        r#"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import { List } from './list';
createRoot(document.getElementById('root')!).render(<List />);
// @file list.tsx
import { useState } from 'react';
function* ids() { yield 1; }
let prev: any = null;
const track = (el: any) => { if (el) { console.log('button', el === prev ? 'same' : 'new'); prev = el; } };
export function List() {
  const [on, setOn] = useState<number[]>([]);
  const [w, setW] = useState(10);
  const rows = [1, 2, 3];
  return (
    <div>
      <button id="go" ref={track} onClick={() => setW(w + 5)}>go</button>
      <ul>
        {[
          <li key="head" style={{ width: w + '%', color: 'red' }}>head</li>,
          rows.map((r) => (
            <li key={r}>
              <input type="checkbox" data-row={r} checked={on.includes(r)} onChange={() => setOn(on.includes(r) ? on.filter((x) => x !== r) : [...on, r])} />
            </li>
          )),
        ]}
      </ul>
    </div>
  );
}
"#,
        &[
            Step::Click("input[data-row=\"2\"]"),
            Step::Click("#go"),
            Step::Click("input[data-row=\"3\"]"),
            Step::Click("input[data-row=\"2\"]"),
            Step::Click("#go"),
        ],
    );
}

#[test]
fn intl_locale_methods_and_package_classes_run_on_the_island() {
    // Intl and the toLocale… methods are the island VM's (the jsvm Intl the
    // fallback runs too); `new` of a package's class constructs it there.
    same_as_react(
        r#"
// @file main.tsx
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Tally } from 'island-kit';
const money = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' });
const day = new Intl.DateTimeFormat('en-US', { timeZone: 'UTC', weekday: 'short', month: 'short', day: 'numeric' });
function App() {
  const [n, setN] = useState(1234.5);
  const [t] = useState(() => new Tally(3));
  const when = new Date(Date.UTC(2024, 1, 29, 13, 5));
  return (
    <div>
      <p id="a">{money.format(n)} | {n.toLocaleString()} | {n.toLocaleString('en-US', { maximumFractionDigits: 0 })}</p>
      <p id="b">{day.format(when)} | {when.toLocaleDateString('en-US', { timeZone: 'UTC' })} | {when.toLocaleTimeString('en-US', { timeZone: 'UTC' })}</p>
      <p id="c">{t.total} {t.doubled}</p>
      <button id="go" onClick={() => { t.add(2); setN(n * 3); }}>go</button>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn island_code_reaches_the_page_through_cw_ui() {
    // prefs.tsx runs on the island (a generator); its storage, location, window
    // size and dialogs are cw-ui's own builtins, so they are the page's.
    same_as_react(
        r#"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import { Prefs } from './prefs';
createRoot(document.getElementById('root')!).render(<Prefs />);
// @file prefs.tsx
import { useState } from 'react';
function* keys() { yield 'theme'; }
export function Prefs() {
  const [theme, setTheme] = useState(() => localStorage.getItem('theme') ?? 'light');
  const where = location.pathname + location.search + '|' + window.location.host;
  return (
    <div>
      <p id="out">{theme} {where} {innerWidth > 0 ? 'wide' : 'none'} {window.innerHeight > 0 ? 'tall' : 'none'} {sessionStorage.length}</p>
      <button id="go" onClick={() => {
        const next = theme === 'light' ? 'dark' : 'light';
        if (confirm('switch to ' + next + '?')) {
          localStorage.setItem('theme', next);
          setTheme(localStorage.getItem('theme') + ':' + localStorage.length + ':' + [...keys()].join(''));
        }
      }}>go</button>
    </div>
  );
}
"#,
        &[Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn island_code_fetches_through_the_page() {
    // api.tsx runs on the island (a generator); its fetch is the page's, through
    // cw-ui, and its promises are the VM's own.
    same_as_react(
        r#"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import { Items } from './api';
createRoot(document.getElementById('root')!).render(<Items />);
// @file api.tsx
import { useEffect, useState } from 'react';
function* ids() { yield 1; }
async function load(): Promise<{ id: number; name: string }[]> {
  const r = await fetch('/api/items');
  console.log('status', r.status, r.ok, typeof r.json);
  return r.json();
}
export function Items() {
  const [items, setItems] = useState<{ id: number; name: string }[]>([]);
  const [note, setNote] = useState('loading');
  useEffect(() => {
    load().then((xs) => { setItems(xs); setNote('done ' + xs.length); });
    fetch('/api/missing').then((r) => console.log('missing', r.status, r.ok));
    Promise.all([fetch('/api/items'), Promise.resolve(2)]).then(([r, n]) => console.log('all', r.status, n));
  }, []);
  return <div><p>{note}</p><ul>{items.map((it) => <li key={it.id}>{it.name}</li>)}</ul></div>;
}
"#,
        &[Step::Wait(50)],
    );
}

#[test]
fn hash_and_history_navigation_follow_the_page() {
    // Links to a fragment, `location.hash = …`, pushState/replaceState, back and
    // location.assign, with popstate then hashchange as the Realm fires them.
    same_as_react(
        r##"
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';

function useRoute(): string {
  const [route, setRoute] = useState(location.hash || '#/');
  useEffect(() => {
    const onHash = (e: HashChangeEvent) => { console.log('hashchange', e.newURL.split('#')[1] ?? '', 'from', e.oldURL.split('#')[1] ?? ''); setRoute(location.hash || '#/'); };
    const onPop = (e: PopStateEvent) => { console.log('popstate', JSON.stringify(e.state), location.pathname, location.hash); setRoute(location.hash || '#/'); };
    window.addEventListener('hashchange', onHash);
    window.addEventListener('popstate', onPop);
    return () => { window.removeEventListener('hashchange', onHash); window.removeEventListener('popstate', onPop); };
  }, []);
  return route;
}
export function App() {
  const route = useRoute();
  return (
    <div>
      <p id="route">{route} {history.length} {JSON.stringify(history.state)} {location.pathname}</p>
      <a id="to-b" href="#/b">b</a>
      <button id="to-c" onClick={() => { location.hash = '#/c'; console.log('after set', location.hash, history.length); }}>c</button>
      <button id="push" onClick={() => { history.pushState({ n: history.length }, '', '/app.html?x=1#/d'); console.log('pushed', location.search, location.hash); }}>push</button>
      <button id="replace" onClick={() => history.replaceState({ r: 1 }, '')}>replace</button>
      <button id="back" onClick={() => history.back()}>back</button>
      <button id="assign" onClick={() => location.assign('#/e')}>assign</button>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"##,
        &[
            Step::Click("#to-b"),
            Step::Click("#to-c"),
            Step::Click("#push"),
            Step::Click("#replace"),
            Step::Click("#back"),
            Step::Wait(20),
            Step::Click("#back"),
            Step::Wait(20),
            Step::Click("#assign"),
        ],
    );
}

#[test]
fn hash_and_history_navigation_follow_the_page_on_the_island() {
    // The same router, on the island (a generator puts it there).
    same_as_react(
        r##"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import { App } from './router';
createRoot(document.getElementById('root')!).render(<App />);
// @file router.tsx
import { useEffect, useState } from 'react';
function* g() { yield 1; }

function useRoute(): string {
  const [route, setRoute] = useState(location.hash || '#/');
  useEffect(() => {
    const onHash = (e: HashChangeEvent) => { console.log('hashchange', e.newURL.split('#')[1] ?? '', 'from', e.oldURL.split('#')[1] ?? ''); setRoute(location.hash || '#/'); };
    const onPop = (e: PopStateEvent) => { console.log('popstate', JSON.stringify(e.state), location.pathname, location.hash); setRoute(location.hash || '#/'); };
    window.addEventListener('hashchange', onHash);
    window.addEventListener('popstate', onPop);
    return () => { window.removeEventListener('hashchange', onHash); window.removeEventListener('popstate', onPop); };
  }, []);
  return route;
}
export function App() {
  const route = useRoute();
  return (
    <div>
      <p id="route">{route} {history.length} {JSON.stringify(history.state)} {location.pathname}</p>
      <a id="to-b" href="#/b">b</a>
      <button id="to-c" onClick={() => { location.hash = '#/c'; console.log('after set', location.hash, history.length); }}>c</button>
      <button id="push" onClick={() => { history.pushState({ n: history.length }, '', '/app.html?x=1#/d'); console.log('pushed', location.search, location.hash); }}>push</button>
      <button id="replace" onClick={() => history.replaceState({ r: 1 }, '')}>replace</button>
      <button id="back" onClick={() => history.back()}>back</button>
      <button id="assign" onClick={() => location.assign('#/e')}>assign</button>
    </div>
  );
}
"##,
        &[
            Step::Click("#to-b"),
            Step::Click("#to-c"),
            Step::Click("#push"),
            Step::Click("#replace"),
            Step::Click("#back"),
            Step::Wait(20),
            Step::Click("#back"),
            Step::Wait(20),
            Step::Click("#assign"),
        ],
    );
}

#[test]
fn animation_frames_run_on_the_world_clock() {
    same_as_react(
        r#"
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';

export function App() {
  const [frames, setFrames] = useState(0);
  const [running, setRunning] = useState(false);
  useEffect(() => {
    if (!running) return;
    let n = 0;
    let first = -1;
    let id = requestAnimationFrame(function step(t: number) {
      if (first < 0) first = t;
      n += 1;
      setFrames(n);
      console.log('frame', n, Math.floor((t - first) / 16));
      if (n < 5) id = requestAnimationFrame(step);
      else setRunning(false);
    });
    const never = requestAnimationFrame(() => console.log('never'));
    cancelAnimationFrame(never);
    return () => cancelAnimationFrame(id);
  }, [running]);
  return <div><p id="n">{frames} {String(running)}</p><button id="go" onClick={() => setRunning(true)}>go</button></div>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Wait(40), Step::Wait(100)],
    );
}

#[test]
fn animation_frames_run_on_the_world_clock_on_the_island() {
    same_as_react(
        r#"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import { App } from './anim';
createRoot(document.getElementById('root')!).render(<App />);
// @file anim.tsx
import { useEffect, useState } from 'react';
function* g() { yield 1; }

export function App() {
  const [frames, setFrames] = useState(0);
  const [running, setRunning] = useState(false);
  useEffect(() => {
    if (!running) return;
    let n = 0;
    let first = -1;
    let id = requestAnimationFrame(function step(t: number) {
      if (first < 0) first = t;
      n += 1;
      setFrames(n);
      console.log('frame', n, Math.floor((t - first) / 16));
      if (n < 5) id = requestAnimationFrame(step);
      else setRunning(false);
    });
    const never = requestAnimationFrame(() => console.log('never'));
    cancelAnimationFrame(never);
    return () => cancelAnimationFrame(id);
  }, [running]);
  return <div><p id="n">{frames} {String(running)}</p><button id="go" onClick={() => setRunning(true)}>go</button></div>;
}
"#,
        &[Step::Click("#go"), Step::Wait(40), Step::Wait(100)],
    );
}

#[test]
fn match_media_follows_the_page_media_on_resize() {
    same_as_react(
        r#"
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';

export function App() {
  const [wide, setWide] = useState(() => window.matchMedia('(min-width: 700px)').matches);
  const [log, setLog] = useState<string[]>([]);
  useEffect(() => {
    const mq = window.matchMedia('(min-width: 700px)');
    const legacy = matchMedia('(max-width: 500px)');
    const onChange = (e: { matches: boolean; media: string }) => { setWide(e.matches); setLog((l) => [...l, e.media + '=' + e.matches]); };
    mq.addEventListener('change', onChange);
    legacy.addListener((e: { matches: boolean }) => console.log('narrow', e.matches, legacy.matches));
    return () => mq.removeEventListener('change', onChange);
  }, []);
  return <div><p id="w">{String(wide)} {log.join(' ')}</p></div>;
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Resize(400, 600),
            Step::Wait(10),
            Step::Resize(900, 600),
            Step::Wait(10),
        ],
    );
}

#[test]
fn match_media_follows_the_page_media_on_resize_on_the_island() {
    same_as_react(
        r#"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import { App } from './mq';
createRoot(document.getElementById('root')!).render(<App />);
// @file mq.tsx
import { useEffect, useState } from 'react';
function* g() { yield 1; }

export function App() {
  const [wide, setWide] = useState(() => window.matchMedia('(min-width: 700px)').matches);
  const [log, setLog] = useState<string[]>([]);
  useEffect(() => {
    const mq = window.matchMedia('(min-width: 700px)');
    const legacy = matchMedia('(max-width: 500px)');
    const onChange = (e: { matches: boolean; media: string }) => { setWide(e.matches); setLog((l) => [...l, e.media + '=' + e.matches]); };
    mq.addEventListener('change', onChange);
    legacy.addListener((e: { matches: boolean }) => console.log('narrow', e.matches, legacy.matches));
    return () => mq.removeEventListener('change', onChange);
  }, []);
  return <div><p id="w">{String(wide)} {log.join(' ')}</p></div>;
}
"#,
        &[
            Step::Resize(400, 600),
            Step::Wait(10),
            Step::Resize(900, 600),
            Step::Wait(10),
        ],
    );
}

#[test]
fn inner_html_is_parsed_as_the_page_parses_it() {
    same_as_react(
        r#"
import { useState } from 'react';
import { createRoot } from 'react-dom/client';

export function App() {
  const [n, setN] = useState(1);
  const html = n % 3 === 0 ? null : '<b id="b' + n + '">bold ' + n + '</b> &amp; <i>it</i><script>console.log("never")</script>';
  return (
    <div>
      <button id="go" onClick={() => setN(n + 1)}>go</button>
      {html === null ? <p id="plain">plain {n}</p> : <div id="raw" className={'r' + n} dangerouslySetInnerHTML={{ __html: html }} />}
      <table><tbody dangerouslySetInnerHTML={{ __html: '<tr><td>cell</td></tr>' }} /></table>
    </div>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[Step::Click("#go"), Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn inner_html_is_parsed_as_the_page_parses_it_on_the_island() {
    same_as_react(
        r#"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import { App } from './raw';
createRoot(document.getElementById('root')!).render(<App />);
// @file raw.tsx
import { useState } from 'react';
function* g() { yield 1; }

export function App() {
  const [n, setN] = useState(1);
  const html = n % 3 === 0 ? null : '<b id="b' + n + '">bold ' + n + '</b> &amp; <i>it</i><script>console.log("never")</script>';
  return (
    <div>
      <button id="go" onClick={() => setN(n + 1)}>go</button>
      {html === null ? <p id="plain">plain {n}</p> : <div id="raw" className={'r' + n} dangerouslySetInnerHTML={{ __html: html }} />}
      <table><tbody dangerouslySetInnerHTML={{ __html: '<tr><td>cell</td></tr>' }} /></table>
    </div>
  );
}
"#,
        &[Step::Click("#go"), Step::Click("#go"), Step::Click("#go")],
    );
}

#[test]
fn portals_render_elsewhere_and_bubble_through_react() {
    same_as_react(
        r#"
import { createContext, useContext, useState } from 'react';
import { createPortal } from 'react-dom';
import { createRoot } from 'react-dom/client';

const Theme = createContext('light');
function Modal({ onClose }: { onClose: () => void }) {
  const theme = useContext(Theme);
  return createPortal(
    <div id="modal" className={theme}>
      <p id="inside">in {theme}</p>
      <button id="close" onClick={() => { console.log('close'); onClose(); }}>close</button>
    </div>,
    document.body,
  );
}
export function App() {
  const [open, setOpen] = useState(false);
  const [clicks, setClicks] = useState(0);
  return (
    <Theme.Provider value="dark">
      <section id="outer" onClick={() => { console.log('bubbled to section'); setClicks((c) => c + 1); }}>
        <button id="open" onClick={() => setOpen(true)}>open</button>
        <p id="count">{clicks} {String(open)}</p>
        {open && <Modal onClose={() => setOpen(false)} />}
      </section>
    </Theme.Provider>
  );
}
createRoot(document.getElementById('root')!).render(<App />);
"#,
        &[
            Step::Click("#open"),
            Step::Click("#inside"),
            Step::Click("#close"),
            Step::Click("#open"),
        ],
    );
}

#[test]
fn portals_render_elsewhere_and_bubble_through_react_on_the_island() {
    same_as_react(
        r#"
// @file main.tsx
import { createRoot } from 'react-dom/client';
import { App } from './modal';
createRoot(document.getElementById('root')!).render(<App />);
// @file modal.tsx
import { createContext, useContext, useState } from 'react';
import { createPortal } from 'react-dom';
function* g() { yield 1; }

const Theme = createContext('light');
function Modal({ onClose }: { onClose: () => void }) {
  const theme = useContext(Theme);
  return createPortal(
    <div id="modal" className={theme}>
      <p id="inside">in {theme}</p>
      <button id="close" onClick={() => { console.log('close'); onClose(); }}>close</button>
    </div>,
    document.body,
  );
}
export function App() {
  const [open, setOpen] = useState(false);
  const [clicks, setClicks] = useState(0);
  return (
    <Theme.Provider value="dark">
      <section id="outer" onClick={() => { console.log('bubbled to section'); setClicks((c) => c + 1); }}>
        <button id="open" onClick={() => setOpen(true)}>open</button>
        <p id="count">{clicks} {String(open)}</p>
        {open && <Modal onClose={() => setOpen(false)} />}
      </section>
    </Theme.Provider>
  );
}
"#,
        &[
            Step::Click("#open"),
            Step::Click("#inside"),
            Step::Click("#close"),
            Step::Click("#open"),
        ],
    );
}
