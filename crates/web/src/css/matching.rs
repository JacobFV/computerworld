//! Selector matching against `dom::Document`: right-to-left with combinator
//! backtracking, the dynamic state the DOM does not hold (`MatchContext`), a rule
//! index bucketed by the rightmost compound so the cascade tests only candidates, and
//! the dependency sets the style track uses for invalidation.

use super::selector::{AttrCase, AttrOp, Combinator, ComplexSelector, CompoundSelector, Direction, NthKind, PseudoClass, RelativeSelector, SelectorList, SimpleSelector};
use crate::dom::{Document, Namespace, NodeId, NodeKind};
use std::collections::{BTreeMap, BTreeSet};

/// Form control state that lives outside the DOM attributes (the "dirty" checkedness
/// and value the user or script set). The default reads the attributes.
pub trait FormState {
    /// Checkedness of a checkbox/radio input, selectedness of an `<option>`.
    fn checked(&self, doc: &Document, element: NodeId) -> bool;
    /// The `indeterminate` IDL attribute of a checkbox.
    fn indeterminate(&self, doc: &Document, element: NodeId) -> bool;
    /// The current value of an input or textarea, for `:placeholder-shown`.
    fn value(&self, doc: &Document, element: NodeId) -> String;
}

/// `FormState` from attributes only: `checked`, `selected`, `value`, text content.
#[derive(Clone, Copy, Debug, Default)]
pub struct AttributeFormState;

impl FormState for AttributeFormState {
    fn checked(&self, doc: &Document, element: NodeId) -> bool {
        if doc.is(element, "option") {
            doc.has_attr(element, "selected")
        } else {
            doc.has_attr(element, "checked")
        }
    }
    fn indeterminate(&self, _doc: &Document, _element: NodeId) -> bool {
        false
    }
    fn value(&self, doc: &Document, element: NodeId) -> String {
        if doc.is(element, "textarea") {
            doc.text_content(element)
        } else {
            doc.attr(element, "value").unwrap_or("").to_owned()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum VisitedPolicy {
    /// Deterministic default: no link is ever visited.
    #[default]
    NeverVisited,
    AllVisited,
    /// Links whose `href` is in the set are visited.
    Urls(BTreeSet<String>),
}

/// The dynamic state selector matching needs beyond the document.
#[derive(Clone, Default)]
pub struct MatchContext<'a> {
    /// Elements under the pointer: the hovered element and its ancestors.
    pub hovered: BTreeSet<NodeId>,
    /// Elements being activated: the active element and its ancestors.
    pub active: BTreeSet<NodeId>,
    pub focused: Option<NodeId>,
    /// Whether the focused element should show a focus ring (`:focus-visible`).
    pub focus_visible: bool,
    /// The fragment identifier of the document URL, for `:target`.
    pub target_id: Option<String>,
    pub visited: VisitedPolicy,
    /// The `:scope` element; the root element when `None`.
    pub scope: Option<NodeId>,
    /// The document's language (from the `Content-Language` header or `<html lang>`),
    /// used when no ancestor has a `lang` attribute.
    pub document_lang: String,
    pub form: Option<&'a dyn FormState>,
}

impl<'a> MatchContext<'a> {
    pub fn new() -> MatchContext<'a> {
        MatchContext::default()
    }
    /// Sets the hovered element; its ancestors are hovered too.
    pub fn set_hovered(&mut self, doc: &Document, element: Option<NodeId>) {
        self.hovered = inclusive_ancestors(doc, element);
    }
    pub fn set_active(&mut self, doc: &Document, element: Option<NodeId>) {
        self.active = inclusive_ancestors(doc, element);
    }
    fn form_checked(&self, doc: &Document, el: NodeId) -> bool {
        match self.form {
            Some(f) => f.checked(doc, el),
            None => AttributeFormState.checked(doc, el),
        }
    }
    fn form_indeterminate(&self, doc: &Document, el: NodeId) -> bool {
        match self.form {
            Some(f) => f.indeterminate(doc, el),
            None => AttributeFormState.indeterminate(doc, el),
        }
    }
    fn form_value(&self, doc: &Document, el: NodeId) -> String {
        match self.form {
            Some(f) => f.value(doc, el),
            None => AttributeFormState.value(doc, el),
        }
    }
}

fn inclusive_ancestors(doc: &Document, element: Option<NodeId>) -> BTreeSet<NodeId> {
    let mut set = BTreeSet::new();
    if let Some(e) = element {
        set.insert(e);
        for a in doc.ancestors(e) {
            if doc.is_element(a) {
                set.insert(a);
            }
        }
    }
    set
}

/// Whether `element` matches `selector` (its pseudo-element, if any, is ignored: the
/// caller decides what to do with `selector.pseudo_element`).
pub fn matches(doc: &Document, element: NodeId, selector: &ComplexSelector, ctx: &MatchContext) -> bool {
    if !doc.is_element(element) || selector.compounds.is_empty() {
        return false;
    }
    let idx = selector.compounds.len() - 1;
    match_from(doc, element, selector, idx, ctx, None)
}

/// Whether `element` matches any selector of the list.
pub fn matches_list(doc: &Document, element: NodeId, list: &SelectorList, ctx: &MatchContext) -> bool {
    list.0.iter().any(|s| matches(doc, element, s, ctx))
}

/// The anchor of a relative selector: the `:has()` subject and the combinator that
/// joins it to the leftmost compound.
#[derive(Clone, Copy)]
struct Anchor {
    element: NodeId,
    combinator: Combinator,
}

fn match_from(doc: &Document, element: NodeId, sel: &ComplexSelector, idx: usize, ctx: &MatchContext, anchor: Option<Anchor>) -> bool {
    if !matches_compound(doc, element, &sel.compounds[idx], ctx) {
        return false;
    }
    if idx == 0 {
        return match anchor {
            None => true,
            Some(a) => related(doc, a.element, a.combinator, element),
        };
    }
    match sel.combinators[idx - 1] {
        Combinator::Child => match parent_element(doc, element) {
            Some(p) => match_from(doc, p, sel, idx - 1, ctx, anchor),
            None => false,
        },
        Combinator::Descendant => {
            let mut cur = parent_element(doc, element);
            while let Some(p) = cur {
                if match_from(doc, p, sel, idx - 1, ctx, anchor) {
                    return true;
                }
                cur = parent_element(doc, p);
            }
            false
        }
        Combinator::NextSibling => match prev_element_sibling(doc, element) {
            Some(s) => match_from(doc, s, sel, idx - 1, ctx, anchor),
            None => false,
        },
        Combinator::SubsequentSibling => {
            let mut cur = prev_element_sibling(doc, element);
            while let Some(s) = cur {
                if match_from(doc, s, sel, idx - 1, ctx, anchor) {
                    return true;
                }
                cur = prev_element_sibling(doc, s);
            }
            false
        }
    }
}

/// Is `element` reachable from `anchor` through `combinator`?
fn related(doc: &Document, anchor: NodeId, combinator: Combinator, element: NodeId) -> bool {
    match combinator {
        Combinator::Child => parent_element(doc, element) == Some(anchor),
        Combinator::Descendant => doc.ancestors(element).any(|a| a == anchor),
        Combinator::NextSibling => prev_element_sibling(doc, element) == Some(anchor),
        Combinator::SubsequentSibling => {
            let mut cur = prev_element_sibling(doc, element);
            while let Some(s) = cur {
                if s == anchor {
                    return true;
                }
                cur = prev_element_sibling(doc, s);
            }
            false
        }
    }
}

fn parent_element(doc: &Document, element: NodeId) -> Option<NodeId> {
    doc.parent(element).filter(|p| doc.is_element(*p))
}

fn prev_element_sibling(doc: &Document, element: NodeId) -> Option<NodeId> {
    let mut cur = doc.prev_sibling(element);
    while let Some(s) = cur {
        if doc.is_element(s) {
            return Some(s);
        }
        cur = doc.prev_sibling(s);
    }
    None
}

fn next_element_sibling(doc: &Document, element: NodeId) -> Option<NodeId> {
    let mut cur = doc.next_sibling(element);
    while let Some(s) = cur {
        if doc.is_element(s) {
            return Some(s);
        }
        cur = doc.next_sibling(s);
    }
    None
}

fn is_html(doc: &Document, element: NodeId) -> bool {
    matches!(doc.kind(element), NodeKind::Element { ns: Namespace::Html, .. })
}

/// Attributes whose values HTML compares case-insensitively in selectors.
const CASE_INSENSITIVE_ATTRS: &[&str] = &["accept", "accept-charset", "align", "alink", "axis", "bgcolor", "charset", "checked", "clear", "codetype", "color", "compact", "declare", "defer", "dir", "direction", "disabled", "enctype", "face", "frame", "hreflang", "http-equiv", "lang", "language", "link", "media", "method", "multiple", "nohref", "noresize", "noshade", "nowrap", "readonly", "rel", "rev", "rules", "scope", "scrolling", "selected", "shape", "target", "text", "type", "valign", "valuetype", "vlink"];

pub fn matches_compound(doc: &Document, element: NodeId, compound: &CompoundSelector, ctx: &MatchContext) -> bool {
    compound.simple.iter().all(|s| matches_simple(doc, element, s, ctx))
}

fn matches_simple(doc: &Document, element: NodeId, simple: &SimpleSelector, ctx: &MatchContext) -> bool {
    match simple {
        SimpleSelector::Universal => true,
        SimpleSelector::Type(name) => {
            let tag = doc.tag(element).unwrap_or("");
            if is_html(doc, element) {
                name.eq_ignore_ascii_case(tag)
            } else {
                name == tag
            }
        }
        SimpleSelector::Id(id) => doc.attr(element, "id") == Some(id.as_str()),
        SimpleSelector::Class(c) => doc.has_class(element, c),
        SimpleSelector::Attribute { name, op, value, case } => {
            let html = is_html(doc, element);
            let attr = if html {
                let lname = name.to_ascii_lowercase();
                doc.attrs(element).iter().find(|a| a.name == lname)
            } else {
                doc.attrs(element).iter().find(|a| a.name == *name)
            };
            let Some(attr) = attr else { return false };
            let insensitive = match case {
                AttrCase::Insensitive => true,
                AttrCase::Sensitive => false,
                AttrCase::Default => html && CASE_INSENSITIVE_ATTRS.contains(&attr.name.as_str()),
            };
            attr_matches(&attr.value, *op, value, insensitive)
        }
        SimpleSelector::Nesting => matches_simple(doc, element, &SimpleSelector::PseudoClass(PseudoClass::Scope), ctx),
        SimpleSelector::PseudoClass(pc) => matches_pseudo(doc, element, pc, ctx),
    }
}

fn attr_matches(actual: &str, op: AttrOp, wanted: &str, insensitive: bool) -> bool {
    let (a, w) = if insensitive { (actual.to_ascii_lowercase(), wanted.to_ascii_lowercase()) } else { (actual.to_owned(), wanted.to_owned()) };
    match op {
        AttrOp::Exists => true,
        AttrOp::Equals => a == w,
        AttrOp::Includes => !w.is_empty() && !w.contains(|c: char| c.is_ascii_whitespace()) && a.split_ascii_whitespace().any(|t| t == w),
        AttrOp::DashMatch => a == w || a.starts_with(&format!("{w}-")),
        AttrOp::Prefix => !w.is_empty() && a.starts_with(&w),
        AttrOp::Suffix => !w.is_empty() && a.ends_with(&w),
        AttrOp::Substring => !w.is_empty() && a.contains(&w),
    }
}

fn is_form_control(doc: &Document, el: NodeId) -> bool {
    matches!(doc.tag(el), Some("button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"))
}

fn input_type(doc: &Document, el: NodeId) -> String {
    doc.attr(el, "type").unwrap_or("text").to_ascii_lowercase()
}

fn is_disabled(doc: &Document, el: NodeId) -> bool {
    if !is_form_control(doc, el) {
        return false;
    }
    if doc.has_attr(el, "disabled") {
        return true;
    }
    if doc.is(el, "option") {
        if let Some(p) = parent_element(doc, el) {
            if doc.is(p, "optgroup") && doc.has_attr(p, "disabled") {
                return true;
            }
        }
    }
    if matches!(doc.tag(el), Some("button" | "input" | "select" | "textarea")) {
        // Inside a disabled fieldset, unless inside that fieldset's first legend.
        let mut child = el;
        for anc in doc.ancestors(el) {
            if doc.is(anc, "fieldset") && doc.has_attr(anc, "disabled") {
                let first_legend = doc.element_children(anc).find(|c| doc.is(*c, "legend"));
                if first_legend != Some(child) {
                    return true;
                }
            }
            child = anc;
        }
    }
    false
}

fn is_any_link(doc: &Document, el: NodeId) -> bool {
    matches!(doc.tag(el), Some("a" | "area" | "link")) && doc.has_attr(el, "href")
}

fn is_visited(doc: &Document, el: NodeId, ctx: &MatchContext) -> bool {
    match &ctx.visited {
        VisitedPolicy::NeverVisited => false,
        VisitedPolicy::AllVisited => true,
        VisitedPolicy::Urls(set) => doc.attr(el, "href").is_some_and(|h| set.contains(h)),
    }
}

fn is_read_write(doc: &Document, el: NodeId) -> bool {
    match doc.tag(el) {
        Some("input") => {
            let t = input_type(doc, el);
            let textual = matches!(t.as_str(), "text" | "search" | "url" | "tel" | "email" | "password" | "date" | "month" | "week" | "time" | "datetime-local" | "number");
            textual && !doc.has_attr(el, "readonly") && !is_disabled(doc, el)
        }
        Some("textarea") => !doc.has_attr(el, "readonly") && !is_disabled(doc, el),
        _ => {
            let mut cur = Some(el);
            while let Some(e) = cur {
                if let Some(ce) = doc.attr(e, "contenteditable") {
                    return !ce.eq_ignore_ascii_case("false");
                }
                cur = parent_element(doc, e);
            }
            false
        }
    }
}

fn element_lang(doc: &Document, el: NodeId, ctx: &MatchContext) -> String {
    let mut cur = Some(el);
    while let Some(e) = cur {
        if let Some(l) = doc.attr(e, "lang").or_else(|| doc.attr(e, "xml:lang")) {
            return l.to_owned();
        }
        cur = parent_element(doc, e);
    }
    ctx.document_lang.clone()
}

/// RFC 4647 extended filtering of one language range against a tag.
fn lang_matches(range: &str, lang: &str) -> bool {
    if lang.is_empty() {
        return false;
    }
    let r: Vec<String> = range.split('-').map(|s| s.to_ascii_lowercase()).collect();
    let l: Vec<String> = lang.split('-').map(|s| s.to_ascii_lowercase()).collect();
    if r.is_empty() || l.is_empty() {
        return false;
    }
    if r[0] != "*" && r[0] != l[0] {
        return false;
    }
    let mut i = 1;
    let mut j = 1;
    while i < r.len() {
        if r[i] == "*" {
            i += 1;
            continue;
        }
        if j >= l.len() {
            return false;
        }
        if r[i] == l[j] {
            i += 1;
            j += 1;
            continue;
        }
        if l[j].len() == 1 {
            return false;
        }
        j += 1;
    }
    true
}

fn element_dir(doc: &Document, el: NodeId) -> Direction {
    let mut cur = Some(el);
    while let Some(e) = cur {
        if let Some(d) = doc.attr(e, "dir") {
            if d.eq_ignore_ascii_case("rtl") {
                return Direction::Rtl;
            }
            if d.eq_ignore_ascii_case("ltr") {
                return Direction::Ltr;
            }
        }
        cur = parent_element(doc, e);
    }
    Direction::Ltr
}

/// Position among siblings (1-based) counting elements, optionally of the same type
/// or matching `of`, from the start or the end.
fn nth_index(doc: &Document, el: NodeId, kind: NthKind, of: Option<&SelectorList>, ctx: &MatchContext) -> usize {
    let from_end = matches!(kind, NthKind::LastChild | NthKind::LastOfType);
    let of_type = matches!(kind, NthKind::OfType | NthKind::LastOfType);
    let tag = doc.tag(el).unwrap_or("");
    let html = is_html(doc, el);
    let counts = |s: NodeId| {
        if of_type {
            let t = doc.tag(s).unwrap_or("");
            if html && is_html(doc, s) {
                t.eq_ignore_ascii_case(tag)
            } else {
                t == tag
            }
        } else if let Some(list) = of {
            matches_list(doc, s, list, ctx)
        } else {
            true
        }
    };
    let mut i = 1;
    let mut cur = if from_end { next_element_sibling(doc, el) } else { prev_element_sibling(doc, el) };
    while let Some(s) = cur {
        if counts(s) {
            i += 1;
        }
        cur = if from_end { next_element_sibling(doc, s) } else { prev_element_sibling(doc, s) };
    }
    i
}

fn anb_matches(a: i32, b: i32, index: usize) -> bool {
    let i = index as i64;
    let (a, b) = (a as i64, b as i64);
    if a == 0 {
        return i == b;
    }
    let d = i - b;
    d % a == 0 && d / a >= 0
}

fn matches_pseudo(doc: &Document, el: NodeId, pc: &PseudoClass, ctx: &MatchContext) -> bool {
    match pc {
        PseudoClass::Root => doc.parent(el).is_some_and(|p| matches!(doc.kind(p), NodeKind::Document)),
        PseudoClass::Empty => doc.children(el).all(|c| match doc.kind(c) {
            NodeKind::Element { .. } => false,
            NodeKind::Text(t) => t.is_empty(),
            _ => true,
        }),
        PseudoClass::FirstChild => doc.parent(el).is_some() && prev_element_sibling(doc, el).is_none(),
        PseudoClass::LastChild => doc.parent(el).is_some() && next_element_sibling(doc, el).is_none(),
        PseudoClass::OnlyChild => doc.parent(el).is_some() && prev_element_sibling(doc, el).is_none() && next_element_sibling(doc, el).is_none(),
        PseudoClass::FirstOfType => doc.parent(el).is_some() && nth_index(doc, el, NthKind::OfType, None, ctx) == 1,
        PseudoClass::LastOfType => doc.parent(el).is_some() && nth_index(doc, el, NthKind::LastOfType, None, ctx) == 1,
        PseudoClass::OnlyOfType => doc.parent(el).is_some() && nth_index(doc, el, NthKind::OfType, None, ctx) == 1 && nth_index(doc, el, NthKind::LastOfType, None, ctx) == 1,
        PseudoClass::Nth { kind, a, b, of } => {
            if doc.parent(el).is_none() {
                return false;
            }
            if let Some(list) = of {
                if !matches_list(doc, el, list, ctx) {
                    return false;
                }
            }
            anb_matches(*a, *b, nth_index(doc, el, *kind, of.as_ref(), ctx))
        }
        PseudoClass::Not(list) => !matches_list(doc, el, list, ctx),
        PseudoClass::Is(list) | PseudoClass::Where(list) => matches_list(doc, el, list, ctx),
        PseudoClass::Has(rel) => rel.iter().any(|r| has_matches(doc, el, r, ctx)),
        PseudoClass::Hover => ctx.hovered.contains(&el),
        PseudoClass::Active => ctx.active.contains(&el),
        PseudoClass::Focus => ctx.focused == Some(el),
        PseudoClass::FocusVisible => ctx.focused == Some(el) && ctx.focus_visible,
        PseudoClass::FocusWithin => ctx.focused.is_some_and(|f| f == el || doc.ancestors(f).any(|a| a == el)),
        PseudoClass::Visited => is_any_link(doc, el) && is_visited(doc, el, ctx),
        PseudoClass::Link => is_any_link(doc, el) && !is_visited(doc, el, ctx),
        PseudoClass::AnyLink => is_any_link(doc, el),
        PseudoClass::Target => ctx.target_id.as_deref().is_some_and(|t| doc.attr(el, "id") == Some(t)),
        PseudoClass::Checked => match doc.tag(el) {
            Some("input") => matches!(input_type(doc, el).as_str(), "checkbox" | "radio") && ctx.form_checked(doc, el),
            Some("option") => ctx.form_checked(doc, el),
            _ => false,
        },
        PseudoClass::Disabled => is_disabled(doc, el),
        PseudoClass::Enabled => is_form_control(doc, el) && !is_disabled(doc, el),
        PseudoClass::Required => matches!(doc.tag(el), Some("input" | "select" | "textarea")) && doc.has_attr(el, "required"),
        PseudoClass::Optional => matches!(doc.tag(el), Some("input" | "select" | "textarea")) && !doc.has_attr(el, "required"),
        PseudoClass::ReadWrite => is_read_write(doc, el),
        PseudoClass::ReadOnly => !is_read_write(doc, el),
        PseudoClass::PlaceholderShown => matches!(doc.tag(el), Some("input" | "textarea")) && doc.attr(el, "placeholder").is_some_and(|p| !p.is_empty()) && ctx.form_value(doc, el).is_empty(),
        PseudoClass::Indeterminate => match doc.tag(el) {
            Some("input") => match input_type(doc, el).as_str() {
                "checkbox" => ctx.form_indeterminate(doc, el),
                "radio" => !radio_group_has_checked(doc, el, ctx),
                _ => false,
            },
            Some("progress") => !doc.has_attr(el, "value"),
            _ => false,
        },
        PseudoClass::Default => match doc.tag(el) {
            Some("input") => match input_type(doc, el).as_str() {
                "checkbox" | "radio" => doc.has_attr(el, "checked"),
                "submit" | "image" => is_default_submit(doc, el),
                _ => false,
            },
            Some("button") => !matches!(doc.attr(el, "type").map(|t| t.to_ascii_lowercase()).as_deref(), Some("button" | "reset")) && is_default_submit(doc, el),
            Some("option") => doc.has_attr(el, "selected"),
            _ => false,
        },
        PseudoClass::Lang(ranges) => {
            let lang = element_lang(doc, el, ctx);
            ranges.iter().any(|r| lang_matches(r, &lang))
        }
        PseudoClass::Dir(d) => element_dir(doc, el) == *d,
        PseudoClass::Scope => match ctx.scope {
            Some(s) => s == el,
            None => doc.document_element() == Some(el),
        },
        PseudoClass::Defined => !is_html(doc, el) || !doc.tag(el).unwrap_or("").contains('-'),
    }
}

fn form_owner(doc: &Document, el: NodeId) -> Option<NodeId> {
    if let Some(id) = doc.attr(el, "form") {
        return doc.by_id(id).first().copied();
    }
    doc.ancestors(el).find(|a| doc.is(*a, "form"))
}

fn radio_group_has_checked(doc: &Document, el: NodeId, ctx: &MatchContext) -> bool {
    let name = doc.attr(el, "name").unwrap_or("");
    if name.is_empty() {
        return ctx.form_checked(doc, el);
    }
    let owner = form_owner(doc, el);
    let root = owner.unwrap_or(Document::ROOT);
    doc.descendants(root).any(|d| doc.is(d, "input") && input_type(doc, d) == "radio" && doc.attr(d, "name") == Some(name) && form_owner(doc, d) == owner && ctx.form_checked(doc, d))
}

fn is_default_submit(doc: &Document, el: NodeId) -> bool {
    let Some(form) = form_owner(doc, el) else { return false };
    let first = doc.descendants(form).find(|d| {
        if *d == form || !doc.is_element(*d) {
            return false;
        }
        let submit = match doc.tag(*d) {
            Some("input") => matches!(input_type(doc, *d).as_str(), "submit" | "image"),
            Some("button") => !matches!(doc.attr(*d, "type").map(|t| t.to_ascii_lowercase()).as_deref(), Some("button" | "reset")),
            _ => false,
        };
        submit && form_owner(doc, *d) == Some(form)
    });
    first == Some(el)
}

fn has_matches(doc: &Document, anchor: NodeId, rel: &RelativeSelector, ctx: &MatchContext) -> bool {
    let sel = &rel.selector;
    if sel.compounds.is_empty() {
        return false;
    }
    let idx = sel.compounds.len() - 1;
    let a = Some(Anchor { element: anchor, combinator: rel.combinator });
    match rel.combinator {
        Combinator::Descendant | Combinator::Child => doc.descendants(anchor).skip(1).any(|d| doc.is_element(d) && match_from(doc, d, sel, idx, ctx, a)),
        Combinator::NextSibling | Combinator::SubsequentSibling => {
            let mut cur = next_element_sibling(doc, anchor);
            while let Some(s) = cur {
                if doc.descendants(s).any(|d| doc.is_element(d) && match_from(doc, d, sel, idx, ctx, a)) {
                    return true;
                }
                cur = next_element_sibling(doc, s);
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Rule index

#[derive(Clone, Debug)]
pub struct IndexEntry<T> {
    pub selector: ComplexSelector,
    pub data: T,
}

/// Rules bucketed by their rightmost compound's id, first class, type name, or none,
/// so that only plausible rules are matched against an element.
#[derive(Clone, Debug)]
pub struct SelectorIndex<T> {
    entries: Vec<IndexEntry<T>>,
    ids: BTreeMap<String, Vec<usize>>,
    classes: BTreeMap<String, Vec<usize>>,
    tags: BTreeMap<String, Vec<usize>>,
    other: Vec<usize>,
}

impl<T> Default for SelectorIndex<T> {
    fn default() -> Self {
        SelectorIndex { entries: Vec::new(), ids: BTreeMap::new(), classes: BTreeMap::new(), tags: BTreeMap::new(), other: Vec::new() }
    }
}

impl<T> SelectorIndex<T> {
    pub fn new() -> SelectorIndex<T> {
        SelectorIndex::default()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn entries(&self) -> &[IndexEntry<T>] {
        &self.entries
    }
    /// Adds a selector; returns its index (insertion order).
    pub fn insert(&mut self, selector: ComplexSelector, data: T) -> usize {
        let i = self.entries.len();
        let right = selector.rightmost();
        if let Some(id) = right.id() {
            self.ids.entry(id.to_owned()).or_default().push(i);
        } else if let Some(c) = right.classes().next() {
            self.classes.entry(c.to_owned()).or_default().push(i);
        } else if let Some(t) = right.type_name() {
            self.tags.entry(t.to_ascii_lowercase()).or_default().push(i);
        } else {
            self.other.push(i);
        }
        self.entries.push(IndexEntry { selector, data });
        i
    }
    /// Every entry whose bucket the element falls in, in insertion order. Matching is
    /// still required; pseudo-elements are not filtered.
    pub fn candidates<'a>(&'a self, doc: &Document, element: NodeId) -> impl Iterator<Item = &'a IndexEntry<T>> + 'a {
        let mut idx: Vec<usize> = Vec::new();
        if let Some(id) = doc.attr(element, "id") {
            if let Some(v) = self.ids.get(id) {
                idx.extend(v);
            }
        }
        for c in doc.classes(element) {
            if let Some(v) = self.classes.get(c) {
                idx.extend(v);
            }
        }
        if let Some(t) = doc.tag(element) {
            if let Some(v) = self.tags.get(&t.to_ascii_lowercase()) {
                idx.extend(v);
            }
        }
        idx.extend(&self.other);
        idx.sort_unstable();
        idx.dedup();
        idx.into_iter().map(move |i| &self.entries[i])
    }
    /// The candidates that match, in insertion order.
    pub fn matching<'a>(&'a self, doc: &Document, element: NodeId, ctx: &MatchContext) -> Vec<&'a IndexEntry<T>> {
        self.candidates(doc, element).filter(|e| matches(doc, element, &e.selector, ctx)).collect()
    }
}

// ---------------------------------------------------------------------------
// Dependencies for invalidation

/// What a selector reads, so a style change can be scoped: when a class flips, only
/// rules whose `classes` contain it (or that are `structural`/`has`) can change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectorDeps {
    pub ids: BTreeSet<String>,
    pub classes: BTreeSet<String>,
    /// Attribute names (lower-cased).
    pub attributes: BTreeSet<String>,
    /// Type names (lower-cased).
    pub tags: BTreeSet<String>,
    /// Pseudo-class names without the colon (`hover`, `nth-child`, ...).
    pub pseudo_classes: BTreeSet<String>,
    /// Reads sibling or child structure (`:nth-*`, `:first-child`, `:empty`, sibling
    /// combinators): insertions and removals invalidate.
    pub structural: bool,
    /// Contains `:has()`: descendant changes invalidate ancestors.
    pub has: bool,
    /// Reads `MatchContext` state (hover, focus, active, target, visited, scope).
    pub state: bool,
    /// Reads form state (checked, value, disabled, ...).
    pub form: bool,
}

impl SelectorDeps {
    fn merge(&mut self, o: SelectorDeps) {
        self.ids.extend(o.ids);
        self.classes.extend(o.classes);
        self.attributes.extend(o.attributes);
        self.tags.extend(o.tags);
        self.pseudo_classes.extend(o.pseudo_classes);
        self.structural |= o.structural;
        self.has |= o.has;
        self.state |= o.state;
        self.form |= o.form;
    }
}

impl ComplexSelector {
    pub fn dependencies(&self) -> SelectorDeps {
        let mut d = SelectorDeps::default();
        if self.combinators.iter().any(|c| matches!(c, Combinator::NextSibling | Combinator::SubsequentSibling)) {
            d.structural = true;
        }
        for s in self.simple_selectors() {
            d.merge(simple_deps(s));
        }
        d
    }
}

impl SelectorList {
    pub fn dependencies(&self) -> SelectorDeps {
        let mut d = SelectorDeps::default();
        for s in &self.0 {
            d.merge(s.dependencies());
        }
        d
    }
}

fn simple_deps(s: &SimpleSelector) -> SelectorDeps {
    let mut d = SelectorDeps::default();
    match s {
        SimpleSelector::Universal => {}
        SimpleSelector::Nesting => d.state = true,
        SimpleSelector::Type(t) => {
            d.tags.insert(t.to_ascii_lowercase());
        }
        SimpleSelector::Id(i) => {
            d.ids.insert(i.clone());
            d.attributes.insert("id".into());
        }
        SimpleSelector::Class(c) => {
            d.classes.insert(c.clone());
            d.attributes.insert("class".into());
        }
        SimpleSelector::Attribute { name, .. } => {
            d.attributes.insert(name.to_ascii_lowercase());
        }
        SimpleSelector::PseudoClass(pc) => {
            let name = pc.to_string();
            let name = name.trim_start_matches(':');
            let name = name.split('(').next().unwrap_or(name);
            d.pseudo_classes.insert(name.to_owned());
            match pc {
                PseudoClass::Root | PseudoClass::Empty | PseudoClass::FirstChild | PseudoClass::LastChild | PseudoClass::OnlyChild | PseudoClass::FirstOfType | PseudoClass::LastOfType | PseudoClass::OnlyOfType => d.structural = true,
                PseudoClass::Nth { of, .. } => {
                    d.structural = true;
                    if let Some(l) = of {
                        d.merge(l.dependencies());
                    }
                }
                PseudoClass::Not(l) | PseudoClass::Is(l) | PseudoClass::Where(l) => d.merge(l.dependencies()),
                PseudoClass::Has(rel) => {
                    d.has = true;
                    for r in rel {
                        if matches!(r.combinator, Combinator::NextSibling | Combinator::SubsequentSibling) {
                            d.structural = true;
                        }
                        d.merge(r.selector.dependencies());
                    }
                }
                PseudoClass::Hover | PseudoClass::Active | PseudoClass::Focus | PseudoClass::FocusVisible | PseudoClass::FocusWithin | PseudoClass::Target | PseudoClass::Scope => d.state = true,
                PseudoClass::Visited | PseudoClass::Link | PseudoClass::AnyLink => {
                    d.state = true;
                    d.attributes.insert("href".into());
                }
                PseudoClass::Checked | PseudoClass::Indeterminate | PseudoClass::PlaceholderShown | PseudoClass::Default => {
                    d.form = true;
                    d.attributes.extend(["checked", "selected", "value", "type", "placeholder", "name"].map(String::from));
                }
                PseudoClass::Disabled | PseudoClass::Enabled => {
                    d.form = true;
                    d.attributes.insert("disabled".into());
                }
                PseudoClass::Required | PseudoClass::Optional => {
                    d.attributes.insert("required".into());
                }
                PseudoClass::ReadOnly | PseudoClass::ReadWrite => {
                    d.form = true;
                    d.attributes.extend(["readonly", "disabled", "contenteditable", "type"].map(String::from));
                }
                PseudoClass::Lang(_) => {
                    d.attributes.insert("lang".into());
                }
                PseudoClass::Dir(_) => {
                    d.attributes.insert("dir".into());
                }
                PseudoClass::Defined => {}
            }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::selector::parse_selector_list;
    use crate::dom::Attribute;

    /// Builds a document from a tiny bracket notation: `tag#id.class[attr=value]{children}`,
    /// `"text"`; siblings separated by whitespace.
    fn build(src: &str) -> Document {
        let mut doc = Document::new();
        let chars: Vec<char> = src.chars().collect();
        let mut pos = 0;
        build_children(&mut doc, Document::ROOT, &chars, &mut pos);
        doc
    }
    fn build_children(doc: &mut Document, parent: NodeId, c: &[char], pos: &mut usize) {
        while *pos < c.len() {
            let ch = c[*pos];
            if ch.is_whitespace() {
                *pos += 1;
                continue;
            }
            if ch == '}' {
                *pos += 1;
                return;
            }
            if ch == '"' {
                *pos += 1;
                let start = *pos;
                while c[*pos] != '"' {
                    *pos += 1;
                }
                let t: String = c[start..*pos].iter().collect();
                *pos += 1;
                let n = doc.create_text(&t);
                doc.append(parent, n);
                continue;
            }
            let start = *pos;
            while *pos < c.len() && (c[*pos].is_alphanumeric() || c[*pos] == '-' || c[*pos] == ':') {
                *pos += 1;
            }
            let tag: String = c[start..*pos].iter().collect();
            let mut attrs = Vec::new();
            loop {
                match c.get(*pos) {
                    Some('#') | Some('.') => {
                        let kind = c[*pos];
                        *pos += 1;
                        let s = *pos;
                        while *pos < c.len() && (c[*pos].is_alphanumeric() || c[*pos] == '-' || c[*pos] == '_') {
                            *pos += 1;
                        }
                        let v: String = c[s..*pos].iter().collect();
                        if kind == '#' {
                            attrs.push(Attribute { name: "id".into(), value: v });
                        } else if let Some(a) = attrs.iter_mut().find(|a: &&mut Attribute| a.name == "class") {
                            a.value.push(' ');
                            a.value.push_str(&v);
                        } else {
                            attrs.push(Attribute { name: "class".into(), value: v });
                        }
                    }
                    Some('[') => {
                        *pos += 1;
                        let s = *pos;
                        while c[*pos] != ']' {
                            *pos += 1;
                        }
                        let body: String = c[s..*pos].iter().collect();
                        *pos += 1;
                        let (n, v) = body.split_once('=').unwrap_or((&body, ""));
                        attrs.push(Attribute { name: n.to_ascii_lowercase(), value: v.to_owned() });
                    }
                    _ => break,
                }
            }
            let el = doc.create_element(&tag, attrs);
            doc.append(parent, el);
            let mut look = *pos;
            while look < c.len() && c[look] == ' ' {
                look += 1;
            }
            if c.get(look) == Some(&'{') {
                *pos = look;
                *pos += 1;
                build_children(doc, el, c, pos);
            }
        }
    }
    fn find(doc: &Document, id: &str) -> NodeId {
        doc.by_id(id)[0]
    }
    fn sel(s: &str) -> ComplexSelector {
        parse_selector_list(s).unwrap().0.remove(0)
    }
    fn m(doc: &Document, id: &str, s: &str) -> bool {
        matches(doc, find(doc, id), &sel(s), &MatchContext::new())
    }
    fn mc(doc: &Document, id: &str, s: &str, ctx: &MatchContext) -> bool {
        matches(doc, find(doc, id), &sel(s), ctx)
    }
    /// Ids of the elements matching `s`, in document order.
    fn all(doc: &Document, s: &str) -> Vec<String> {
        let ctx = MatchContext::new();
        let list = parse_selector_list(s).unwrap();
        doc.descendants(Document::ROOT).filter(|n| doc.is_element(*n) && matches_list(doc, *n, &list, &ctx)).filter_map(|n| doc.attr(n, "id").map(str::to_owned)).collect()
    }

    const DOC: &str = r#"html#html{ head#head{} body#body{ div#a.x.y[data-foo=Bar] { p#p1.first{"t"} p#p2{} span#s1{} p#p3.last{} } div#b { "" } div#c { p#c1{} "x" } ul#list{ li#l1{} li#l2{} li#l3.imp{} li#l4{} li#l5.imp{} li#l6{} li#l7{} } a#link[href=/x]{} a#nolink{} } }"#;

    #[test]
    fn simple_selectors_and_case() {
        let d = build(DOC);
        assert!(m(&d, "a", "div"));
        assert!(m(&d, "a", "DIV"));
        assert!(m(&d, "a", "*"));
        assert!(m(&d, "a", "#a"));
        assert!(!m(&d, "a", "#A"));
        assert!(m(&d, "a", ".x.y"));
        assert!(!m(&d, "a", ".X"));
        assert!(m(&d, "a", "[data-foo]"));
        assert!(m(&d, "a", "[DATA-FOO]"));
        assert!(m(&d, "a", "[data-foo=Bar]"));
        assert!(!m(&d, "a", "[data-foo=bar]"));
        assert!(m(&d, "a", "[data-foo=bar i]"));
        assert!(!m(&d, "a", "[data-foo=bar s]"));
        assert!(m(&d, "a", "[data-foo^=B][data-foo$=r][data-foo*=a][data-foo~=Bar][data-foo|=Bar]"));
        assert!(!m(&d, "a", "[data-foo^='']"));
        assert!(m(&d, "a", "[class~=x][class^='x y'][class$=y]"));
        assert!(!m(&d, "a", "[class~='x y']"));
        assert!(!m(&d, "a", "[class|=x]"));
        assert!(m(&d, "html", "[id|=html]"));
        assert!(m(&d, "html", "svg|html"));
        // SVG elements keep case and match case-sensitively.
        let mut d2 = Document::new();
        let svg = d2.create(NodeKind::Element { ns: Namespace::Svg, tag: "linearGradient".into(), attrs: vec![Attribute { name: "viewBox".into(), value: "0".into() }] });
        d2.append(Document::ROOT, svg);
        let ctx = MatchContext::new();
        assert!(matches(&d2, svg, &sel("linearGradient"), &ctx));
        assert!(!matches(&d2, svg, &sel("lineargradient"), &ctx));
        assert!(matches(&d2, svg, &sel("[viewBox]"), &ctx));
        assert!(!matches(&d2, svg, &sel("[viewbox]"), &ctx));
        // HTML case-insensitive attribute list.
        let d3 = build("input#i[type=CheckBox][value=ABC]{}");
        assert!(m(&d3, "i", "[type=checkbox]"));
        assert!(!m(&d3, "i", "[value=abc]"));
        assert!(!m(&d3, "i", "[type=checkbox s]"));
    }

    #[test]
    fn combinators_backtrack() {
        let d = build(DOC);
        assert!(m(&d, "p1", "body p"));
        assert!(m(&d, "p1", "html body div p"));
        assert!(!m(&d, "p1", "head p"));
        assert!(m(&d, "p1", "div > p"));
        assert!(!m(&d, "p1", "body > p"));
        assert!(m(&d, "p2", "p + p"));
        assert!(m(&d, "p3", "span + p"));
        assert!(!m(&d, "p3", "p + p"));
        assert!(m(&d, "p3", "p ~ p"));
        assert!(m(&d, "p3", ".first ~ .last"));
        assert!(!m(&d, "p1", "p ~ p"));
        assert!(m(&d, "p3", "#p1 ~ span + p"));
        // Backtracking: `div div p` fails while `body div p` needs the right ancestor.
        assert!(!m(&d, "p1", "div div p"));
        assert!(m(&d, "p1", "#html #a > .first"));
        assert!(m(&d, "p1", "#head ~ #body #a > .first"));
        assert!(m(&d, "p1", "#head + #body #a > .first"));
        assert!(!m(&d, "p1", "#body ~ #head #a > .first"));
        assert!(!m(&d, "p1", "#body + #a > .first"));
        assert!(m(&d, "s1", "body .x > span"));
        assert!(m(&d, "l7", "li ~ li ~ li"));
        assert!(!m(&d, "l2", "li ~ li ~ li"));
    }

    #[test]
    fn structural_pseudo_classes() {
        let d = build(DOC);
        assert!(m(&d, "html", ":root"));
        assert!(!m(&d, "body", ":root"));
        assert!(m(&d, "b", ":empty"));
        assert!(m(&d, "p2", ":empty"));
        assert!(!m(&d, "c", ":empty"));
        assert!(!m(&d, "p1", ":empty"));
        assert!(m(&d, "p1", ":first-child"));
        assert!(!m(&d, "p2", ":first-child"));
        assert!(m(&d, "p3", ":last-child"));
        assert!(m(&d, "c1", ":only-child"));
        assert!(m(&d, "c1", ":empty"));
        assert!(m(&d, "p1", ":first-of-type"));
        assert!(m(&d, "s1", ":first-of-type:last-of-type:only-of-type"));
        assert!(m(&d, "p3", ":last-of-type"));
        assert!(!m(&d, "p2", ":last-of-type"));
        assert!(!m(&d, "p1", ":only-of-type"));
        assert!(m(&d, "html", ":first-child"));
        assert!(m(&d, "html", ":only-child"));
    }

    #[test]
    fn nth_formulas() {
        let d = build(DOC);
        assert_eq!(all(&d, "li:nth-child(2n)"), ["l2", "l4", "l6"]);
        assert_eq!(all(&d, "li:nth-child(odd)"), ["l1", "l3", "l5", "l7"]);
        assert_eq!(all(&d, "li:nth-child(3)"), ["l3"]);
        assert_eq!(all(&d, "li:nth-child(-n+2)"), ["l1", "l2"]);
        assert_eq!(all(&d, "li:nth-child(-2n+5)"), ["l1", "l3", "l5"]);
        assert_eq!(all(&d, "li:nth-child(n+6)"), ["l6", "l7"]);
        assert_eq!(all(&d, "li:nth-child(3n-1)"), ["l2", "l5"]);
        assert_eq!(all(&d, "li:nth-child(0n+0)"), Vec::<String>::new());
        assert_eq!(all(&d, "li:nth-child(-n)"), Vec::<String>::new());
        assert_eq!(all(&d, "li:nth-last-child(2)"), ["l6"]);
        assert_eq!(all(&d, "li:nth-last-child(-n+2)"), ["l6", "l7"]);
        assert_eq!(all(&d, "li:nth-child(2 of .imp)"), ["l5"]);
        assert_eq!(all(&d, "li:nth-child(odd of .imp)"), ["l3"]);
        assert_eq!(all(&d, "li:nth-last-child(1 of .imp)"), ["l5"]);
        assert_eq!(all(&d, ":nth-child(1 of li, p)"), ["p1", "c1", "l1"]);
        assert_eq!(all(&d, "#a > :nth-of-type(2)"), ["p2"]);
        assert_eq!(all(&d, "#a > p:nth-of-type(odd)"), ["p1", "p3"]);
        assert_eq!(all(&d, "#a > :nth-last-of-type(1)"), ["s1", "p3"]);
        assert_eq!(all(&d, "#a > p:nth-child(2n)"), ["p2", "p3"]);
        assert_eq!(all(&d, "#a > p:nth-child(2n+1)"), ["p1"]);
        assert!(!m(&d, "html", ":nth-child(2)"));
    }

    #[test]
    fn logical_pseudo_classes() {
        let d = build(DOC);
        assert!(m(&d, "p1", ":not(.last)"));
        assert!(!m(&d, "p1", ":not(.first, span)"));
        assert!(m(&d, "s1", ":is(p, span)"));
        assert!(m(&d, "s1", ":where(#a > *)"));
        assert!(!m(&d, "s1", ":is(p)"));
        assert!(m(&d, "p3", ":is(#a, #b) > :is(.last)"));
        assert!(m(&d, "a", ":has(p)"));
        assert!(m(&d, "a", ":has(> p)"));
        assert!(m(&d, "a", ":has(> .first + p)"));
        assert!(!m(&d, "a", ":has(> .first + span)"));
        assert!(m(&d, "body", ":has(.first + p)"));
        assert!(!m(&d, "body", ":has(> p)"));
        assert!(m(&d, "body", ":has(> div > p)"));
        assert!(!m(&d, "b", ":has(*)"));
        assert!(m(&d, "p1", ":has(+ p)"));
        assert!(m(&d, "p1", ":has(~ span)"));
        assert!(!m(&d, "p3", ":has(~ span)"));
        assert!(m(&d, "a", ":has(+ div)"));
        assert!(m(&d, "a", ":has(~ ul > li)"));
        assert!(!m(&d, "a", ":has(+ ul)"));
        assert!(!m(&d, "a", ":has(> span > p)"));
        assert!(m(&d, "body", "body:has(#a):has(#b)"));
        assert!(!m(&d, "p1", ":has(p)"));
        assert_eq!(all(&d, "div:has(> p:first-child)"), ["a", "c"]);
        assert_eq!(all(&d, "li:has(+ .imp)"), ["l2", "l4"]);
    }

    #[test]
    fn dynamic_state() {
        let d = build(DOC);
        let mut ctx = MatchContext::new();
        assert!(!mc(&d, "p1", ":hover", &ctx));
        ctx.set_hovered(&d, Some(find(&d, "p1")));
        assert!(mc(&d, "p1", ":hover", &ctx));
        assert!(mc(&d, "a", ":hover", &ctx));
        assert!(mc(&d, "body", ":hover", &ctx));
        assert!(!mc(&d, "p2", ":hover", &ctx));
        assert!(mc(&d, "p1", "div:hover > p:hover", &ctx));
        ctx.set_active(&d, Some(find(&d, "p2")));
        assert!(mc(&d, "p2", ":active", &ctx));
        assert!(!mc(&d, "p1", ":active", &ctx));
        ctx.focused = Some(find(&d, "p3"));
        assert!(mc(&d, "p3", ":focus", &ctx));
        assert!(!mc(&d, "p3", ":focus-visible", &ctx));
        ctx.focus_visible = true;
        assert!(mc(&d, "p3", ":focus-visible", &ctx));
        assert!(mc(&d, "a", ":focus-within", &ctx));
        assert!(mc(&d, "p3", ":focus-within", &ctx));
        assert!(!mc(&d, "b", ":focus-within", &ctx));
        ctx.target_id = Some("b".into());
        assert!(mc(&d, "b", ":target", &ctx));
        assert!(!mc(&d, "a", ":target", &ctx));
        assert!(mc(&d, "link", ":link", &ctx));
        assert!(mc(&d, "link", ":any-link", &ctx));
        assert!(!mc(&d, "link", ":visited", &ctx));
        assert!(!mc(&d, "nolink", ":any-link", &ctx));
        ctx.visited = VisitedPolicy::AllVisited;
        assert!(mc(&d, "link", ":visited", &ctx));
        assert!(!mc(&d, "link", ":link", &ctx));
        ctx.visited = VisitedPolicy::Urls(["/y".to_string()].into_iter().collect());
        assert!(!mc(&d, "link", ":visited", &ctx));
        ctx.visited = VisitedPolicy::Urls(["/x".to_string()].into_iter().collect());
        assert!(mc(&d, "link", ":visited", &ctx));
        assert!(mc(&d, "html", ":scope", &ctx));
        ctx.scope = Some(find(&d, "a"));
        assert!(mc(&d, "a", ":scope", &ctx));
        assert!(mc(&d, "p1", ":scope > p", &ctx));
        assert!(!mc(&d, "html", ":scope", &ctx));
        assert!(mc(&d, "a", "&", &ctx));
    }

    #[test]
    fn form_pseudo_classes() {
        let d = build(r#"form#f{ input#t[type=text][required]{} input#cb[type=checkbox][checked]{} input#cb2[type=checkbox]{} input#r1[type=radio][name=g]{} input#r2[type=radio][name=g]{} input#r3[type=radio][name=h][checked]{} input#r4[type=radio][name=h]{} select#sel{ option#o1[selected]{} optgroup#og[disabled]{ option#o2{} } } fieldset#fs[disabled]{ legend#lg{ input#in-legend{} } input#in-fs{} } input#dis[disabled]{} button#btn{} button#btn2{} input#ro[readonly]{} input#ph[placeholder=Name]{} input#ph2[placeholder=Name][value=x]{} textarea#ta[placeholder=x]{} textarea#ta2{"filled"} progress#pr{} progress#pr2[value=1]{} div#ce[contenteditable]{ span#ce-child{} } div#ce2[contenteditable=false]{} custom-el#ce3{} p#plain{} }"#);
        assert!(m(&d, "cb", ":checked"));
        assert!(!m(&d, "cb2", ":checked"));
        assert!(m(&d, "o1", ":checked"));
        assert!(!m(&d, "t", ":checked"));
        assert!(m(&d, "cb", ":default"));
        assert!(!m(&d, "cb2", ":default"));
        assert!(m(&d, "o1", ":default"));
        assert!(m(&d, "btn", ":default"));
        assert!(!m(&d, "btn2", ":default"));
        assert!(m(&d, "r1", ":indeterminate"));
        assert!(m(&d, "r2", ":indeterminate"));
        assert!(!m(&d, "r3", ":indeterminate"));
        assert!(!m(&d, "r4", ":indeterminate"));
        assert!(!m(&d, "cb2", ":indeterminate"));
        assert!(m(&d, "pr", ":indeterminate"));
        assert!(!m(&d, "pr2", ":indeterminate"));
        assert!(m(&d, "dis", ":disabled"));
        assert!(!m(&d, "dis", ":enabled"));
        assert!(m(&d, "t", ":enabled"));
        assert!(!m(&d, "plain", ":enabled"));
        assert!(!m(&d, "plain", ":disabled"));
        assert!(m(&d, "o2", ":disabled"));
        assert!(m(&d, "og", ":disabled"));
        assert!(!m(&d, "o1", ":disabled"));
        assert!(m(&d, "in-fs", ":disabled"));
        assert!(!m(&d, "in-legend", ":disabled"));
        assert!(m(&d, "fs", ":disabled"));
        assert!(m(&d, "t", ":required"));
        assert!(!m(&d, "t", ":optional"));
        assert!(m(&d, "cb", ":optional"));
        assert!(!m(&d, "plain", ":optional"));
        assert!(m(&d, "t", ":read-write"));
        assert!(m(&d, "ro", ":read-only"));
        assert!(m(&d, "dis", ":read-only"));
        assert!(m(&d, "cb", ":read-only"));
        assert!(m(&d, "ta", ":read-write"));
        assert!(m(&d, "ce", ":read-write"));
        assert!(m(&d, "ce-child", ":read-write"));
        assert!(m(&d, "ce2", ":read-only"));
        assert!(m(&d, "plain", ":read-only"));
        assert!(m(&d, "ph", ":placeholder-shown"));
        assert!(!m(&d, "ph2", ":placeholder-shown"));
        assert!(!m(&d, "t", ":placeholder-shown"));
        assert!(m(&d, "ta", ":placeholder-shown"));
        assert!(!m(&d, "ta2", ":placeholder-shown"));
        assert!(m(&d, "plain", ":defined"));
        assert!(!m(&d, "ce3", ":defined"));
        // A form state reader overrides attributes.
        struct S;
        impl FormState for S {
            fn checked(&self, doc: &Document, e: NodeId) -> bool {
                doc.attr(e, "id") == Some("cb2")
            }
            fn indeterminate(&self, doc: &Document, e: NodeId) -> bool {
                doc.attr(e, "id") == Some("cb")
            }
            fn value(&self, _doc: &Document, _e: NodeId) -> String {
                "typed".into()
            }
        }
        let s = S;
        let ctx = MatchContext { form: Some(&s), ..MatchContext::new() };
        assert!(mc(&d, "cb2", ":checked", &ctx));
        assert!(!mc(&d, "cb", ":checked", &ctx));
        assert!(mc(&d, "cb", ":indeterminate", &ctx));
        assert!(!mc(&d, "ph", ":placeholder-shown", &ctx));
        assert!(mc(&d, "cb", ":default", &ctx));
    }

    #[test]
    fn lang_and_dir() {
        let d = build(r#"html#h[lang=en-US]{ body#b{ p#p{} div#de[lang=de-CH]{ span#s{} } div#none[lang=]{} bdo#r[dir=rtl]{ i#ri{} } bdo#l[dir=LTR]{} } }"#);
        assert!(m(&d, "p", ":lang(en)"));
        assert!(m(&d, "p", ":lang(EN-us)"));
        assert!(!m(&d, "p", ":lang(en-GB)"));
        assert!(!m(&d, "p", ":lang(e)"));
        assert!(m(&d, "s", ":lang(de)"));
        assert!(m(&d, "s", ":lang('*-CH')"));
        assert!(m(&d, "s", ":lang(\"*\")"));
        assert!(!m(&d, "s", ":lang(en)"));
        assert!(m(&d, "s", ":lang(en, de)"));
        assert!(!m(&d, "none", ":lang(en)"));
        assert!(!m(&d, "none", ":lang('*')"));
        let mut ctx = MatchContext::new();
        let d2 = build("p#p{}");
        assert!(!mc(&d2, "p", ":lang(fr)", &ctx));
        ctx.document_lang = "fr-CA".into();
        assert!(mc(&d2, "p", ":lang(fr)", &ctx));
        assert!(mc(&d2, "p", ":lang(fr-CA)", &ctx));
        assert!(m(&d, "r", ":dir(rtl)"));
        assert!(m(&d, "ri", ":dir(rtl)"));
        assert!(!m(&d, "ri", ":dir(ltr)"));
        assert!(m(&d, "l", ":dir(ltr)"));
        assert!(m(&d, "p", ":dir(ltr)"));
        assert!(lang_matches("de-*-DE", "de-Latn-DE"));
        assert!(lang_matches("de-DE", "de-Latn-DE"));
        assert!(!lang_matches("de-DE", "de-x-DE"));
        assert!(lang_matches("*", "en"));
    }

    #[test]
    fn selector_index_candidates() {
        let d = build(DOC);
        let mut idx: SelectorIndex<u32> = SelectorIndex::new();
        for (i, s) in ["#a", ".x", "div", "p", "*", ":hover", "body .first", "div > span#s1", ".x.y", "P"].iter().enumerate() {
            idx.insert(sel(s), i as u32);
        }
        let cands: Vec<u32> = idx.candidates(&d, find(&d, "a")).map(|e| e.data).collect();
        assert_eq!(cands, vec![0, 1, 2, 4, 5, 8]);
        let cands: Vec<u32> = idx.candidates(&d, find(&d, "p1")).map(|e| e.data).collect();
        assert_eq!(cands, vec![3, 4, 5, 6, 9]);
        let cands: Vec<u32> = idx.candidates(&d, find(&d, "s1")).map(|e| e.data).collect();
        assert_eq!(cands, vec![4, 5, 7]);
        let matched: Vec<u32> = idx.matching(&d, find(&d, "p1"), &MatchContext::new()).iter().map(|e| e.data).collect();
        assert_eq!(matched, vec![3, 4, 6, 9]);
        let matched: Vec<u32> = idx.matching(&d, find(&d, "s1"), &MatchContext::new()).iter().map(|e| e.data).collect();
        assert_eq!(matched, vec![4, 7]);
        assert_eq!(idx.len(), 10);
        // A pseudo-element selector is bucketed by its originating compound.
        idx.insert(sel("p::before"), 10);
        assert!(idx.candidates(&d, find(&d, "p1")).any(|e| e.data == 10));
    }

    #[test]
    fn dependencies() {
        let d = sel("div.a#b[data-x]:hover > p:nth-child(2 of .c):not(.d)").dependencies();
        assert_eq!(d.tags, ["div", "p"].map(String::from).into_iter().collect());
        assert_eq!(d.classes, ["a", "c", "d"].map(String::from).into_iter().collect());
        assert_eq!(d.ids, ["b"].map(String::from).into_iter().collect());
        assert_eq!(d.attributes, ["class", "data-x", "id"].map(String::from).into_iter().collect());
        assert_eq!(d.pseudo_classes, ["hover", "not", "nth-child"].map(String::from).into_iter().collect());
        assert!(d.structural);
        assert!(d.state);
        assert!(!d.has);
        assert!(!d.form);
        let d = sel("a:has(+ input:checked)").dependencies();
        assert!(d.has && d.structural && d.form);
        assert!(d.attributes.contains("checked"));
        let d = sel("p").dependencies();
        assert!(!d.structural && !d.state && !d.form && !d.has);
        let d = sel("li ~ li").dependencies();
        assert!(d.structural);
    }
}
