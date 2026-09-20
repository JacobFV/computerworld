//! The document: an arena of nodes with stable ids, tree operations, attributes and a
//! mutation log that restyle and relayout read. Shared by every module. Ids are dense
//! and deterministic (allocation order), never reused within one document, so they are
//! the snapshot representation and the key of every side table.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct NodeId(pub u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The document's compatibility mode, set by the parser from the doctype.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum QuirksMode {
    #[default]
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Namespace {
    #[default]
    Html,
    Svg,
    MathMl,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Attribute {
    /// Lower-cased for HTML elements.
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NodeKind {
    Document,
    DocumentFragment,
    DocType {
        name: String,
        /// Empty when the doctype had none.
        public_id: String,
        system_id: String,
    },
    Element {
        ns: Namespace,
        /// Lower-cased local name for HTML elements (`div`, `p`), case preserved for SVG.
        tag: String,
        attrs: Vec<Attribute>,
    },
    Text(String),
    Comment(String),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub prev_sibling: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
    /// True once removed from the tree; the slot is kept so ids stay stable.
    pub detached: bool,
}

/// What changed since the log was last drained; consumers decide what to invalidate.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mutation {
    Inserted(NodeId),
    Removed { node: NodeId, old_parent: NodeId },
    AttributeChanged { node: NodeId, name: String, old: Option<String> },
    TextChanged(NodeId),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Document {
    nodes: Vec<Node>,
    pub quirks: QuirksMode,
    /// Document URL, for resolving relative references; empty when unknown.
    pub url: String,
    pub mutations: Vec<Mutation>,
    /// Index from `id` attribute to element, kept in sync by attribute changes.
    ids: BTreeMap<String, Vec<NodeId>>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub const ROOT: NodeId = NodeId(0);

    pub fn new() -> Document {
        Document {
            nodes: vec![Node { kind: NodeKind::Document, parent: None, first_child: None, last_child: None, prev_sibling: None, next_sibling: None, detached: false }],
            quirks: QuirksMode::NoQuirks,
            url: String::new(),
            mutations: Vec::new(),
            ids: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }
    pub fn kind(&self, id: NodeId) -> &NodeKind {
        &self.nodes[id.index()].kind
    }

    /// Allocates a detached node.
    pub fn create(&mut self, kind: NodeKind) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node { kind, parent: None, first_child: None, last_child: None, prev_sibling: None, next_sibling: None, detached: true });
        if let NodeKind::Element { attrs, .. } = &self.nodes[id.index()].kind {
            if let Some(a) = attrs.iter().find(|a| a.name == "id") {
                let v = a.value.clone();
                self.ids.entry(v).or_default().push(id);
            }
        }
        id
    }
    pub fn create_element(&mut self, tag: &str, attrs: Vec<Attribute>) -> NodeId {
        self.create(NodeKind::Element { ns: Namespace::Html, tag: tag.to_ascii_lowercase(), attrs })
    }
    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.create(NodeKind::Text(text.to_owned()))
    }
    /// The template contents of a `<template>` element: its DocumentFragment child,
    /// where the parser puts everything between the template tags.
    pub fn template_contents(&self, id: NodeId) -> Option<NodeId> {
        if !self.is(id, "template") {
            return None;
        }
        self.children(id).find(|c| matches!(self.kind(*c), NodeKind::DocumentFragment))
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.index()].parent
    }
    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.index()].first_child
    }
    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.index()].last_child
    }
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.index()].next_sibling
    }
    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.index()].prev_sibling
    }
    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children { doc: self, next: self.first_child(id) }
    }
    /// The node and every descendant, in document order.
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants { doc: self, root: id, next: Some(id) }
    }
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_> {
        Ancestors { doc: self, next: self.parent(id) }
    }

    pub fn is_element(&self, id: NodeId) -> bool {
        matches!(self.kind(id), NodeKind::Element { .. })
    }
    pub fn tag(&self, id: NodeId) -> Option<&str> {
        match self.kind(id) {
            NodeKind::Element { tag, .. } => Some(tag),
            _ => None,
        }
    }
    pub fn is(&self, id: NodeId, tag: &str) -> bool {
        self.tag(id) == Some(tag)
    }
    pub fn text(&self, id: NodeId) -> Option<&str> {
        match self.kind(id) {
            NodeKind::Text(t) => Some(t),
            _ => None,
        }
    }
    pub fn attrs(&self, id: NodeId) -> &[Attribute] {
        match self.kind(id) {
            NodeKind::Element { attrs, .. } => attrs,
            _ => &[],
        }
    }
    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        self.attrs(id).iter().find(|a| a.name == name).map(|a| a.value.as_str())
    }
    pub fn has_attr(&self, id: NodeId, name: &str) -> bool {
        self.attr(id, name).is_some()
    }
    pub fn set_attr(&mut self, id: NodeId, name: &str, value: &str) {
        let name = name.to_ascii_lowercase();
        let old = self.attr(id, &name).map(str::to_owned);
        if old.as_deref() == Some(value) {
            return;
        }
        if name == "id" {
            if let Some(o) = &old {
                if let Some(v) = self.ids.get_mut(o) {
                    v.retain(|n| *n != id);
                }
            }
            self.ids.entry(value.to_owned()).or_default().push(id);
        }
        if let NodeKind::Element { attrs, .. } = &mut self.nodes[id.index()].kind {
            match attrs.iter_mut().find(|a| a.name == name) {
                Some(a) => a.value = value.to_owned(),
                None => attrs.push(Attribute { name: name.clone(), value: value.to_owned() }),
            }
        }
        self.mutations.push(Mutation::AttributeChanged { node: id, name, old });
    }
    pub fn remove_attr(&mut self, id: NodeId, name: &str) {
        let name = name.to_ascii_lowercase();
        let Some(old) = self.attr(id, &name).map(str::to_owned) else { return };
        if name == "id" {
            if let Some(v) = self.ids.get_mut(&old) {
                v.retain(|n| *n != id);
            }
        }
        if let NodeKind::Element { attrs, .. } = &mut self.nodes[id.index()].kind {
            attrs.retain(|a| a.name != name);
        }
        self.mutations.push(Mutation::AttributeChanged { node: id, name, old: Some(old) });
    }
    pub fn set_text(&mut self, id: NodeId, text: &str) {
        if let NodeKind::Text(t) = &mut self.nodes[id.index()].kind {
            if t == text {
                return;
            }
            *t = text.to_owned();
            self.mutations.push(Mutation::TextChanged(id));
        }
    }
    /// The element(s) with this `id`, in document order of insertion; the first is
    /// what `getElementById` returns.
    pub fn by_id(&self, id: &str) -> &[NodeId] {
        self.ids.get(id).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn classes(&self, id: NodeId) -> impl Iterator<Item = &str> {
        self.attr(id, "class").unwrap_or("").split_ascii_whitespace()
    }
    pub fn has_class(&self, id: NodeId, class: &str) -> bool {
        self.classes(id).any(|c| c == class)
    }

    pub fn append(&mut self, parent: NodeId, child: NodeId) {
        self.insert_before(parent, child, None);
    }
    /// Inserts `child` into `parent` before `before` (or at the end). Detaches it from
    /// its current position first.
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        assert_ne!(parent, child);
        debug_assert!(!self.ancestors(parent).any(|a| a == child), "cycle");
        if self.nodes[child.index()].parent.is_some() {
            self.detach(child);
        }
        let n = &mut self.nodes[child.index()];
        n.parent = Some(parent);
        n.detached = false;
        match before {
            Some(b) => {
                debug_assert_eq!(self.nodes[b.index()].parent, Some(parent));
                let prev = self.nodes[b.index()].prev_sibling;
                self.nodes[child.index()].prev_sibling = prev;
                self.nodes[child.index()].next_sibling = Some(b);
                self.nodes[b.index()].prev_sibling = Some(child);
                match prev {
                    Some(p) => self.nodes[p.index()].next_sibling = Some(child),
                    None => self.nodes[parent.index()].first_child = Some(child),
                }
            }
            None => {
                let last = self.nodes[parent.index()].last_child;
                self.nodes[child.index()].prev_sibling = last;
                self.nodes[child.index()].next_sibling = None;
                match last {
                    Some(l) => self.nodes[l.index()].next_sibling = Some(child),
                    None => self.nodes[parent.index()].first_child = Some(child),
                }
                self.nodes[parent.index()].last_child = Some(child);
            }
        }
        self.mutations.push(Mutation::Inserted(child));
    }
    /// Removes the node from its parent; it and its subtree keep their ids.
    pub fn detach(&mut self, id: NodeId) {
        let n = self.nodes[id.index()].clone();
        let Some(parent) = n.parent else { return };
        match n.prev_sibling {
            Some(p) => self.nodes[p.index()].next_sibling = n.next_sibling,
            None => self.nodes[parent.index()].first_child = n.next_sibling,
        }
        match n.next_sibling {
            Some(x) => self.nodes[x.index()].prev_sibling = n.prev_sibling,
            None => self.nodes[parent.index()].last_child = n.prev_sibling,
        }
        let m = &mut self.nodes[id.index()];
        m.parent = None;
        m.prev_sibling = None;
        m.next_sibling = None;
        m.detached = true;
        self.mutations.push(Mutation::Removed { node: id, old_parent: parent });
    }
    /// Deep clone of a subtree into new ids (detached).
    pub fn clone_subtree(&mut self, id: NodeId) -> NodeId {
        let kind = self.kind(id).clone();
        let new = self.create(kind);
        let kids: Vec<NodeId> = self.children(id).collect();
        for k in kids {
            let c = self.clone_subtree(k);
            self.append(new, c);
        }
        new
    }
    pub fn drain_mutations(&mut self) -> Vec<Mutation> {
        std::mem::take(&mut self.mutations)
    }

    /// Concatenated text of all descendant text nodes.
    pub fn text_content(&self, id: NodeId) -> String {
        let mut s = String::new();
        for d in self.descendants(id) {
            if let NodeKind::Text(t) = self.kind(d) {
                s.push_str(t);
            }
        }
        s
    }
    /// The `<html>` element, if the document has one.
    pub fn document_element(&self) -> Option<NodeId> {
        self.children(Self::ROOT).find(|c| self.is_element(*c))
    }
    pub fn body(&self) -> Option<NodeId> {
        let html = self.document_element()?;
        self.children(html).find(|c| self.is(*c, "body") || self.is(*c, "frameset"))
    }
    pub fn head(&self) -> Option<NodeId> {
        let html = self.document_element()?;
        self.children(html).find(|c| self.is(*c, "head"))
    }
    /// Element children only.
    pub fn element_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(id).filter(move |c| self.is_element(*c))
    }
    /// Position among element siblings, 1-based, for `:nth-child`.
    pub fn element_index(&self, id: NodeId) -> usize {
        let mut i = 1;
        let mut cur = self.prev_sibling(id);
        while let Some(c) = cur {
            if self.is_element(c) {
                i += 1;
            }
            cur = self.prev_sibling(c);
        }
        i
    }
    pub fn element_index_from_end(&self, id: NodeId) -> usize {
        let mut i = 1;
        let mut cur = self.next_sibling(id);
        while let Some(c) = cur {
            if self.is_element(c) {
                i += 1;
            }
            cur = self.next_sibling(c);
        }
        i
    }
}

pub struct Children<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}
impl Iterator for Children<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let c = self.next?;
        self.next = self.doc.next_sibling(c);
        Some(c)
    }
}

pub struct Descendants<'a> {
    doc: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
}
impl Iterator for Descendants<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        // Pre-order: first child, else next sibling, else climb until a sibling exists.
        self.next = if let Some(c) = self.doc.first_child(cur) {
            Some(c)
        } else {
            let mut n = cur;
            loop {
                if n == self.root {
                    break None;
                }
                if let Some(s) = self.doc.next_sibling(n) {
                    break Some(s);
                }
                match self.doc.parent(n) {
                    Some(p) if p != self.root => n = p,
                    _ => break None,
                }
            }
        };
        Some(cur)
    }
}

pub struct Ancestors<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}
impl Iterator for Ancestors<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let c = self.next?;
        self.next = self.doc.parent(c);
        Some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_ops_and_traversal() {
        let mut d = Document::new();
        let html = d.create_element("HTML", vec![]);
        d.append(Document::ROOT, html);
        let body = d.create_element("body", vec![Attribute { name: "id".into(), value: "b".into() }]);
        d.append(html, body);
        let p1 = d.create_element("p", vec![]);
        let p2 = d.create_element("p", vec![]);
        let t = d.create_text("hi");
        d.append(body, p2);
        d.insert_before(body, p1, Some(p2));
        d.append(p1, t);
        assert_eq!(d.tag(html), Some("html"));
        assert_eq!(d.body(), Some(body));
        assert_eq!(d.children(body).collect::<Vec<_>>(), vec![p1, p2]);
        assert_eq!(d.descendants(Document::ROOT).collect::<Vec<_>>(), vec![Document::ROOT, html, body, p1, t, p2]);
        assert_eq!(d.by_id("b"), &[body]);
        assert_eq!(d.element_index(p2), 2);
        assert_eq!(d.element_index_from_end(p1), 2);
        assert_eq!(d.text_content(body), "hi");
        d.detach(p1);
        assert_eq!(d.children(body).collect::<Vec<_>>(), vec![p2]);
        assert!(d.node(p1).detached);
        d.set_attr(body, "ID", "c");
        assert!(d.by_id("b").is_empty());
        assert_eq!(d.by_id("c"), &[body]);
        assert!(matches!(d.drain_mutations().last(), Some(Mutation::AttributeChanged { .. })));
        let c = d.clone_subtree(p1);
        assert_eq!(d.text_content(c), "hi");
        assert_ne!(c, p1);
    }
}
