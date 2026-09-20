//! The HTML tree builder: WHATWG HTML §13.2.6, every insertion mode, the stack of open
//! elements with its scopes, the list of active formatting elements with the adoption
//! agency algorithm, foster parenting, `<template>` contents, and foreign content.
//!
//! The spec as of 2026 no longer has "in select" and "in select in table" insertion
//! modes: `<select>` content is parsed in body, with `select` in the default scope list
//! and the option/optgroup/hr/input rules checking for a select in scope. This follows
//! that (the conformance suite requires it). Processing instructions are the one recent
//! addition not followed: the DOM has no such node, so `<?...>` is a bogus comment as
//! before.
//!
//! No scripts run here. The scripting flag only decides how `<noscript>` parses.

use super::tokenizer::{Doctype, State, Tag, Token, Tokenizer};
use crate::dom::{Attribute, Document, Namespace, NodeId, NodeKind, QuirksMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InTemplate,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharKind {
    Whitespace,
    Null,
    Other,
}

/// A token as the tree builder sees it: character runs are split by kind so each run
/// takes one branch of the rules.
#[derive(Debug)]
enum Tok {
    Doctype(Doctype),
    Start(Tag),
    End(Tag),
    Comment(String),
    Chars(String, CharKind),
    Eof,
}

enum Afe {
    Marker,
    Element { node: NodeId, tag: Tag },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Default,
    ListItem,
    Button,
    Table,
}

fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | ' ')
}

fn char_kind(c: char) -> CharKind {
    if c == '\0' {
        CharKind::Null
    } else if is_ws(c) {
        CharKind::Whitespace
    } else {
        CharKind::Other
    }
}

fn is_special(ns: Namespace, tag: &str) -> bool {
    match ns {
        Namespace::Html => matches!(
            tag,
            "address" | "applet" | "area" | "article" | "aside" | "base" | "basefont" | "bgsound" | "blockquote" | "body" | "br" | "button" | "caption" | "center" | "col" | "colgroup" | "dd" | "details" | "dir" | "div" | "dl" | "dt" | "embed" | "fieldset" | "figcaption" | "figure" | "footer" | "form" | "frame" | "frameset" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "head" | "header" | "hgroup" | "hr" | "html" | "iframe" | "img" | "input" | "keygen" | "li" | "link" | "listing" | "main" | "marquee" | "menu" | "meta" | "nav" | "noembed" | "noframes" | "noscript" | "object" | "ol" | "p" | "param" | "plaintext" | "pre" | "script" | "search" | "section" | "select" | "source" | "style" | "summary" | "table" | "tbody" | "td" | "template" | "textarea" | "tfoot" | "th" | "thead" | "title" | "tr" | "track" | "ul" | "wbr" | "xmp"
        ),
        Namespace::MathMl => matches!(tag, "mi" | "mo" | "mn" | "ms" | "mtext" | "annotation-xml"),
        Namespace::Svg => matches!(tag, "foreignObject" | "desc" | "title"),
    }
}

fn is_heading(tag: &str) -> bool {
    matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

const IMPLIED_END: &[&str] = &["dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc"];
const IMPLIED_END_THOROUGH: &[&str] = &["caption", "colgroup", "dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc", "tbody", "td", "tfoot", "th", "thead", "tr"];

fn svg_tag_adjust(name: &str) -> Option<&'static str> {
    Some(match name {
        "altglyph" => "altGlyph",
        "altglyphdef" => "altGlyphDef",
        "altglyphitem" => "altGlyphItem",
        "animatecolor" => "animateColor",
        "animatemotion" => "animateMotion",
        "animatetransform" => "animateTransform",
        "clippath" => "clipPath",
        "feblend" => "feBlend",
        "fecolormatrix" => "feColorMatrix",
        "fecomponenttransfer" => "feComponentTransfer",
        "fecomposite" => "feComposite",
        "feconvolvematrix" => "feConvolveMatrix",
        "fediffuselighting" => "feDiffuseLighting",
        "fedisplacementmap" => "feDisplacementMap",
        "fedistantlight" => "feDistantLight",
        "fedropshadow" => "feDropShadow",
        "feflood" => "feFlood",
        "fefunca" => "feFuncA",
        "fefuncb" => "feFuncB",
        "fefuncg" => "feFuncG",
        "fefuncr" => "feFuncR",
        "fegaussianblur" => "feGaussianBlur",
        "feimage" => "feImage",
        "femerge" => "feMerge",
        "femergenode" => "feMergeNode",
        "femorphology" => "feMorphology",
        "feoffset" => "feOffset",
        "fepointlight" => "fePointLight",
        "fespecularlighting" => "feSpecularLighting",
        "fespotlight" => "feSpotLight",
        "fetile" => "feTile",
        "feturbulence" => "feTurbulence",
        "foreignobject" => "foreignObject",
        "glyphref" => "glyphRef",
        "lineargradient" => "linearGradient",
        "radialgradient" => "radialGradient",
        "textpath" => "textPath",
        _ => return None,
    })
}

fn svg_attr_adjust(name: &str) -> Option<&'static str> {
    Some(match name {
        "attributename" => "attributeName",
        "attributetype" => "attributeType",
        "basefrequency" => "baseFrequency",
        "baseprofile" => "baseProfile",
        "calcmode" => "calcMode",
        "clippathunits" => "clipPathUnits",
        "diffuseconstant" => "diffuseConstant",
        "edgemode" => "edgeMode",
        "filterunits" => "filterUnits",
        "glyphref" => "glyphRef",
        "gradienttransform" => "gradientTransform",
        "gradientunits" => "gradientUnits",
        "kernelmatrix" => "kernelMatrix",
        "kernelunitlength" => "kernelUnitLength",
        "keypoints" => "keyPoints",
        "keysplines" => "keySplines",
        "keytimes" => "keyTimes",
        "lengthadjust" => "lengthAdjust",
        "limitingconeangle" => "limitingConeAngle",
        "markerheight" => "markerHeight",
        "markerunits" => "markerUnits",
        "markerwidth" => "markerWidth",
        "maskcontentunits" => "maskContentUnits",
        "maskunits" => "maskUnits",
        "numoctaves" => "numOctaves",
        "pathlength" => "pathLength",
        "patterncontentunits" => "patternContentUnits",
        "patterntransform" => "patternTransform",
        "patternunits" => "patternUnits",
        "pointsatx" => "pointsAtX",
        "pointsaty" => "pointsAtY",
        "pointsatz" => "pointsAtZ",
        "preservealpha" => "preserveAlpha",
        "preserveaspectratio" => "preserveAspectRatio",
        "primitiveunits" => "primitiveUnits",
        "refx" => "refX",
        "refy" => "refY",
        "repeatcount" => "repeatCount",
        "repeatdur" => "repeatDur",
        "requiredextensions" => "requiredExtensions",
        "requiredfeatures" => "requiredFeatures",
        "specularconstant" => "specularConstant",
        "specularexponent" => "specularExponent",
        "spreadmethod" => "spreadMethod",
        "startoffset" => "startOffset",
        "stddeviation" => "stdDeviation",
        "stitchtiles" => "stitchTiles",
        "surfacescale" => "surfaceScale",
        "systemlanguage" => "systemLanguage",
        "tablevalues" => "tableValues",
        "targetx" => "targetX",
        "targety" => "targetY",
        "textlength" => "textLength",
        "viewbox" => "viewBox",
        "viewtarget" => "viewTarget",
        "xchannelselector" => "xChannelSelector",
        "ychannelselector" => "yChannelSelector",
        "zoomandpan" => "zoomAndPan",
        _ => return None,
    })
}

/// The namespaced attributes of foreign elements (`xlink:href`, `xml:lang`, `xmlns`,
/// `xmlns:xlink`). They are stored under their qualified name; this reports the
/// `(prefix, local name)` split for consumers that need the namespace.
pub fn foreign_attribute_namespace(qualified: &str) -> Option<(&'static str, &str)> {
    match qualified {
        "xlink:actuate" | "xlink:arcrole" | "xlink:href" | "xlink:role" | "xlink:show" | "xlink:title" | "xlink:type" => Some(("xlink", &qualified[6..])),
        "xml:lang" | "xml:space" => Some(("xml", &qualified[4..])),
        "xmlns" => Some(("xmlns", "xmlns")),
        "xmlns:xlink" => Some(("xmlns", "xlink")),
        _ => None,
    }
}

fn adjust_mathml_attrs(tag: &mut Tag) {
    for a in &mut tag.attrs {
        if a.name == "definitionurl" {
            a.name = "definitionURL".to_owned();
        }
    }
}

fn adjust_svg_attrs(tag: &mut Tag) {
    for a in &mut tag.attrs {
        if let Some(n) = svg_attr_adjust(&a.name) {
            a.name = n.to_owned();
        }
    }
}

fn same_attrs(a: &[Attribute], b: &[Attribute]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| x.name == y.name && x.value == y.value))
}

pub struct TreeBuilder<'a> {
    doc: &'a mut Document,
    tok: Tokenizer,
    mode: Mode,
    original_mode: Mode,
    template_modes: Vec<Mode>,
    open: Vec<NodeId>,
    afe: Vec<Afe>,
    head: Option<NodeId>,
    form: Option<NodeId>,
    frameset_ok: bool,
    foster: bool,
    /// The fragment context element, if parsing a fragment.
    context: Option<NodeId>,
    /// Where nodes that would go into the root `html` element go instead (fragment case).
    root_target: Option<NodeId>,
    pending_table_text: String,
    pending_table_non_ws: bool,
    ignore_lf: bool,
    scripting: bool,
    stopped: bool,
}

impl<'a> TreeBuilder<'a> {
    pub fn new(doc: &'a mut Document, tok: Tokenizer, scripting: bool) -> TreeBuilder<'a> {
        TreeBuilder {
            doc,
            tok,
            mode: Mode::Initial,
            original_mode: Mode::Initial,
            template_modes: Vec::new(),
            open: Vec::new(),
            afe: Vec::new(),
            head: None,
            form: None,
            frameset_ok: true,
            foster: false,
            context: None,
            root_target: None,
            pending_table_text: String::new(),
            pending_table_non_ws: false,
            ignore_lf: false,
            scripting,
            stopped: false,
        }
    }

    /// Sets up the fragment case (§13.4) for `context`; the parsed nodes are appended
    /// to `fragment`, which must be a detached DocumentFragment in the same document.
    pub fn set_fragment_context(&mut self, context: NodeId, fragment: NodeId) {
        let root = self.doc.create(NodeKind::Element { ns: Namespace::Html, tag: "html".to_owned(), attrs: Vec::new() });
        self.open.push(root);
        self.context = Some(context);
        self.root_target = Some(fragment);
        let (ns, tag) = self.name(context);
        let tag = tag.to_owned();
        if ns == Namespace::Html {
            match tag.as_str() {
                "title" | "textarea" => self.tok.state = State::Rcdata,
                "style" | "xmp" | "iframe" | "noembed" | "noframes" => self.tok.state = State::Rawtext,
                "script" => self.tok.state = State::ScriptData,
                "noscript" if self.scripting => self.tok.state = State::Rawtext,
                "plaintext" => self.tok.state = State::Plaintext,
                _ => {}
            }
            if tag == "template" {
                self.template_modes.push(Mode::InTemplate);
            }
        }
        self.reset_insertion_mode();
        let mut n = Some(context);
        while let Some(id) = n {
            if self.is_html(id, "form") {
                self.form = Some(id);
                break;
            }
            n = self.doc.parent(id);
        }
    }

    pub fn run(&mut self) {
        while !self.stopped {
            self.tok.allow_cdata = self.adjusted_current_is_foreign();
            let ignore_lf = std::mem::take(&mut self.ignore_lf);
            match self.tok.next() {
                Token::Chars(mut s) => {
                    if ignore_lf && s.starts_with('\n') {
                        s.remove(0);
                    }
                    self.process_chars(s);
                }
                Token::Eof => {
                    self.dispatch(Tok::Eof);
                    self.stopped = true;
                }
                Token::Doctype(d) => self.dispatch(Tok::Doctype(d)),
                Token::StartTag(t) => self.dispatch(Tok::Start(t)),
                Token::EndTag(t) => self.dispatch(Tok::End(t)),
                Token::Comment(c) => self.dispatch(Tok::Comment(c)),
            }
        }
        self.open.clear();
        self.update_selectedcontent();
    }

    /// The customizable `<select>` model: a `<selectedcontent>` inside a select mirrors
    /// the selected option's children. Browsers do this as the option is inserted and
    /// its children change; doing it once at the end of the parse gives the same tree.
    fn update_selectedcontent(&mut self) {
        let root = self.root_target.unwrap_or(Document::ROOT);
        let selects: Vec<NodeId> = self.doc.descendants(root).filter(|&n| self.is_html(n, "select")).collect();
        for select in selects {
            let Some(target) = self.doc.descendants(select).find(|&n| n != select && self.is_html(n, "selectedcontent")) else { continue };
            let options: Vec<NodeId> = self.doc.descendants(select).filter(|&n| self.is_html(n, "option")).collect();
            let single = !self.doc.has_attr(select, "multiple") && self.doc.attr(select, "size").and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(1) <= 1;
            let chosen = options.iter().rev().find(|&&o| self.doc.has_attr(o, "selected")).copied().or_else(|| if single { options.first().copied() } else { None });
            let old: Vec<NodeId> = self.doc.children(target).collect();
            for c in old {
                self.doc.detach(c);
            }
            if let Some(option) = chosen {
                let kids: Vec<NodeId> = self.doc.children(option).collect();
                for k in kids {
                    let clone = self.doc.clone_subtree(k);
                    self.doc.append(target, clone);
                }
            }
        }
    }

    fn process_chars(&mut self, s: String) {
        let mut rest = s.as_str();
        while !rest.is_empty() {
            let first = rest.chars().next().unwrap_or(' ');
            let kind = char_kind(first);
            let end = rest.char_indices().find(|(_, c)| char_kind(*c) != kind).map(|(i, _)| i).unwrap_or(rest.len());
            let (seg, tail) = rest.split_at(end);
            self.dispatch(Tok::Chars(seg.to_owned(), kind));
            rest = tail;
            if self.stopped {
                break;
            }
        }
    }

    // ----- node helpers -----

    fn name(&self, id: NodeId) -> (Namespace, &str) {
        match self.doc.kind(id) {
            NodeKind::Element { ns, tag, .. } => (*ns, tag.as_str()),
            _ => (Namespace::Html, ""),
        }
    }

    fn is_html(&self, id: NodeId, tag: &str) -> bool {
        matches!(self.doc.kind(id), NodeKind::Element { ns: Namespace::Html, tag: t, .. } if t == tag)
    }

    fn is_html_in(&self, id: NodeId, tags: &[&str]) -> bool {
        matches!(self.doc.kind(id), NodeKind::Element { ns: Namespace::Html, tag: t, .. } if tags.contains(&t.as_str()))
    }

    fn current(&self) -> NodeId {
        *self.open.last().expect("stack of open elements is empty")
    }

    fn adjusted_current(&self) -> NodeId {
        match self.context {
            Some(c) if self.open.len() == 1 => c,
            _ => self.current(),
        }
    }

    fn adjusted_current_is_foreign(&self) -> bool {
        if self.open.is_empty() {
            return false;
        }
        self.name(self.adjusted_current()).0 != Namespace::Html
    }

    fn is_mathml_text_integration_point(&self, id: NodeId) -> bool {
        matches!(self.name(id), (Namespace::MathMl, "mi" | "mo" | "mn" | "ms" | "mtext"))
    }

    fn is_html_integration_point(&self, id: NodeId) -> bool {
        match self.name(id) {
            (Namespace::MathMl, "annotation-xml") => self.doc.attr(id, "encoding").is_some_and(|v| v.eq_ignore_ascii_case("text/html") || v.eq_ignore_ascii_case("application/xhtml+xml")),
            (Namespace::Svg, "foreignObject" | "desc" | "title") => true,
            _ => false,
        }
    }

    fn special(&self, id: NodeId) -> bool {
        let (ns, tag) = self.name(id);
        is_special(ns, tag)
    }

    fn scope_boundary(&self, scope: Scope, id: NodeId) -> bool {
        let (ns, tag) = self.name(id);
        match scope {
            Scope::Table => ns == Namespace::Html && matches!(tag, "html" | "table" | "template"),
            _ => {
                let base = match ns {
                    Namespace::Html => matches!(tag, "applet" | "caption" | "html" | "table" | "td" | "th" | "marquee" | "object" | "select" | "template"),
                    Namespace::MathMl => matches!(tag, "mi" | "mo" | "mn" | "ms" | "mtext" | "annotation-xml"),
                    Namespace::Svg => matches!(tag, "foreignObject" | "desc" | "title"),
                };
                base || match scope {
                    Scope::ListItem => ns == Namespace::Html && matches!(tag, "ol" | "ul"),
                    Scope::Button => ns == Namespace::Html && tag == "button",
                    _ => false,
                }
            }
        }
    }

    fn has_in_scope_by(&self, scope: Scope, pred: impl Fn(&Self, NodeId) -> bool) -> bool {
        for &n in self.open.iter().rev() {
            if pred(self, n) {
                return true;
            }
            if self.scope_boundary(scope, n) {
                return false;
            }
        }
        false
    }

    fn has_in_scope(&self, tag: &str) -> bool {
        self.has_in_scope_by(Scope::Default, |s, n| s.is_html(n, tag))
    }
    fn has_in_list_scope(&self, tag: &str) -> bool {
        self.has_in_scope_by(Scope::ListItem, |s, n| s.is_html(n, tag))
    }
    fn has_in_button_scope(&self, tag: &str) -> bool {
        self.has_in_scope_by(Scope::Button, |s, n| s.is_html(n, tag))
    }
    fn has_in_table_scope(&self, tag: &str) -> bool {
        self.has_in_scope_by(Scope::Table, |s, n| s.is_html(n, tag))
    }
    fn has_node_in_scope(&self, node: NodeId) -> bool {
        self.has_in_scope_by(Scope::Default, |_, n| n == node)
    }

    fn has_template_on_stack(&self) -> bool {
        self.open.iter().any(|&n| self.is_html(n, "template"))
    }

    fn parsing_template_contents(&self) -> bool {
        self.has_template_on_stack() || self.context.is_some_and(|c| self.is_html(c, "template"))
    }

    /// Pops until an HTML element with this tag has been popped.
    fn pop_until(&mut self, tag: &str) {
        while let Some(n) = self.open.pop() {
            if self.is_html(n, tag) {
                break;
            }
        }
    }

    fn pop_until_any(&mut self, tags: &[&str]) {
        while let Some(n) = self.open.pop() {
            if self.is_html_in(n, tags) {
                break;
            }
        }
    }

    fn generate_implied_end_tags(&mut self, except: Option<&str>) {
        while let Some(&n) = self.open.last() {
            let (ns, tag) = self.name(n);
            if ns == Namespace::Html && IMPLIED_END.contains(&tag) && except != Some(tag) {
                self.open.pop();
            } else {
                break;
            }
        }
    }

    fn generate_implied_end_tags_thoroughly(&mut self) {
        while let Some(&n) = self.open.last() {
            let (ns, tag) = self.name(n);
            if ns == Namespace::Html && IMPLIED_END_THOROUGH.contains(&tag) {
                self.open.pop();
            } else {
                break;
            }
        }
    }

    fn close_p(&mut self) {
        self.generate_implied_end_tags(Some("p"));
        self.pop_until("p");
    }

    // ----- insertion -----

    /// The adjusted insertion location (§13.2.6.1) given an optional override target.
    fn insertion_location(&self, override_target: Option<NodeId>) -> (NodeId, Option<NodeId>) {
        let mut target = override_target.unwrap_or_else(|| self.current());
        let mut before = None;
        if self.foster && self.is_html_in(target, &["table", "tbody", "tfoot", "thead", "tr"]) {
            match self.open.iter().rposition(|&n| self.is_html_in(n, &["template", "table"])) {
                None => {
                    target = self.open[0];
                }
                Some(i) => {
                    let n = self.open[i];
                    if self.is_html(n, "template") {
                        target = n;
                    } else if let Some(p) = self.doc.parent(n) {
                        target = p;
                        before = Some(n);
                    } else {
                        target = self.open[i - 1];
                    }
                }
            }
        }
        if self.is_html(target, "template") {
            if let Some(c) = self.doc.template_contents(target) {
                return (c, None);
            }
        }
        if let Some(rt) = self.root_target {
            if !self.open.is_empty() && target == self.open[0] {
                return (rt, None);
            }
        }
        (target, before)
    }

    fn insert_node_at(&mut self, (target, before): (NodeId, Option<NodeId>), node: NodeId) {
        // `node` was just created and has no children, so it cannot be an ancestor of
        // `target`; that clause of the spec needs no walk here.
        if self.doc.parent(node).is_some() || target == node {
            return;
        }
        if target == Document::ROOT && self.doc.element_children(target).next().is_some() {
            return;
        }
        if let Some(b) = before {
            if self.doc.parent(b) != Some(target) {
                self.doc.append(target, node);
                return;
            }
        }
        self.doc.insert_before(target, node, before);
    }

    fn create_element(&mut self, tag: &Tag, ns: Namespace) -> NodeId {
        let el = self.doc.create(NodeKind::Element { ns, tag: tag.name.clone(), attrs: tag.attrs.clone() });
        if ns == Namespace::Html && tag.name == "template" {
            let contents = self.doc.create(NodeKind::DocumentFragment);
            self.doc.append(el, contents);
        }
        el
    }

    fn insert_foreign_element(&mut self, tag: &Tag, ns: Namespace) -> NodeId {
        let loc = self.insertion_location(None);
        let el = self.create_element(tag, ns);
        self.insert_node_at(loc, el);
        self.open.push(el);
        el
    }

    fn insert_html_element(&mut self, tag: &Tag) -> NodeId {
        self.insert_foreign_element(tag, Namespace::Html)
    }

    fn insert_html_element_named(&mut self, name: &str) -> NodeId {
        let tag = Tag { name: name.to_owned(), attrs: Vec::new(), self_closing: false };
        self.insert_html_element(&tag)
    }

    fn insert_text(&mut self, text: &str) {
        let (target, before) = self.insertion_location(None);
        if target == Document::ROOT {
            return;
        }
        let prev = match before {
            Some(b) => self.doc.prev_sibling(b),
            None => self.doc.last_child(target),
        };
        if let Some(p) = prev {
            if let NodeKind::Text(t) = &mut self.doc.node_mut(p).kind {
                t.push_str(text);
                return;
            }
        }
        let t = self.doc.create_text(text);
        self.doc.insert_before(target, t, before);
    }

    fn insert_comment(&mut self, data: String, location: Option<(NodeId, Option<NodeId>)>) {
        let loc = self.insertion_location(location.map(|(target, _)| target));
        let c = self.doc.create(NodeKind::Comment(data));
        self.doc.insert_before(loc.0, c, loc.1);
    }

    // ----- active formatting elements -----

    fn afe_index_of(&self, node: NodeId) -> Option<usize> {
        self.afe.iter().rposition(|e| matches!(e, Afe::Element { node: n, .. } if *n == node))
    }

    fn afe_is_in_open(&self, i: usize) -> bool {
        match &self.afe[i] {
            Afe::Marker => false,
            // Formatting elements sit near the top of the stack; scan from there.
            Afe::Element { node, .. } => self.open.iter().rev().any(|n| n == node),
        }
    }

    fn push_afe(&mut self, node: NodeId, tag: &Tag) {
        let mut count = 0;
        let mut earliest = None;
        for (i, e) in self.afe.iter().enumerate().rev() {
            match e {
                Afe::Marker => break,
                Afe::Element { tag: t, .. } => {
                    if t.name == tag.name && same_attrs(&t.attrs, &tag.attrs) {
                        count += 1;
                        earliest = Some(i);
                    }
                }
            }
        }
        if count >= 3 {
            if let Some(i) = earliest {
                self.afe.remove(i);
            }
        }
        self.afe.push(Afe::Element { node, tag: tag.clone() });
    }

    fn reconstruct_afe(&mut self) {
        let len = self.afe.len();
        if len == 0 {
            return;
        }
        if matches!(self.afe[len - 1], Afe::Marker) || self.afe_is_in_open(len - 1) {
            return;
        }
        let mut i = len - 1;
        while i > 0 && !(matches!(self.afe[i - 1], Afe::Marker) || self.afe_is_in_open(i - 1)) {
            i -= 1;
        }
        for j in i..len {
            let tag = match &self.afe[j] {
                Afe::Element { tag, .. } => tag.clone(),
                Afe::Marker => continue,
            };
            let new = self.insert_html_element(&tag);
            if let Afe::Element { node, .. } = &mut self.afe[j] {
                *node = new;
            }
        }
    }

    fn clear_afe_to_marker(&mut self) {
        while let Some(e) = self.afe.pop() {
            if matches!(e, Afe::Marker) {
                break;
            }
        }
    }

    fn remove_from_afe(&mut self, node: NodeId) {
        if let Some(i) = self.afe_index_of(node) {
            self.afe.remove(i);
        }
    }

    fn remove_from_open(&mut self, node: NodeId) {
        if let Some(i) = self.open.iter().rposition(|&n| n == node) {
            self.open.remove(i);
        }
    }

    // ----- insertion mode -----

    fn reset_insertion_mode(&mut self) {
        let mut last = false;
        let mut i = self.open.len();
        loop {
            if i == 0 {
                self.mode = Mode::InBody;
                return;
            }
            i -= 1;
            let mut node = self.open[i];
            if i == 0 {
                last = true;
                if let Some(c) = self.context {
                    node = c;
                }
            }
            let (ns, tag) = self.name(node);
            if ns == Namespace::Html {
                match tag {
                    "td" | "th" if !last => {
                        self.mode = Mode::InCell;
                        return;
                    }
                    "tr" => {
                        self.mode = Mode::InRow;
                        return;
                    }
                    "tbody" | "thead" | "tfoot" => {
                        self.mode = Mode::InTableBody;
                        return;
                    }
                    "caption" => {
                        self.mode = Mode::InCaption;
                        return;
                    }
                    "colgroup" => {
                        self.mode = Mode::InColumnGroup;
                        return;
                    }
                    "table" => {
                        self.mode = Mode::InTable;
                        return;
                    }
                    "template" => {
                        self.mode = *self.template_modes.last().unwrap_or(&Mode::InTemplate);
                        return;
                    }
                    "head" if !last => {
                        self.mode = Mode::InHead;
                        return;
                    }
                    "body" => {
                        self.mode = Mode::InBody;
                        return;
                    }
                    "frameset" => {
                        self.mode = Mode::InFrameset;
                        return;
                    }
                    "html" => {
                        self.mode = if self.head.is_none() { Mode::BeforeHead } else { Mode::AfterHead };
                        return;
                    }
                    _ => {}
                }
            }
            if last {
                self.mode = Mode::InBody;
                return;
            }
        }
    }

    fn stop_parsing(&mut self) {
        self.stopped = true;
    }

    // ----- dispatch -----

    fn dispatch(&mut self, mut tok: Tok) {
        loop {
            let use_html = self.open.is_empty() || matches!(tok, Tok::Eof) || {
                let acn = self.adjusted_current();
                let (ns, tag) = self.name(acn);
                ns == Namespace::Html
                    || (self.is_mathml_text_integration_point(acn) && matches!(&tok, Tok::Start(t) if t.name != "mglyph" && t.name != "malignmark"))
                    || (self.is_mathml_text_integration_point(acn) && matches!(tok, Tok::Chars(..)))
                    || (ns == Namespace::MathMl && tag == "annotation-xml" && matches!(&tok, Tok::Start(t) if t.name == "svg"))
                    || (self.is_html_integration_point(acn) && matches!(tok, Tok::Start(_) | Tok::Chars(..)))
            };
            let next = if use_html {
                self.step(self.mode, tok)
            } else {
                match self.foreign(tok) {
                    None => None,
                    Some(t) => self.step(self.mode, t),
                }
            };
            match next {
                None => return,
                Some(t) => tok = t,
            }
        }
    }

    fn step(&mut self, mode: Mode, tok: Tok) -> Option<Tok> {
        match mode {
            Mode::Initial => self.initial(tok),
            Mode::BeforeHtml => self.before_html(tok),
            Mode::BeforeHead => self.before_head(tok),
            Mode::InHead => self.in_head(tok),
            Mode::InHeadNoscript => self.in_head_noscript(tok),
            Mode::AfterHead => self.after_head(tok),
            Mode::InBody => self.in_body(tok),
            Mode::Text => self.text(tok),
            Mode::InTable => self.in_table(tok),
            Mode::InTableText => self.in_table_text(tok),
            Mode::InCaption => self.in_caption(tok),
            Mode::InColumnGroup => self.in_column_group(tok),
            Mode::InTableBody => self.in_table_body(tok),
            Mode::InRow => self.in_row(tok),
            Mode::InCell => self.in_cell(tok),
            Mode::InTemplate => self.in_template(tok),
            Mode::AfterBody => self.after_body(tok),
            Mode::InFrameset => self.in_frameset(tok),
            Mode::AfterFrameset => self.after_frameset(tok),
            Mode::AfterAfterBody => self.after_after_body(tok),
            Mode::AfterAfterFrameset => self.after_after_frameset(tok),
        }
    }

    // ----- initial / before html / before head -----

    fn initial(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(_, CharKind::Whitespace) => None,
            Tok::Comment(c) => {
                self.insert_comment(c, Some((Document::ROOT, None)));
                None
            }
            Tok::Doctype(d) => {
                let name = d.name.clone().unwrap_or_default();
                let public_id = d.public_id.clone().unwrap_or_default();
                let system_id = d.system_id.clone().unwrap_or_default();
                let has_child = self.doc.children(Document::ROOT).any(|c| matches!(self.doc.kind(c), NodeKind::DocType { .. } | NodeKind::Element { .. }));
                if !has_child {
                    let dt = self.doc.create(NodeKind::DocType { name, public_id, system_id });
                    self.doc.append(Document::ROOT, dt);
                }
                self.doc.quirks = quirks_mode_for(&d);
                self.mode = Mode::BeforeHtml;
                None
            }
            other => {
                self.doc.quirks = QuirksMode::Quirks;
                self.mode = Mode::BeforeHtml;
                Some(other)
            }
        }
    }

    fn before_html(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Doctype(_) => None,
            Tok::Comment(c) => {
                self.insert_comment(c, Some((Document::ROOT, None)));
                None
            }
            Tok::Chars(_, CharKind::Whitespace) => None,
            Tok::Start(t) if t.name == "html" => {
                let el = self.create_element(&t, Namespace::Html);
                self.insert_node_at((Document::ROOT, None), el);
                self.open.push(el);
                self.mode = Mode::BeforeHead;
                None
            }
            Tok::End(t) if !matches!(t.name.as_str(), "head" | "body" | "html" | "br") => None,
            other => {
                let el = self.doc.create(NodeKind::Element { ns: Namespace::Html, tag: "html".to_owned(), attrs: Vec::new() });
                self.doc.append(Document::ROOT, el);
                self.open.push(el);
                self.mode = Mode::BeforeHead;
                Some(other)
            }
        }
    }

    fn before_head(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(_, CharKind::Whitespace) => None,
            Tok::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(t) if t.name == "html" => self.in_body(Tok::Start(t)),
            Tok::Start(t) if t.name == "head" => {
                let el = self.insert_html_element(&t);
                self.head = Some(el);
                self.mode = Mode::InHead;
                None
            }
            Tok::End(t) if !matches!(t.name.as_str(), "head" | "body" | "html" | "br") => None,
            other => {
                let el = self.insert_html_element_named("head");
                self.head = Some(el);
                self.mode = Mode::InHead;
                Some(other)
            }
        }
    }

    // ----- in head -----

    fn in_head(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(s, CharKind::Whitespace) => {
                self.insert_text(&s);
                None
            }
            Tok::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(t) => match t.name.as_str() {
                "html" => self.in_body(Tok::Start(t)),
                "base" | "basefont" | "bgsound" | "link" | "meta" => {
                    self.insert_html_element(&t);
                    self.open.pop();
                    None
                }
                "title" => {
                    self.parse_rcdata(&t);
                    None
                }
                "noscript" if self.scripting => {
                    self.parse_rawtext(&t);
                    None
                }
                "noframes" | "style" => {
                    self.parse_rawtext(&t);
                    None
                }
                "noscript" => {
                    self.insert_html_element(&t);
                    self.mode = Mode::InHeadNoscript;
                    None
                }
                "script" => {
                    let loc = self.insertion_location(None);
                    let el = self.create_element(&t, Namespace::Html);
                    self.insert_node_at(loc, el);
                    self.open.push(el);
                    self.tok.state = State::ScriptData;
                    self.original_mode = self.mode;
                    self.mode = Mode::Text;
                    None
                }
                "template" => {
                    self.afe.push(Afe::Marker);
                    self.frameset_ok = false;
                    self.mode = Mode::InTemplate;
                    self.template_modes.push(Mode::InTemplate);
                    self.insert_html_element(&t);
                    None
                }
                "head" => None,
                _ => {
                    self.open.pop();
                    self.mode = Mode::AfterHead;
                    Some(Tok::Start(t))
                }
            },
            Tok::End(t) => match t.name.as_str() {
                "head" => {
                    self.open.pop();
                    self.mode = Mode::AfterHead;
                    None
                }
                "body" | "html" | "br" => {
                    self.open.pop();
                    self.mode = Mode::AfterHead;
                    Some(Tok::End(t))
                }
                "template" => {
                    if !self.has_template_on_stack() {
                        return None;
                    }
                    self.generate_implied_end_tags_thoroughly();
                    self.pop_until("template");
                    self.clear_afe_to_marker();
                    self.template_modes.pop();
                    self.reset_insertion_mode();
                    None
                }
                _ => None,
            },
            other => {
                self.open.pop();
                self.mode = Mode::AfterHead;
                Some(other)
            }
        }
    }

    fn parse_rcdata(&mut self, t: &Tag) {
        self.insert_html_element(t);
        self.tok.state = State::Rcdata;
        self.original_mode = self.mode;
        self.mode = Mode::Text;
    }

    fn parse_rawtext(&mut self, t: &Tag) {
        self.insert_html_element(t);
        self.tok.state = State::Rawtext;
        self.original_mode = self.mode;
        self.mode = Mode::Text;
    }

    fn in_head_noscript(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Doctype(_) => None,
            Tok::Start(t) if t.name == "html" => self.in_body(Tok::Start(t)),
            Tok::End(t) if t.name == "noscript" => {
                self.open.pop();
                self.mode = Mode::InHead;
                None
            }
            Tok::Chars(_, CharKind::Whitespace) | Tok::Comment(_) => self.in_head(tok),
            Tok::Start(t) if matches!(t.name.as_str(), "basefont" | "bgsound" | "link" | "meta" | "noframes" | "style") => self.in_head(Tok::Start(t)),
            Tok::End(t) if t.name == "br" => {
                self.open.pop();
                self.mode = Mode::InHead;
                Some(Tok::End(t))
            }
            Tok::Start(t) if matches!(t.name.as_str(), "head" | "noscript") => None,
            Tok::End(_) => None,
            other => {
                self.open.pop();
                self.mode = Mode::InHead;
                Some(other)
            }
        }
    }

    fn after_head(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(s, CharKind::Whitespace) => {
                self.insert_text(&s);
                None
            }
            Tok::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(t) => match t.name.as_str() {
                "html" => self.in_body(Tok::Start(t)),
                "body" => {
                    self.insert_html_element(&t);
                    self.frameset_ok = false;
                    self.mode = Mode::InBody;
                    None
                }
                "frameset" => {
                    self.insert_html_element(&t);
                    self.mode = Mode::InFrameset;
                    None
                }
                "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style" | "template" | "title" => {
                    let head = self.head.expect("head pointer set after head");
                    self.open.push(head);
                    let r = self.in_head(Tok::Start(t));
                    self.remove_from_open(head);
                    r
                }
                "head" => None,
                _ => {
                    self.insert_html_element_named("body");
                    self.frameset_ok = true;
                    self.mode = Mode::InBody;
                    Some(Tok::Start(t))
                }
            },
            Tok::End(t) => match t.name.as_str() {
                "template" => self.in_head(Tok::End(t)),
                "body" | "html" | "br" => {
                    self.insert_html_element_named("body");
                    self.frameset_ok = true;
                    self.mode = Mode::InBody;
                    Some(Tok::End(t))
                }
                _ => None,
            },
            other => {
                self.insert_html_element_named("body");
                self.frameset_ok = true;
                self.mode = Mode::InBody;
                Some(other)
            }
        }
    }

    // ----- in body -----

    fn in_body(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(_, CharKind::Null) => None,
            Tok::Chars(s, CharKind::Whitespace) => {
                self.reconstruct_afe();
                self.insert_text(&s);
                None
            }
            Tok::Chars(s, CharKind::Other) => {
                self.reconstruct_afe();
                self.insert_text(&s);
                self.frameset_ok = false;
                None
            }
            Tok::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(t) => self.in_body_start(t),
            Tok::End(t) => self.in_body_end(t),
            Tok::Eof => {
                if !self.template_modes.is_empty() {
                    return self.in_template(Tok::Eof);
                }
                self.stop_parsing();
                None
            }
        }
    }

    fn add_missing_attrs(&mut self, node: NodeId, attrs: &[Attribute]) {
        for a in attrs {
            if !self.doc.has_attr(node, &a.name) {
                self.doc.set_attr(node, &a.name, &a.value);
            }
        }
    }

    fn in_body_start(&mut self, mut t: Tag) -> Option<Tok> {
        match t.name.as_str() {
            "html" => {
                if self.has_template_on_stack() {
                    return None;
                }
                let top = self.open[0];
                self.add_missing_attrs(top, &t.attrs);
                None
            }
            "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style" | "template" | "title" => self.in_head(Tok::Start(t)),
            "body" => {
                if self.open.len() < 2 || !self.is_html(self.open[1], "body") || self.has_template_on_stack() {
                    return None;
                }
                self.frameset_ok = false;
                let body = self.open[1];
                self.add_missing_attrs(body, &t.attrs);
                None
            }
            "frameset" => {
                if self.open.len() < 2 || !self.is_html(self.open[1], "body") || !self.frameset_ok {
                    return None;
                }
                let body = self.open[1];
                self.doc.detach(body);
                self.open.truncate(1);
                self.insert_html_element(&t);
                self.mode = Mode::InFrameset;
                None
            }
            "address" | "article" | "aside" | "blockquote" | "center" | "details" | "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer" | "header" | "hgroup" | "main" | "menu" | "nav" | "ol" | "p" | "search" | "section" | "summary" | "ul" => {
                if self.has_in_button_scope("p") {
                    self.close_p();
                }
                self.insert_html_element(&t);
                None
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if self.has_in_button_scope("p") {
                    self.close_p();
                }
                let cur = self.current();
                if let (Namespace::Html, tag) = self.name(cur) {
                    if is_heading(tag) {
                        self.open.pop();
                    }
                }
                self.insert_html_element(&t);
                None
            }
            "pre" | "listing" => {
                if self.has_in_button_scope("p") {
                    self.close_p();
                }
                self.insert_html_element(&t);
                self.ignore_lf = true;
                self.frameset_ok = false;
                None
            }
            "form" => {
                if self.form.is_some() && !self.parsing_template_contents() {
                    return None;
                }
                if self.has_in_button_scope("p") {
                    self.close_p();
                }
                let el = self.insert_html_element(&t);
                if !self.parsing_template_contents() {
                    self.form = Some(el);
                }
                None
            }
            "li" => {
                self.frameset_ok = false;
                for i in (0..self.open.len()).rev() {
                    let node = self.open[i];
                    if self.is_html(node, "li") {
                        self.generate_implied_end_tags(Some("li"));
                        self.pop_until("li");
                        break;
                    }
                    if self.special(node) && !self.is_html_in(node, &["address", "div", "p"]) {
                        break;
                    }
                }
                if self.has_in_button_scope("p") {
                    self.close_p();
                }
                self.insert_html_element(&t);
                None
            }
            "dd" | "dt" => {
                self.frameset_ok = false;
                for i in (0..self.open.len()).rev() {
                    let node = self.open[i];
                    if self.is_html(node, "dd") {
                        self.generate_implied_end_tags(Some("dd"));
                        self.pop_until("dd");
                        break;
                    }
                    if self.is_html(node, "dt") {
                        self.generate_implied_end_tags(Some("dt"));
                        self.pop_until("dt");
                        break;
                    }
                    if self.special(node) && !self.is_html_in(node, &["address", "div", "p"]) {
                        break;
                    }
                }
                if self.has_in_button_scope("p") {
                    self.close_p();
                }
                self.insert_html_element(&t);
                None
            }
            "plaintext" => {
                if self.has_in_button_scope("p") {
                    self.close_p();
                }
                self.insert_html_element(&t);
                self.tok.state = State::Plaintext;
                None
            }
            "button" => {
                if self.has_in_scope("button") {
                    self.generate_implied_end_tags(None);
                    self.pop_until("button");
                }
                self.reconstruct_afe();
                self.insert_html_element(&t);
                self.frameset_ok = false;
                None
            }
            "a" => {
                let mut existing = None;
                for e in self.afe.iter().rev() {
                    match e {
                        Afe::Marker => break,
                        Afe::Element { node, tag } => {
                            if tag.name == "a" {
                                existing = Some(*node);
                                break;
                            }
                        }
                    }
                }
                if let Some(a) = existing {
                    self.adoption_agency(&t);
                    self.remove_from_afe(a);
                    self.remove_from_open(a);
                }
                self.reconstruct_afe();
                let el = self.insert_html_element(&t);
                self.push_afe(el, &t);
                None
            }
            "b" | "big" | "code" | "em" | "font" | "i" | "s" | "small" | "strike" | "strong" | "tt" | "u" => {
                self.reconstruct_afe();
                let el = self.insert_html_element(&t);
                self.push_afe(el, &t);
                None
            }
            "nobr" => {
                self.reconstruct_afe();
                if self.has_in_scope("nobr") {
                    self.adoption_agency(&t);
                    self.reconstruct_afe();
                }
                let el = self.insert_html_element(&t);
                self.push_afe(el, &t);
                None
            }
            "applet" | "marquee" | "object" => {
                self.reconstruct_afe();
                self.insert_html_element(&t);
                self.afe.push(Afe::Marker);
                self.frameset_ok = false;
                None
            }
            "table" => {
                if self.doc.quirks != QuirksMode::Quirks && self.has_in_button_scope("p") {
                    self.close_p();
                }
                self.insert_html_element(&t);
                self.frameset_ok = false;
                self.mode = Mode::InTable;
                None
            }
            "area" | "br" | "embed" | "img" | "keygen" | "wbr" => {
                self.reconstruct_afe();
                self.insert_html_element(&t);
                self.open.pop();
                self.frameset_ok = false;
                None
            }
            "input" => {
                if self.context.is_some_and(|c| self.is_html(c, "select")) {
                    return None;
                }
                if self.has_in_scope("select") {
                    self.pop_until("select");
                }
                self.reconstruct_afe();
                self.insert_html_element(&t);
                self.open.pop();
                let hidden = t.attrs.iter().any(|a| a.name == "type" && a.value.eq_ignore_ascii_case("hidden"));
                if !hidden {
                    self.frameset_ok = false;
                }
                None
            }
            "param" | "source" | "track" => {
                self.insert_html_element(&t);
                self.open.pop();
                None
            }
            "hr" => {
                if self.has_in_button_scope("p") {
                    self.close_p();
                }
                if self.has_in_scope("select") {
                    self.generate_implied_end_tags(None);
                }
                self.insert_html_element(&t);
                self.open.pop();
                self.frameset_ok = false;
                None
            }
            "image" => {
                t.name = "img".to_owned();
                Some(Tok::Start(t))
            }
            "textarea" => {
                self.insert_html_element(&t);
                self.ignore_lf = true;
                self.tok.state = State::Rcdata;
                self.original_mode = self.mode;
                self.frameset_ok = false;
                self.mode = Mode::Text;
                None
            }
            "xmp" => {
                if self.has_in_button_scope("p") {
                    self.close_p();
                }
                self.reconstruct_afe();
                self.frameset_ok = false;
                self.parse_rawtext(&t);
                None
            }
            "iframe" => {
                self.frameset_ok = false;
                self.parse_rawtext(&t);
                None
            }
            "noembed" => {
                self.parse_rawtext(&t);
                None
            }
            "noscript" if self.scripting => {
                self.parse_rawtext(&t);
                None
            }
            "select" => {
                if self.context.is_some_and(|c| self.is_html(c, "select")) {
                    return None;
                }
                if self.has_in_scope("select") {
                    self.pop_until("select");
                    return None;
                }
                self.reconstruct_afe();
                self.insert_html_element(&t);
                self.frameset_ok = false;
                None
            }
            "option" => {
                if self.has_in_scope("select") {
                    self.generate_implied_end_tags(Some("optgroup"));
                } else if self.is_html(self.current(), "option") {
                    self.open.pop();
                }
                self.reconstruct_afe();
                self.insert_html_element(&t);
                None
            }
            "optgroup" => {
                if self.has_in_scope("select") {
                    self.generate_implied_end_tags(None);
                } else if self.is_html(self.current(), "option") {
                    self.open.pop();
                }
                self.reconstruct_afe();
                self.insert_html_element(&t);
                None
            }
            "rb" | "rtc" => {
                if self.has_in_scope("ruby") {
                    self.generate_implied_end_tags(None);
                }
                self.insert_html_element(&t);
                None
            }
            "rp" | "rt" => {
                if self.has_in_scope("ruby") {
                    self.generate_implied_end_tags(Some("rtc"));
                }
                self.insert_html_element(&t);
                None
            }
            "math" => {
                self.reconstruct_afe();
                adjust_mathml_attrs(&mut t);
                self.insert_foreign_element(&t, Namespace::MathMl);
                if t.self_closing {
                    self.open.pop();
                }
                None
            }
            "svg" => {
                self.reconstruct_afe();
                adjust_svg_attrs(&mut t);
                self.insert_foreign_element(&t, Namespace::Svg);
                if t.self_closing {
                    self.open.pop();
                }
                None
            }
            "caption" | "col" | "colgroup" | "frame" | "head" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr" => None,
            _ => {
                self.reconstruct_afe();
                self.insert_html_element(&t);
                None
            }
        }
    }

    fn in_body_end(&mut self, t: Tag) -> Option<Tok> {
        match t.name.as_str() {
            "template" => self.in_head(Tok::End(t)),
            "body" => {
                if !self.has_in_scope("body") {
                    return None;
                }
                self.mode = Mode::AfterBody;
                None
            }
            "html" => {
                if !self.has_in_scope("body") {
                    return None;
                }
                self.mode = Mode::AfterBody;
                Some(Tok::End(t))
            }
            "address" | "article" | "aside" | "blockquote" | "button" | "center" | "details" | "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer" | "header" | "hgroup" | "listing" | "main" | "menu" | "nav" | "ol" | "pre" | "search" | "section" | "select" | "summary" | "ul" => {
                if !self.has_in_scope(&t.name) {
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until(&t.name);
                None
            }
            "form" => {
                if !self.parsing_template_contents() {
                    let node = self.form.take();
                    let node = node?;
                    if !self.has_node_in_scope(node) {
                        return None;
                    }
                    self.generate_implied_end_tags(None);
                    self.remove_from_open(node);
                } else {
                    if !self.has_in_scope("form") {
                        return None;
                    }
                    self.generate_implied_end_tags(None);
                    self.pop_until("form");
                }
                None
            }
            "p" => {
                if !self.has_in_button_scope("p") {
                    self.insert_html_element_named("p");
                }
                self.close_p();
                None
            }
            "li" => {
                if !self.has_in_list_scope("li") {
                    return None;
                }
                self.generate_implied_end_tags(Some("li"));
                self.pop_until("li");
                None
            }
            "dd" | "dt" => {
                if !self.has_in_scope(&t.name) {
                    return None;
                }
                self.generate_implied_end_tags(Some(&t.name));
                self.pop_until(&t.name);
                None
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if !self.has_in_scope_by(Scope::Default, |s, n| matches!(s.name(n), (Namespace::Html, tag) if is_heading(tag))) {
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until_any(&["h1", "h2", "h3", "h4", "h5", "h6"]);
                None
            }
            "a" | "b" | "big" | "code" | "em" | "font" | "i" | "nobr" | "s" | "small" | "strike" | "strong" | "tt" | "u" => {
                self.adoption_agency(&t);
                None
            }
            "applet" | "marquee" | "object" => {
                if !self.has_in_scope(&t.name) {
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until(&t.name);
                self.clear_afe_to_marker();
                None
            }
            "br" => {
                let tag = Tag { name: "br".to_owned(), attrs: Vec::new(), self_closing: false };
                self.in_body_start(tag)
            }
            _ => {
                self.any_other_end_tag(&t.name);
                None
            }
        }
    }

    fn any_other_end_tag(&mut self, name: &str) {
        for i in (0..self.open.len()).rev() {
            let node = self.open[i];
            if self.is_html(node, name) {
                self.generate_implied_end_tags(Some(name));
                self.open.truncate(i);
                return;
            }
            if self.special(node) {
                return;
            }
        }
    }

    fn adoption_agency(&mut self, t: &Tag) {
        let subject = t.name.as_str();
        let cur = self.current();
        if self.is_html(cur, subject) && self.afe_index_of(cur).is_none() {
            self.open.pop();
            return;
        }
        for _ in 0..8 {
            // The formatting element: last in the AFE after the last marker with this tag.
            let mut fe = None;
            for (i, e) in self.afe.iter().enumerate().rev() {
                match e {
                    Afe::Marker => break,
                    Afe::Element { node, tag } => {
                        if tag.name == subject {
                            fe = Some((i, *node));
                            break;
                        }
                    }
                }
            }
            let Some((fe_afe_idx, fe)) = fe else {
                self.any_other_end_tag(subject);
                return;
            };
            let Some(fe_idx) = self.open.iter().rposition(|&n| n == fe) else {
                self.afe.remove(fe_afe_idx);
                return;
            };
            if !self.has_node_in_scope(fe) {
                return;
            }
            let fb = self.open[fe_idx + 1..].iter().position(|&n| self.special(n)).map(|i| fe_idx + 1 + i);
            let Some(fb_idx) = fb else {
                self.open.truncate(fe_idx);
                self.afe.remove(fe_afe_idx);
                return;
            };
            let furthest_block = self.open[fb_idx];
            let common_ancestor = self.open[fe_idx - 1];
            let mut bookmark = fe_afe_idx;
            let mut node_idx = fb_idx;
            let mut last_node = furthest_block;
            let mut inner = 0;
            loop {
                inner += 1;
                node_idx -= 1;
                let node = self.open[node_idx];
                if node == fe {
                    break;
                }
                let mut node_afe = self.afe_index_of(node);
                if inner > 3 {
                    if let Some(i) = node_afe {
                        self.afe.remove(i);
                        if i < bookmark {
                            bookmark -= 1;
                        }
                        node_afe = None;
                    }
                }
                let Some(entry_idx) = node_afe else {
                    self.open.remove(node_idx);
                    continue;
                };
                let tag = match &self.afe[entry_idx] {
                    Afe::Element { tag, .. } => tag.clone(),
                    Afe::Marker => unreachable!(),
                };
                let new = self.create_element(&tag, Namespace::Html);
                if let Afe::Element { node: n, .. } = &mut self.afe[entry_idx] {
                    *n = new;
                }
                self.open[node_idx] = new;
                if last_node == furthest_block {
                    bookmark = entry_idx + 1;
                }
                self.doc.append(new, last_node);
                last_node = new;
            }
            let (target, before) = self.insertion_location(Some(common_ancestor));
            if self.doc.parent(last_node).is_some() {
                self.doc.detach(last_node);
            }
            let ancestor_cycle = target == last_node || self.doc.ancestors(target).any(|a| a == last_node);
            let doc_full = target == Document::ROOT && self.doc.element_children(target).next().is_some();
            let ref_ok = before.is_none_or(|b| self.doc.parent(b) == Some(target));
            if !ancestor_cycle && !doc_full && ref_ok {
                self.doc.insert_before(target, last_node, before);
            }
            let fe_tag = match &self.afe[fe_afe_idx] {
                Afe::Element { tag, .. } => tag.clone(),
                Afe::Marker => unreachable!(),
            };
            let new_el = self.create_element(&fe_tag, Namespace::Html);
            let kids: Vec<NodeId> = self.doc.children(furthest_block).collect();
            for k in kids {
                self.doc.append(new_el, k);
            }
            self.doc.append(furthest_block, new_el);
            // AFE: remove fe, insert the new element at the bookmark.
            self.afe.remove(fe_afe_idx);
            if fe_afe_idx < bookmark {
                bookmark -= 1;
            }
            let bookmark = bookmark.min(self.afe.len());
            self.afe.insert(bookmark, Afe::Element { node: new_el, tag: fe_tag });
            // Stack: remove fe, insert the new element below the furthest block.
            self.remove_from_open(fe);
            if let Some(i) = self.open.iter().rposition(|&n| n == furthest_block) {
                self.open.insert(i + 1, new_el);
            }
        }
    }

    // ----- text -----

    fn text(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(s, _) => {
                self.insert_text(&s);
                None
            }
            Tok::Eof => {
                self.open.pop();
                self.mode = self.original_mode;
                Some(Tok::Eof)
            }
            Tok::End(_) => {
                self.open.pop();
                self.mode = self.original_mode;
                None
            }
            _ => None,
        }
    }

    // ----- tables -----

    fn clear_stack_to_table_context(&mut self) {
        while !self.is_html_in(self.current(), &["table", "template", "html"]) {
            self.open.pop();
        }
    }

    fn clear_stack_to_table_body_context(&mut self) {
        while !self.is_html_in(self.current(), &["tbody", "tfoot", "thead", "template", "html"]) {
            self.open.pop();
        }
    }

    fn clear_stack_to_table_row_context(&mut self) {
        while !self.is_html_in(self.current(), &["tr", "template", "html"]) {
            self.open.pop();
        }
    }

    fn in_table(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(..) if self.is_html_in(self.current(), &["table", "tbody", "template", "tfoot", "thead", "tr"]) => {
                self.pending_table_text.clear();
                self.pending_table_non_ws = false;
                self.original_mode = self.mode;
                self.mode = Mode::InTableText;
                Some(tok)
            }
            Tok::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(t) => match t.name.as_str() {
                "caption" => {
                    self.clear_stack_to_table_context();
                    self.afe.push(Afe::Marker);
                    self.insert_html_element(&t);
                    self.mode = Mode::InCaption;
                    None
                }
                "colgroup" => {
                    self.clear_stack_to_table_context();
                    self.insert_html_element(&t);
                    self.mode = Mode::InColumnGroup;
                    None
                }
                "col" => {
                    self.clear_stack_to_table_context();
                    self.insert_html_element_named("colgroup");
                    self.mode = Mode::InColumnGroup;
                    Some(Tok::Start(t))
                }
                "tbody" | "tfoot" | "thead" => {
                    self.clear_stack_to_table_context();
                    self.insert_html_element(&t);
                    self.mode = Mode::InTableBody;
                    None
                }
                "td" | "th" | "tr" => {
                    self.clear_stack_to_table_context();
                    self.insert_html_element_named("tbody");
                    self.mode = Mode::InTableBody;
                    Some(Tok::Start(t))
                }
                "table" => {
                    if !self.has_in_table_scope("table") {
                        return None;
                    }
                    self.pop_until("table");
                    self.reset_insertion_mode();
                    Some(Tok::Start(t))
                }
                "style" | "script" | "template" => self.in_head(Tok::Start(t)),
                "input" => {
                    let hidden = t.attrs.iter().any(|a| a.name == "type" && a.value.eq_ignore_ascii_case("hidden"));
                    if !hidden {
                        return self.in_table_anything_else(Tok::Start(t));
                    }
                    self.insert_html_element(&t);
                    self.open.pop();
                    None
                }
                "form" => {
                    if self.form.is_some() && !self.parsing_template_contents() {
                        return None;
                    }
                    let el = self.insert_html_element(&t);
                    if !self.parsing_template_contents() {
                        self.form = Some(el);
                    }
                    self.open.pop();
                    None
                }
                _ => self.in_table_anything_else(Tok::Start(t)),
            },
            Tok::End(t) => match t.name.as_str() {
                "table" => {
                    if !self.has_in_table_scope("table") {
                        return None;
                    }
                    self.pop_until("table");
                    self.reset_insertion_mode();
                    None
                }
                "body" | "caption" | "col" | "colgroup" | "html" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr" => None,
                "template" => self.in_head(Tok::End(t)),
                _ => self.in_table_anything_else(Tok::End(t)),
            },
            Tok::Eof => self.in_body(Tok::Eof),
            other => self.in_table_anything_else(other),
        }
    }

    fn in_table_anything_else(&mut self, tok: Tok) -> Option<Tok> {
        self.foster = true;
        let r = self.in_body(tok);
        self.foster = false;
        r
    }

    fn in_table_text(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(_, CharKind::Null) => None,
            Tok::Chars(s, kind) => {
                if kind == CharKind::Other {
                    self.pending_table_non_ws = true;
                }
                self.pending_table_text.push_str(&s);
                None
            }
            other => {
                let text = std::mem::take(&mut self.pending_table_text);
                if self.pending_table_non_ws {
                    // Reprocess the pending characters with foster parenting, as the
                    // "anything else" entry of "in table" would.
                    let mut rest = text.as_str();
                    while !rest.is_empty() {
                        let first = rest.chars().next().unwrap_or(' ');
                        let kind = char_kind(first);
                        let end = rest.char_indices().find(|(_, c)| char_kind(*c) != kind).map(|(i, _)| i).unwrap_or(rest.len());
                        let (seg, tail) = rest.split_at(end);
                        self.in_table_anything_else(Tok::Chars(seg.to_owned(), kind));
                        rest = tail;
                    }
                } else if !text.is_empty() {
                    self.insert_text(&text);
                }
                self.mode = self.original_mode;
                Some(other)
            }
        }
    }

    fn in_caption(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::End(t) if t.name == "caption" => {
                if !self.has_in_table_scope("caption") {
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until("caption");
                self.clear_afe_to_marker();
                self.mode = Mode::InTable;
                None
            }
            Tok::Start(t) if matches!(t.name.as_str(), "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr") => {
                if !self.has_in_table_scope("caption") {
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until("caption");
                self.clear_afe_to_marker();
                self.mode = Mode::InTable;
                Some(Tok::Start(t))
            }
            Tok::End(t) if t.name == "table" => {
                if !self.has_in_table_scope("caption") {
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until("caption");
                self.clear_afe_to_marker();
                self.mode = Mode::InTable;
                Some(Tok::End(t))
            }
            Tok::End(t) if matches!(t.name.as_str(), "body" | "col" | "colgroup" | "html" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr") => None,
            other => self.in_body(other),
        }
    }

    fn in_column_group(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(s, CharKind::Whitespace) => {
                self.insert_text(&s);
                None
            }
            Tok::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(t) if t.name == "html" => self.in_body(Tok::Start(t)),
            Tok::Start(t) if t.name == "col" => {
                self.insert_html_element(&t);
                self.open.pop();
                None
            }
            Tok::End(t) if t.name == "colgroup" => {
                if !self.is_html(self.current(), "colgroup") {
                    return None;
                }
                self.open.pop();
                self.mode = Mode::InTable;
                None
            }
            Tok::End(t) if t.name == "col" => None,
            Tok::Start(t) if t.name == "template" => self.in_head(Tok::Start(t)),
            Tok::End(t) if t.name == "template" => self.in_head(Tok::End(t)),
            Tok::Eof => self.in_body(Tok::Eof),
            other => {
                if !self.is_html(self.current(), "colgroup") {
                    return None;
                }
                self.open.pop();
                self.mode = Mode::InTable;
                Some(other)
            }
        }
    }

    fn in_table_body(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Start(t) if t.name == "tr" => {
                self.clear_stack_to_table_body_context();
                self.insert_html_element(&t);
                self.mode = Mode::InRow;
                None
            }
            Tok::Start(t) if matches!(t.name.as_str(), "th" | "td") => {
                self.clear_stack_to_table_body_context();
                self.insert_html_element_named("tr");
                self.mode = Mode::InRow;
                Some(Tok::Start(t))
            }
            Tok::End(t) if matches!(t.name.as_str(), "tbody" | "tfoot" | "thead") => {
                if !self.has_in_table_scope(&t.name) {
                    return None;
                }
                self.clear_stack_to_table_body_context();
                self.open.pop();
                self.mode = Mode::InTable;
                None
            }
            Tok::Start(t) if matches!(t.name.as_str(), "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead") => {
                if !self.has_in_table_scope("tbody") && !self.has_in_table_scope("thead") && !self.has_in_table_scope("tfoot") {
                    return None;
                }
                self.clear_stack_to_table_body_context();
                self.open.pop();
                self.mode = Mode::InTable;
                Some(Tok::Start(t))
            }
            Tok::End(t) if t.name == "table" => {
                if !self.has_in_table_scope("tbody") && !self.has_in_table_scope("thead") && !self.has_in_table_scope("tfoot") {
                    return None;
                }
                self.clear_stack_to_table_body_context();
                self.open.pop();
                self.mode = Mode::InTable;
                Some(Tok::End(t))
            }
            Tok::End(t) if matches!(t.name.as_str(), "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th" | "tr") => None,
            other => self.in_table(other),
        }
    }

    fn in_row(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Start(t) if matches!(t.name.as_str(), "th" | "td") => {
                self.clear_stack_to_table_row_context();
                self.insert_html_element(&t);
                self.mode = Mode::InCell;
                self.afe.push(Afe::Marker);
                None
            }
            Tok::End(t) if t.name == "tr" => {
                if !self.has_in_table_scope("tr") {
                    return None;
                }
                self.clear_stack_to_table_row_context();
                self.open.pop();
                self.mode = Mode::InTableBody;
                None
            }
            Tok::Start(t) if matches!(t.name.as_str(), "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr") => {
                if !self.has_in_table_scope("tr") {
                    return None;
                }
                self.clear_stack_to_table_row_context();
                self.open.pop();
                self.mode = Mode::InTableBody;
                Some(Tok::Start(t))
            }
            Tok::End(t) if t.name == "table" => {
                if !self.has_in_table_scope("tr") {
                    return None;
                }
                self.clear_stack_to_table_row_context();
                self.open.pop();
                self.mode = Mode::InTableBody;
                Some(Tok::End(t))
            }
            Tok::End(t) if matches!(t.name.as_str(), "tbody" | "tfoot" | "thead") => {
                if !self.has_in_table_scope(&t.name) || !self.has_in_table_scope("tr") {
                    return None;
                }
                self.clear_stack_to_table_row_context();
                self.open.pop();
                self.mode = Mode::InTableBody;
                Some(Tok::End(t))
            }
            Tok::End(t) if matches!(t.name.as_str(), "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th") => None,
            other => self.in_table(other),
        }
    }

    fn close_cell(&mut self) {
        self.generate_implied_end_tags(None);
        self.pop_until_any(&["td", "th"]);
        self.clear_afe_to_marker();
        self.mode = Mode::InRow;
    }

    fn in_cell(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::End(t) if matches!(t.name.as_str(), "td" | "th") => {
                if !self.has_in_table_scope(&t.name) {
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until(&t.name);
                self.clear_afe_to_marker();
                self.mode = Mode::InRow;
                None
            }
            Tok::Start(t) if matches!(t.name.as_str(), "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr") => {
                if !self.has_in_table_scope("td") && !self.has_in_table_scope("th") {
                    return None;
                }
                self.close_cell();
                Some(Tok::Start(t))
            }
            Tok::End(t) if matches!(t.name.as_str(), "body" | "caption" | "col" | "colgroup" | "html") => None,
            Tok::End(t) if matches!(t.name.as_str(), "table" | "tbody" | "tfoot" | "thead" | "tr") => {
                if !self.has_in_table_scope(&t.name) {
                    return None;
                }
                self.close_cell();
                Some(Tok::End(t))
            }
            other => self.in_body(other),
        }
    }

    fn in_template(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(..) | Tok::Comment(_) | Tok::Doctype(_) => self.in_body(tok),
            Tok::Start(t) => match t.name.as_str() {
                "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style" | "template" | "title" => self.in_head(Tok::Start(t)),
                "caption" | "colgroup" | "tbody" | "tfoot" | "thead" => {
                    self.template_modes.pop();
                    self.template_modes.push(Mode::InTable);
                    self.mode = Mode::InTable;
                    Some(Tok::Start(t))
                }
                "col" => {
                    self.template_modes.pop();
                    self.template_modes.push(Mode::InColumnGroup);
                    self.mode = Mode::InColumnGroup;
                    Some(Tok::Start(t))
                }
                "tr" => {
                    self.template_modes.pop();
                    self.template_modes.push(Mode::InTableBody);
                    self.mode = Mode::InTableBody;
                    Some(Tok::Start(t))
                }
                "td" | "th" => {
                    self.template_modes.pop();
                    self.template_modes.push(Mode::InRow);
                    self.mode = Mode::InRow;
                    Some(Tok::Start(t))
                }
                _ => {
                    self.template_modes.pop();
                    self.template_modes.push(Mode::InBody);
                    self.mode = Mode::InBody;
                    Some(Tok::Start(t))
                }
            },
            Tok::End(t) if t.name == "template" => self.in_head(Tok::End(t)),
            Tok::End(_) => None,
            Tok::Eof => {
                if !self.has_template_on_stack() {
                    self.stop_parsing();
                    return None;
                }
                self.pop_until("template");
                self.clear_afe_to_marker();
                self.template_modes.pop();
                self.reset_insertion_mode();
                Some(Tok::Eof)
            }
        }
    }

    // ----- after body, framesets -----

    fn after_body(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(_, CharKind::Whitespace) => self.in_body(tok),
            Tok::Comment(c) => {
                let html = self.open[0];
                self.insert_comment(c, Some((html, None)));
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(t) if t.name == "html" => self.in_body(Tok::Start(t)),
            Tok::End(t) if t.name == "html" => {
                if self.context.is_some() {
                    return None;
                }
                self.mode = Mode::AfterAfterBody;
                None
            }
            Tok::Eof => {
                self.stop_parsing();
                None
            }
            other => {
                self.mode = Mode::InBody;
                Some(other)
            }
        }
    }

    fn in_frameset(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(s, CharKind::Whitespace) => {
                self.insert_text(&s);
                None
            }
            Tok::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(t) if t.name == "html" => self.in_body(Tok::Start(t)),
            Tok::Start(t) if t.name == "frameset" => {
                self.insert_html_element(&t);
                None
            }
            Tok::End(t) if t.name == "frameset" => {
                if self.open.len() == 1 {
                    return None;
                }
                self.open.pop();
                if self.context.is_none() && !self.is_html(self.current(), "frameset") {
                    self.mode = Mode::AfterFrameset;
                }
                None
            }
            Tok::Start(t) if t.name == "frame" => {
                self.insert_html_element(&t);
                self.open.pop();
                None
            }
            Tok::Start(t) if t.name == "noframes" => self.in_head(Tok::Start(t)),
            Tok::Eof => {
                self.stop_parsing();
                None
            }
            _ => None,
        }
    }

    fn after_frameset(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(s, CharKind::Whitespace) => {
                self.insert_text(&s);
                None
            }
            Tok::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(t) if t.name == "html" => self.in_body(Tok::Start(t)),
            Tok::End(t) if t.name == "html" => {
                self.mode = Mode::AfterAfterFrameset;
                None
            }
            Tok::Start(t) if t.name == "noframes" => self.in_head(Tok::Start(t)),
            Tok::Eof => {
                self.stop_parsing();
                None
            }
            _ => None,
        }
    }

    fn after_after_body(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Comment(c) => {
                self.insert_comment(c, Some((Document::ROOT, None)));
                None
            }
            Tok::Doctype(_) | Tok::Chars(_, CharKind::Whitespace) => self.in_body(tok),
            Tok::Start(t) if t.name == "html" => self.in_body(Tok::Start(t)),
            Tok::Eof => {
                self.stop_parsing();
                None
            }
            other => {
                self.mode = Mode::InBody;
                Some(other)
            }
        }
    }

    fn after_after_frameset(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Comment(c) => {
                self.insert_comment(c, Some((Document::ROOT, None)));
                None
            }
            Tok::Doctype(_) | Tok::Chars(_, CharKind::Whitespace) => self.in_body(tok),
            Tok::Start(t) if t.name == "html" => self.in_body(Tok::Start(t)),
            Tok::Eof => {
                self.stop_parsing();
                None
            }
            Tok::Start(t) if t.name == "noframes" => self.in_head(Tok::Start(t)),
            _ => None,
        }
    }

    // ----- foreign content -----

    /// Returns `Some(tok)` when the token must be processed by the HTML rules for the
    /// current insertion mode.
    fn foreign(&mut self, tok: Tok) -> Option<Tok> {
        match tok {
            Tok::Chars(s, CharKind::Null) => {
                let r: String = s.chars().map(|_| '\u{FFFD}').collect();
                self.insert_text(&r);
                None
            }
            Tok::Chars(s, CharKind::Whitespace) => {
                self.insert_text(&s);
                None
            }
            Tok::Chars(s, CharKind::Other) => {
                self.insert_text(&s);
                self.frameset_ok = false;
                None
            }
            Tok::Comment(c) => {
                self.insert_comment(c, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(mut t) => {
                let breakout = matches!(
                    t.name.as_str(),
                    "b" | "big" | "blockquote" | "body" | "br" | "center" | "code" | "dd" | "div" | "dl" | "dt" | "em" | "embed" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "head" | "hr" | "i" | "img" | "li" | "listing" | "menu" | "meta" | "nobr" | "ol" | "p" | "pre" | "ruby" | "s" | "small" | "span" | "strong" | "strike" | "sub" | "sup" | "table" | "tt" | "u" | "ul" | "var"
                ) || (t.name == "font" && t.attrs.iter().any(|a| matches!(a.name.as_str(), "color" | "face" | "size")));
                if breakout {
                    self.pop_foreign_to_html();
                    return Some(Tok::Start(t));
                }
                let acn = self.adjusted_current();
                let ns = self.name(acn).0;
                match ns {
                    Namespace::MathMl => adjust_mathml_attrs(&mut t),
                    Namespace::Svg => {
                        if let Some(n) = svg_tag_adjust(&t.name) {
                            t.name = n.to_owned();
                        }
                        adjust_svg_attrs(&mut t);
                    }
                    Namespace::Html => {}
                }
                self.insert_foreign_element(&t, ns);
                if t.self_closing {
                    // SVG <script/> would run the script; without scripts the two
                    // branches both pop the element.
                    self.open.pop();
                }
                None
            }
            Tok::End(t) => {
                if matches!(t.name.as_str(), "br" | "p") {
                    self.pop_foreign_to_html();
                    return Some(Tok::End(t));
                }
                if t.name == "script" && matches!(self.name(self.current()), (Namespace::Svg, "script")) {
                    self.open.pop();
                    return None;
                }
                let mut i = self.open.len() - 1;
                loop {
                    if i == 0 {
                        return None;
                    }
                    if self.name(self.open[i]).1.eq_ignore_ascii_case(&t.name) {
                        self.open.truncate(i);
                        return None;
                    }
                    i -= 1;
                    if self.name(self.open[i]).0 == Namespace::Html {
                        return Some(Tok::End(t));
                    }
                }
            }
            Tok::Eof => Some(Tok::Eof),
        }
    }

    fn pop_foreign_to_html(&mut self) {
        loop {
            let cur = self.current();
            if self.is_mathml_text_integration_point(cur) || self.is_html_integration_point(cur) || self.name(cur).0 == Namespace::Html {
                break;
            }
            self.open.pop();
        }
    }
}

fn quirks_mode_for(d: &Doctype) -> QuirksMode {
    let name = d.name.as_deref().unwrap_or("");
    let public = d.public_id.as_deref();
    let system = d.system_id.as_deref();
    let pub_lower = public.map(|p| p.to_ascii_lowercase()).unwrap_or_default();
    let sys_lower = system.map(|s| s.to_ascii_lowercase());
    const QUIRKY_PREFIXES: &[&str] = &[
        "+//silmaril//dtd html pro v0r11 19970101//",
        "-//as//dtd html 3.0 aswedit + extensions//",
        "-//advasoft ltd//dtd html 3.0 aswedit + extensions//",
        "-//ietf//dtd html 2.0 level 1//",
        "-//ietf//dtd html 2.0 level 2//",
        "-//ietf//dtd html 2.0 strict level 1//",
        "-//ietf//dtd html 2.0 strict level 2//",
        "-//ietf//dtd html 2.0 strict//",
        "-//ietf//dtd html 2.0//",
        "-//ietf//dtd html 2.1e//",
        "-//ietf//dtd html 3.0//",
        "-//ietf//dtd html 3.2 final//",
        "-//ietf//dtd html 3.2//",
        "-//ietf//dtd html 3//",
        "-//ietf//dtd html level 0//",
        "-//ietf//dtd html level 1//",
        "-//ietf//dtd html level 2//",
        "-//ietf//dtd html level 3//",
        "-//ietf//dtd html strict level 0//",
        "-//ietf//dtd html strict level 1//",
        "-//ietf//dtd html strict level 2//",
        "-//ietf//dtd html strict level 3//",
        "-//ietf//dtd html strict//",
        "-//ietf//dtd html//",
        "-//metrius//dtd metrius presentational//",
        "-//microsoft//dtd internet explorer 2.0 html strict//",
        "-//microsoft//dtd internet explorer 2.0 html//",
        "-//microsoft//dtd internet explorer 2.0 tables//",
        "-//microsoft//dtd internet explorer 3.0 html strict//",
        "-//microsoft//dtd internet explorer 3.0 html//",
        "-//microsoft//dtd internet explorer 3.0 tables//",
        "-//netscape comm. corp.//dtd html//",
        "-//netscape comm. corp.//dtd strict html//",
        "-//o'reilly and associates//dtd html 2.0//",
        "-//o'reilly and associates//dtd html extended 1.0//",
        "-//o'reilly and associates//dtd html extended relaxed 1.0//",
        "-//sq//dtd html 2.0 hotmetal + extensions//",
        "-//softquad software//dtd hotmetal pro 6.0::19990601::extensions to html 4.0//",
        "-//softquad//dtd hotmetal pro 4.0::19971010::extensions to html 4.0//",
        "-//spyglass//dtd html 2.0 extended//",
        "-//sun microsystems corp.//dtd hotjava html//",
        "-//sun microsystems corp.//dtd hotjava strict html//",
        "-//w3c//dtd html 3 1995-03-24//",
        "-//w3c//dtd html 3.2 draft//",
        "-//w3c//dtd html 3.2 final//",
        "-//w3c//dtd html 3.2//",
        "-//w3c//dtd html 3.2s draft//",
        "-//w3c//dtd html 4.0 frameset//",
        "-//w3c//dtd html 4.0 transitional//",
        "-//w3c//dtd html experimental 19960712//",
        "-//w3c//dtd html experimental 970421//",
        "-//w3c//dtd w3 html//",
        "-//w3o//dtd w3 html 3.0//",
        "-//webtechs//dtd mozilla html 2.0//",
        "-//webtechs//dtd mozilla html//",
    ];
    if d.force_quirks
        || name != "html"
        || matches!(pub_lower.as_str(), "-//w3o//dtd w3 html strict 3.0//en//" | "-/w3c/dtd html 4.0 transitional/en" | "html")
        || sys_lower.as_deref() == Some("http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd")
        || QUIRKY_PREFIXES.iter().any(|p| pub_lower.starts_with(p))
        || (system.is_none() && (pub_lower.starts_with("-//w3c//dtd html 4.01 frameset//") || pub_lower.starts_with("-//w3c//dtd html 4.01 transitional//")))
    {
        return QuirksMode::Quirks;
    }
    if pub_lower.starts_with("-//w3c//dtd xhtml 1.0 frameset//")
        || pub_lower.starts_with("-//w3c//dtd xhtml 1.0 transitional//")
        || (system.is_some() && (pub_lower.starts_with("-//w3c//dtd html 4.01 frameset//") || pub_lower.starts_with("-//w3c//dtd html 4.01 transitional//")))
    {
        return QuirksMode::LimitedQuirks;
    }
    QuirksMode::NoQuirks
}
