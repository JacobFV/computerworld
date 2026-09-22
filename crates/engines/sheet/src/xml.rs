//! A small XML reader and escaping helpers, enough for SpreadsheetML and OpenDocument.
//! Namespaces are kept as prefixes; lookups match on local names.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}
impl Element {
    pub fn local(&self) -> &str {
        local(&self.name)
    }
    /// Attribute by local name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| local(k) == name || k == name)
            .map(|(_, v)| v.as_str())
    }
    /// Attribute by its full prefixed name.
    pub fn attr_exact(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.local() == name)
    }
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements().filter(move |e| e.local() == name)
    }
    /// All text beneath this element, concatenated.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for n in &self.children {
            match n {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => out.push_str(&e.text()),
            }
        }
        out
    }
    /// The first descendant with this local name, depth first.
    pub fn find(&self, name: &str) -> Option<&Element> {
        for e in self.elements() {
            if e.local() == name {
                return Some(e);
            }
            if let Some(f) = e.find(name) {
                return Some(f);
            }
        }
        None
    }
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Characters XML 1.0 cannot carry are dropped.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
/// Parse a document and return its root element.
pub fn parse(text: &str) -> Result<Element, String> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut stack: Vec<Element> = vec![Element::default()];
    while i < b.len() {
        if b[i] == b'<' {
            if text[i..].starts_with("<?") {
                i = text[i..]
                    .find("?>")
                    .map(|p| i + p + 2)
                    .ok_or("unterminated declaration")?;
            } else if text[i..].starts_with("<!--") {
                i = text[i..]
                    .find("-->")
                    .map(|p| i + p + 3)
                    .ok_or("unterminated comment")?;
            } else if text[i..].starts_with("<![CDATA[") {
                let end = text[i..]
                    .find("]]>")
                    .map(|p| i + p)
                    .ok_or("unterminated CDATA")?;
                stack
                    .last_mut()
                    .unwrap()
                    .children
                    .push(Node::Text(text[i + 9..end].to_owned()));
                i = end + 3;
            } else if text[i..].starts_with("<!") {
                i = text[i..]
                    .find('>')
                    .map(|p| i + p + 1)
                    .ok_or("unterminated declaration")?;
            } else if text[i..].starts_with("</") {
                let end = text[i..]
                    .find('>')
                    .map(|p| i + p)
                    .ok_or("unterminated end tag")?;
                let name = text[i + 2..end].trim();
                let done = stack.pop().ok_or("unbalanced end tag")?;
                if done.name != name {
                    return Err(format!("expected </{}>, found </{name}>", done.name));
                }
                stack
                    .last_mut()
                    .ok_or("unbalanced end tag")?
                    .children
                    .push(Node::Element(done));
                i = end + 1;
            } else {
                // Start tag: name, attributes, and maybe a self-close.
                let mut j = i + 1;
                let mut quote: Option<u8> = None;
                while j < b.len() {
                    match (quote, b[j]) {
                        (Some(q), c) if c == q => quote = None,
                        (None, b'"' | b'\'') => quote = Some(b[j]),
                        (None, b'>') => break,
                        _ => {}
                    }
                    j += 1;
                }
                if j >= b.len() {
                    return Err("unterminated start tag".into());
                }
                let inner = &text[i + 1..j];
                let self_closing = inner.ends_with('/');
                let inner = inner.trim_end_matches('/');
                let name_end = inner
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(inner.len());
                let mut el = Element {
                    name: inner[..name_end].to_owned(),
                    ..Element::default()
                };
                let mut rest = inner[name_end..].trim_start();
                while !rest.is_empty() {
                    let eq = rest.find('=').ok_or("attribute without a value")?;
                    let key = rest[..eq].trim().to_owned();
                    let after = rest[eq + 1..].trim_start();
                    let q = after.chars().next().ok_or("attribute without a value")?;
                    if q != '"' && q != '\'' {
                        return Err("unquoted attribute".into());
                    }
                    let close = after[1..].find(q).ok_or("unterminated attribute")?;
                    el.attrs.push((key, unescape(&after[1..1 + close])));
                    rest = after[close + 2..].trim_start();
                }
                if self_closing {
                    stack.last_mut().unwrap().children.push(Node::Element(el));
                } else {
                    stack.push(el);
                }
                i = j + 1;
            }
        } else {
            let end = text[i..].find('<').map_or(b.len(), |p| i + p);
            let t = unescape(&text[i..end]);
            if !t.is_empty() {
                stack.last_mut().unwrap().children.push(Node::Text(t));
            }
            i = end;
        }
    }
    if stack.len() != 1 {
        return Err("the document ends inside an element".into());
    }
    let root = stack.pop().unwrap();
    root.children
        .into_iter()
        .find_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
        .ok_or_else(|| "the document has no root element".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_elements_attributes_text_and_entities() {
        let doc = parse("<?xml version=\"1.0\"?><x:a xmlns:x=\"u\" k='v &amp; w'><b>1 &lt; 2</b><c/><![CDATA[<raw>]]></x:a>").unwrap();
        assert_eq!(doc.local(), "a");
        assert_eq!(doc.attr("k"), Some("v & w"));
        assert_eq!(doc.child("b").unwrap().text(), "1 < 2");
        assert!(doc.child("c").is_some());
        assert_eq!(doc.text(), "1 < 2<raw>");
        assert_eq!(escape("a<b&\"c\""), "a&lt;b&amp;&quot;c&quot;");
        assert!(parse("<a><b></a>").is_err());
    }
}
