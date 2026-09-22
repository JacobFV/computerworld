//! Node wrappers, tree operations, attributes and the native accessors on
//! `Node.prototype`, `Element.prototype` and `CharacterData.prototype`.

use cw_jsvm::value::{Args, HostHooks, JsResult, Key, Obj, Value};
use cw_jsvm::vm::Vm;

use super::{
    arg_bool, arg_node, arg_str, dom_exception, inner, node_array, node_of, opt_node, string_val,
    this_node,
};
use crate::dom::{Document, Namespace, NodeId, NodeKind};

// ---------------------------------------------------------------- wrappers

fn none_set(_vm: &mut Vm, _o: &Obj, _k: &Key, _v: &Value) -> JsResult<Option<bool>> {
    Ok(None)
}
fn none_delete(_vm: &mut Vm, _o: &Obj, _k: &Key) -> JsResult<Option<bool>> {
    Ok(None)
}
fn none_keys(_vm: &mut Vm, _o: &Obj) -> JsResult<Vec<Key>> {
    Ok(Vec::new())
}
fn node_get(_vm: &mut Vm, _o: &Obj, _k: &Key) -> JsResult<Option<Value>> {
    Ok(None)
}

/// `form.name` / `form[0]`: listed elements by index, name or id.
fn form_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let Key::Str(s) = k else { return Ok(None) };
    let Some(id) = o.host_id() else {
        return Ok(None);
    };
    let form = NodeId(id);
    let found = {
        let rc = inner(vm);
        let inner = rc.borrow();
        if let Some(i) = k.array_index() {
            inner.form_elements(form).get(i as usize).copied()
        } else {
            let first = s.as_bytes().first().copied().unwrap_or(b'_');
            // Names that are IDL members never reach the document walk.
            if !first.is_ascii_alphabetic() || o.borrow().props.find_str(s).is_some() {
                None
            } else {
                let name = s.as_str();
                if is_form_member(name) {
                    None
                } else {
                    inner.form_elements(form).into_iter().find(|n| {
                        inner.doc.attr(*n, "name") == Some(name)
                            || inner.doc.attr(*n, "id") == Some(name)
                    })
                }
            }
        }
    };
    Ok(found.map(|n| wrap_node(vm, n)))
}

fn is_form_member(name: &str) -> bool {
    matches!(
        name,
        "submit"
            | "reset"
            | "elements"
            | "length"
            | "action"
            | "method"
            | "target"
            | "name"
            | "enctype"
            | "encoding"
            | "noValidate"
            | "checkValidity"
            | "reportValidity"
            | "requestSubmit"
            | "acceptCharset"
            | "autocomplete"
            | "then"
            | "constructor"
            | "style"
            | "id"
            | "className"
            | "children"
            | "parentNode"
            | "nodeType"
            | "tagName"
            | "nodeName"
    ) || name.starts_with("on")
        || name.starts_with("__")
}

/// `select[i]`: the option at an index.
fn select_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let Some(i) = k.array_index() else {
        return Ok(None);
    };
    let Some(id) = o.host_id() else {
        return Ok(None);
    };
    let opt = inner(vm)
        .borrow()
        .options_of(NodeId(id))
        .get(i as usize)
        .copied();
    Ok(opt.map(|n| wrap_node(vm, n)))
}

pub static NODE_HOOKS: HostHooks = HostHooks {
    class: "Node",
    get: node_get,
    set: none_set,
    delete: none_delete,
    keys: none_keys,
};
pub static FORM_HOOKS: HostHooks = HostHooks {
    class: "HTMLFormElement",
    get: form_get,
    set: none_set,
    delete: none_delete,
    keys: none_keys,
};
pub static SELECT_HOOKS: HostHooks = HostHooks {
    class: "HTMLSelectElement",
    get: select_get,
    set: none_set,
    delete: none_delete,
    keys: none_keys,
};

/// The prototype key of a node: the element's local name, or a generic class.
fn proto_key(doc: &Document, id: NodeId) -> (String, &'static str) {
    match doc.kind(id) {
        NodeKind::Element {
            ns: Namespace::Html,
            tag,
            ..
        } => (
            tag.clone(),
            if tag.contains('-') {
                "*custom"
            } else {
                "*unknown"
            },
        ),
        NodeKind::Element {
            ns: Namespace::Svg,
            tag,
            ..
        } => (format!("svg:{tag}"), "*svg"),
        NodeKind::Element { .. } => ("*element".into(), "*element"),
        NodeKind::Text(_) => ("#text".into(), "#text"),
        NodeKind::Comment(_) => ("#comment".into(), "#comment"),
        NodeKind::Document => ("#document".into(), "#document"),
        NodeKind::DocumentFragment => ("#fragment".into(), "#fragment"),
        NodeKind::DocType { .. } => ("#doctype".into(), "#doctype"),
    }
}

/// The JS wrapper of a node, created on first use and cached so identity holds.
pub fn wrap_node(vm: &mut Vm, id: NodeId) -> Value {
    let rc = inner(vm);
    let (proto, hooks) = {
        let inner = rc.borrow();
        if let Some(Some(o)) = inner.wrappers.get(id.index()) {
            return Value::Obj(o.clone());
        }
        let (key, fallback) = proto_key(&inner.doc, id);
        let proto = inner
            .protos
            .get(&key)
            .or_else(|| inner.protos.get(fallback))
            .or_else(|| inner.protos.get("*element"))
            .cloned();
        let hooks: &'static HostHooks = match key.as_str() {
            "form" => &FORM_HOOKS,
            "select" => &SELECT_HOOKS,
            _ => &NODE_HOOKS,
        };
        (proto, hooks)
    };
    let o = vm.host_obj(proto, hooks, vec![Value::Num(id.0 as f64)]);
    let mut inner = rc.borrow_mut();
    if inner.wrappers.len() <= id.index() {
        inner.wrappers.resize(id.index() + 1, None);
    }
    inner.wrappers[id.index()] = Some(o.clone());
    Value::Obj(o)
}

/// Re-prototypes a wrapper (custom element upgrade, `attachShadow` hosts).
fn set_proto(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let (Value::Obj(o), Value::Obj(p)) = (a.arg(0), a.arg(1)) {
        o.borrow_mut().proto = Some(p);
    }
    Ok(Value::Undefined)
}

// ---------------------------------------------------------------- notifications

/// Calls a prelude hook after a mutation, when someone listens for it.
fn notify(vm: &mut Vm, hook: &str, args: Vec<Value>) -> JsResult<()> {
    let Some(hooks) = vm.global.own_value("%hooks") else {
        return Ok(());
    };
    let f = vm.get_str(&hooks, hook)?;
    if f.is_callable() {
        vm.call(&f, hooks, args)?;
    }
    Ok(())
}

fn subtree_needs_reaction(doc: &Document, custom: bool, root: NodeId) -> bool {
    doc.descendants(root).any(|n| match doc.tag(n) {
        Some(t) => {
            matches!(t, "script" | "style" | "link" | "img" | "iframe")
                || (custom && t.contains('-'))
                || doc.has_attr(n, "is")
        }
        None => false,
    })
}

// ---------------------------------------------------------------- tree reads

fn node_type(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let t = match inner(vm).borrow().doc.kind(n) {
        NodeKind::Element { .. } => 1,
        NodeKind::Text(_) => 3,
        NodeKind::Comment(_) => 8,
        NodeKind::Document => 9,
        NodeKind::DocType { .. } => 10,
        NodeKind::DocumentFragment => 11,
    };
    Ok(Value::Num(t as f64))
}

fn node_name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let s = match inner(vm).borrow().doc.kind(n) {
        NodeKind::Element {
            ns: Namespace::Html,
            tag,
            ..
        } => tag.to_ascii_uppercase(),
        NodeKind::Element { tag, .. } => tag.clone(),
        NodeKind::Text(_) => "#text".into(),
        NodeKind::Comment(_) => "#comment".into(),
        NodeKind::Document => "#document".into(),
        NodeKind::DocType { name, .. } => name.clone(),
        NodeKind::DocumentFragment => "#document-fragment".into(),
    };
    Ok(string_val(s))
}

fn local_name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let s = inner(vm).borrow().doc.tag(n).unwrap_or("").to_owned();
    Ok(string_val(s))
}

fn namespace_uri(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let s = match inner(vm).borrow().doc.kind(n) {
        NodeKind::Element {
            ns: Namespace::Html,
            ..
        } => "http://www.w3.org/1999/xhtml",
        NodeKind::Element {
            ns: Namespace::Svg, ..
        } => "http://www.w3.org/2000/svg",
        NodeKind::Element {
            ns: Namespace::MathMl,
            ..
        } => "http://www.w3.org/1998/Math/MathML",
        _ => return Ok(Value::Null),
    };
    Ok(Value::str(s))
}

macro_rules! link_getter {
    ($name:ident, $f:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
            let n = this_node(vm, a)?;
            let r: Option<NodeId> = {
                let rc = inner(vm);
                let i = rc.borrow();
                let f: fn(&Document, NodeId) -> Option<NodeId> = $f;
                f(&i.doc, n)
            };
            Ok(opt_node(vm, r))
        }
    };
}

/// The parent as script sees it: a template's content fragment has no parent.
fn script_parent(d: &Document, n: NodeId) -> Option<NodeId> {
    let p = d.parent(n)?;
    if matches!(d.kind(n), NodeKind::DocumentFragment) && d.is(p, "template") {
        return None;
    }
    Some(p)
}

/// Children as script sees them: a template's content fragment is not a child.
fn visible(d: &Document, c: Option<NodeId>, forward: bool) -> Option<NodeId> {
    let mut cur = c;
    while let Some(n) = cur {
        let hidden = matches!(d.kind(n), NodeKind::DocumentFragment)
            && d.parent(n).map(|p| d.is(p, "template")).unwrap_or(false);
        if !hidden {
            return Some(n);
        }
        cur = if forward {
            d.next_sibling(n)
        } else {
            d.prev_sibling(n)
        };
    }
    None
}

link_getter!(parent_node, script_parent);
link_getter!(parent_element, |d, n| script_parent(d, n)
    .filter(|p| d.is_element(*p)));
link_getter!(first_child, |d, n| visible(d, d.first_child(n), true));
link_getter!(last_child, |d, n| visible(d, d.last_child(n), false));
link_getter!(next_sibling, |d, n| visible(d, d.next_sibling(n), true));
link_getter!(prev_sibling, |d, n| visible(d, d.prev_sibling(n), false));
link_getter!(first_element_child, |d, n| d.element_children(n).next());
link_getter!(last_element_child, |d, n| {
    let mut c = d.last_child(n);
    while let Some(x) = c {
        if d.is_element(x) {
            return Some(x);
        }
        c = d.prev_sibling(x);
    }
    None
});
link_getter!(next_element_sibling, |d, n| {
    let mut c = d.next_sibling(n);
    while let Some(x) = c {
        if d.is_element(x) {
            return Some(x);
        }
        c = d.next_sibling(x);
    }
    None
});
link_getter!(prev_element_sibling, |d, n| {
    let mut c = d.prev_sibling(n);
    while let Some(x) = c {
        if d.is_element(x) {
            return Some(x);
        }
        c = d.prev_sibling(x);
    }
    None
});

pub fn is_connected(d: &Document, n: NodeId) -> bool {
    if n == Document::ROOT {
        return true;
    }
    if d.node(n).detached {
        return false;
    }
    d.ancestors(n).last() == Some(Document::ROOT)
}

fn is_connected_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let c = is_connected(&inner(vm).borrow().doc, n);
    Ok(Value::Bool(c))
}

fn has_child_nodes(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(Value::Bool(
        visible(&i.doc, i.doc.first_child(n), true).is_some(),
    ))
}

fn child_element_count(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let c = inner(vm).borrow().doc.element_children(n).count();
    Ok(Value::Num(c as f64))
}

fn contains(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let Some(o) = node_of(&a.arg(0)) else {
        return Ok(Value::Bool(false));
    };
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(Value::Bool(o == n || i.doc.ancestors(o).any(|x| x == n)))
}

fn compare_document_position(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let o = arg_node(vm, a, 0)?;
    if n == o {
        return Ok(Value::Num(0.0));
    }
    let rc = inner(vm);
    let i = rc.borrow();
    let d = &i.doc;
    let root = |x: NodeId| d.ancestors(x).last().unwrap_or(x);
    if root(n) != root(o) {
        // Disconnected, implementation specific, consistent ordering by id.
        return Ok(Value::Num((1 | 32 | if o.0 < n.0 { 2 } else { 4 }) as f64));
    }
    if d.ancestors(n).any(|x| x == o) {
        return Ok(Value::Num((8 | 2) as f64));
    }
    if d.ancestors(o).any(|x| x == n) {
        return Ok(Value::Num((16 | 4) as f64));
    }
    let r = root(n);
    for x in d.descendants(r) {
        if x == n {
            return Ok(Value::Num(4.0));
        }
        if x == o {
            return Ok(Value::Num(2.0));
        }
    }
    Ok(Value::Num(1.0))
}

fn is_equal(d: &Document, a: NodeId, b: NodeId) -> bool {
    let same = match (d.kind(a), d.kind(b)) {
        (
            NodeKind::Element {
                ns: n1,
                tag: t1,
                attrs: a1,
            },
            NodeKind::Element {
                ns: n2,
                tag: t2,
                attrs: a2,
            },
        ) => n1 == n2 && t1 == t2 && a1.len() == a2.len() && a1.iter().all(|x| a2.contains(x)),
        (x, y) => x == y,
    };
    if !same {
        return false;
    }
    let ka: Vec<NodeId> = d.children(a).collect();
    let kb: Vec<NodeId> = d.children(b).collect();
    ka.len() == kb.len() && ka.iter().zip(kb.iter()).all(|(x, y)| is_equal(d, *x, *y))
}

fn is_equal_node(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let Some(o) = node_of(&a.arg(0)) else {
        return Ok(Value::Bool(false));
    };
    let r = is_equal(&inner(vm).borrow().doc, n, o);
    Ok(Value::Bool(r))
}

// ---------------------------------------------------------------- text

fn text_content_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(match i.doc.kind(n) {
        NodeKind::Document | NodeKind::DocType { .. } => Value::Null,
        NodeKind::Text(t) | NodeKind::Comment(t) => Value::str(t),
        _ => {
            let root = i.doc.template_contents(n).unwrap_or(n);
            let _ = root;
            string_val(i.doc.text_content(n))
        }
    })
}

/// Replaces all children with one text node (or none for the empty string).
pub fn replace_children_with_text(vm: &mut Vm, n: NodeId, text: &str) -> JsResult<()> {
    let removed: Vec<NodeId> = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let kids: Vec<NodeId> = i.doc.children(n).collect();
        for k in &kids {
            i.doc.detach(*k);
        }
        i.touch();
        kids
    };
    let added = if text.is_empty() {
        None
    } else {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let t = i.doc.create_text(text);
        i.doc.append(n, t);
        Some(t)
    };
    after_children_changed(
        vm,
        n,
        &added.into_iter().collect::<Vec<_>>(),
        &removed,
        None,
        None,
    )
}

fn text_content_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let v = a.arg(0);
    let text = if v.is_nullish() {
        String::new()
    } else {
        vm.to_string(&v)?.to_string()
    };
    let is_char = matches!(
        inner(vm).borrow().doc.kind(n),
        NodeKind::Text(_) | NodeKind::Comment(_)
    );
    if is_char {
        set_char_data(vm, n, &text)?;
    } else {
        replace_children_with_text(vm, n, &text)?;
    }
    Ok(Value::Undefined)
}

fn data_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(match i.doc.kind(n) {
        NodeKind::Text(t) | NodeKind::Comment(t) => Value::str(t),
        _ => Value::str(""),
    })
}

pub fn set_char_data(vm: &mut Vm, n: NodeId, text: &str) -> JsResult<()> {
    let (old, observing) = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let old = match i.doc.kind(n) {
            NodeKind::Text(t) | NodeKind::Comment(t) => t.clone(),
            _ => String::new(),
        };
        if matches!(i.doc.kind(n), NodeKind::Text(_)) {
            i.doc.set_text(n, text);
        } else if let NodeKind::Comment(c) = &mut i.doc.node_mut(n).kind {
            *c = text.to_owned();
        }
        i.touch();
        if i.doc
            .parent(n)
            .map(|p| i.doc.is(p, "style"))
            .unwrap_or(false)
        {
            i.sheets_dirty = true;
        }
        (old, i.observing)
    };
    if observing {
        let w = wrap_node(vm, n);
        notify(vm, "characterData", vec![w, string_val(old)])?;
    }
    Ok(())
}

fn data_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let v = a.arg(0);
    let text = if matches!(v, Value::Null) {
        String::new()
    } else {
        vm.to_string(&v)?.to_string()
    };
    set_char_data(vm, n, &text)?;
    Ok(Value::Undefined)
}

// ---------------------------------------------------------------- creation

fn create_element(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let tag = arg_str(vm, a, 0)?;
    let ns = arg_str(vm, a, 1)?;
    if tag.is_empty()
        || tag
            .chars()
            .any(|c| c.is_whitespace() || c == '>' || c == '<')
    {
        return Err(dom_exception(
            vm,
            "InvalidCharacterError",
            &format!("The tag name provided ('{tag}') is not a valid name."),
        ));
    }
    let id = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let (ns, tag) = match ns.as_str() {
            "http://www.w3.org/2000/svg" => (Namespace::Svg, tag),
            "http://www.w3.org/1998/Math/MathML" => (Namespace::MathMl, tag),
            _ => (Namespace::Html, tag.to_ascii_lowercase()),
        };
        let id = i.doc.create(NodeKind::Element {
            ns,
            tag: tag.clone(),
            attrs: Vec::new(),
        });
        if tag == "template" && ns == Namespace::Html {
            let frag = i.doc.create(NodeKind::DocumentFragment);
            i.doc.append(id, frag);
            i.doc.mutations.clear();
        }
        id
    };
    Ok(wrap_node(vm, id))
}

fn create_text(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = arg_str(vm, a, 0)?;
    let id = inner(vm).borrow_mut().doc.create_text(&s);
    Ok(wrap_node(vm, id))
}

fn create_comment(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = arg_str(vm, a, 0)?;
    let id = inner(vm).borrow_mut().doc.create(NodeKind::Comment(s));
    Ok(wrap_node(vm, id))
}

fn create_fragment(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let id = inner(vm)
        .borrow_mut()
        .doc
        .create(NodeKind::DocumentFragment);
    Ok(wrap_node(vm, id))
}

fn create_doctype(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let name = arg_str(vm, a, 0)?;
    let public_id = arg_str(vm, a, 1)?;
    let system_id = arg_str(vm, a, 2)?;
    let id = inner(vm).borrow_mut().doc.create(NodeKind::DocType {
        name,
        public_id,
        system_id,
    });
    Ok(wrap_node(vm, id))
}

fn clone_node(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let deep = arg_bool(a, 1);
    let id = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let marks = i.doc.mutations.len();
        let id = if deep {
            i.doc.clone_subtree(n)
        } else {
            let kind = i.doc.kind(n).clone();
            let id = i.doc.create(kind);
            if i.doc.is(id, "template") {
                let frag = i.doc.create(NodeKind::DocumentFragment);
                i.doc.append(id, frag);
            }
            id
        };
        i.doc.mutations.truncate(marks);
        // Dirty form state travels with the clone.
        let pairs: Vec<(NodeId, NodeId)> = if deep {
            i.doc.descendants(n).zip(i.doc.descendants(id)).collect()
        } else {
            vec![(n, id)]
        };
        for (from, to) in pairs {
            if let Some(v) = i.form.values.get(&from).cloned() {
                i.form.values.insert(to, v);
            }
            if let Some(c) = i.form.checked.get(&from).copied() {
                i.form.checked.insert(to, c);
            }
        }
        id
    };
    Ok(wrap_node(vm, id))
}

// ---------------------------------------------------------------- tree mutation

/// Observer, custom element, script and stylesheet reactions after a child list
/// changed.
pub fn after_children_changed(
    vm: &mut Vm,
    parent: NodeId,
    added: &[NodeId],
    removed: &[NodeId],
    prev: Option<NodeId>,
    next: Option<NodeId>,
) -> JsResult<()> {
    let (observing, react_added, react_removed) = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        i.touch();
        let custom = !i.custom_defined.is_empty();
        let mut style_touched = i.doc.is(parent, "style");
        let mut ra = Vec::new();
        for n in added {
            if subtree_needs_reaction(&i.doc, custom, *n) {
                ra.push(*n);
                style_touched = true;
            }
        }
        let mut rr = Vec::new();
        for n in removed {
            if subtree_needs_reaction(&i.doc, custom, *n) {
                rr.push(*n);
                style_touched = true;
            }
            if i.focused
                .map(|f| f == *n || i.doc.ancestors(f).any(|x| x == *n))
                .unwrap_or(false)
            {
                i.focused = None;
            }
            if i.hovered
                .map(|f| f == *n || i.doc.ancestors(f).any(|x| x == *n))
                .unwrap_or(false)
            {
                i.hovered = None;
            }
        }
        if style_touched {
            i.sheets_dirty = true;
        }
        (i.observing, ra, rr)
    };
    if observing && (!added.is_empty() || !removed.is_empty()) {
        let p = wrap_node(vm, parent);
        let a = node_array(vm, added);
        let r = node_array(vm, removed);
        let pv = opt_node(vm, prev);
        let nx = opt_node(vm, next);
        notify(vm, "childList", vec![p, a, r, pv, nx])?;
    }
    for n in react_removed {
        let w = wrap_node(vm, n);
        notify(vm, "removed", vec![w])?;
    }
    for n in react_added {
        let w = wrap_node(vm, n);
        notify(vm, "inserted", vec![w])?;
    }
    Ok(())
}

fn check_insert(vm: &mut Vm, parent: NodeId, child: NodeId) -> JsResult<()> {
    let bad = {
        let rc = inner(vm);
        let i = rc.borrow();
        let d = &i.doc;
        if !matches!(
            d.kind(parent),
            NodeKind::Element { .. } | NodeKind::Document | NodeKind::DocumentFragment
        ) {
            Some("This node type does not support this method.")
        } else if child == parent || d.ancestors(parent).any(|x| x == child) {
            Some("The new child element contains the parent.")
        } else if matches!(d.kind(child), NodeKind::Document) {
            Some("Nodes of type '#document' may not be inserted inside nodes of this type.")
        } else {
            None
        }
    };
    match bad {
        Some(m) => Err(dom_exception(vm, "HierarchyRequestError", m)),
        None => Ok(()),
    }
}

/// `insertBefore(parent, child, ref)`: fragments insert their children.
pub fn insert(vm: &mut Vm, parent: NodeId, child: NodeId, before: Option<NodeId>) -> JsResult<()> {
    check_insert(vm, parent, child)?;
    let (target, added, old_parent, prev, before) = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        // A template's children live in its content fragment only when parsed; script
        // appends to the element itself, as the DOM does.
        if let Some(b) = before {
            if i.doc.parent(b) != Some(parent) {
                drop(i);
                return Err(dom_exception(vm, "NotFoundError", "The node before which the new node is to be inserted is not a child of this node."));
            }
        }
        let before = if before == Some(child) {
            i.doc.next_sibling(child)
        } else {
            before
        };
        let is_fragment = matches!(i.doc.kind(child), NodeKind::DocumentFragment);
        let nodes: Vec<NodeId> = if is_fragment {
            i.doc.children(child).collect()
        } else {
            vec![child]
        };
        let old_parent = if is_fragment {
            None
        } else {
            i.doc.parent(child)
        };
        let prev = match before {
            Some(b) => i.doc.prev_sibling(b),
            None => i.doc.last_child(parent),
        };
        for n in &nodes {
            i.doc.insert_before(parent, *n, before);
        }
        (parent, nodes, old_parent, prev, before)
    };
    if let Some(op) = old_parent {
        after_children_changed(vm, op, &[], &[child], None, None)?;
    }
    let prev = prev.filter(|p| !added.contains(p));
    after_children_changed(vm, target, &added, &[], prev, before)
}

fn insert_before(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let parent = arg_node(vm, a, 0)?;
    let child = arg_node(vm, a, 1)?;
    let before = node_of(&a.arg(2));
    insert(vm, parent, child, before)?;
    Ok(a.arg(1))
}

pub fn remove(vm: &mut Vm, child: NodeId) -> JsResult<()> {
    let info = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        match i.doc.parent(child) {
            Some(p) => {
                let prev = i.doc.prev_sibling(child);
                let next = i.doc.next_sibling(child);
                i.doc.detach(child);
                Some((p, prev, next))
            }
            None => None,
        }
    };
    if let Some((p, prev, next)) = info {
        after_children_changed(vm, p, &[], &[child], prev, next)?;
    }
    Ok(())
}

fn remove_child(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let parent = arg_node(vm, a, 0)?;
    let child = arg_node(vm, a, 1)?;
    let ok = inner(vm).borrow().doc.parent(child) == Some(parent);
    if !ok {
        return Err(dom_exception(
            vm,
            "NotFoundError",
            "The node to be removed is not a child of this node.",
        ));
    }
    remove(vm, child)?;
    Ok(a.arg(1))
}

fn remove_self(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    remove(vm, n)?;
    Ok(Value::Undefined)
}

fn replace_child(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let parent = arg_node(vm, a, 0)?;
    let new = arg_node(vm, a, 1)?;
    let old = arg_node(vm, a, 2)?;
    let ok = inner(vm).borrow().doc.parent(old) == Some(parent);
    if !ok {
        return Err(dom_exception(
            vm,
            "NotFoundError",
            "The node to be replaced is not a child of this node.",
        ));
    }
    if new == old {
        return Ok(a.arg(2));
    }
    let next = {
        let rc = inner(vm);
        let i = rc.borrow();
        let nx = i.doc.next_sibling(old);
        if nx == Some(new) {
            i.doc.next_sibling(new)
        } else {
            nx
        }
    };
    check_insert(vm, parent, new)?;
    remove(vm, old)?;
    insert(vm, parent, new, next)?;
    Ok(a.arg(2))
}

fn normalize(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let all: Vec<NodeId> = i.doc.descendants(n).collect();
    for x in all {
        if i.doc.node(x).detached && x != n {
            continue;
        }
        let Some(t) = i.doc.text(x).map(str::to_owned) else {
            continue;
        };
        if t.is_empty() {
            i.doc.detach(x);
            continue;
        }
        let mut merged = t;
        let mut changed = false;
        while let Some(nx) = i.doc.next_sibling(x) {
            let Some(t2) = i.doc.text(nx).map(str::to_owned) else {
                break;
            };
            merged.push_str(&t2);
            i.doc.detach(nx);
            changed = true;
        }
        if changed {
            i.doc.set_text(x, &merged);
        }
    }
    i.touch();
    Ok(Value::Undefined)
}

// ---------------------------------------------------------------- attributes

fn attr_name(d: &Document, n: NodeId, name: &str) -> String {
    match d.kind(n) {
        NodeKind::Element {
            ns: Namespace::Html,
            ..
        } => name.to_ascii_lowercase(),
        _ => name.to_owned(),
    }
}

fn get_attribute(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let name = arg_str(vm, a, 0)?;
    let rc = inner(vm);
    let i = rc.borrow();
    let name = attr_name(&i.doc, n, &name);
    Ok(match i.doc.attr(n, &name) {
        Some(v) => Value::str(v),
        None => Value::Null,
    })
}

fn has_attribute(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let name = arg_str(vm, a, 0)?;
    let rc = inner(vm);
    let i = rc.borrow();
    let name = attr_name(&i.doc, n, &name);
    Ok(Value::Bool(i.doc.has_attr(n, &name)))
}

/// Sets (or with `None` removes) an attribute with all its side effects.
pub fn set_attribute_value(
    vm: &mut Vm,
    n: NodeId,
    name: &str,
    value: Option<&str>,
) -> JsResult<()> {
    let (old, notify_attr, custom, name) = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let name = attr_name(&i.doc, n, name);
        let old = i.doc.attr(n, &name).map(str::to_owned);
        if old.as_deref() == value && !i.observing {
            return Ok(());
        }
        match value {
            Some(v) => {
                // `Document::set_attr` lower-cases; SVG attributes keep their case.
                if name.bytes().any(|b| b.is_ascii_uppercase()) {
                    if let NodeKind::Element { attrs, .. } = &mut i.doc.node_mut(n).kind {
                        match attrs.iter_mut().find(|x| x.name == name) {
                            Some(x) => x.value = v.to_owned(),
                            None => attrs.push(crate::dom::Attribute {
                                name: name.clone(),
                                value: v.to_owned(),
                            }),
                        }
                    }
                    i.doc
                        .mutations
                        .push(crate::dom::Mutation::AttributeChanged {
                            node: n,
                            name: name.clone(),
                            old: old.clone(),
                        });
                } else {
                    i.doc.set_attr(n, &name, v);
                }
            }
            None => {
                if name.bytes().any(|b| b.is_ascii_uppercase()) {
                    if let NodeKind::Element { attrs, .. } = &mut i.doc.node_mut(n).kind {
                        attrs.retain(|x| x.name != name);
                    }
                    i.doc
                        .mutations
                        .push(crate::dom::Mutation::AttributeChanged {
                            node: n,
                            name: name.clone(),
                            old: old.clone(),
                        });
                } else {
                    i.doc.remove_attr(n, &name);
                }
            }
        }
        i.touch();
        if name == "style" {
            i.inline_cache.remove(&n);
        }
        let tag = i.doc.tag(n).unwrap_or("").to_owned();
        let tag = tag.as_str();
        if (tag == "link" && matches!(name.as_str(), "href" | "rel" | "media" | "disabled"))
            || (tag == "style" && matches!(name.as_str(), "media" | "type"))
        {
            i.sheets_dirty = true;
        }
        if tag == "canvas" && matches!(name.as_str(), "width" | "height") {
            let w = i
                .doc
                .attr(n, "width")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(300);
            let h = i
                .doc
                .attr(n, "height")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(150);
            if let Some(c) = i.canvases.get_mut(&n) {
                c.resize(w, h);
            }
        }
        let custom = i.custom_defined.contains(tag)
            || (!i.custom_defined.is_empty() && i.doc.has_attr(n, "is"));
        (old, i.observing, custom, name)
    };
    if notify_attr || custom {
        let w = wrap_node(vm, n);
        let old_v = match &old {
            Some(o) => Value::str(o),
            None => Value::Null,
        };
        let new_v = match value {
            Some(v) => Value::str(v),
            None => Value::Null,
        };
        notify(vm, "attribute", vec![w, string_val(name), old_v, new_v])?;
    }
    Ok(())
}

fn set_attribute(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let name = arg_str(vm, a, 0)?;
    if name.is_empty()
        || name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '/' | '>' | '=' | '\0'))
    {
        return Err(dom_exception(
            vm,
            "InvalidCharacterError",
            &format!("'{name}' is not a valid attribute name."),
        ));
    }
    let v = a.arg(1);
    let value = vm.to_string(&v)?.to_string();
    set_attribute_value(vm, n, &name, Some(&value))?;
    Ok(Value::Undefined)
}

fn remove_attribute(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let name = arg_str(vm, a, 0)?;
    set_attribute_value(vm, n, &name, None)?;
    Ok(Value::Undefined)
}

fn toggle_attribute(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let name = arg_str(vm, a, 0)?;
    let has = {
        let rc = inner(vm);
        let i = rc.borrow();
        let nm = attr_name(&i.doc, n, &name);
        i.doc.has_attr(n, &nm)
    };
    let want = if a.arg(1).is_undefined() {
        !has
    } else {
        a.arg(1).truthy()
    };
    if want && !has {
        set_attribute_value(vm, n, &name, Some(""))?;
    } else if !want && has {
        set_attribute_value(vm, n, &name, None)?;
    }
    Ok(Value::Bool(want))
}

fn get_attribute_names(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let names: Vec<String> = inner(vm)
        .borrow()
        .doc
        .attrs(n)
        .iter()
        .map(|x| x.name.clone())
        .collect();
    Ok(super::str_array(vm, &names))
}

fn has_attributes(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let r = !inner(vm).borrow().doc.attrs(n).is_empty();
    Ok(Value::Bool(r))
}

/// `[name, value]` of the attribute at an index, for `NamedNodeMap`.
fn attr_at(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let idx = super::arg_num(vm, a, 1)? as usize;
    let pair = inner(vm)
        .borrow()
        .doc
        .attrs(n)
        .get(idx)
        .map(|x| (x.name.clone(), x.value.clone()));
    Ok(match pair {
        Some((k, v)) => vm.arr(vec![string_val(k), string_val(v)]),
        None => Value::Null,
    })
}

/// A reflected string attribute getter/setter pair is common enough to be native:
/// `id`, `className`.
fn id_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(Value::str(i.doc.attr(n, "id").unwrap_or("")))
}
fn id_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let v = arg_str(vm, a, 0)?;
    set_attribute_value(vm, n, "id", Some(&v))?;
    Ok(Value::Undefined)
}
fn class_name_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(Value::str(i.doc.attr(n, "class").unwrap_or("")))
}
fn class_name_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let v = arg_str(vm, a, 0)?;
    set_attribute_value(vm, n, "class", Some(&v))?;
    Ok(Value::Undefined)
}

fn tag_name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    node_name(vm, a)
}

// ---------------------------------------------------------------- install

pub fn install_node_accessors(vm: &mut Vm, p: &Obj) {
    vm.accessor(p, "nodeType", node_type, None);
    vm.accessor(p, "nodeName", node_name, None);
    vm.accessor(p, "parentNode", parent_node, None);
    vm.accessor(p, "parentElement", parent_element, None);
    vm.accessor(p, "firstChild", first_child, None);
    vm.accessor(p, "lastChild", last_child, None);
    vm.accessor(p, "nextSibling", next_sibling, None);
    vm.accessor(p, "previousSibling", prev_sibling, None);
    vm.accessor(p, "isConnected", is_connected_get, None);
    vm.accessor(p, "textContent", text_content_get, Some(text_content_set));
    vm.method(p, "hasChildNodes", 0, has_child_nodes);
    vm.method(p, "contains", 1, contains);
    vm.method(p, "compareDocumentPosition", 1, compare_document_position);
    vm.method(p, "isEqualNode", 1, is_equal_node);
}

pub fn install_element_accessors(vm: &mut Vm, p: &Obj) {
    vm.accessor(p, "tagName", tag_name, None);
    vm.accessor(p, "localName", local_name, None);
    vm.accessor(p, "namespaceURI", namespace_uri, None);
    vm.accessor(p, "id", id_get, Some(id_set));
    vm.accessor(p, "className", class_name_get, Some(class_name_set));
    vm.accessor(p, "firstElementChild", first_element_child, None);
    vm.accessor(p, "lastElementChild", last_element_child, None);
    vm.accessor(p, "nextElementSibling", next_element_sibling, None);
    vm.accessor(p, "previousElementSibling", prev_element_sibling, None);
    vm.accessor(p, "childElementCount", child_element_count, None);
    vm.method(p, "getAttribute", 1, get_attribute);
    vm.method(p, "setAttribute", 2, set_attribute);
    vm.method(p, "removeAttribute", 1, remove_attribute);
    vm.method(p, "hasAttribute", 1, has_attribute);
    vm.method(p, "toggleAttribute", 1, toggle_attribute);
    vm.method(p, "getAttributeNames", 0, get_attribute_names);
    vm.method(p, "hasAttributes", 0, has_attributes);
}

pub fn install_character_data_accessors(vm: &mut Vm, p: &Obj) {
    vm.accessor(p, "data", data_get, Some(data_set));
    vm.accessor(p, "nodeValue", data_get, Some(data_set));
}

/// Element-child accessors are also wanted on Document and DocumentFragment.
fn install_parent_node(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(p) = a.arg(0) {
        vm.accessor(&p, "firstElementChild", first_element_child, None);
        vm.accessor(&p, "lastElementChild", last_element_child, None);
        vm.accessor(&p, "childElementCount", child_element_count, None);
    }
    Ok(Value::Undefined)
}

/// `W.getAttributeRaw(el, name)`, `W.hasAttribute(el, name)`, `W.setAttribute(el,
/// name, value)`: attribute access as functions (for prelude internals that must
/// not go through overridable methods).
fn get_attribute_raw(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let name = arg_str(vm, a, 1)?;
    let rc = inner(vm);
    let i = rc.borrow();
    let name = attr_name(&i.doc, n, &name);
    Ok(match i.doc.attr(n, &name) {
        Some(v) => Value::str(v),
        None => Value::Null,
    })
}
fn has_attribute_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let name = arg_str(vm, a, 1)?;
    let rc = inner(vm);
    let i = rc.borrow();
    let name = attr_name(&i.doc, n, &name);
    Ok(Value::Bool(i.doc.has_attr(n, &name)))
}
fn set_attribute_fn(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let name = arg_str(vm, a, 1)?;
    let value = arg_str(vm, a, 2)?;
    set_attribute_value(vm, n, &name, Some(&value))?;
    Ok(Value::Undefined)
}
/// The attribute name of a token list part.
fn part_attr(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(match a.arg(0) {
        Value::Obj(o) => o.host_slot(1).unwrap_or(Value::Undefined),
        _ => Value::Undefined,
    })
}

fn wrap_id(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let id = super::arg_num(vm, a, 0)? as u32;
    let ok = (id as usize) < inner(vm).borrow().doc.len();
    Ok(if ok {
        wrap_node(vm, NodeId(id))
    } else {
        Value::Null
    })
}

fn node_id(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(match node_of(&a.arg(0)) {
        Some(n) => Value::Num(n.0 as f64),
        None => Value::Null,
    })
}

pub fn install(vm: &mut Vm, w: &Obj) {
    vm.method(w, "setProto", 2, set_proto);
    vm.method(w, "createElement", 2, create_element);
    vm.method(w, "createText", 1, create_text);
    vm.method(w, "createComment", 1, create_comment);
    vm.method(w, "createFragment", 0, create_fragment);
    vm.method(w, "createDoctype", 3, create_doctype);
    vm.method(w, "cloneNode", 2, clone_node);
    vm.method(w, "insertBefore", 3, insert_before);
    vm.method(w, "removeChild", 2, remove_child);
    vm.method(w, "remove", 1, remove_self);
    vm.method(w, "replaceChild", 3, replace_child);
    vm.method(w, "normalize", 1, normalize);
    vm.method(w, "attrAt", 2, attr_at);
    vm.method(w, "installParentNode", 1, install_parent_node);
    vm.method(w, "wrapId", 1, wrap_id);
    vm.method(w, "getAttributeRaw", 2, get_attribute_raw);
    vm.method(w, "hasAttribute", 2, has_attribute_fn);
    vm.method(w, "setAttribute", 3, set_attribute_fn);
    vm.method(w, "partAttr", 1, part_attr);
    vm.method(w, "nodeId", 1, node_id);
    super::dom_query::install(vm, w);
    super::dom_forms::install(vm, w);
}
