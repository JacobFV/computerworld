//! Style invalidation: which elements a DOM or matching-state change can restyle.
//!
//! Every selector the cascade engine indexes is folded into an [`InvalidationMap`]:
//! for each feature a selector reads (a class, an id, an attribute, a dynamic
//! pseudo-class, or sibling structure) it records where in a selector the feature
//! sits, and so which elements a change of it on element `E` can make match or stop
//! matching:
//!
//! - in the subject (rightmost) compound: `E` itself;
//! - in a compound to the left of a descendant or child combinator: descendants of
//!   `E` that carry one of the subject compound's classes, ids, tags or attributes
//!   (every descendant when the subject names none);
//! - to the left of a sibling combinator: `E`'s following siblings and their
//!   subtrees;
//! - inside the `of S` of `:nth-*()`: every child of `E`'s parent and their subtrees.
//!
//! `:has()` anywhere makes every change restyle the document. A change to one element
//! turns into a set of elements to rematch and a set of subtrees to rematch; the
//! cascade then walks only the paths to them, and recomputes a child whose parent's
//! computed style changed (inheritance) without rematching it.

use std::collections::{BTreeMap, BTreeSet};

use crate::css::{
    Combinator, ComplexSelector, CompoundSelector, MatchContext, PseudoClass, SimpleSelector,
};
use crate::dom::{Document, NodeId};

/// Where a feature sits in the selectors that read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Invalidation {
    /// In a subject compound: the element itself.
    pub self_: bool,
    /// Left of a descendant or child combinator: these descendants.
    pub descendants: Option<Descendants>,
    /// Left of a sibling combinator: the following siblings' subtrees.
    pub siblings: bool,
    /// In the `of S` of `:nth-*()`: every child of the parent, with subtrees.
    pub parent_subtree: bool,
}

/// The descendants a feature change can affect.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Descendants {
    pub all: bool,
    pub classes: BTreeSet<String>,
    pub ids: BTreeSet<String>,
    pub tags: BTreeSet<String>,
    pub attributes: BTreeSet<String>,
}

impl Descendants {
    fn merge(&mut self, o: &Descendants) {
        self.all |= o.all;
        if self.all {
            self.classes.clear();
            self.ids.clear();
            self.tags.clear();
            self.attributes.clear();
            return;
        }
        self.classes.extend(o.classes.iter().cloned());
        self.ids.extend(o.ids.iter().cloned());
        self.tags.extend(o.tags.iter().cloned());
        self.attributes.extend(o.attributes.iter().cloned());
    }
    fn all() -> Descendants {
        Descendants {
            all: true,
            ..Descendants::default()
        }
    }
    /// Whether `node` is one of these descendants.
    fn covers(&self, doc: &Document, node: NodeId) -> bool {
        if self.all {
            return true;
        }
        if let Some(t) = doc.tag(node) {
            if self.tags.contains(&t.to_ascii_lowercase()) {
                return true;
            }
        }
        if !self.classes.is_empty() && doc.classes(node).any(|c| self.classes.contains(c)) {
            return true;
        }
        if let Some(id) = doc.attr(node, "id") {
            if self.ids.contains(id) {
                return true;
            }
        }
        !self.attributes.is_empty()
            && doc
                .attrs(node)
                .iter()
                .any(|a| self.attributes.contains(&a.name.to_ascii_lowercase()))
    }
}

impl Invalidation {
    fn merge(&mut self, o: &Invalidation) {
        self.self_ |= o.self_;
        self.siblings |= o.siblings;
        self.parent_subtree |= o.parent_subtree;
        if let Some(d) = &o.descendants {
            self.descendants
                .get_or_insert_with(Descendants::default)
                .merge(d);
        }
    }
}

/// Features whose matching depends on element state the realm keeps outside the
/// document (form state), for elements a caller reports as changed.
const FORM_PSEUDOS: &[&str] = &[
    "checked",
    "indeterminate",
    "placeholder-shown",
    "default",
    "disabled",
    "enabled",
    "read-only",
    "read-write",
    "required",
    "optional",
];

/// What every indexed selector reads, by feature.
#[derive(Clone, Debug, Default)]
pub(crate) struct InvalidationMap {
    classes: BTreeMap<String, Invalidation>,
    ids: BTreeMap<String, Invalidation>,
    /// Lower-cased attribute names, including those a pseudo-class reads.
    attributes: BTreeMap<String, Invalidation>,
    /// Dynamic pseudo-classes by name (`hover`, `focus-within`, `checked`, ...).
    pseudos: BTreeMap<&'static str, Invalidation>,
    /// Sibling structure: compounds with a structural pseudo-class or beside a
    /// sibling combinator. Applied to every child of a parent whose children change.
    structural: Invalidation,
    /// `:empty` compounds, applied to a parent whose children change.
    empty: Invalidation,
    /// `:has()` somewhere: every change restyles the document.
    pub has: bool,
    /// `:lang()`, `:dir()`: read attributes of ancestors.
    lang_or_dir: bool,
    /// `:disabled`/`:enabled`/`:read-only`/`:read-write`: read ancestors (a disabled
    /// fieldset, `contenteditable`).
    ancestor_form: bool,
    /// `:default`: reads the form's first submit button.
    default: bool,
}

/// The dynamic pseudo-classes by name, with the attributes their matching reads.
fn pseudo_name(pc: &PseudoClass) -> Option<(&'static str, &'static [&'static str])> {
    Some(match pc {
        PseudoClass::Hover => ("hover", &[]),
        PseudoClass::Active => ("active", &[]),
        PseudoClass::Focus => ("focus", &[]),
        PseudoClass::FocusVisible => ("focus-visible", &[]),
        PseudoClass::FocusWithin => ("focus-within", &[]),
        PseudoClass::Target => ("target", &["id", "name"]),
        PseudoClass::Visited => ("visited", &["href"]),
        PseudoClass::Link => ("link", &["href"]),
        PseudoClass::AnyLink => ("any-link", &["href"]),
        PseudoClass::Checked => (
            "checked",
            &["checked", "selected", "value", "type", "name", "multiple"],
        ),
        PseudoClass::Indeterminate => ("indeterminate", &["type", "value"]),
        PseudoClass::PlaceholderShown => ("placeholder-shown", &["placeholder", "value", "type"]),
        PseudoClass::Default => ("default", &["checked", "selected", "type"]),
        PseudoClass::Disabled => ("disabled", &["disabled", "type"]),
        PseudoClass::Enabled => ("enabled", &["disabled", "type"]),
        PseudoClass::Required => ("required", &["required", "type"]),
        PseudoClass::Optional => ("optional", &["required", "type"]),
        PseudoClass::ReadOnly => (
            "read-only",
            &["readonly", "disabled", "contenteditable", "type"],
        ),
        PseudoClass::ReadWrite => (
            "read-write",
            &["readonly", "disabled", "contenteditable", "type"],
        ),
        PseudoClass::Lang(_) => ("lang", &["lang", "xml:lang"]),
        PseudoClass::Dir(_) => ("dir", &["dir"]),
        _ => return None,
    })
}

fn is_structural(pc: &PseudoClass) -> bool {
    matches!(
        pc,
        PseudoClass::FirstChild
            | PseudoClass::LastChild
            | PseudoClass::OnlyChild
            | PseudoClass::FirstOfType
            | PseudoClass::LastOfType
            | PseudoClass::OnlyOfType
            | PseudoClass::Nth { .. }
    )
}

/// The descendants a change left of a descendant combinator reaches: those the
/// subject compound names (by its positive class, id, tag and attribute selectors).
fn subject_features(subject: &CompoundSelector) -> Descendants {
    let mut d = Descendants::default();
    for s in &subject.simple {
        match s {
            SimpleSelector::Class(c) => {
                d.classes.insert(c.clone());
            }
            SimpleSelector::Id(i) => {
                d.ids.insert(i.clone());
            }
            SimpleSelector::Type(t) => {
                d.tags.insert(t.to_ascii_lowercase());
            }
            SimpleSelector::Attribute { name, .. } => {
                d.attributes.insert(name.to_ascii_lowercase());
            }
            _ => {}
        }
    }
    // One positive feature is enough to find every element the compound can match
    // (it must carry all of them); keep the whole set, which is no less exact.
    if d.classes.is_empty() && d.ids.is_empty() && d.tags.is_empty() && d.attributes.is_empty() {
        d.all = true;
    }
    d
}

impl InvalidationMap {
    /// Folds one indexed selector in.
    pub fn add(&mut self, sel: &ComplexSelector) {
        let n = sel.compounds.len();
        if n == 0 {
            return;
        }
        let subject = subject_features(&sel.compounds[n - 1]);
        for (k, compound) in sel.compounds.iter().enumerate() {
            let right = &sel.combinators[k..];
            let mut inv = Invalidation::default();
            if k + 1 == n {
                inv.self_ = true;
            }
            if right
                .iter()
                .any(|c| matches!(c, Combinator::Descendant | Combinator::Child))
            {
                inv.descendants = Some(subject.clone());
            }
            if right
                .iter()
                .any(|c| matches!(c, Combinator::NextSibling | Combinator::SubsequentSibling))
            {
                inv.siblings = true;
            }
            // Beside a sibling combinator (on either side), the compound's match
            // depends on sibling structure.
            let beside_sibling = right.first().is_some_and(|c| {
                matches!(c, Combinator::NextSibling | Combinator::SubsequentSibling)
            }) || (k > 0
                && matches!(
                    sel.combinators[k - 1],
                    Combinator::NextSibling | Combinator::SubsequentSibling
                ));
            if beside_sibling {
                self.structural.merge(&inv);
            }
            self.add_compound(compound, &inv);
        }
    }

    fn add_compound(&mut self, compound: &CompoundSelector, inv: &Invalidation) {
        for s in &compound.simple {
            self.add_simple(s, inv);
        }
    }

    fn add_simple(&mut self, s: &SimpleSelector, inv: &Invalidation) {
        match s {
            SimpleSelector::Class(c) => {
                self.classes.entry(c.clone()).or_default().merge(inv);
            }
            SimpleSelector::Id(i) => {
                self.ids.entry(i.clone()).or_default().merge(inv);
            }
            SimpleSelector::Attribute { name, .. } => {
                self.attributes
                    .entry(name.to_ascii_lowercase())
                    .or_default()
                    .merge(inv);
            }
            SimpleSelector::Type(_) | SimpleSelector::Universal | SimpleSelector::Nesting => {}
            SimpleSelector::PseudoClass(pc) => {
                if is_structural(pc) {
                    self.structural.merge(inv);
                }
                match pc {
                    PseudoClass::Empty => self.empty.merge(inv),
                    PseudoClass::Has(_) => self.has = true,
                    PseudoClass::Lang(_) | PseudoClass::Dir(_) => self.lang_or_dir = true,
                    PseudoClass::Disabled
                    | PseudoClass::Enabled
                    | PseudoClass::ReadOnly
                    | PseudoClass::ReadWrite => self.ancestor_form = true,
                    PseudoClass::Default => self.default = true,
                    _ => {}
                }
                if let Some((name, attrs)) = pseudo_name(pc) {
                    self.pseudos.entry(name).or_default().merge(inv);
                    for a in attrs {
                        self.attributes
                            .entry((*a).to_owned())
                            .or_default()
                            .merge(inv);
                    }
                }
                match pc {
                    PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
                        for c in &list.0 {
                            self.add_nested(c, inv, false);
                        }
                    }
                    PseudoClass::Nth { of: Some(list), .. } => {
                        for c in &list.0 {
                            self.add_nested(c, inv, true);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// A selector inside `:is()`, `:not()`, `:where()` or `:nth-*(of S)`, which sits
    /// where its enclosing compound sits. Its own combinators, if any, are treated
    /// conservatively: every descendant and every following sibling.
    fn add_nested(&mut self, sel: &ComplexSelector, outer: &Invalidation, nth_of: bool) {
        let mut inv = outer.clone();
        if nth_of {
            inv.parent_subtree = true;
        }
        if !sel.combinators.is_empty() {
            inv.self_ = true;
            inv.siblings = true;
            inv.descendants = Some(Descendants::all());
        }
        for c in &sel.compounds {
            self.add_compound(c, &inv);
        }
    }
}

/// The elements a batch of changes restyles.
#[derive(Debug, Default)]
pub(crate) struct Targets {
    /// Rematch these elements (their pseudo-elements too).
    pub rematch: BTreeSet<NodeId>,
    /// Rematch these nodes and everything under them.
    pub subtrees: BTreeSet<NodeId>,
    /// Restyle the whole document.
    pub whole: bool,
}

impl Targets {
    pub fn is_empty(&self) -> bool {
        !self.whole && self.rematch.is_empty() && self.subtrees.is_empty()
    }
}

/// The matching state a style set was computed against, kept so the next flush can
/// tell which elements' dynamic pseudo-classes flipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchState {
    hovered: BTreeSet<NodeId>,
    active: BTreeSet<NodeId>,
    focused: Option<NodeId>,
    focus_visible: bool,
    target_id: Option<String>,
    document_lang: String,
}

impl MatchState {
    pub fn of(ctx: &MatchContext) -> MatchState {
        MatchState {
            hovered: ctx.hovered.clone(),
            active: ctx.active.clone(),
            focused: ctx.focused,
            focus_visible: ctx.focus_visible,
            target_id: ctx.target_id.clone(),
            document_lang: ctx.document_lang.clone(),
        }
    }
}

impl InvalidationMap {
    /// Applies `inv` for a change on `node`.
    pub fn apply(&self, doc: &Document, node: NodeId, inv: &Invalidation, t: &mut Targets) {
        if inv.self_ {
            t.rematch.insert(node);
        }
        if inv.parent_subtree {
            match doc.parent(node) {
                Some(p) if doc.is_element(p) => {
                    for c in doc.children(p) {
                        t.subtrees.insert(c);
                    }
                }
                _ => {
                    t.subtrees.insert(node);
                }
            }
        }
        if inv.siblings {
            let mut s = doc.next_sibling(node);
            while let Some(x) = s {
                t.subtrees.insert(x);
                s = doc.next_sibling(x);
            }
        }
        if let Some(d) = &inv.descendants {
            if d.all {
                t.subtrees.insert(node);
            } else {
                for x in doc.descendants(node).skip(1) {
                    if doc.is_element(x) && d.covers(doc, x) {
                        t.rematch.insert(x);
                    }
                }
            }
        }
    }

    fn apply_pseudo(&self, doc: &Document, node: NodeId, name: &str, t: &mut Targets) {
        if let Some(inv) = self.pseudos.get(name) {
            self.apply(doc, node, inv, t);
        }
    }

    /// A change to the children of `parent` (an insertion, removal or text change).
    pub fn children_changed(&self, doc: &Document, parent: NodeId, t: &mut Targets) {
        if !doc.is_element(parent) {
            return;
        }
        if self.empty != Invalidation::default() {
            self.apply(doc, parent, &self.empty, t);
        }
        if self.structural != Invalidation::default() {
            for c in doc.children(parent) {
                if doc.is_element(c) {
                    self.apply(doc, c, &self.structural, t);
                }
            }
        }
    }

    /// An attribute change on `node`.
    pub fn attribute_changed(
        &self,
        doc: &Document,
        node: NodeId,
        name: &str,
        old: Option<&str>,
        t: &mut Targets,
    ) {
        match name {
            "class" => {
                let new: BTreeSet<&str> = doc.classes(node).collect();
                let old: BTreeSet<&str> = old
                    .map(|o| o.split_ascii_whitespace().collect())
                    .unwrap_or_default();
                for c in new.symmetric_difference(&old) {
                    if let Some(inv) = self.classes.get(*c) {
                        self.apply(doc, node, inv, t);
                    }
                }
            }
            "id" => {
                for i in [doc.attr(node, "id"), old].into_iter().flatten() {
                    if let Some(inv) = self.ids.get(i) {
                        self.apply(doc, node, inv, t);
                    }
                }
            }
            _ => {}
        }
        if let Some(inv) = self.attributes.get(name) {
            self.apply(doc, node, inv, t);
        }
        if name == "style" {
            t.rematch.insert(node);
        }
        if super::hints::is_hint_attribute(doc, node, name) {
            // A table's `border`, `cellpadding` and `rules` are its cells' hints.
            if doc.is(node, "table") {
                t.subtrees.insert(node);
            } else {
                t.rematch.insert(node);
            }
        }
        // Inherited through the tree: `lang` sets the font's language, and `:lang()`
        // and `:dir()` read the nearest ancestor's attribute.
        if matches!(name, "lang" | "xml:lang") || (name == "dir" && self.lang_or_dir) {
            t.subtrees.insert(node);
        }
        if self.ancestor_form && matches!(name, "disabled" | "contenteditable") {
            t.subtrees.insert(node);
        }
        // An option's `:checked` depends on its select's other options.
        if name == "selected" {
            if let Some(s) = doc.ancestors(node).find(|a| doc.is(*a, "select")) {
                t.subtrees.insert(s);
            }
        }
        if self.default && matches!(name, "type" | "checked" | "selected" | "form") {
            t.whole = true;
        }
        // `<body text>` colours quirks-mode tables; `<body link>` colours links.
        if doc.body() == Some(node) && matches!(name, "text" | "link" | "vlink" | "alink") {
            t.whole = true;
        }
    }

    /// Form state (checked, value, ...) that changed on `node` outside the document.
    pub fn form_state_changed(&self, doc: &Document, node: NodeId, t: &mut Targets) {
        for p in FORM_PSEUDOS {
            self.apply_pseudo(doc, node, p, t);
        }
        if self.default {
            t.whole = true;
        }
    }

    /// The dynamic pseudo-classes that flipped between two matching states.
    pub fn state_changed(
        &self,
        doc: &Document,
        old: &MatchState,
        new: &MatchState,
        t: &mut Targets,
    ) {
        if old.document_lang != new.document_lang {
            t.whole = true;
            return;
        }
        for n in old.hovered.symmetric_difference(&new.hovered) {
            if connected_element(doc, *n) {
                self.apply_pseudo(doc, *n, "hover", t);
            }
        }
        for n in old.active.symmetric_difference(&new.active) {
            if connected_element(doc, *n) {
                self.apply_pseudo(doc, *n, "active", t);
            }
        }
        if old.focused != new.focused || old.focus_visible != new.focus_visible {
            for n in [old.focused, new.focused].into_iter().flatten() {
                if connected_element(doc, n) {
                    self.apply_pseudo(doc, n, "focus", t);
                    self.apply_pseudo(doc, n, "focus-visible", t);
                }
            }
        }
        if old.focused != new.focused && self.pseudos.contains_key("focus-within") {
            let chain = |f: Option<NodeId>| -> BTreeSet<NodeId> {
                let mut s = BTreeSet::new();
                if let Some(f) = f {
                    if connected_element(doc, f) {
                        s.insert(f);
                        s.extend(doc.ancestors(f).filter(|a| doc.is_element(*a)));
                    }
                }
                s
            };
            let (a, b) = (chain(old.focused), chain(new.focused));
            for n in a.symmetric_difference(&b) {
                self.apply_pseudo(doc, *n, "focus-within", t);
            }
        }
        if old.target_id != new.target_id {
            for id in [&old.target_id, &new.target_id].into_iter().flatten() {
                for &n in doc.by_id(id) {
                    if connected_element(doc, n) {
                        self.apply_pseudo(doc, n, "target", t);
                    }
                }
            }
        }
    }
}

pub(crate) fn connected_element(doc: &Document, n: NodeId) -> bool {
    n.index() < doc.len() && doc.is_element(n) && is_connected(doc, n)
}

pub(crate) fn is_connected(doc: &Document, n: NodeId) -> bool {
    if doc.node(n).detached {
        return false;
    }
    n == Document::ROOT || doc.ancestors(n).last() == Some(Document::ROOT)
}
