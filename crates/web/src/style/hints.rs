//! Presentational hints: HTML attributes mapped to declarations at the
//! presentational-hint level of the cascade, per the HTML Living Standard's
//! Rendering section (§15.3 onward). Layout-only attributes (`colspan`, `rowspan`)
//! are not hints; `lang` is applied by the cascade directly to `Font.lang`.

use super::values::*;
use crate::css::token::{ComponentValue, Declaration, Number};
use crate::dom::{Document, NodeId};

fn decl(name: &str, value: Vec<ComponentValue>) -> Declaration {
    Declaration { name: name.to_owned(), value, important: false }
}

fn kw(name: &str, value: &str) -> Declaration {
    decl(name, vec![tok_ident(value)])
}

/// A "dimension value" (`width="50"`, `"50%"`, `"50px"`): a number is px, a
/// percentage is kept. Returns `None` for anything else.
pub fn parse_dimension(s: &str) -> Option<ComponentValue> {
    let s = s.trim();
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    if digits.is_empty() || digits == "." {
        return None;
    }
    let n = Number::parse(&digits)?;
    if n.is_negative() {
        return None;
    }
    let rest = s[digits.len()..].trim_start();
    if rest.starts_with('%') {
        return Some(tok_percent(n));
    }
    Some(tok_dimension(n, "px"))
}

/// A non-negative integer attribute (`border="2"`, `cellpadding="4"`).
pub fn parse_non_negative_integer(s: &str) -> Option<i64> {
    let s = s.trim();
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// `<font size>`: 1..7 absolute or a relative `+n`/`-n` from 3.
pub fn font_size_keyword(s: &str) -> Option<&'static str> {
    let s = s.trim();
    let (relative, sign, rest) = match s.chars().next()? {
        '+' => (true, 1i64, &s[1..]),
        '-' => (true, -1i64, &s[1..]),
        _ => (false, 0, s),
    };
    let n: i64 = rest.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
    let size = if relative { 3 + sign * n } else { n }.clamp(1, 7);
    Some(match size {
        1 => "x-small",
        2 => "small",
        3 => "medium",
        4 => "large",
        5 => "x-large",
        6 => "xx-large",
        _ => "xxx-large",
    })
}

fn color_decl(name: &str, value: &str) -> Option<Declaration> {
    parse_legacy_color(value).map(|c| decl(name, vec![tok_color(c)]))
}

/// The attribute names that map to hints on this element, for restyle invalidation.
pub fn is_hint_attribute(doc: &Document, node: NodeId, attr: &str) -> bool {
    let tag = doc.tag(node).unwrap_or("");
    match attr {
        "align" | "width" | "height" | "bgcolor" | "background" | "border" | "valign" | "nowrap" | "color" | "size" | "face" | "hspace" | "vspace" | "noshade" | "type" | "start"
        | "reversed" | "value" | "clear" | "dir" | "hidden" | "cols" | "rows" | "cellpadding" | "cellspacing" | "rules" | "frame" | "text" | "link" | "vlink" | "alink" | "char"
        | "charoff" | "compact" | "behavior" | "direction" | "scrollamount" | "wrap" | "span" => true,
        _ => {
            let _ = tag;
            false
        }
    }
}

/// The declarations an element's attributes imply.
pub fn presentational_hints(doc: &Document, node: NodeId) -> Vec<Declaration> {
    let Some(tag) = doc.tag(node) else { return Vec::new() };
    let mut out = Vec::new();
    let attr = |n: &str| doc.attr(node, n);
    // `dir` on any element.
    if let Some(d) = attr("dir") {
        match d.trim().to_ascii_lowercase().as_str() {
            "ltr" => out.push(kw("direction", "ltr")),
            "rtl" => out.push(kw("direction", "rtl")),
            _ => {}
        }
    }
    match tag {
        "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "caption" | "legend" | "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" | "col" | "colgroup" => {
            if let Some(a) = attr("align") {
                let a = a.trim().to_ascii_lowercase();
                let is_cell = matches!(tag, "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" | "col" | "colgroup");
                match a.as_str() {
                    "left" => out.push(kw("text-align", "left")),
                    "right" => out.push(kw("text-align", "right")),
                    "center" | "middle" => out.push(kw("text-align", "center")),
                    "justify" => out.push(kw("text-align", "justify")),
                    "start" | "end" if !is_cell => out.push(kw("text-align", &a)),
                    _ => {}
                }
            }
            if is_table_part(tag) {
                table_cell_hints(doc, node, tag, &mut out);
            }
        }
        "table" => {
            if let Some(a) = attr("align") {
                match a.trim().to_ascii_lowercase().as_str() {
                    "left" => out.push(kw("float", "left")),
                    "right" => out.push(kw("float", "right")),
                    "center" => {
                        out.push(kw("margin-left", "auto"));
                        out.push(kw("margin-right", "auto"));
                    }
                    _ => {}
                }
            }
            if let Some(w) = attr("width").and_then(parse_dimension) {
                out.push(decl("width", vec![w]));
            }
            if let Some(h) = attr("height").and_then(parse_dimension) {
                out.push(decl("height", vec![h]));
            }
            if let Some(c) = attr("bgcolor").and_then(|v| color_decl("background-color", v)) {
                out.push(c);
            }
            if let Some(b) = attr("background") {
                out.push(decl("background-image", vec![ComponentValue::Token(crate::css::token::Token::Url(b.trim().to_owned()))]));
            }
            if let Some(cs) = attr("cellspacing").and_then(parse_dimension) {
                out.push(decl("border-spacing", vec![cs]));
            }
            // `border`: a non-zero value gives the table an outset border and its
            // cells inset borders (the cell rules are in `table_cell_hints`).
            let border = attr("border").map(|b| parse_non_negative_integer(b).unwrap_or(1));
            if let Some(b) = border {
                if b > 0 {
                    out.push(decl("border-top-width", vec![tok_px(b)]));
                    out.push(decl("border-right-width", vec![tok_px(b)]));
                    out.push(decl("border-bottom-width", vec![tok_px(b)]));
                    out.push(decl("border-left-width", vec![tok_px(b)]));
                    out.push(kw("border-top-style", "outset"));
                    out.push(kw("border-right-style", "outset"));
                    out.push(kw("border-bottom-style", "outset"));
                    out.push(kw("border-left-style", "outset"));
                }
            }
            // `frame`: which outer sides show.
            if let Some(f) = attr("frame") {
                let f = f.trim().to_ascii_lowercase();
                let sides: (bool, bool, bool, bool) = match f.as_str() {
                    "void" => (false, false, false, false),
                    "above" => (true, false, false, false),
                    "below" => (false, false, true, false),
                    "hsides" => (true, false, true, false),
                    "lhs" => (false, false, false, true),
                    "rhs" => (false, true, false, false),
                    "vsides" => (false, true, false, true),
                    "box" | "border" => (true, true, true, true),
                    _ => (true, true, true, true),
                };
                for (name, on) in [("border-top-style", sides.0), ("border-right-style", sides.1), ("border-bottom-style", sides.2), ("border-left-style", sides.3)] {
                    out.push(kw(name, if on { "solid" } else { "hidden" }));
                }
            }
            if let Some(r) = attr("rules") {
                let r = r.trim().to_ascii_lowercase();
                if matches!(r.as_str(), "none" | "groups" | "rows" | "cols" | "all") {
                    out.push(kw("border-collapse", "collapse"));
                }
            }
            if let Some(hs) = attr("hspace").and_then(parse_dimension) {
                out.push(decl("margin-left", vec![hs.clone()]));
                out.push(decl("margin-right", vec![hs]));
            }
            if let Some(vs) = attr("vspace").and_then(parse_dimension) {
                out.push(decl("margin-top", vec![vs.clone()]));
                out.push(decl("margin-bottom", vec![vs]));
            }
        }
        "body" => {
            if let Some(c) = attr("bgcolor").and_then(|v| color_decl("background-color", v)) {
                out.push(c);
            }
            if let Some(c) = attr("text").and_then(|v| color_decl("color", v)) {
                out.push(c);
            }
            if let Some(b) = attr("background") {
                out.push(decl("background-image", vec![ComponentValue::Token(crate::css::token::Token::Url(b.trim().to_owned()))]));
            }
            // `link`/`vlink`/`alink` colour anchors; the UA sheet's `a:link` rules are
            // at the same level, so the cascade applies them through `body_link_colors`.
            for (a, name) in [("marginwidth", "margin-left"), ("marginwidth", "margin-right"), ("marginheight", "margin-top"), ("marginheight", "margin-bottom"), ("leftmargin", "margin-left"), ("rightmargin", "margin-right"), ("topmargin", "margin-top"), ("bottommargin", "margin-bottom")] {
                if let Some(v) = attr(a).and_then(parse_dimension) {
                    out.push(decl(name, vec![v]));
                }
            }
        }
        "img" | "iframe" | "embed" | "object" | "video" | "canvas" | "input" | "audio" | "source" => {
            let is_input = tag == "input";
            let input_type = attr("type").map(|t| t.trim().to_ascii_lowercase()).unwrap_or_else(|| "text".into());
            let sized_input = is_input && matches!(input_type.as_str(), "image");
            if !is_input || sized_input {
                if let Some(w) = attr("width").and_then(parse_dimension) {
                    out.push(decl("width", vec![w]));
                }
                if let Some(h) = attr("height").and_then(parse_dimension) {
                    out.push(decl("height", vec![h]));
                }
            }
            if is_input && matches!(input_type.as_str(), "text" | "search" | "url" | "tel" | "email" | "password" | "number") {
                if let Some(size) = attr("size").and_then(parse_non_negative_integer).filter(|n| *n > 0) {
                    out.push(decl("width", vec![tok_dimension(Number::from_i64(size), "ch")]));
                }
            }
            if let Some(hs) = attr("hspace").and_then(parse_dimension) {
                out.push(decl("margin-left", vec![hs.clone()]));
                out.push(decl("margin-right", vec![hs]));
            }
            if let Some(vs) = attr("vspace").and_then(parse_dimension) {
                out.push(decl("margin-top", vec![vs.clone()]));
                out.push(decl("margin-bottom", vec![vs]));
            }
            if let Some(b) = attr("border").and_then(parse_non_negative_integer) {
                if tag == "img" || tag == "object" || tag == "embed" || sized_input {
                    for side in ["top", "right", "bottom", "left"] {
                        out.push(decl(&format!("border-{side}-width"), vec![tok_px(b)]));
                        out.push(kw(&format!("border-{side}-style"), "solid"));
                    }
                }
            }
            if let Some(a) = attr("align") {
                match a.trim().to_ascii_lowercase().as_str() {
                    "left" => out.push(kw("float", "left")),
                    "right" => out.push(kw("float", "right")),
                    "top" => out.push(kw("vertical-align", "top")),
                    "middle" => out.push(kw("vertical-align", "middle")),
                    "bottom" => out.push(kw("vertical-align", "baseline")),
                    "center" | "abscenter" | "absmiddle" => out.push(kw("vertical-align", "middle")),
                    "texttop" => out.push(kw("vertical-align", "text-top")),
                    "baseline" => out.push(kw("vertical-align", "baseline")),
                    _ => {}
                }
            }
            if tag == "iframe" {
                if let Some(fb) = attr("frameborder") {
                    if fb.trim() == "0" || fb.trim().eq_ignore_ascii_case("no") {
                        for side in ["top", "right", "bottom", "left"] {
                            out.push(decl(&format!("border-{side}-width"), vec![tok_px(0)]));
                        }
                    }
                }
            }
        }
        "hr" => {
            if let Some(a) = attr("align") {
                match a.trim().to_ascii_lowercase().as_str() {
                    "left" => {
                        out.push(kw("margin-left", "0"));
                        out.push(kw("margin-right", "auto"));
                    }
                    "right" => {
                        out.push(kw("margin-left", "auto"));
                        out.push(kw("margin-right", "0"));
                    }
                    "center" => {
                        out.push(kw("margin-left", "auto"));
                        out.push(kw("margin-right", "auto"));
                    }
                    _ => {}
                }
            }
            if let Some(w) = attr("width").and_then(parse_dimension) {
                out.push(decl("width", vec![w]));
            }
            if let Some(c) = attr("color").and_then(|v| color_decl("border-top-color", v)) {
                out.push(c.clone());
                out.push(decl("border-right-color", c.value.clone()));
                out.push(decl("border-bottom-color", c.value.clone()));
                out.push(decl("border-left-color", c.value.clone()));
                out.push(decl("background-color", c.value));
            }
            let noshade = doc.has_attr(node, "noshade");
            if noshade || attr("color").is_some() {
                for side in ["top", "right", "bottom", "left"] {
                    out.push(kw(&format!("border-{side}-style"), "solid"));
                }
            }
            if let Some(size) = attr("size").and_then(parse_non_negative_integer) {
                if noshade || attr("color").is_some() {
                    out.push(decl("height", vec![tok_px(size)]));
                    out.push(decl("border-top-width", vec![tok_px(0)]));
                    out.push(decl("border-right-width", vec![tok_px(0)]));
                    out.push(decl("border-bottom-width", vec![tok_px(0)]));
                    out.push(decl("border-left-width", vec![tok_px(0)]));
                } else if size == 1 {
                    out.push(decl("border-bottom-width", vec![tok_px(0)]));
                } else if size >= 2 {
                    out.push(decl("height", vec![tok_px(size - 2)]));
                }
            }
        }
        "font" => {
            if let Some(c) = attr("color").and_then(|v| color_decl("color", v)) {
                out.push(c);
            }
            if let Some(s) = attr("size").and_then(font_size_keyword) {
                out.push(kw("font-size", s));
            }
            if let Some(f) = attr("face") {
                let families: Vec<ComponentValue> = f
                    .split(',')
                    .map(|n| n.trim())
                    .filter(|n| !n.is_empty())
                    .enumerate()
                    .flat_map(|(i, n)| {
                        let mut v = Vec::new();
                        if i > 0 {
                            v.push(tok_comma());
                        }
                        v.push(tok_string(n));
                        v
                    })
                    .collect();
                if !families.is_empty() {
                    out.push(decl("font-family", families));
                }
            }
        }
        "ol" => {
            if let Some(t) = attr("type") {
                let ty = match t.trim() {
                    "1" => Some("decimal"),
                    "a" => Some("lower-alpha"),
                    "A" => Some("upper-alpha"),
                    "i" => Some("lower-roman"),
                    "I" => Some("upper-roman"),
                    _ => None,
                };
                if let Some(ty) = ty {
                    out.push(kw("list-style-type", ty));
                }
            }
            // `start` and `reversed` are counters: layout numbers the items; the
            // reset is expressed so `counter-reset` carries the start value.
            if let Some(start) = attr("start").and_then(|s| s.trim().parse::<i64>().ok()) {
                out.push(decl("counter-reset", vec![tok_ident("list-item"), tok_ws(), tok_int(start - 1)]));
            }
            if doc.has_attr(node, "reversed") {
                out.push(decl("counter-increment", vec![tok_ident("list-item"), tok_ws(), tok_int(-1)]));
            }
            if doc.has_attr(node, "compact") {
                out.push(decl("padding-left", vec![tok_px(20)]));
            }
        }
        "ul" | "menu" | "dir" => {
            if let Some(t) = attr("type") {
                let ty = match t.trim().to_ascii_lowercase().as_str() {
                    "disc" => Some("disc"),
                    "circle" | "round" => Some("circle"),
                    "square" => Some("square"),
                    "none" => Some("none"),
                    _ => None,
                };
                if let Some(ty) = ty {
                    out.push(kw("list-style-type", ty));
                }
            }
            if doc.has_attr(node, "compact") {
                out.push(decl("padding-left", vec![tok_px(20)]));
            }
        }
        "li" => {
            if let Some(t) = attr("type") {
                let ty = match t.trim() {
                    "1" => Some("decimal"),
                    "a" => Some("lower-alpha"),
                    "A" => Some("upper-alpha"),
                    "i" => Some("lower-roman"),
                    "I" => Some("upper-roman"),
                    "disc" => Some("disc"),
                    "circle" => Some("circle"),
                    "square" => Some("square"),
                    _ => None,
                };
                if let Some(ty) = ty {
                    out.push(kw("list-style-type", ty));
                }
            }
            if let Some(v) = attr("value").and_then(|s| s.trim().parse::<i64>().ok()) {
                out.push(decl("counter-reset", vec![tok_ident("list-item"), tok_ws(), tok_int(v - 1)]));
            }
        }
        "br" => {
            if let Some(c) = attr("clear") {
                match c.trim().to_ascii_lowercase().as_str() {
                    "left" => out.push(kw("clear", "left")),
                    "right" => out.push(kw("clear", "right")),
                    "all" | "both" => out.push(kw("clear", "both")),
                    _ => {}
                }
            }
        }
        "textarea" => {
            if let Some(cols) = attr("cols").and_then(parse_non_negative_integer).filter(|n| *n > 0) {
                out.push(decl("width", vec![tok_dimension(Number::from_i64(cols), "ch")]));
            }
            if let Some(rows) = attr("rows").and_then(parse_non_negative_integer).filter(|n| *n > 0) {
                out.push(decl("height", vec![tok_dimension(Number::from_i64(rows), "lh")]));
            }
            if let Some(w) = attr("wrap") {
                if w.trim().eq_ignore_ascii_case("off") {
                    out.push(kw("white-space", "pre"));
                }
            }
        }
        "marquee" => {
            if let Some(w) = attr("width").and_then(parse_dimension) {
                out.push(decl("width", vec![w]));
            }
            if let Some(h) = attr("height").and_then(parse_dimension) {
                out.push(decl("height", vec![h]));
            }
            if let Some(c) = attr("bgcolor").and_then(|v| color_decl("background-color", v)) {
                out.push(c);
            }
            if let Some(hs) = attr("hspace").and_then(parse_dimension) {
                out.push(decl("margin-left", vec![hs.clone()]));
                out.push(decl("margin-right", vec![hs]));
            }
            if let Some(vs) = attr("vspace").and_then(parse_dimension) {
                out.push(decl("margin-top", vec![vs.clone()]));
                out.push(decl("margin-bottom", vec![vs]));
            }
        }
        "pre" | "listing" | "xmp" | "plaintext" => {
            if let Some(w) = attr("width").and_then(parse_non_negative_integer) {
                out.push(decl("width", vec![tok_dimension(Number::from_i64(w), "ch")]));
            }
        }
        "select" => {
            if let Some(size) = attr("size").and_then(parse_non_negative_integer).filter(|n| *n > 1) {
                out.push(decl("height", vec![tok_dimension(Number::from_i64(size), "lh")]));
            }
        }
        _ => {}
    }
    out
}

fn is_table_part(tag: &str) -> bool {
    matches!(tag, "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" | "col" | "colgroup" | "caption")
}

fn table_cell_hints(doc: &Document, node: NodeId, tag: &str, out: &mut Vec<Declaration>) {
    let attr = |n: &str| doc.attr(node, n);
    if matches!(tag, "td" | "th" | "col" | "colgroup") {
        if let Some(w) = attr("width").and_then(parse_dimension) {
            out.push(decl("width", vec![w]));
        }
    }
    if matches!(tag, "td" | "th" | "tr") {
        if let Some(h) = attr("height").and_then(parse_dimension) {
            out.push(decl("height", vec![h]));
        }
    }
    if let Some(c) = attr("bgcolor").and_then(|v| color_decl("background-color", v)) {
        out.push(c);
    }
    if let Some(b) = attr("background") {
        out.push(decl("background-image", vec![ComponentValue::Token(crate::css::token::Token::Url(b.trim().to_owned()))]));
    }
    if let Some(v) = attr("valign") {
        match v.trim().to_ascii_lowercase().as_str() {
            "top" => out.push(kw("vertical-align", "top")),
            "middle" | "center" => out.push(kw("vertical-align", "middle")),
            "bottom" => out.push(kw("vertical-align", "bottom")),
            "baseline" => out.push(kw("vertical-align", "baseline")),
            _ => {}
        }
    }
    if matches!(tag, "td" | "th") {
        if doc.has_attr(node, "nowrap") {
            out.push(kw("white-space", "nowrap"));
        }
        // The enclosing table's `border`, `cellpadding` and `rules` attributes.
        let table = doc.ancestors(node).find(|a| doc.is(*a, "table"));
        if let Some(t) = table {
            if let Some(cp) = doc.attr(t, "cellpadding").and_then(parse_dimension) {
                for side in ["top", "right", "bottom", "left"] {
                    out.push(decl(&format!("padding-{side}"), vec![cp.clone()]));
                }
            }
            let border = doc.attr(t, "border").map(|b| parse_non_negative_integer(b).unwrap_or(1)).unwrap_or(0);
            let rules = doc.attr(t, "rules").map(|r| r.trim().to_ascii_lowercase());
            if border > 0 || rules.is_some() {
                let style = if rules.is_some() { "solid" } else { "inset" };
                let (h, v) = match rules.as_deref() {
                    Some("none") => (false, false),
                    Some("rows") => (true, false),
                    Some("cols") => (false, true),
                    Some("groups") => (false, false),
                    _ => (true, true),
                };
                for (side, on) in [("top", h), ("bottom", h), ("left", v), ("right", v)] {
                    if on {
                        out.push(decl(&format!("border-{side}-width"), vec![tok_px(1)]));
                        out.push(kw(&format!("border-{side}-style"), style));
                    }
                }
            }
        }
    }
}

/// `<body link vlink alink>`: the colours for anchors, applied by the cascade's UA
/// rules through these declarations on matching `a` elements.
pub fn body_link_color(doc: &Document, node: NodeId, state: &str) -> Option<Declaration> {
    if !doc.is(node, "a") || !doc.has_attr(node, "href") {
        return None;
    }
    let body = doc.body()?;
    color_decl("color", doc.attr(body, state)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::Attribute;

    fn el(tag: &str, attrs: &[(&str, &str)]) -> (Document, NodeId) {
        let mut d = Document::new();
        let html = d.create_element("html", vec![]);
        d.append(Document::ROOT, html);
        let body = d.create_element("body", vec![]);
        d.append(html, body);
        let n = d.create_element(tag, attrs.iter().map(|(k, v)| Attribute { name: k.to_string(), value: v.to_string() }).collect());
        d.append(body, n);
        (d, n)
    }

    fn names(doc: &Document, n: NodeId) -> Vec<(String, String)> {
        presentational_hints(doc, n).into_iter().map(|d| (d.name, serialize_component_values(&d.value))).collect()
    }

    #[test]
    fn dimensions_and_integers() {
        assert_eq!(parse_dimension("50"), Some(tok_px(50)));
        assert_eq!(parse_dimension("50%"), Some(tok_percent(Number::from_i64(50))));
        assert_eq!(parse_dimension(" 12.5px"), Some(tok_dimension(Number { micro: 12_500_000, int: false }, "px")));
        assert_eq!(parse_dimension("abc"), None);
        assert_eq!(parse_dimension("-5"), None);
        assert_eq!(parse_non_negative_integer("3px"), Some(3));
        assert_eq!(parse_non_negative_integer("x"), None);
        assert_eq!(font_size_keyword("1"), Some("x-small"));
        assert_eq!(font_size_keyword("7"), Some("xxx-large"));
        assert_eq!(font_size_keyword("+1"), Some("large"));
        assert_eq!(font_size_keyword("-2"), Some("x-small"));
        assert_eq!(font_size_keyword("9"), Some("xxx-large"));
        assert_eq!(font_size_keyword("q"), None);
    }

    #[test]
    fn alignment_hints() {
        let (d, n) = el("p", &[("align", "CENTER")]);
        assert_eq!(names(&d, n), vec![("text-align".to_string(), "center".to_string())]);
        let (d, n) = el("table", &[("align", "center"), ("width", "80%"), ("border", "1"), ("cellspacing", "0")]);
        let h = names(&d, n);
        assert!(h.contains(&("margin-left".into(), "auto".into())));
        assert!(h.contains(&("width".into(), "80%".into())));
        assert!(h.contains(&("border-top-style".into(), "outset".into())));
        assert!(h.contains(&("border-spacing".into(), "0px".into())));
        let (d, n) = el("img", &[("align", "left"), ("width", "10"), ("hspace", "4")]);
        let h = names(&d, n);
        assert!(h.contains(&("float".into(), "left".into())));
        assert!(h.contains(&("width".into(), "10px".into())));
        assert!(h.contains(&("margin-right".into(), "4px".into())));
        let (d, n) = el("img", &[("align", "middle")]);
        assert!(names(&d, n).contains(&("vertical-align".into(), "middle".into())));
    }

    #[test]
    fn colours_fonts_lists() {
        let (d, n) = el("font", &[("color", "red"), ("size", "+2"), ("face", "Arial, Helvetica")]);
        let h = names(&d, n);
        assert_eq!(h[0].0, "color");
        assert!(h.contains(&("font-size".into(), "x-large".into())));
        assert!(h.contains(&("font-family".into(), "\"Arial\",\"Helvetica\"".into())));
        let (d, n) = el("body", &[("bgcolor", "#ff0"), ("text", "blue")]);
        let h = names(&d, n);
        assert_eq!(h[0].0, "background-color");
        assert_eq!(h[1].0, "color");
        let (d, n) = el("ol", &[("type", "I"), ("start", "5")]);
        let h = names(&d, n);
        assert!(h.contains(&("list-style-type".into(), "upper-roman".into())));
        assert!(h.contains(&("counter-reset".into(), "list-item 4".into())));
        let (d, n) = el("ul", &[("type", "square")]);
        assert!(names(&d, n).contains(&("list-style-type".into(), "square".into())));
        let (d, n) = el("li", &[("value", "3")]);
        assert!(names(&d, n).contains(&("counter-reset".into(), "list-item 2".into())));
        let (d, n) = el("br", &[("clear", "all")]);
        assert!(names(&d, n).contains(&("clear".into(), "both".into())));
        let (d, n) = el("hr", &[("size", "4"), ("noshade", ""), ("color", "gray")]);
        let h = names(&d, n);
        assert!(h.contains(&("height".into(), "4px".into())));
        assert!(h.contains(&("border-top-style".into(), "solid".into())));
    }

    #[test]
    fn form_sizes_and_direction() {
        let (d, n) = el("input", &[("size", "20"), ("type", "text")]);
        assert!(names(&d, n).contains(&("width".into(), "20ch".into())));
        let (d, n) = el("input", &[("size", "20"), ("type", "checkbox")]);
        assert!(!names(&d, n).iter().any(|(k, _)| k == "width"));
        let (d, n) = el("textarea", &[("cols", "40"), ("rows", "5")]);
        let h = names(&d, n);
        assert!(h.contains(&("width".into(), "40ch".into())));
        assert!(h.contains(&("height".into(), "5lh".into())));
        let (d, n) = el("span", &[("dir", "rtl")]);
        assert_eq!(names(&d, n), vec![("direction".to_string(), "rtl".to_string())]);
        let (d, n) = el("td", &[("valign", "top"), ("nowrap", ""), ("bgcolor", "silver")]);
        let h = names(&d, n);
        assert!(h.contains(&("vertical-align".into(), "top".into())));
        assert!(h.contains(&("white-space".into(), "nowrap".into())));
    }

    #[test]
    fn table_border_reaches_cells() {
        let mut d = Document::new();
        let html = d.create_element("html", vec![]);
        d.append(Document::ROOT, html);
        let table = d.create_element("table", vec![Attribute { name: "border".into(), value: "1".into() }, Attribute { name: "cellpadding".into(), value: "3".into() }]);
        d.append(html, table);
        let tr = d.create_element("tr", vec![]);
        d.append(table, tr);
        let td = d.create_element("td", vec![]);
        d.append(tr, td);
        let h = names(&d, td);
        assert!(h.contains(&("padding-left".into(), "3px".into())));
        assert!(h.contains(&("border-left-width".into(), "1px".into())));
        assert!(h.contains(&("border-left-style".into(), "inset".into())));
        assert!(is_hint_attribute(&d, table, "border"));
        assert!(!is_hint_attribute(&d, table, "data-x"));
    }
}
