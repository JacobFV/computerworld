//! React 18 semantics, checked against React itself: each test is a small TSX app
//! that logs what it observes (renders, effects, cleanups, values). It is compiled
//! by `cw-tsx` and run twice through the same interaction: natively on `cw-ui`, and
//! as the emitted fallback on React 18's production build on the engine's JS
//! `Realm`. The console logs and the documents must be identical.

use cw_ui::UiApp;
use cw_web::dom::{Document, NodeId, NodeKind};
use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};

const BASE: &str = "https://example.test/";

fn vendor(h: MemoryHost) -> MemoryHost {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/vendor");
    let mut h = h;
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
            NodeKind::Element { tag, attrs, .. } => {
                if tag == "script" {
                    return;
                }
                out.push('<');
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
    }
    out
}

fn compile(tsx: &str) -> (cw_ui::ir::Module, String) {
    let b = cw_tsx::build(tsx, "app.tsx");
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
    }
}

fn run_compiled(module: &cw_ui::ir::Module, steps: &[Step]) -> Outcome {
    let mut app = UiApp::new(
        module.clone(),
        SHELL,
        &format!("{BASE}app.html"),
        Box::new(vendor(MemoryHost::new())),
    )
    .unwrap();
    app.boot();
    for s in steps {
        let at = match s {
            Step::Click(sel) => {
                let n = app
                    .query_selector(sel)
                    .unwrap_or_else(|| panic!("compiled: no {sel}"));
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
    }
    let values = app.form_values();
    let doc = app.document().clone();
    let selections = app.inner().form.selection.clone();
    Outcome {
        logs: app
            .logs()
            .into_iter()
            .map(|l| format!("{:?}: {}", l.level, l.text))
            .collect(),
        dom: dom_text(&doc, &values, &|n| selections.get(&n).copied()),
    }
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
    let a = run_compiled(&module, steps);
    let b = run_fallback(&js, steps);
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
    let mut app = UiApp::new(
        module,
        SHELL,
        &format!("{BASE}app.html"),
        Box::new(vendor(MemoryHost::new())),
    )
    .unwrap();
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
}
