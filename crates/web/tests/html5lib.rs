//! Runs the html5lib-tests tree-construction suite (tests/html5lib/*.dat, see the
//! README there) against `cw_web::html`. Each test parses a document or a fragment,
//! dumps the tree in the suite's `| ` format (attributes sorted, namespaces prefixed)
//! and compares it with the expectation. Tests without a scripting flag run in both
//! modes.

use cw_web::dom::{Document, Namespace, NodeId, NodeKind};
use cw_web::html;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Tests that are expected to fail, as `"file:index"` with the reason. The `scripted/`
/// tests exercise `document.write` and DOM mutation from inline scripts while parsing;
/// this parser never runs script (the `script` module will, through the session).
const KNOWN_FAILURES: &[(&str, &str)] = &[
    ("scripted/adoption01.dat:0", "script mutates an attribute mid-parse"),
    ("scripted/ark.dat:0", "script mutates an attribute mid-parse"),
    ("scripted/webkit01.dat:0", "document.write"),
    ("scripted/webkit01.dat:1", "document.write of nested scripts"),
];

#[derive(Debug, Default, Clone)]
struct Case {
    file: String,
    index: usize,
    line: usize,
    data: String,
    fragment: Option<String>,
    /// `Some(true)` for `#script-on`, `Some(false)` for `#script-off`.
    scripting: Option<bool>,
    expected: String,
}

fn parse_dat(file: &str, text: &str) -> Vec<Case> {
    #[derive(PartialEq, Clone, Copy)]
    enum Section {
        None,
        Data,
        Errors,
        Fragment,
        Document,
    }
    let mut cases = Vec::new();
    let mut cur: Option<Case> = None;
    let mut section = Section::None;
    let mut data_lines: Vec<&str> = Vec::new();
    let mut doc_lines: Vec<&str> = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let finish = |cur: &mut Option<Case>, data_lines: &mut Vec<&str>, doc_lines: &mut Vec<&str>, cases: &mut Vec<Case>| {
        if let Some(mut c) = cur.take() {
            c.data = data_lines.join("\n");
            // The document section runs to the blank line before the next #data (or EOF).
            while doc_lines.last().is_some_and(|l| l.is_empty()) {
                doc_lines.pop();
            }
            c.expected = doc_lines.join("\n");
            c.index = cases.len();
            cases.push(c);
        }
        data_lines.clear();
        doc_lines.clear();
    };
    for (i, line) in lines.iter().enumerate() {
        if *line == "#data" && (section == Section::None || section == Section::Document) {
            finish(&mut cur, &mut data_lines, &mut doc_lines, &mut cases);
            cur = Some(Case { file: file.to_owned(), line: i + 1, ..Default::default() });
            section = Section::Data;
            continue;
        }
        match section {
            Section::Data => {
                if *line == "#errors" {
                    section = Section::Errors;
                } else {
                    data_lines.push(line);
                }
            }
            Section::Errors => match *line {
                "#new-errors" => {}
                "#document-fragment" => section = Section::Fragment,
                "#script-on" => cur.as_mut().unwrap().scripting = Some(true),
                "#script-off" => cur.as_mut().unwrap().scripting = Some(false),
                "#document" => section = Section::Document,
                _ => {}
            },
            Section::Fragment => {
                cur.as_mut().unwrap().fragment = Some((*line).to_owned());
                section = Section::Errors;
            }
            Section::Document => match *line {
                "#script-on" => cur.as_mut().unwrap().scripting = Some(true),
                "#script-off" => cur.as_mut().unwrap().scripting = Some(false),
                _ => doc_lines.push(line),
            },
            Section::None => {}
        }
    }
    finish(&mut cur, &mut data_lines, &mut doc_lines, &mut cases);
    cases
}

fn ns_prefix(ns: Namespace) -> &'static str {
    match ns {
        Namespace::Html => "",
        Namespace::Svg => "svg ",
        Namespace::MathMl => "math ",
    }
}

fn dump_node(doc: &Document, node: NodeId, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    match doc.kind(node) {
        NodeKind::Document | NodeKind::DocumentFragment => {
            for c in doc.children(node) {
                dump_node(doc, c, depth, out);
            }
            return;
        }
        NodeKind::DocType { name, public_id, system_id } => {
            if public_id.is_empty() && system_id.is_empty() {
                let _ = writeln!(out, "| {indent}<!DOCTYPE {name}>");
            } else {
                let _ = writeln!(out, "| {indent}<!DOCTYPE {name} \"{public_id}\" \"{system_id}\">");
            }
        }
        NodeKind::Comment(c) => {
            let _ = writeln!(out, "| {indent}<!-- {c} -->");
        }
        NodeKind::Text(t) => {
            let _ = writeln!(out, "| {indent}\"{t}\"");
        }
        NodeKind::Element { ns, tag, attrs } => {
            let _ = writeln!(out, "| {indent}<{}{tag}>", ns_prefix(*ns));
            let mut names: Vec<(String, &str)> = attrs
                .iter()
                .map(|a| {
                    let name = match (*ns != Namespace::Html).then(|| html::foreign_attribute_namespace(&a.name)).flatten() {
                        Some((prefix, local)) => format!("{prefix} {local}"),
                        None => a.name.clone(),
                    };
                    (name, a.value.as_str())
                })
                .collect();
            names.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            for (name, value) in names {
                let _ = writeln!(out, "| {indent}  {name}=\"{value}\"");
            }
            if *ns == Namespace::Html && tag == "template" {
                if let Some(contents) = doc.template_contents(node) {
                    let _ = writeln!(out, "| {indent}  content");
                    for c in doc.children(contents) {
                        dump_node(doc, c, depth + 2, out);
                    }
                }
                return;
            }
        }
    }
    for c in doc.children(node) {
        if matches!(doc.kind(c), NodeKind::DocumentFragment) {
            continue;
        }
        dump_node(doc, c, depth + 1, out);
    }
}

fn run_case(case: &Case, scripting: bool) -> String {
    let options = html::ParseOptions { scripting };
    let mut out = String::new();
    match &case.fragment {
        None => {
            let doc = html::parse_with_options(&case.data, &options);
            dump_node(&doc, Document::ROOT, 0, &mut out);
        }
        Some(ctx) => {
            let (ns, tag) = if let Some(t) = ctx.strip_prefix("svg ") {
                (Namespace::Svg, t)
            } else if let Some(t) = ctx.strip_prefix("math ") {
                (Namespace::MathMl, t)
            } else {
                (Namespace::Html, ctx.as_str())
            };
            let mut doc = Document::new();
            let context = doc.create(NodeKind::Element { ns, tag: tag.to_owned(), attrs: Vec::new() });
            let nodes = html::parse_fragment_with_options(&mut doc, context, &case.data, &options);
            let holder = doc.create(NodeKind::DocumentFragment);
            for n in nodes {
                doc.append(holder, n);
            }
            dump_node(&doc, holder, 0, &mut out);
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

fn dat_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/html5lib");
    let mut files = Vec::new();
    for sub in [dir.clone(), dir.join("scripted")] {
        for entry in fs::read_dir(&sub).expect("tests/html5lib exists; run tools/fetch-html5lib.py") {
            let p = entry.unwrap().path();
            if p.extension().is_some_and(|e| e == "dat") {
                files.push(p);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn html5lib_tree_construction() {
    let files = dat_files();
    assert!(files.len() > 50, "found only {} .dat files", files.len());
    let mut total = 0;
    let mut passed = 0;
    let mut failures: Vec<String> = Vec::new();
    let mut unexpected: Vec<String> = Vec::new();
    let mut fixed: Vec<String> = Vec::new();
    for path in &files {
        let text = fs::read_to_string(path).unwrap();
        let rel = path.strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/html5lib")).unwrap().to_string_lossy().into_owned();
        for case in parse_dat(&rel, &text) {
            let modes: Vec<bool> = match case.scripting {
                Some(s) => vec![s],
                None => vec![true, false],
            };
            for scripting in modes {
                total += 1;
                let key = format!("{}:{}", case.file, case.index);
                let known = KNOWN_FAILURES.iter().find(|(k, _)| *k == key);
                let actual = run_case(&case, scripting);
                if actual == case.expected {
                    passed += 1;
                    if known.is_some() {
                        fixed.push(key);
                    }
                } else {
                    let report = format!(
                        "{} (line {}, scripting {}):\n#data\n{}\n#document-fragment {:?}\n--- expected ---\n{}\n--- actual ---\n{}\n",
                        key, case.line, scripting, case.data, case.fragment, case.expected, actual
                    );
                    failures.push(report.clone());
                    if known.is_none() {
                        unexpected.push(report);
                    }
                }
            }
        }
    }
    eprintln!("html5lib tree-construction: {passed}/{total} passed, {} failed, {} known failures", failures.len(), KNOWN_FAILURES.len());
    for f in &unexpected {
        eprintln!("{f}");
    }
    assert!(fixed.is_empty(), "known failures now pass; remove them: {fixed:?}");
    assert!(unexpected.is_empty(), "{} unexpected html5lib failures ({}/{} passed)", unexpected.len(), passed, total);
}

/// Reports parse throughput on a synthetic 1 MB document. Run with
/// `cargo test -p cw-web --release -- --ignored html_parse_timing --nocapture`.
#[test]
#[ignore]
fn html_parse_timing() {
    let mut src = String::from("<!DOCTYPE html><html><head><title>t</title><style>body{margin:0}</style></head><body>");
    let mut i = 0;
    while src.len() < 1_000_000 {
        let _ = writeln!(
            src,
            "<div class=\"row r{i}\" id=\"d{i}\"><h2>Heading {i} &amp; more</h2><p>Some <b>bold <i>and italic</b> text</i> with a <a href=\"/x?a=1&b=2\">link</a>.</p><table><tr><td>{i}<td>x</table><ul><li>one<li>two</ul></div>"
        );
        i += 1;
    }
    src.push_str("</body></html>");
    let bytes = src.len();
    let started = std::time::Instant::now();
    let doc = html::parse(&src);
    let elapsed = started.elapsed();
    let nodes = doc.len();
    let started = std::time::Instant::now();
    let out = html::serialize(&doc, Document::ROOT);
    let ser = started.elapsed();
    eprintln!("parsed {bytes} bytes into {nodes} nodes in {elapsed:?} ({:.1} MB/s); serialized {} bytes in {ser:?}", bytes as f64 / elapsed.as_secs_f64() / 1e6, out.len());
    // Adversarial: adoption agency on many misnested formatting elements.
    let mut src = String::from("<body>");
    for _ in 0..3000 {
        src.push_str("<b><i><a>x</b>y</i>");
    }
    let started = std::time::Instant::now();
    let doc = html::parse(&src);
    eprintln!("adoption stress: {} nodes in {:?}", doc.len(), started.elapsed());
    let src = "<b>".repeat(50_000) + "x";
    let started = std::time::Instant::now();
    let doc = html::parse(&src);
    eprintln!("50k nested <b>: {} nodes in {:?}", doc.len(), started.elapsed());
    assert!(elapsed.as_secs_f64() < 1.0, "1 MB parse took {elapsed:?}");
}
