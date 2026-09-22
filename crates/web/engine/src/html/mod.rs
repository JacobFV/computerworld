//! HTML parsing and serialization: a WHATWG HTML 5 tokenizer and tree builder that turn
//! text into a `dom::Document` with all the error recovery real pages depend on, the
//! fragment parsing algorithm for `innerHTML`, and the fragment serialization algorithm
//! for reading it back. See DESIGN.md for the contracts and `tests/html5lib.rs` for the
//! conformance run.
//!
//! Scripts never run here. Documents are parsed with the scripting flag on (the browser
//! will run script), so `<noscript>` content is raw text; `ParseOptions::scripting`
//! turns that off. A script-aware parse goes through `Parser`, which pauses at each
//! `</script>` so the script layer can run the element (and `document.write` into the
//! input stream) before parsing resumes.

pub mod entities;
mod tokenizer;
mod tree_builder;

pub use tokenizer::{longest_named_reference, numeric_reference_char};
pub use tree_builder::foreign_attribute_namespace;

/// An incremental document parse that stops at every `<script>` end tag: the hook the
/// script layer drives so scripts run as the parser reaches them, in document order,
/// with `document.write` inserting into the input stream at the current position.
///
/// ```ignore
/// let mut p = Parser::new(Document::new(), source);
/// while let Some(script) = p.next_script() {
///     // p.document() holds the tree so far; run the script, then
///     p.write("<p>text written by the script</p>");
/// }
/// let doc = p.finish();
/// ```
pub struct Parser {
    builder: TreeBuilder<'static>,
}

impl Parser {
    pub fn new(doc: Document, source: &str) -> Parser {
        Parser::with_options(doc, source, &ParseOptions::default())
    }

    pub fn with_options(doc: Document, source: &str, options: &ParseOptions) -> Parser {
        let tok = Tokenizer::new(preprocess(source));
        let mut builder = TreeBuilder::owned(doc, tok, options.scripting);
        builder.pause_on_script = true;
        Parser { builder }
    }

    /// Parses up to and including the next `</script>`, returning the script
    /// element, or `None` when the input is exhausted (the parse is then complete).
    pub fn next_script(&mut self) -> Option<NodeId> {
        self.builder.parse_step()
    }

    /// The document being built; the caller may mutate it (and swap it out with
    /// `std::mem::swap`) while the parser is paused at a script.
    pub fn document(&mut self) -> &mut Document {
        self.builder.document()
    }

    /// `document.write`: inserts markup into the input stream at the parser's
    /// current position, to be tokenized when parsing resumes.
    pub fn write(&mut self, markup: &str) {
        self.builder.insert_input(&preprocess(markup));
    }

    pub fn finish(self) -> Document {
        let mut doc = self.builder.into_document().expect("owned document");
        doc.mutations.clear();
        doc
    }
}

use crate::dom::{Document, Namespace, NodeId, NodeKind};
use tokenizer::Tokenizer;
use tree_builder::TreeBuilder;

/// Options for a parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseOptions {
    /// The scripting flag: with it on (the default), `<noscript>` content is raw text.
    pub scripting: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions { scripting: true }
    }
}

/// Decodes bytes as UTF-8, replacing invalid sequences with U+FFFD.
pub fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Preprocesses the input stream (§13.2.3.5): strips a leading BOM and normalises CRLF
/// and CR to LF.
pub fn preprocess(source: &str) -> String {
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);
    if !source.as_bytes().contains(&b'\r') {
        return source.to_owned();
    }
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses a complete document.
pub fn parse(source: &str) -> Document {
    parse_with_options(source, &ParseOptions::default())
}

/// Parses a complete document and records its URL.
pub fn parse_with_url(source: &str, url: &str) -> Document {
    let mut doc = parse(source);
    doc.url = url.to_owned();
    doc
}

pub fn parse_with_options(source: &str, options: &ParseOptions) -> Document {
    let mut doc = Document::new();
    let tok = Tokenizer::new(preprocess(source));
    let mut builder = TreeBuilder::new(&mut doc, tok, options.scripting);
    builder.run();
    doc.mutations.clear();
    doc
}

/// The HTML fragment parsing algorithm (§13.4) with `context` as the context element:
/// what `innerHTML` assignment parses. The new nodes are created in `doc`, detached,
/// and returned in order for the caller to insert.
pub fn parse_fragment(doc: &mut Document, context: NodeId, source: &str) -> Vec<NodeId> {
    parse_fragment_with_options(doc, context, source, &ParseOptions::default())
}

pub fn parse_fragment_with_options(
    doc: &mut Document,
    context: NodeId,
    source: &str,
    options: &ParseOptions,
) -> Vec<NodeId> {
    let mutations = doc.mutations.len();
    let fragment = doc.create(NodeKind::DocumentFragment);
    let tok = Tokenizer::new(preprocess(source));
    let mut builder = TreeBuilder::new(doc, tok, options.scripting);
    builder.set_fragment_context(context, fragment);
    builder.run();
    let children: Vec<NodeId> = doc.children(fragment).collect();
    for &c in &children {
        doc.detach(c);
    }
    doc.mutations.truncate(mutations);
    children
}

fn serializes_as_void(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "basefont"
            | "bgsound"
            | "br"
            | "col"
            | "embed"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "keygen"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn escape_into(out: &mut String, s: &str, attribute: bool) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{A0}' => out.push_str("&nbsp;"),
            '<' if !attribute => out.push_str("&lt;"),
            '>' if !attribute => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// The HTML fragment serialization algorithm (§13.3): the children of `node`, as
/// `innerHTML` reads them. Scripting is taken as enabled (`<noscript>` text is literal).
pub fn serialize(doc: &Document, node: NodeId) -> String {
    let mut out = String::new();
    serialize_children(doc, node, &mut out);
    out
}

/// `node` itself followed by its contents: `outerHTML`.
pub fn serialize_node(doc: &Document, node: NodeId) -> String {
    let mut out = String::new();
    serialize_one(doc, node, &mut out);
    out
}

fn serialize_children(doc: &Document, node: NodeId, out: &mut String) {
    let node = match doc.kind(node) {
        NodeKind::Element {
            ns: Namespace::Html,
            tag,
            ..
        } if serializes_as_void(tag) => return,
        NodeKind::Element {
            ns: Namespace::Html,
            tag,
            ..
        } if tag == "template" => doc.template_contents(node).unwrap_or(node),
        _ => node,
    };
    for child in doc.children(node) {
        serialize_one(doc, child, out);
    }
}

fn serialize_one(doc: &Document, child: NodeId, out: &mut String) {
    match doc.kind(child) {
        NodeKind::Element { ns, tag, attrs } => {
            out.push('<');
            out.push_str(tag);
            for a in attrs {
                out.push(' ');
                out.push_str(&a.name);
                out.push_str("=\"");
                escape_into(out, &a.value, true);
                out.push('"');
            }
            out.push('>');
            if *ns == Namespace::Html && serializes_as_void(tag) {
                return;
            }
            serialize_children(doc, child, out);
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        NodeKind::Text(t) => {
            let literal = doc.parent(child).is_some_and(|p| matches!(doc.kind(p), NodeKind::Element { ns: Namespace::Html, tag, .. } if matches!(tag.as_str(), "style" | "script" | "xmp" | "iframe" | "noembed" | "noframes" | "plaintext" | "noscript")));
            if literal {
                out.push_str(t);
            } else {
                escape_into(out, t, false);
            }
        }
        NodeKind::Comment(c) => {
            out.push_str("<!--");
            out.push_str(c);
            out.push_str("-->");
        }
        NodeKind::DocType { name, .. } => {
            out.push_str("<!DOCTYPE ");
            out.push_str(name);
            out.push('>');
        }
        NodeKind::Document | NodeKind::DocumentFragment => {
            for c in doc.children(child) {
                serialize_one(doc, c, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{Attribute, QuirksMode};

    fn body_html(src: &str) -> String {
        let doc = parse(src);
        serialize(&doc, doc.body().expect("body"))
    }

    #[test]
    fn entity_table_edges() {
        assert_eq!(entities::COUNT, 2231);
        assert_eq!(entities::ENTITIES.len(), 2231);
        assert_eq!(
            entities::ENTITIES
                .iter()
                .filter(|(n, _)| !n.ends_with(';'))
                .count(),
            106
        );
        assert!(
            entities::ENTITIES.windows(2).all(|w| w[0].0 < w[1].0),
            "sorted"
        );
        assert_eq!(entities::lookup("amp;"), Some("&"));
        assert_eq!(entities::lookup("amp"), Some("&"));
        assert_eq!(entities::lookup("AMP"), Some("&"));
        assert_eq!(entities::lookup("nbsp"), Some("\u{A0}"));
        assert_eq!(entities::lookup("zwnj;"), Some("\u{200C}"));
        assert_eq!(entities::lookup("zwnj"), None);
        assert_eq!(
            entities::lookup("CounterClockwiseContourIntegral;"),
            Some("\u{2233}")
        );
        assert_eq!(entities::lookup("ngE;"), Some("\u{2267}\u{338}"));
        assert_eq!(longest_named_reference("notin;x"), Some((6, "\u{2209}")));
        assert_eq!(longest_named_reference("notit;"), Some((3, "\u{AC}")));
        assert_eq!(longest_named_reference("xyzzy"), None);
        assert_eq!(numeric_reference_char(0x80), '\u{20AC}');
        assert_eq!(numeric_reference_char(0x9F), '\u{178}');
        assert_eq!(numeric_reference_char(0x81), '\u{81}');
        assert_eq!(numeric_reference_char(0), '\u{FFFD}');
        assert_eq!(numeric_reference_char(0xD800), '\u{FFFD}');
        assert_eq!(numeric_reference_char(0x110000), '\u{FFFD}');
        assert_eq!(numeric_reference_char(0x10FFFF), '\u{10FFFF}');
    }

    #[test]
    fn preprocessing() {
        assert_eq!(preprocess("\u{FEFF}a\r\nb\rc\n"), "a\nb\nc\n");
        assert_eq!(decode(b"a\xffb"), "a\u{FFFD}b");
        let doc = parse("<pre>\r\nx</pre>");
        assert_eq!(doc.text_content(doc.body().unwrap()), "x");
    }

    #[test]
    fn document_structure_and_quirks() {
        let doc = parse("<!DOCTYPE html><title>t</title><p>hi");
        assert_eq!(doc.quirks, QuirksMode::NoQuirks);
        assert!(
            matches!(doc.kind(doc.first_child(Document::ROOT).unwrap()), NodeKind::DocType { name, .. } if name == "html")
        );
        assert_eq!(doc.text_content(doc.head().unwrap()), "t");
        assert_eq!(serialize(&doc, doc.body().unwrap()), "<p>hi</p>");
        assert_eq!(parse("<p>x").quirks, QuirksMode::Quirks);
        assert_eq!(
            parse("<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\">").quirks,
            QuirksMode::Quirks
        );
        assert_eq!(parse("<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\">").quirks, QuirksMode::LimitedQuirks);
        assert_eq!(
            parse("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \"x\">").quirks,
            QuirksMode::NoQuirks
        );
        let doc = parse_with_url("<p>", "https://example.test/");
        assert_eq!(doc.url, "https://example.test/");
    }

    #[test]
    fn error_recovery() {
        assert_eq!(
            body_html("<p>a<p>b<ul><li>1<li>2</ul>"),
            "<p>a</p><p>b</p><ul><li>1</li><li>2</li></ul>"
        );
        assert_eq!(body_html("<b>1<i>2</b>3</i>"), "<b>1<i>2</i></b><i>3</i>");
        assert_eq!(
            body_html("<table><tr><td>x</table>"),
            "<table><tbody><tr><td>x</td></tr></tbody></table>"
        );
        assert_eq!(
            body_html("<table>foo<tr><td>x</table>"),
            "foo<table><tbody><tr><td>x</td></tr></tbody></table>"
        );
        assert_eq!(
            body_html("<a href=x>1<div>2</a>3"),
            "<a href=\"x\">1</a><div><a href=\"x\">2</a>3</div>"
        );
        assert_eq!(
            body_html("<select><option>a<option>b</select>"),
            "<select><option>a</option><option>b</option></select>"
        );
    }

    #[test]
    fn raw_text_and_noscript() {
        assert_eq!(
            body_html("<body><script>if (a < b && c) {}</script>"),
            "<script>if (a < b && c) {}</script>"
        );
        assert_eq!(
            body_html("<body><style>a > b { }</style>"),
            "<style>a > b { }</style>"
        );
        assert_eq!(
            body_html("<textarea>\n<b>&amp;</textarea>"),
            "<textarea>&lt;b&gt;&amp;</textarea>"
        );
        assert_eq!(
            body_html("<body><noscript><p>x</p></noscript>"),
            "<noscript><p>x</p></noscript>"
        );
        let doc = parse_with_options(
            "<body><noscript><p>x</p></noscript>",
            &ParseOptions { scripting: false },
        );
        let noscript = doc.first_child(doc.body().unwrap()).unwrap();
        assert!(doc.is(doc.first_child(noscript).unwrap(), "p"));
        let doc = parse_with_options(
            "<head><noscript><link><p>x</noscript>",
            &ParseOptions { scripting: false },
        );
        let noscript = doc.last_child(doc.head().unwrap()).unwrap();
        assert!(doc.is(noscript, "noscript"));
        assert!(doc.is(doc.first_child(noscript).unwrap(), "link"));
        assert!(doc.is(doc.first_child(doc.body().unwrap()).unwrap(), "p"));
    }

    #[test]
    fn template_and_foreign() {
        let doc = parse("<template><tr><td>x</td></tr></template>");
        let head = doc.head().unwrap();
        let template = doc.first_child(head).unwrap();
        let contents = doc.template_contents(template).unwrap();
        assert!(doc.is(doc.first_child(contents).unwrap(), "tr"));
        assert_eq!(serialize(&doc, template), "<tr><td>x</td></tr>");
        let doc = parse(
            "<svg viewbox='0 0 1 1'><lineargradient xlink:href='#a'/></svg><math><mi>x</mi></math>",
        );
        let body = doc.body().unwrap();
        let svg = doc.first_child(body).unwrap();
        assert!(
            matches!(doc.kind(svg), NodeKind::Element { ns: Namespace::Svg, tag, .. } if tag == "svg")
        );
        assert_eq!(doc.attr(svg, "viewBox"), Some("0 0 1 1"));
        let grad = doc.first_child(svg).unwrap();
        assert_eq!(doc.tag(grad), Some("linearGradient"));
        assert_eq!(
            foreign_attribute_namespace("xlink:href"),
            Some(("xlink", "href"))
        );
        let math = doc.last_child(body).unwrap();
        assert!(matches!(
            doc.kind(math),
            NodeKind::Element {
                ns: Namespace::MathMl,
                ..
            }
        ));
        assert_eq!(serialize(&doc, body), "<svg viewBox=\"0 0 1 1\"><linearGradient xlink:href=\"#a\"></linearGradient></svg><math><mi>x</mi></math>");
    }

    #[test]
    fn serializer_escaping() {
        let mut doc = Document::new();
        let div = doc.create_element(
            "div",
            vec![Attribute {
                name: "title".into(),
                value: "a\"b<c>&\u{A0}".into(),
            }],
        );
        let t = doc.create_text("x<y>&z\u{A0}\"");
        doc.append(div, t);
        let br = doc.create_element("br", vec![]);
        doc.append(div, br);
        let junk = doc.create_text("ignored");
        doc.append(br, junk);
        let c = doc.create(NodeKind::Comment(" c ".into()));
        doc.append(div, c);
        assert_eq!(
            serialize_node(&doc, div),
            "<div title=\"a&quot;b<c>&amp;&nbsp;\">x&lt;y&gt;&amp;z&nbsp;\"<br><!-- c --></div>"
        );
        assert_eq!(serialize(&doc, br), "");
        let doc = parse("<!DOCTYPE html><p>x");
        assert_eq!(
            serialize(&doc, Document::ROOT),
            "<!DOCTYPE html><html><head></head><body><p>x</p></body></html>"
        );
    }

    #[test]
    fn fragment_parsing() {
        let mut doc = parse("<div id=host></div><table><tbody id=tb></tbody></table>");
        let host = doc.by_id("host")[0];
        // The context element is not on the stack, so `</div>` is ignored.
        let nodes = parse_fragment(&mut doc, host, "<p>a<p>b</div>c");
        assert_eq!(nodes.len(), 2);
        for n in &nodes {
            assert!(doc.node(*n).detached);
            doc.append(host, *n);
        }
        assert_eq!(serialize(&doc, host), "<p>a</p><p>bc</p>");
        // Context rules: a table body context parses rows directly.
        let tb = doc.by_id("tb")[0];
        let rows = parse_fragment(&mut doc, tb, "<tr><td>1<td>2");
        assert_eq!(rows.len(), 1);
        doc.append(tb, rows[0]);
        assert_eq!(serialize(&doc, tb), "<tr><td>1</td><td>2</td></tr>");
        // Raw text context.
        let mut d2 = Document::new();
        let style = d2.create_element("style", vec![]);
        let kids = parse_fragment(&mut d2, style, "a<b>c</style>");
        assert_eq!(kids.len(), 1);
        assert_eq!(d2.text(kids[0]), Some("a<b>c</style>"));
        // Mutation log untouched by fragment parsing.
        let before = doc.mutations.len();
        let _ = parse_fragment(&mut doc, host, "<span>");
        assert_eq!(doc.mutations.len(), before);
    }

    #[test]
    fn deep_nesting_and_many_attributes_are_linear() {
        let mut src = String::new();
        for _ in 0..20000 {
            src.push_str("<b>");
        }
        src.push_str("<div>x</div>");
        let doc = parse(&src);
        assert!(doc.len() > 20000);
        let mut src = String::from("<p");
        for i in 0..5000 {
            src.push_str(&format!(" a{i}=v"));
        }
        src.push('>');
        let doc = parse(&src);
        let p = doc.first_child(doc.body().unwrap()).unwrap();
        assert_eq!(doc.attrs(p).len(), 5000);
    }
}
