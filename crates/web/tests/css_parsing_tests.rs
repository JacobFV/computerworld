//! Runs the css-parsing-tests corpus (tests/css-parsing-tests, from
//! https://github.com/SimonSapin/css-parsing-tests at the commit in `COMMIT`) against
//! the raw parser, comparing component-value JSON dumps.
//!
//! Known deviations, normalised by the runner rather than the parser:
//! - The corpus predates the 2021 CR removal of the `~=`, `|=`, `^=`, `$=`, `*=`, `||`
//!   and `<unicode-range>` tokens. Match tokens are split into two delims on the
//!   expected side (one case, `~/**/=`, becomes indistinguishable); inputs whose
//!   expectation contains a `unicode-range` token are skipped and counted.
//! - `["error", "eof-in-string"]` and `["error", "eof-in-url"]` are tinycss2 markers,
//!   not tokens; the specification returns the string/url token alone. They are dropped.
//! - Declaration values are trimmed of surrounding whitespace on both sides, as the
//!   2021 CR requires; the corpus keeps the untrimmed value.

use cw_web::css::parser::{parse_block_contents, parse_component_value, parse_component_value_list, parse_declaration, parse_declaration_list, parse_rule, parse_rule_list, parse_stylesheet_rules, Item, ParseError};
use cw_web::css::selector::parse_anb;
use cw_web::css::{ComponentValue, Declaration, Token};
use serde_json::{json, Value};

fn corpus(name: &str) -> Vec<Value> {
    let path = format!("{}/tests/css-parsing-tests/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    match serde_json::from_str(&text).unwrap() {
        Value::Array(v) => v,
        _ => panic!("{name}: not an array"),
    }
}

fn num(text: &str, value: cw_web::css::Number) -> (Value, Value, Value) {
    let ty = if value.int { "integer" } else { "number" };
    (json!(text), json!(value.to_f64()), json!(ty))
}

fn dump(v: &ComponentValue) -> Value {
    match v {
        ComponentValue::Function { name, args } => {
            let mut out = vec![json!("function"), json!(name)];
            out.extend(args.iter().map(dump));
            Value::Array(out)
        }
        ComponentValue::Block { open, contents } => {
            let name = match open {
                Token::OpenCurly => "{}",
                Token::OpenSquare => "[]",
                _ => "()",
            };
            let mut out = vec![json!(name)];
            out.extend(contents.iter().map(dump));
            Value::Array(out)
        }
        ComponentValue::Token(t) => match t {
            Token::Ident(s) => json!(["ident", s]),
            Token::Function(s) => json!(["function", s]),
            Token::AtKeyword(s) => json!(["at-keyword", s]),
            Token::Hash { value, id } => json!(["hash", value, if *id { "id" } else { "unrestricted" }]),
            Token::String(s) => json!(["string", s]),
            Token::BadString => json!(["error", "bad-string"]),
            Token::Url(s) => json!(["url", s]),
            Token::BadUrl => json!(["error", "bad-url"]),
            Token::Delim(c) => json!(c.to_string()),
            Token::Number { text, value } => {
                let (t, v, ty) = num(text, *value);
                json!(["number", t, v, ty])
            }
            Token::Percentage { text, value } => {
                let (t, v, ty) = num(text, *value);
                json!(["percentage", t, v, ty])
            }
            Token::Dimension { text, value, unit } => {
                let (t, v, ty) = num(text, *value);
                json!(["dimension", t, v, ty, unit])
            }
            Token::Whitespace => json!(" "),
            Token::Cdo => json!("<!--"),
            Token::Cdc => json!("-->"),
            Token::Colon => json!(":"),
            Token::Semicolon => json!(";"),
            Token::Comma => json!(","),
            Token::OpenSquare => json!("["),
            Token::CloseSquare => json!(["error", "]"]),
            Token::OpenParen => json!("("),
            Token::CloseParen => json!(["error", ")"]),
            Token::OpenCurly => json!("{"),
            Token::CloseCurly => json!(["error", "}"]),
        },
    }
}

fn dump_list(v: &[ComponentValue]) -> Value {
    Value::Array(v.iter().map(dump).collect())
}

fn dump_decl(d: &Declaration) -> Value {
    json!(["declaration", d.name, dump_list(&d.value), d.important])
}

fn dump_item(i: &Item) -> Value {
    match i {
        Item::Declaration(d) => dump_decl(d),
        Item::AtRule(a) => json!(["at-rule", a.name, dump_list(&a.prelude), a.block.as_ref().map(|b| dump_list(b))]),
        Item::QualifiedRule(q) => json!(["qualified rule", dump_list(&q.prelude), dump_list(&q.block)]),
        Item::Invalid => json!(["error", "invalid"]),
    }
}

fn dump_err(e: ParseError) -> Value {
    match e {
        ParseError::Empty => json!(["error", "empty"]),
        ParseError::Invalid => json!(["error", "invalid"]),
        ParseError::ExtraInput => json!(["error", "extra-input"]),
    }
}

/// Normalises an expected value to the 2021 CR token set (see the module comment).
fn normalise(v: &Value) -> Value {
    match v {
        Value::Array(items) => {
            let mut out = Vec::new();
            let head = items.first().and_then(Value::as_str);
            // Declarations: trim surrounding whitespace of the value.
            if head == Some("declaration") && items.len() == 4 {
                let value = normalise(&items[2]);
                let mut vals = value.as_array().cloned().unwrap_or_default();
                while vals.first().is_some_and(|x| x == " ") {
                    vals.remove(0);
                }
                while vals.last().is_some_and(|x| x == " ") {
                    vals.pop();
                }
                return json!(["declaration", items[1], Value::Array(vals), items[3]]);
            }
            let mut i = 0;
            while i < items.len() {
                let item = &items[i];
                if let Some(s) = item.as_str() {
                    match s {
                        "~=" | "|=" | "^=" | "$=" | "*=" => {
                            out.push(json!(s[..1].to_string()));
                            out.push(json!("="));
                            i += 1;
                            continue;
                        }
                        "||" => {
                            out.push(json!("|"));
                            out.push(json!("|"));
                            i += 1;
                            continue;
                        }
                        _ => {}
                    }
                }
                if let Value::Array(a) = item {
                    if a.len() == 2 && a[0] == "error" && (a[1] == "eof-in-string" || a[1] == "eof-in-url") {
                        i += 1;
                        continue;
                    }
                }
                out.push(normalise(item));
                i += 1;
            }
            Value::Array(out)
        }
        Value::Number(n) => {
            // Compare numbers at six decimals, the parser's precision.
            let f = n.as_f64().unwrap();
            json!((f * 1e6).round() / 1e6)
        }
        other => other.clone(),
    }
}

fn contains_unicode_range(v: &Value) -> bool {
    match v {
        Value::Array(items) => items.first().is_some_and(|h| h == "unicode-range") || items.iter().any(contains_unicode_range),
        _ => false,
    }
}

struct Report {
    name: &'static str,
    passed: usize,
    skipped: usize,
    failures: Vec<String>,
}

fn run(name: &'static str, f: impl Fn(&str) -> Value) -> Report {
    let cases = corpus(name);
    assert!(cases.len().is_multiple_of(2), "{name}: odd number of items");
    let mut r = Report { name, passed: 0, skipped: 0, failures: Vec::new() };
    for pair in cases.chunks(2) {
        let input = pair[0].as_str().unwrap_or_else(|| panic!("{name}: non-string input"));
        if contains_unicode_range(&pair[1]) {
            r.skipped += 1;
            continue;
        }
        let expected = normalise(&pair[1]);
        let actual = normalise(&f(input));
        if expected == actual {
            r.passed += 1;
        } else {
            r.failures.push(format!("{name}: input {input:?}\n  expected {expected}\n  actual   {actual}"));
        }
    }
    r
}

fn items(v: Vec<Item>) -> Value {
    Value::Array(v.iter().map(dump_item).collect())
}

#[test]
fn css_parsing_tests_corpus() {
    let reports = vec![
        run("component_value_list.json", |s| dump_list(&parse_component_value_list(s))),
        run("one_component_value.json", |s| parse_component_value(s).map(|v| dump(&v)).unwrap_or_else(dump_err)),
        run("one_declaration.json", |s| parse_declaration(s).map(|d| dump_decl(&d)).unwrap_or_else(dump_err)),
        run("declaration_list.json", |s| items(parse_declaration_list(s))),
        run("blocks_contents.json", |s| items(parse_block_contents(s))),
        run("one_rule.json", |s| parse_rule(s).map(|i| dump_item(&i)).unwrap_or_else(dump_err)),
        run("rule_list.json", |s| items(parse_rule_list(s))),
        run("stylesheet.json", |s| items(parse_stylesheet_rules(s))),
        run("An+B.json", |s| match parse_anb(s) {
            Some((a, b)) => json!([a, b]),
            None => Value::Null,
        }),
    ];
    let mut failed = 0;
    for r in &reports {
        println!("{}: {} passed, {} skipped (unicode-range), {} failed", r.name, r.passed, r.skipped, r.failures.len());
        for f in &r.failures {
            println!("{f}");
        }
        failed += r.failures.len();
    }
    assert_eq!(failed, 0, "{failed} corpus cases failed");
}
