//! What `cw-tsx` accepts, what it refuses (with a line and column), and that a
//! refused module still gets its React fallback.

fn diags(src: &str) -> Vec<String> {
    let b = cw_tsx::build(src, "app.tsx");
    b.diagnostics.iter().map(|d| d.to_string()).collect()
}

const HEAD: &str = "import { useState, useEffect } from 'react';\nimport { createRoot } from 'react-dom/client';\n";
const TAIL: &str = "\ncreateRoot(document.getElementById('root')!).render(<App />);\n";

fn module(body: &str) -> String {
    format!("{HEAD}{body}{TAIL}")
}

fn assert_refused(body: &str, line: u32, needle: &str) {
    let src = module(body);
    let b = cw_tsx::build(&src, "app.tsx");
    assert!(b.ir.is_none(), "accepted:\n{src}");
    assert!(
        b.js.is_some(),
        "no fallback for:\n{src}\n{:?}",
        b.diagnostics
    );
    assert!(
        b.diagnostics
            .iter()
            .any(|d| d.line == line && d.message.contains(needle)),
        "expected line {line}: …{needle}…, got {:?}",
        b.diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_typed_component_compiles() {
    let src = module(
        "interface P { name: string; n?: number }\nfunction Hello({ name, n = 1 }: P) { return <b>{name.toUpperCase()} x{n}</b>; }\nfunction App() { const [s] = useState('a'); return <Hello name={s} />; }",
    );
    assert_eq!(diags(&src), Vec::<String>::new());
}

#[test]
fn any_is_refused_with_its_position() {
    assert_refused(
        "function App() { const x: any = 1; return <p>{x}</p>; }",
        3,
        "value of type any",
    );
}

#[test]
fn untyped_parameters_are_refused() {
    assert_refused(
        "function show(v) { return String(v); }\nfunction App() { return <p>{show(1)}</p>; }",
        3,
        "implicit type any",
    );
}

#[test]
fn hooks_in_conditions_are_refused() {
    assert_refused(
        "function App() {\n  if (Math.random() > 0.5) { useEffect(() => {}, []); }\n  return <p />;\n}",
        4,
        "top level of a component",
    );
}

#[test]
fn async_code_and_classes_are_refused() {
    assert_refused(
        "async function load(): Promise<void> {}\nfunction App() { return <p />; }",
        3,
        "async functions",
    );
    assert_refused("class X {}\nfunction App() { return <p />; }", 3, "class");
}

#[test]
fn other_modules_are_refused() {
    let src = "import { thing } from './thing';\nfunction App() { return <p />; }\n";
    let b = cw_tsx::build(src, "app.tsx");
    assert!(b.ir.is_none());
    assert!(b
        .diagnostics
        .iter()
        .any(|d| d.line == 1 && d.message.contains("./thing")));
}

#[test]
fn a_module_that_never_renders_is_refused() {
    let b = cw_tsx::build("function App() { return <p />; }\n", "app.tsx");
    assert!(b.ir.is_none());
    assert!(b
        .diagnostics
        .iter()
        .any(|d| d.message.contains("never renders")));
}

#[test]
fn reassigned_captures_are_refused() {
    assert_refused(
        "function App() {\n  let n = 0;\n  const f = () => n;\n  n = 2;\n  return <p>{f()}</p>;\n}",
        6,
        "reassigned and also captured",
    );
}

#[test]
fn the_fallback_is_react_create_element_on_globals() {
    let b = cw_tsx::build(
        &module("type T = { a: number };\nexport default function App(): JSX.Element { const t: T = { a: 1 }; return <><i key=\"k\">{t.a}</i></>; }"),
        "app.tsx",
    );
    let js = b.js.unwrap();
    assert!(
        js.contains("const { useState, useEffect } = React;"),
        "{js}"
    );
    assert!(js.contains("const { createRoot } = ReactDOM;"), "{js}");
    assert!(
        js.contains("React.createElement(React.Fragment, null"),
        "{js}"
    );
    assert!(!js.contains("export"), "{js}");
    assert!(!js.contains(": T"), "{js}");
}

#[test]
fn jsx_text_is_cleaned_as_babel_cleans_it() {
    use cw_tsx::lower::clean_jsx_text;
    assert_eq!(clean_jsx_text(" "), Some(" ".into()));
    assert_eq!(clean_jsx_text("\n   "), None);
    assert_eq!(
        clean_jsx_text("\n  Hello\n  world  \n"),
        Some("Hello world".into())
    );
    assert_eq!(
        clean_jsx_text("a &amp; b&nbsp;"),
        Some("a & b\u{a0}".into())
    );
    assert_eq!(clean_jsx_text(" lead"), Some(" lead".into()));
}
