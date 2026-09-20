//! The accessibility layer: `Node::semantic`, `Node::state` and `Node::interaction`
//! from the DOM.
//!
//! Roles come from the `role` attribute, else from the tag: `a[href]` is a link,
//! `button` and `input[type=button|submit|reset]` a button, `h1`..`h6` a heading
//! whose level is in the label (`h2: Title`), text-like inputs and `textarea` a
//! textbox, `input[type=checkbox|radio]` a checkbox or radio, `select` a combobox,
//! `ul`/`ol` a list and `li` a listitem, `table`/`tr`/`td`/`th` table, row and cell,
//! `img` img (label from `alt`), `nav` navigation, `main` main, `header` banner,
//! `footer` contentinfo, `form` form, `article` article, `section[aria-label]` a
//! region, `dialog` dialog.
//!
//! Labels: `aria-label`, then `aria-labelledby` (the referenced elements' text),
//! then a `<label for>` pointing at the element or a wrapping `<label>`, then
//! `alt`, `title`, and finally the element's own text content, trimmed and
//! whitespace-collapsed, capped at 200 characters.
//!
//! An element with an `id` also gets an entry, with `generic` for a role when the
//! tag gives none: the id is how an agent addresses the element, so it has to be
//! reachable through the tree and not only through the DOM.
//!
//! Interaction ids follow the existing browser's scheme, where a control's
//! interaction string is the element's own id (`Browser::click(id)`, `fill(id)` and
//! `submit(id)` look the element up by it and the session keys typed values by it).
//! An element with an `id` attribute uses it verbatim; one without gets a
//! deterministic path (`/html/body/div[2]/a[1]`, 1-based among same-tag siblings),
//! which is stable across paints and across unrelated edits elsewhere in the
//! document. Scroll containers are `pane:<interaction id>`; the document is
//! `pane:page`.

use std::collections::BTreeMap;

use cw_scene::{NodeState, Primitive, Rect as SRect, Semantic};

use super::{parts, Painter, State};
use crate::dom::{Document, NodeId, NodeKind};
use crate::layout::fragment::{ControlKind, StyleSource};

/// Precomputed lookups over the document.
#[derive(Clone, Debug, Default)]
pub struct Tables {
    /// `for` attribute → label text, for `<label for=…>`.
    pub labels_for: BTreeMap<String, String>,
    /// Document order of every node, for selection ranges.
    pub order: BTreeMap<NodeId, u32>,
}

impl Tables {
    pub fn build(doc: &Document) -> Tables {
        let mut t = Tables::default();
        for (i, n) in doc.descendants(Document::ROOT).enumerate() {
            t.order.insert(n, i as u32);
            if doc.is(n, "label") {
                if let Some(f) = doc.attr(n, "for") {
                    t.labels_for.entry(f.to_owned()).or_insert_with(|| collapse(&doc.text_content(n)));
                }
            }
        }
        t
    }
}

/// Whitespace-collapsed, trimmed, capped text.
pub fn collapse(s: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
        if out.chars().count() >= 200 {
            break;
        }
    }
    out
}

/// The interaction id of an element: its `id`, else its path.
pub fn interaction_id(doc: &Document, node: NodeId) -> String {
    if let Some(id) = doc.attr(node, "id") {
        if !id.is_empty() {
            return id.to_owned();
        }
    }
    path_id(doc, node)
}

/// `/html/body/div[2]/a[1]`: the tag path with 1-based indices among same-tag
/// siblings.
pub fn path_id(doc: &Document, node: NodeId) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = Some(node);
    while let Some(n) = cur {
        let Some(tag) = doc.tag(n) else { break };
        let mut idx = 1;
        let mut prev = doc.prev_sibling(n);
        while let Some(s) = prev {
            if doc.tag(s) == Some(tag) {
                idx += 1;
            }
            prev = doc.prev_sibling(s);
        }
        parts.push(format!("{tag}[{idx}]"));
        cur = doc.parent(n);
    }
    parts.reverse();
    let mut s = String::new();
    for p in parts {
        s.push('/');
        s.push_str(&p);
    }
    s
}

fn input_type(doc: &Document, node: NodeId) -> String {
    doc.attr(node, "type").unwrap_or("text").trim().to_ascii_lowercase()
}

/// The ARIA role of an element, if it has one worth announcing.
pub fn role_of(doc: &Document, node: NodeId) -> Option<String> {
    if let Some(r) = doc.attr(node, "role") {
        let r = r.split_ascii_whitespace().next().unwrap_or("").to_ascii_lowercase();
        if !r.is_empty() {
            return Some(r);
        }
    }
    let tag = doc.tag(node)?;
    let role = match tag {
        "a" | "area" => {
            if doc.has_attr(node, "href") {
                "link"
            } else {
                return None;
            }
        }
        "button" | "summary" => "button",
        "input" => match input_type(doc, node).as_str() {
            "button" | "submit" | "reset" | "image" => "button",
            "checkbox" => "checkbox",
            "radio" => "radio",
            "range" => "slider",
            "hidden" => return None,
            "file" => "button",
            "color" => "button",
            _ => "textbox",
        },
        "textarea" => "textbox",
        "select" => "combobox",
        "option" => "option",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
        "ul" | "ol" | "menu" => "list",
        "li" => "listitem",
        "table" => "table",
        "tr" => "row",
        "td" => "cell",
        "th" => "columnheader",
        "img" => "img",
        "nav" => "navigation",
        "main" => "main",
        "header" => "banner",
        "footer" => "contentinfo",
        "form" => "form",
        "article" => "article",
        "section" | "aside" => {
            if doc.has_attr(node, "aria-label") || doc.has_attr(node, "aria-labelledby") {
                if tag == "aside" {
                    "complementary"
                } else {
                    "region"
                }
            } else if tag == "aside" {
                "complementary"
            } else {
                return None;
            }
        }
        "dialog" => "dialog",
        "hr" => "separator",
        "progress" | "meter" => "progressbar",
        "details" => "group",
        "fieldset" => "group",
        "label" => "label",
        "p" | "blockquote" | "pre" => "paragraph",
        _ => return None,
    };
    Some(role.to_owned())
}

/// The accessible name.
pub fn label_of(doc: &Document, tables: &Tables, node: NodeId) -> String {
    if let Some(l) = doc.attr(node, "aria-label") {
        let l = collapse(l);
        if !l.is_empty() {
            return l;
        }
    }
    if let Some(ids) = doc.attr(node, "aria-labelledby") {
        let mut s = String::new();
        for id in ids.split_ascii_whitespace() {
            if let Some(n) = doc.by_id(id).first() {
                let t = collapse(&doc.text_content(*n));
                if !t.is_empty() {
                    if !s.is_empty() {
                        s.push(' ');
                    }
                    s.push_str(&t);
                }
            }
        }
        if !s.is_empty() {
            return s;
        }
    }
    let tag = doc.tag(node).unwrap_or("");
    let is_control = matches!(tag, "input" | "select" | "textarea" | "button" | "meter" | "progress");
    if is_control {
        if let Some(id) = doc.attr(node, "id") {
            if let Some(l) = tables.labels_for.get(id) {
                if !l.is_empty() {
                    return l.clone();
                }
            }
        }
        if let Some(l) = doc.ancestors(node).find(|a| doc.is(*a, "label")) {
            let t = collapse(&doc.text_content(l));
            if !t.is_empty() {
                return t;
            }
        }
    }
    if let Some(alt) = doc.attr(node, "alt") {
        let a = collapse(alt);
        if !a.is_empty() || tag == "img" {
            return a;
        }
    }
    if tag == "input" {
        let t = input_type(doc, node);
        if matches!(t.as_str(), "submit" | "button" | "reset") {
            if let Some(v) = doc.attr(node, "value") {
                return collapse(v);
            }
            return match t.as_str() {
                "submit" => "Submit".into(),
                "reset" => "Reset".into(),
                _ => String::new(),
            };
        }
        if let Some(pl) = doc.attr(node, "placeholder") {
            return collapse(pl);
        }
    }
    if let Some(t) = doc.attr(node, "title") {
        let t = collapse(t);
        if !t.is_empty() {
            return t;
        }
    }
    let text = collapse(&doc.text_content(node));
    if let Some(level) = tag.strip_prefix('h').and_then(|l| l.parse::<u8>().ok()).filter(|l| (1..=6).contains(l)) {
        return format!("h{level}: {text}");
    }
    text
}

/// The current value of a control.
pub(crate) fn value_of(p: &Painter, node: NodeId) -> Option<String> {
    let doc = p.doc?;
    if let Some(v) = p.ctx.values.get(&node) {
        return Some(v.clone());
    }
    match doc.tag(node)? {
        "input" => {
            let t = input_type(doc, node);
            match t.as_str() {
                "checkbox" | "radio" | "submit" | "button" | "reset" | "hidden" | "image" => None,
                _ => Some(doc.attr(node, "value").unwrap_or("").to_owned()),
            }
        }
        "textarea" => Some(doc.text_content(node)),
        "select" => selected_option(doc, node).map(|o| collapse(&doc.text_content(o))).or_else(|| Some(String::new())),
        "progress" | "meter" => doc.attr(node, "value").map(str::to_owned),
        _ => None,
    }
}

/// The selected `<option>` of a select: the last with `selected`, else the first.
pub fn selected_option(doc: &Document, select: NodeId) -> Option<NodeId> {
    let options: Vec<NodeId> = doc.descendants(select).filter(|n| doc.is(*n, "option")).collect();
    options.iter().rev().find(|o| doc.has_attr(**o, "selected")).or(options.first()).copied()
}

pub fn is_disabled(doc: &Document, node: NodeId) -> bool {
    if doc.has_attr(node, "disabled") || doc.attr(node, "aria-disabled") == Some("true") {
        return true;
    }
    doc.ancestors(node).any(|a| doc.is(a, "fieldset") && doc.has_attr(a, "disabled"))
}

pub fn is_focusable(doc: &Document, node: NodeId) -> bool {
    if let Some(t) = doc.attr(node, "tabindex") {
        return t.trim().parse::<i32>().is_ok_and(|v| v >= 0);
    }
    match doc.tag(node).unwrap_or("") {
        "a" | "area" => doc.has_attr(node, "href"),
        "button" | "select" | "textarea" | "summary" | "iframe" => true,
        "input" => input_type(doc, node) != "hidden",
        _ => doc.attr(node, "contenteditable").is_some_and(|v| v != "false"),
    }
}

/// Whether clicking the element does something the browser routes.
pub fn is_interactive(doc: &Document, node: NodeId) -> bool {
    if is_focusable(doc, node) {
        return true;
    }
    match doc.tag(node).unwrap_or("") {
        "form" | "label" | "option" | "details" => true,
        _ => matches!(doc.attr(node, "role"), Some("button" | "link" | "checkbox" | "radio" | "tab" | "menuitem" | "switch" | "option")) || doc.has_attr(node, "onclick"),
    }
}

/// The checked/selected/expanded state of an element.
pub fn state_of(doc: &Document, node: NodeId, focused: bool) -> Option<NodeState> {
    let mut s = NodeState { focused, ..NodeState::default() };
    match doc.tag(node).unwrap_or("") {
        "input" => {
            let t = input_type(doc, node);
            if t == "checkbox" || t == "radio" {
                s.checked = Some(doc.has_attr(node, "checked") || doc.attr(node, "aria-checked") == Some("true"));
            }
        }
        "option" => {
            s.selected = Some(doc.has_attr(node, "selected"));
        }
        "details" => s.expanded = Some(doc.has_attr(node, "open")),
        _ => {}
    }
    if let Some(v) = doc.attr(node, "aria-checked") {
        s.checked = Some(v == "true");
    }
    if let Some(v) = doc.attr(node, "aria-expanded") {
        s.expanded = Some(v == "true");
    }
    if let Some(v) = doc.attr(node, "aria-selected") {
        s.selected = Some(v == "true");
    }
    (!s.is_empty()).then_some(s)
}

/// What a fragment of `node` announces: the semantic, the interaction id and state.
pub(crate) fn element(p: &Painter, node: NodeId) -> Option<(Semantic, Option<String>, Option<NodeState>)> {
    let doc = p.doc?;
    if node.index() >= doc.len() {
        return None;
    }
    let role = role_of(doc, node);
    let interactive = is_interactive(doc, node);
    // An `id` is an address the author put there for something to use: an agent
    // told to read `#sheet-A2` or `#title` has to find it in the tree, so an
    // element carrying one gets an entry of its own even when its role is generic.
    let addressed = doc.attr(node, "id").is_some_and(|v| !v.trim().is_empty());
    if role.is_none() && !interactive && !addressed {
        return None;
    }
    let focused = p.ctx.focused == Some(node);
    let sem = Semantic {
        role: role.unwrap_or_else(|| "generic".into()),
        label: label_of(doc, &p.semantics, node),
        value: value_of(p, node),
        disabled: is_disabled(doc, node),
        focusable: is_focusable(doc, node),
    };
    let interaction = (interactive || addressed).then(|| interaction_id(doc, node));
    Some((sem, interaction, state_of(doc, node, focused)))
}

/// Emits the interaction region for an element's fragment and fills the scene focus
/// when the element is focused.
pub(crate) fn paint_region(p: &mut Painter, key: (NodeId, u32), state: &State, source: StyleSource, rect: SRect) {
    let StyleSource::Element(node) = source else { return };
    let Some((sem, interaction, node_state)) = element(p, node) else { return };
    let id = p.id(key, parts::REGION);
    let i = p.emit(state, id, rect, Primitive::Region);
    let focused = p.ctx.focused == Some(node);
    let n = &mut p.nodes[i];
    n.interaction = interaction.clone();
    n.state = node_state;
    n.semantic = Some(sem.clone());
    if focused && p.focus.is_none() {
        let doc = p.doc.unwrap();
        let text_entry = match doc.tag(node) {
            Some("textarea") => true,
            Some("input") => !matches!(input_type(doc, node).as_str(), "checkbox" | "radio" | "submit" | "button" | "reset" | "hidden" | "range" | "file" | "color" | "image"),
            _ => doc.attr(node, "contenteditable").is_some_and(|v| v != "false"),
        };
        p.focus = Some(cw_scene::Focus {
            window: None,
            node: Some(id),
            interaction: interaction.clone(),
            role: sem.role.clone(),
            label: sem.label.clone(),
            value: sem.value.clone(),
            caret: None,
            keyboard: cw_scene::Keyboard { route: "page".into(), window: None, target: interaction, text_entry },
        });
    }
}

/// The control kind of an input element, for the paint of its content.
#[allow(dead_code)]
pub fn control_kind(doc: &Document, node: NodeId) -> Option<ControlKind> {
    match doc.tag(node)? {
        "textarea" => Some(ControlKind::TextArea),
        "select" => Some(ControlKind::Select),
        "button" => Some(ControlKind::Button),
        "input" => Some(match input_type(doc, node).as_str() {
            "password" => ControlKind::Password,
            "checkbox" => ControlKind::Checkbox,
            "radio" => ControlKind::Radio,
            "button" | "reset" | "image" => ControlKind::Button,
            "submit" => ControlKind::Submit,
            "range" => ControlKind::Range,
            "file" => ControlKind::File,
            "color" => ControlKind::Color,
            "hidden" => ControlKind::Hidden,
            _ => ControlKind::TextInput,
        }),
        _ => None,
    }
}

/// Text semantic for a run.
#[allow(dead_code)]
pub fn text_semantic(text: &str) -> Semantic {
    Semantic { role: "text".into(), label: text.to_owned(), value: None, disabled: false, focusable: false }
}

#[allow(dead_code)]
fn is_text(doc: &Document, n: NodeId) -> bool {
    matches!(doc.kind(n), NodeKind::Text(_))
}
