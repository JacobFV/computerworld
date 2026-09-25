//! What `cw-tsx` accepts, what it refuses (with a line and column), and that a
//! refused module still gets its React fallback.

/// What keeps the module from compiling: the diagnostics that stop the build,
/// or those that sent its code to the island.
fn refusals(b: &cw_tsx::Build) -> Vec<cw_tsx::Diagnostic> {
    b.diagnostics.iter().chain(&b.outside).cloned().collect()
}

fn diags(src: &str) -> Vec<String> {
    let b = cw_tsx::build(src, "app.tsx");
    refusals(&b).iter().map(|d| d.to_string()).collect()
}

const HEAD: &str = "import { useState, useEffect } from 'react';\nimport { createRoot } from 'react-dom/client';\n";
const TAIL: &str = "\ncreateRoot(document.getElementById('root')!).render(<App />);\n";

fn module(body: &str) -> String {
    format!("{HEAD}{body}{TAIL}")
}

fn assert_refused(body: &str, line: u32, needle: &str) {
    let src = module(body);
    let b = cw_tsx::build(&src, "app.tsx");
    assert!(
        b.ir.is_none() || !b.island_modules.is_empty(),
        "accepted:\n{src}"
    );
    assert!(
        b.js.is_some(),
        "no fallback for:\n{src}\n{:?}",
        b.diagnostics
    );
    assert!(
        refusals(&b)
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
fn types_are_hints_so_any_and_untyped_parameters_compile() {
    // What TypeScript leaves open (`any`, an unannotated parameter, a type the
    // compiler does not model) is resolved when the code runs, as in JavaScript.
    let src = module(
        "function show(v) { return String(v).trim(); }\nfunction first(xs: any) { return xs.at(0).label.toUpperCase(); }\nfunction App() { const x: any = { a: [{ label: 'b' }] }; const d: Date | undefined = undefined; return <p title={show(d)}>{first(x.a)}{show(1)}</p>; }",
    );
    assert_eq!(diags(&src), Vec::<String>::new());
}

#[test]
fn a_builtin_method_the_runtime_lacks_is_refused_on_any_receiver() {
    assert_refused(
        "function f(s: any) { return s.normalize('NFD'); }\nfunction App() { return <p>{f('x')}</p>; }",
        3,
        "built-in method `normalize`",
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
fn awaits_inside_expressions_and_classes_are_refused() {
    assert_refused(
        "async function load(): Promise<number> { return 1 + (await Promise.resolve(2)); }\nfunction App() { return <p />; }",
        3,
        "`await` inside an expression",
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
fn reassigned_captures_are_shared() {
    let b = cw_tsx::build(
        &module("function App() {\n  let n = 0;\n  const f = () => { n += 1; return n; };\n  n = 2;\n  return <p>{f()}</p>;\n}"),
        "app.tsx",
    );
    assert!(b.diagnostics.is_empty(), "{:?}", b.diagnostics);
    let ir = b.ir.unwrap();
    let app = ir.functions.iter().find(|f| f.name == "App").unwrap();
    assert_eq!(app.boxed, vec![0]);
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

fn load_virtual(
    files: &[(&str, &str)],
    entry: &str,
) -> Result<Vec<cw_tsx::Source>, Vec<cw_tsx::Diagnostic>> {
    let map: std::collections::BTreeMap<String, String> = files
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect();
    let mut read = |p: &str| map.get(p).cloned();
    cw_tsx::load(entry, &mut read)
}

#[test]
fn an_app_of_several_modules_compiles_into_one() {
    let files = [
        ("main.tsx", "import { createRoot } from 'react-dom/client';\nimport App from './App';\ncreateRoot(document.getElementById('root')!).render(<App />);\n"),
        ("App.tsx", "import { useState } from 'react';\nimport { greet, type Name } from './lib/greet';\nimport { Badge } from '../shared/badge';\nexport default function App() { const [n] = useState<Name>('Ada'); return <Badge text={greet(n)} />; }\n"),
        ("lib/greet.ts", "export type Name = 'Ada' | 'Bo';\nexport function greet(n: Name): string { return 'hi ' + n; }\n"),
        ("../shared/badge.tsx", "export const Badge = ({ text }: { text: string }) => <b>{text}</b>;\n"),
    ];
    let sources = load_virtual(&files, "main.tsx").unwrap();
    let order: Vec<&str> = sources.iter().map(|s| s.file.as_str()).collect();
    assert_eq!(
        order,
        ["lib/greet.ts", "../shared/badge.tsx", "App.tsx", "main.tsx"]
    );
    let b = cw_tsx::build_modules(&sources);
    assert!(b.diagnostics.is_empty(), "{:?}", b.diagnostics);
    assert!(b.ir.is_some());
    let js = b.js.unwrap();
    // Every module's exports object exists first; exports are live getters, and
    // a use of an imported name reads the exporting module's object.
    assert!(js.contains("const __cw_m = [{}, {}, {}, {}];"), "{js}");
    assert!(
        js.contains("Object.defineProperties(__cw_m[0], { greet: { enumerable: true, get: () => greet } });"),
        "{js}"
    );
    assert!(js.contains("React.createElement(__cw_i2.default"), "{js}");
}

#[test]
fn module_errors_name_their_file() {
    let files = [
        ("main.tsx", "import { createRoot } from 'react-dom/client';\nimport { Missing } from './a';\nimport './nowhere';\ncreateRoot(document.getElementById('root')!).render(<Missing />);\n"),
        ("a.tsx", "export function Other() { const x = 1n; return <p>{String(x)}</p>; }\n"),
    ];
    let err = load_virtual(&files, "main.tsx").unwrap_err();
    assert!(
        err.iter()
            .any(|d| d.file == "main.tsx" && d.line == 3 && d.message.contains("./nowhere")),
        "{err:?}"
    );
    let files = [files[0], files[1]];
    let fixed = [
        (files[0].0, files[0].1.replace("import './nowhere';\n", "")),
        (files[1].0, files[1].1.to_string()),
    ];
    let fixed: Vec<(&str, &str)> = fixed.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let b = cw_tsx::build_modules(&load_virtual(&fixed, "main.tsx").unwrap());
    let shown: Vec<String> = refusals(&b).iter().map(|d| d.to_string()).collect();
    assert!(
        shown.iter().any(|d| d.starts_with("a.tsx: line 1:")),
        "{shown:?}"
    );
    assert!(
        shown
            .iter()
            .any(|d| d.contains("`Missing` is not exported")),
        "{shown:?}"
    );
}

/// A module outside the subset runs on the island only if the island can run it:
/// its React shim is no browser page, so a module needing the page's APIs, a
/// class component or raw HTML keeps the whole app on React.
#[test]
fn island_modules_that_need_a_page_refuse_the_build() {
    let main = "import { createRoot } from 'react-dom/client';\nimport { Widget } from './widget';\ncreateRoot(document.getElementById('root')!).render(<Widget />);\n";
    let cases = [
        (
            "function* g() { yield 1; }\nexport function Widget() { return <p>{String(window.matchMedia('(min-width: 1px)').matches)}</p>; }\n",
            "`window.matchMedia` is not on the island",
        ),
        (
            "import { Component } from 'react';\nexport class Widget extends Component { render() { return <p />; } }\n",
            "a class component on the island",
        ),
        (
            "function* g() { yield 1; }\nexport function Widget() { return <div dangerouslySetInnerHTML={{ __html: '<b>x</b>' }} />; }\n",
            "`dangerouslySetInnerHTML` on the island",
        ),
    ];
    for (widget, needle) in cases {
        let files = [("main.tsx", main), ("widget.tsx", widget)];
        let b = cw_tsx::build_modules(&load_virtual(&files, "main.tsx").unwrap());
        assert!(b.ir.is_none(), "built:\n{widget}");
        assert!(b.js.is_some(), "no fallback for:\n{widget}");
        assert!(
            b.diagnostics
                .iter()
                .any(|d| d.file == "widget.tsx" && d.message.contains(needle)),
            "expected …{needle}…, got {:?}",
            b.diagnostics
        );
    }
    // What a browser and the island agree on is fine: `typeof process` is
    // "undefined" in both, and `process.env.NODE_ENV` is "production", as a
    // bundler defines it.
    let widget = "function* g() { yield 1; }\nexport function Widget() { const dev = process.env.NODE_ENV !== 'production'; return <p>{String(dev)} {typeof process}</p>; }\n";
    let files = [("main.tsx", main), ("widget.tsx", widget)];
    let b = cw_tsx::build_modules(&load_virtual(&files, "main.tsx").unwrap());
    assert!(b.diagnostics.is_empty(), "{:?}", b.diagnostics);
    assert_eq!(b.island_modules, vec!["widget.tsx".to_owned()]);
    let js = b.js.unwrap();
    assert!(js.contains("'production' !== 'production'"), "{js}");
}
