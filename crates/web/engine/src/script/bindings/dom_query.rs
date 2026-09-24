//! Selector queries, live collections, markup (`innerHTML` and friends) and the
//! document-level natives.

use cw_jsvm::value::{Args, HostHooks, JsResult, Key, Obj, Value};
use cw_jsvm::vm::Vm;

use super::dom::{after_children_changed, insert, is_connected, wrap_node};
use super::{
    arg_node, arg_num, arg_str, dom_exception, inner, node_array, node_of, opt_node, string_val,
};
use crate::css;
use crate::dom::{Document, NodeId, NodeKind};
use crate::script::inner::Inner;

// ---------------------------------------------------------------- selectors

fn parse_selectors(vm: &mut Vm, sel: &str) -> JsResult<css::SelectorList> {
    match css::parse_selector_list(sel) {
        Ok(l) => Ok(l),
        Err(_) => Err(dom_exception(
            vm,
            "SyntaxError",
            &format!("'{sel}' is not a valid selector."),
        )),
    }
}

fn with_ctx<R>(i: &Inner, scope: NodeId, f: impl FnOnce(&css::MatchContext) -> R) -> R {
    let mut ctx = css::MatchContext::new();
    ctx.set_hovered(&i.doc, i.hovered);
    ctx.set_active(&i.doc, i.active);
    ctx.focused = i.focused;
    ctx.focus_visible = i.focus_visible;
    ctx.target_id = i.target_id.clone();
    ctx.scope = if i.doc.is_element(scope) {
        Some(scope)
    } else {
        None
    };
    ctx.form = Some(&i.form);
    f(&ctx)
}

/// A selector that is only `#id`, `.class` or `tag` takes a fast path.
enum Simple<'a> {
    Id(&'a str),
    Class(&'a str),
    Tag(&'a str),
    No,
}

fn simple(sel: &str) -> Simple<'_> {
    let s = sel.trim();
    let ident = |x: &str| {
        !x.is_empty()
            && x.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            && !x.as_bytes()[0].is_ascii_digit()
    };
    if let Some(r) = s.strip_prefix('#') {
        if ident(r) {
            return Simple::Id(r);
        }
    } else if let Some(r) = s.strip_prefix('.') {
        if ident(r) {
            return Simple::Class(r);
        }
    } else if ident(s) {
        return Simple::Tag(s);
    }
    Simple::No
}

fn query(vm: &mut Vm, root: NodeId, sel: &str, first_only: bool) -> JsResult<Vec<NodeId>> {
    match simple(sel) {
        Simple::Id(id) => {
            let rc = inner(vm);
            let i = rc.borrow();
            let mut out: Vec<NodeId> = i
                .doc
                .by_id(id)
                .iter()
                .copied()
                .filter(|n| !i.doc.node(*n).detached && i.doc.ancestors(*n).any(|x| x == root))
                .collect();
            if out.is_empty() && !is_connected(&i.doc, root) {
                out = i
                    .doc
                    .descendants(root)
                    .filter(|n| *n != root && i.doc.attr(*n, "id") == Some(id))
                    .collect();
            }
            if out.len() > 1 {
                // Document order, not insertion order.
                let order: Vec<NodeId> = i
                    .doc
                    .descendants(root)
                    .filter(|n| out.contains(n))
                    .collect();
                out = order;
            }
            if first_only {
                out.truncate(1);
            }
            return Ok(out);
        }
        Simple::Class(c) => {
            let rc = inner(vm);
            let i = rc.borrow();
            let it = i
                .doc
                .descendants(root)
                .filter(|n| *n != root && i.doc.is_element(*n) && i.doc.has_class(*n, c));
            return Ok(if first_only {
                it.take(1).collect()
            } else {
                it.collect()
            });
        }
        Simple::Tag(t) => {
            let rc = inner(vm);
            let i = rc.borrow();
            let lower = t.to_ascii_lowercase();
            let it = i.doc.descendants(root).filter(|n| {
                *n != root && i.doc.tag(*n).map(|x| x == lower || x == t).unwrap_or(false)
            });
            return Ok(if first_only {
                it.take(1).collect()
            } else {
                it.collect()
            });
        }
        Simple::No => {}
    }
    let list = parse_selectors(vm, sel)?;
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(with_ctx(&i, root, |ctx| {
        let it = i.doc.descendants(root).filter(|n| {
            *n != root && i.doc.is_element(*n) && css::matches_list(&i.doc, *n, &list, ctx)
        });
        if first_only {
            it.take(1).collect()
        } else {
            it.collect()
        }
    }))
}

fn query_selector(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let root = arg_node(vm, a, 0)?;
    let sel = arg_str(vm, a, 1)?;
    let r = query(vm, root, &sel, true)?;
    Ok(opt_node(vm, r.first().copied()))
}

fn query_selector_all(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let root = arg_node(vm, a, 0)?;
    let sel = arg_str(vm, a, 1)?;
    let r = query(vm, root, &sel, false)?;
    Ok(node_array(vm, &r))
}

fn matches(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let sel = arg_str(vm, a, 1)?;
    let list = parse_selectors(vm, &sel)?;
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(Value::Bool(with_ctx(&i, n, |ctx| {
        css::matches_list(&i.doc, n, &list, ctx)
    })))
}

fn closest(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let sel = arg_str(vm, a, 1)?;
    let list = parse_selectors(vm, &sel)?;
    let found = {
        let rc = inner(vm);
        let i = rc.borrow();
        with_ctx(&i, n, |ctx| {
            std::iter::once(n)
                .chain(i.doc.ancestors(n))
                .find(|x| i.doc.is_element(*x) && css::matches_list(&i.doc, *x, &list, ctx))
        })
    };
    Ok(opt_node(vm, found))
}

fn get_element_by_id(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let root = arg_node(vm, a, 0)?;
    let id = arg_str(vm, a, 1)?;
    let found = {
        let rc = inner(vm);
        let i = rc.borrow();
        let cands = i.doc.by_id(&id);
        let live: Vec<NodeId> = cands
            .iter()
            .copied()
            .filter(|n| {
                !i.doc.node(*n).detached
                    && (root == Document::ROOT && is_connected(&i.doc, *n)
                        || i.doc.ancestors(*n).any(|x| x == root))
            })
            .collect();
        match live.len() {
            0 => {
                if root != Document::ROOT {
                    i.doc
                        .descendants(root)
                        .find(|n| i.doc.attr(*n, "id") == Some(id.as_str()))
                } else {
                    None
                }
            }
            1 => Some(live[0]),
            _ => i.doc.descendants(root).find(|n| live.contains(n)),
        }
    };
    Ok(opt_node(vm, found))
}

// ---------------------------------------------------------------- live collections

/// The nodes of a collection kind under `root`.
pub fn collect(i: &Inner, root: NodeId, kind: &str, arg: &str) -> Vec<NodeId> {
    let d = &i.doc;
    let desc = |pred: &dyn Fn(NodeId) -> bool| -> Vec<NodeId> {
        d.descendants(root)
            .filter(|n| *n != root && d.is_element(*n) && pred(*n))
            .collect()
    };
    match kind {
        "childNodes" => d
            .children(root)
            .filter(|c| {
                !(matches!(d.kind(*c), NodeKind::DocumentFragment) && d.is(root, "template"))
            })
            .collect(),
        "children" => d.element_children(root).collect(),
        "tag" => {
            if arg == "*" {
                desc(&|_| true)
            } else {
                let lower = arg.to_ascii_lowercase();
                desc(&|n| d.tag(n).map(|t| t == lower || t == arg).unwrap_or(false))
            }
        }
        "class" => {
            let classes: Vec<&str> = arg.split_ascii_whitespace().collect();
            if classes.is_empty() {
                return Vec::new();
            }
            desc(&|n| classes.iter().all(|c| d.has_class(n, c)))
        }
        "name" => desc(&|n| d.attr(n, "name") == Some(arg)),
        "forms" => desc(&|n| d.is(n, "form")),
        "images" => desc(&|n| d.is(n, "img")),
        "scripts" => desc(&|n| d.is(n, "script")),
        "embeds" => desc(&|n| d.is(n, "embed")),
        "links" => desc(&|n| (d.is(n, "a") || d.is(n, "area")) && d.has_attr(n, "href")),
        "anchors" => desc(&|n| d.is(n, "a") && d.has_attr(n, "name")),
        "options" => i.options_of(root),
        "selectedOptions" => i.selected_options(root),
        "elements" => i.form_elements(root),
        "rows" => {
            if d.is(root, "table") {
                let mut out = Vec::new();
                let sections = |tag: &str| -> Vec<NodeId> {
                    d.element_children(root).filter(|c| d.is(*c, tag)).collect()
                };
                for s in sections("thead") {
                    out.extend(d.element_children(s).filter(|c| d.is(*c, "tr")));
                }
                for c in d.element_children(root) {
                    if d.is(c, "tr") {
                        out.push(c);
                    } else if d.is(c, "tbody") {
                        out.extend(d.element_children(c).filter(|r| d.is(*r, "tr")));
                    }
                }
                for s in sections("tfoot") {
                    out.extend(d.element_children(s).filter(|c| d.is(*c, "tr")));
                }
                out
            } else {
                d.element_children(root)
                    .filter(|c| d.is(*c, "tr"))
                    .collect()
            }
        }
        "cells" => d
            .element_children(root)
            .filter(|c| d.is(*c, "td") || d.is(*c, "th"))
            .collect(),
        "tBodies" => d
            .element_children(root)
            .filter(|c| d.is(*c, "tbody"))
            .collect(),
        "labels" => {
            let id = d.attr(root, "id").unwrap_or("");
            d.descendants(Document::ROOT)
                .filter(|n| {
                    d.is(*n, "label")
                        && ((!id.is_empty() && d.attr(*n, "for") == Some(id))
                            || (!d.has_attr(*n, "for") && d.descendants(*n).any(|x| x == root)))
                })
                .collect()
        }
        "areas" => desc(&|n| d.is(n, "area")),
        "datalist" => desc(&|n| d.is(n, "option")),
        _ => Vec::new(),
    }
}

/// Slots of a live collection: root, kind, arg, generation, cached array.
fn collection_nodes(vm: &mut Vm, o: &Obj) -> Option<Obj> {
    let rc = inner(vm);
    let (root, kind, arg, gen, cache) = match &o.borrow().kind {
        cw_jsvm::value::Kind::Host(h) => (
            h.data.first().cloned()?,
            h.data.get(1).cloned()?,
            h.data.get(2).cloned()?,
            h.data.get(3).cloned()?,
            h.data.get(4).cloned(),
        ),
        _ => return None,
    };
    let current = rc.borrow().generation;
    if let (Value::Num(g), Some(Value::Obj(c))) = (&gen, &cache) {
        if *g == current as f64 {
            return Some(c.clone());
        }
    }
    let (Value::Num(root), Value::Str(kind), Value::Str(arg)) = (root, kind, arg) else {
        return None;
    };
    let nodes = collect(&rc.borrow(), NodeId(root as u32), &kind, &arg);
    let arr = match node_array(vm, &nodes) {
        Value::Obj(a) => a,
        _ => return None,
    };
    o.set_host_slot(3, Value::Num(current as f64));
    o.set_host_slot(4, Value::Obj(arr.clone()));
    Some(arr)
}

fn collection_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let Key::Str(s) = k else { return Ok(None) };
    if let Some(idx) = k.array_index() {
        let Some(arr) = collection_nodes(vm, o) else {
            return Ok(None);
        };
        let v = match &arr.borrow().kind {
            cw_jsvm::value::Kind::Array(items) => items.get(idx as usize).cloned(),
            _ => None,
        };
        return Ok(v);
    }
    if s.as_str() == "length" {
        let Some(arr) = collection_nodes(vm, o) else {
            return Ok(Some(Value::Num(0.0)));
        };
        let n = vm.array_len(&arr).unwrap_or(0);
        return Ok(Some(Value::Num(n as f64)));
    }
    // Named access (HTMLCollection only, by id then name), never shadowing members.
    let is_node_list = matches!(o.host_slot(1), Some(Value::Str(k)) if k.as_str() == "childNodes");
    if is_node_list
        || s.is_empty()
        || matches!(
            s.as_str(),
            "item"
                | "namedItem"
                | "forEach"
                | "constructor"
                | "then"
                | "entries"
                | "keys"
                | "values"
                | "add"
                | "remove"
                | "selectedIndex"
                | "toString"
        )
    {
        return Ok(None);
    }
    let Some(arr) = collection_nodes(vm, o) else {
        return Ok(None);
    };
    let items: Vec<Value> = match &arr.borrow().kind {
        cw_jsvm::value::Kind::Array(items) => items.clone(),
        _ => Vec::new(),
    };
    let is_form_controls =
        matches!(o.host_slot(1), Some(Value::Str(k)) if k.as_str() == "elements");
    let matches: Vec<Value> = {
        let rc = inner(vm);
        let i = rc.borrow();
        items
            .into_iter()
            .filter(|it| {
                node_of(it)
                    .map(|n| {
                        i.doc.attr(n, "id") == Some(s.as_str())
                            || i.doc.attr(n, "name") == Some(s.as_str())
                    })
                    .unwrap_or(false)
            })
            .collect()
    };
    if matches.is_empty() {
        return Ok(None);
    }
    if is_form_controls && matches.len() > 1 {
        // `form.elements.radioName` is a RadioNodeList.
        if let Some(hooks) = vm.global.own_value("%hooks") {
            let f = vm.get_str(&hooks, "radioList")?;
            if f.is_callable() {
                let arr = vm.arr(matches);
                return Ok(Some(vm.call(&f, hooks, vec![arr])?));
            }
        }
        return Ok(None);
    }
    Ok(Some(matches[0].clone()))
}

fn collection_keys(vm: &mut Vm, o: &Obj) -> JsResult<Vec<Key>> {
    let Some(arr) = collection_nodes(vm, o) else {
        return Ok(Vec::new());
    };
    let n = vm.array_len(&arr).unwrap_or(0);
    Ok((0..n).map(|i| Key::str(&i.to_string())).collect())
}

fn ro_set(_vm: &mut Vm, _o: &Obj, k: &Key, _v: &Value) -> JsResult<Option<bool>> {
    // `length` falls through so a prototype setter (`options.length = n`) runs.
    Ok(if k.array_index().is_some() {
        Some(true)
    } else {
        None
    })
}
fn no_delete(_vm: &mut Vm, _o: &Obj, _k: &Key) -> JsResult<Option<bool>> {
    Ok(None)
}

pub static COLLECTION_HOOKS: HostHooks = HostHooks {
    class: "HTMLCollection",
    get: collection_get,
    set: ro_set,
    delete: no_delete,
    keys: collection_keys,
    plain: false,
};

/// `W.collection(root, kind, arg, protoName)`: a live collection object.
fn collection(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let root = arg_node(vm, a, 0)?;
    let kind = arg_str(vm, a, 1)?;
    let arg = arg_str(vm, a, 2)?;
    let proto_name = arg_str(vm, a, 3)?;
    let proto = inner(vm).borrow().protos.get(&proto_name).cloned();
    let o = vm.host_obj(
        proto,
        &COLLECTION_HOOKS,
        vec![
            Value::Num(root.0 as f64),
            string_val(kind),
            string_val(arg),
            Value::Num(-1.0),
            Value::Undefined,
        ],
    );
    Ok(Value::Obj(o))
}

/// `W.collect(root, kind, arg)`: the collection's nodes now, as an array.
fn collect_now(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let root = arg_node(vm, a, 0)?;
    let kind = arg_str(vm, a, 1)?;
    let arg = arg_str(vm, a, 2)?;
    let nodes = collect(&inner(vm).borrow(), root, &kind, &arg);
    Ok(node_array(vm, &nodes))
}

/// A static list (`querySelectorAll`): slot 0 is the backing array.
fn static_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let Some(Value::Obj(arr)) = o.host_slot(0) else {
        return Ok(None);
    };
    if let Some(idx) = k.array_index() {
        let v = match &arr.borrow().kind {
            cw_jsvm::value::Kind::Array(items) => items.get(idx as usize).cloned(),
            _ => None,
        };
        return Ok(v);
    }
    if k.as_str() == Some("length") {
        return Ok(Some(Value::Num(vm.array_len(&arr).unwrap_or(0) as f64)));
    }
    Ok(None)
}
fn static_keys(vm: &mut Vm, o: &Obj) -> JsResult<Vec<Key>> {
    let Some(Value::Obj(arr)) = o.host_slot(0) else {
        return Ok(Vec::new());
    };
    let n = vm.array_len(&arr).unwrap_or(0);
    Ok((0..n).map(|i| Key::str(&i.to_string())).collect())
}
pub static STATIC_LIST_HOOKS: HostHooks = HostHooks {
    class: "NodeList",
    get: static_get,
    set: ro_set,
    delete: no_delete,
    keys: static_keys,
    plain: false,
};

/// `W.staticList(array, protoName)`.
fn static_list(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let arr = a.arg(0);
    let proto_name = arg_str(vm, a, 1)?;
    let proto = inner(vm).borrow().protos.get(&proto_name).cloned();
    Ok(Value::Obj(vm.host_obj(
        proto,
        &STATIC_LIST_HOOKS,
        vec![arr],
    )))
}

/// `W.listItems(list)`: the backing array of a live or static list.
fn list_items(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else {
        return Ok(vm.arr(vec![]));
    };
    let is_static = o
        .host_hooks()
        .map(|h| std::ptr::eq(h, &STATIC_LIST_HOOKS))
        .unwrap_or(false);
    if is_static {
        return Ok(o.host_slot(0).unwrap_or(Value::Undefined));
    }
    match collection_nodes(vm, &o) {
        Some(arr) => Ok(Value::Obj(arr)),
        None => Ok(vm.arr(vec![])),
    }
}

// ---------------------------------------------------------------- markup

fn markup_root(d: &Document, n: NodeId) -> NodeId {
    d.template_contents(n).unwrap_or(n)
}

fn inner_html(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let s = crate::html::serialize(&inner(vm).borrow().doc, n);
    Ok(string_val(s))
}

fn outer_html(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let s = crate::html::serialize_node(&inner(vm).borrow().doc, n);
    Ok(string_val(s))
}

/// Parses markup in the context of `ctx`, returning detached nodes.
fn parse_into(vm: &mut Vm, ctx: NodeId, html: &str) -> Vec<NodeId> {
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let ctx = if i.doc.is_element(ctx) {
        ctx
    } else {
        // Fragments and documents parse as if in a body.
        match i.doc.body() {
            Some(b) => b,
            None => {
                let marks = i.doc.mutations.len();
                let b = i.doc.create_element("body", vec![]);
                i.doc.mutations.truncate(marks);
                b
            }
        }
    };
    crate::html::parse_fragment(&mut i.doc, ctx, html)
}

fn set_inner_html(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let v = a.arg(1);
    let html = if matches!(v, Value::Null) {
        String::new()
    } else {
        vm.to_string(&v)?.to_string()
    };
    let target = markup_root(&inner(vm).borrow().doc, n);
    let fast_text = !html.contains(['<', '&', '\r'])
        && !matches!(
            inner(vm).borrow().doc.tag(n),
            Some("table" | "tbody" | "thead" | "tfoot" | "tr" | "select" | "template")
        );
    if fast_text {
        super::dom::replace_children_with_text(vm, target, &html)?;
        return Ok(Value::Undefined);
    }
    let nodes = parse_into(vm, n, &html);
    let removed: Vec<NodeId> = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        // Scripts inserted through innerHTML never execute.
        for c in &nodes {
            let scripts: Vec<NodeId> = i
                .doc
                .descendants(*c)
                .filter(|x| i.doc.is(*x, "script"))
                .collect();
            i.executed_scripts.extend(scripts);
        }
        let kids: Vec<NodeId> = i.doc.children(target).collect();
        for k in &kids {
            i.doc.detach(*k);
        }
        for c in &nodes {
            i.doc.append(target, *c);
        }
        kids
    };
    after_children_changed(vm, target, &nodes, &removed, None, None)?;
    Ok(Value::Undefined)
}

/// `W.parseFragment(context, html)`: a DocumentFragment with the parsed nodes.
fn parse_fragment(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let ctx = arg_node(vm, a, 0)?;
    let html = arg_str(vm, a, 1)?;
    let nodes = parse_into(vm, ctx, &html);
    let frag = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let marks = i.doc.mutations.len();
        let f = i.doc.create(NodeKind::DocumentFragment);
        for n in &nodes {
            i.doc.append(f, *n);
        }
        i.doc.mutations.truncate(marks);
        f
    };
    Ok(wrap_node(vm, frag))
}

/// `W.parseDocument(html)`: a detached tree (`DOMParser`, `createHTMLDocument`)
/// rooted at a fragment holding the `<html>` element.
fn parse_document(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let html = arg_str(vm, a, 0)?;
    let parsed = crate::html::parse(&html);
    let frag = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let marks = i.doc.mutations.len();
        let f = i.doc.create(NodeKind::DocumentFragment);
        fn copy(src: &Document, from: NodeId, dst: &mut Document, to: NodeId) {
            for c in src.children(from) {
                let n = dst.create(src.kind(c).clone());
                dst.append(to, n);
                copy(src, c, dst, n);
            }
        }
        copy(&parsed, Document::ROOT, &mut i.doc, f);
        i.doc.mutations.truncate(marks);
        f
    };
    Ok(wrap_node(vm, frag))
}

fn inner_text(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let s = inner(vm).borrow_mut().inner_text(n);
    Ok(string_val(s))
}

// ---------------------------------------------------------------- document

fn document_node(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(wrap_node(vm, Document::ROOT))
}

fn doc_part(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let which = arg_str(vm, a, 0)?;
    let n = {
        let rc = inner(vm);
        let i = rc.borrow();
        match which.as_str() {
            "html" => i.doc.document_element(),
            "head" => i.doc.head(),
            "body" => i.doc.body(),
            "doctype" => i
                .doc
                .children(Document::ROOT)
                .find(|c| matches!(i.doc.kind(*c), NodeKind::DocType { .. })),
            "currentScript" => i.current_script,
            "focused" => i.focused,
            "hovered" => i.hovered,
            _ => None,
        }
    };
    Ok(opt_node(vm, n))
}

fn doc_info(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let which = arg_str(vm, a, 0)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    Ok(match which.as_str() {
        "title" => string_val(i.title()),
        "url" => Value::str(&i.url),
        "readyState" => Value::str(&i.ready_state),
        "referrer" => Value::str(&i.referrer),
        "compatMode" => Value::str(if i.doc.quirks == crate::dom::QuirksMode::Quirks {
            "BackCompat"
        } else {
            "CSS1Compat"
        }),
        "hidden" => Value::Bool(i.hidden),
        "cookie" => string_val(i.host_cookie_get()),
        "parsing" => Value::Bool(i.parsing),
        _ => Value::Undefined,
    })
}

fn doctype_info(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let rc = inner(vm);
    let i = rc.borrow();
    Ok(match i.doc.kind(n) {
        NodeKind::DocType {
            name,
            public_id,
            system_id,
        } => vm.arr(vec![
            Value::str(name),
            Value::str(public_id),
            Value::str(system_id),
        ]),
        _ => Value::Null,
    })
}

fn set_cookie(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let c = arg_str(vm, a, 0)?;
    inner(vm).borrow_mut().host_cookie_set(&c);
    Ok(Value::Undefined)
}

/// `document.write` while parsing: into the tokenizer's input stream.
fn doc_write(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = arg_str(vm, a, 0)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    if i.parsing {
        i.write_buffer.push_str(&s);
        return Ok(Value::Bool(true));
    }
    Ok(Value::Bool(false))
}

/// `document.write` after load: the document is replaced by the written markup.
fn doc_replace(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let html = arg_str(vm, a, 0)?;
    let parsed = crate::html::parse(&html);
    let (removed, added) = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let old: Vec<NodeId> = i.doc.children(Document::ROOT).collect();
        for o in &old {
            i.doc.detach(*o);
        }
        fn copy(src: &Document, from: NodeId, dst: &mut Document, to: NodeId) {
            for c in src.children(from) {
                let n = dst.create(src.kind(c).clone());
                dst.append(to, n);
                copy(src, c, dst, n);
            }
        }
        copy(&parsed, Document::ROOT, &mut i.doc, Document::ROOT);
        let added: Vec<NodeId> = i.doc.children(Document::ROOT).collect();
        i.sheets_dirty = true;
        (old, added)
    };
    after_children_changed(vm, Document::ROOT, &added, &removed, None, None)?;
    Ok(Value::Undefined)
}

// ---------------------------------------------------------------- scripts, custom elements

/// Runs a script element inserted by script: inline text runs now, `src` runs as a
/// task.
fn run_script(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let (src, text, ty, done, connected) = {
        let rc = inner(vm);
        let i = rc.borrow();
        (
            i.doc.attr(n, "src").map(str::to_owned),
            i.doc.text_content(n),
            i.doc
                .attr(n, "type")
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase(),
            i.executed_scripts.contains(&n),
            is_connected(&i.doc, n),
        )
    };
    if done || !connected || inner(vm).borrow().parsing && false {
        return Ok(Value::Undefined);
    }
    let is_js = ty.is_empty()
        || matches!(
            ty.as_str(),
            "text/javascript"
                | "application/javascript"
                | "module"
                | "text/ecmascript"
                | "application/ecmascript"
        );
    if !is_js {
        return Ok(Value::Undefined);
    }
    if src.is_some() || ty == "module" {
        inner(vm).borrow_mut().pending_scripts.push(n);
        return Ok(Value::Undefined);
    }
    if text.trim().is_empty() {
        return Ok(Value::Undefined);
    }
    let name = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        i.executed_scripts.insert(n);
        format!("{}#inline-{}", i.url, n.0)
    };
    let prev = inner(vm).borrow_mut().current_script.replace(n);
    let r = vm.eval_source_with(&text, &name, false, true);
    inner(vm).borrow_mut().current_script = prev;
    r?;
    Ok(Value::Undefined)
}

fn define_custom(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let name = arg_str(vm, a, 0)?;
    let nodes: Vec<NodeId> = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        i.custom_defined.insert(name.clone());
        i.doc
            .descendants(Document::ROOT)
            .filter(|n| {
                i.doc.tag(*n) == Some(name.as_str()) || i.doc.attr(*n, "is") == Some(name.as_str())
            })
            .collect()
    };
    Ok(node_array(vm, &nodes))
}

fn set_flag(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let which = arg_str(vm, a, 0)?;
    let on = a.arg(1).truthy();
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    match which.as_str() {
        "observing" => i.observing = on,
        "raf" => i.has_raf = on,
        "observers" => i.has_layout_observers = on,
        "hidden" => i.hidden = on,
        _ => {}
    }
    Ok(Value::Undefined)
}

/// Descendants (inclusive) that are elements, for reaction walks.
fn subtree_elements(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let nodes: Vec<NodeId> = {
        let rc = inner(vm);
        let i = rc.borrow();
        i.doc
            .descendants(n)
            .filter(|x| i.doc.is_element(*x))
            .collect()
    };
    Ok(node_array(vm, &nodes))
}

fn template_content(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let c = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        match i.doc.template_contents(n) {
            Some(c) => Some(c),
            None if i.doc.is(n, "template") => {
                let marks = i.doc.mutations.len();
                let f = i.doc.create(NodeKind::DocumentFragment);
                let first = i.doc.first_child(n);
                i.doc.insert_before(n, f, first);
                i.doc.mutations.truncate(marks);
                Some(f)
            }
            None => None,
        }
    };
    Ok(opt_node(vm, c))
}

fn insert_node(vm: &mut Vm, parent: NodeId, child: NodeId, before: Option<NodeId>) -> JsResult<()> {
    insert(vm, parent, child, before)
}

/// `W.insertAdjacent(el, position, node)`.
fn insert_adjacent(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let el = arg_node(vm, a, 0)?;
    let pos = arg_str(vm, a, 1)?.to_ascii_lowercase();
    let node = arg_node(vm, a, 2)?;
    let (parent, before) = {
        let rc = inner(vm);
        let i = rc.borrow();
        match pos.as_str() {
            "beforebegin" => (i.doc.parent(el), Some(el)),
            "afterbegin" => (Some(el), i.doc.first_child(el)),
            "beforeend" => (Some(el), None),
            "afterend" => (i.doc.parent(el), i.doc.next_sibling(el)),
            _ => {
                drop(i);
                return Err(dom_exception(vm, "SyntaxError", "The value provided is not one of 'beforebegin', 'afterbegin', 'beforeend', or 'afterend'."));
            }
        }
    };
    let Some(parent) = parent else {
        return Ok(Value::Null);
    };
    insert_node(vm, parent, node, before)?;
    Ok(a.arg(2))
}

fn element_index(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let kind = arg_str(vm, a, 1)?;
    let root = node_of(&a.arg(2));
    let idx = {
        let rc = inner(vm);
        let i = rc.borrow();
        match root {
            Some(r) => collect(&i, r, &kind, "")
                .iter()
                .position(|x| *x == n)
                .map(|p| p as f64)
                .unwrap_or(-1.0),
            None => -1.0,
        }
    };
    Ok(Value::Num(idx))
}

fn generation(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let g = inner(vm).borrow().generation;
    Ok(Value::Num(g as f64))
}

fn set_image_size(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let url = arg_str(vm, a, 0)?;
    let w = arg_num(vm, a, 1)? as u32;
    let h = arg_num(vm, a, 2)? as u32;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    i.images.0.insert(url, (w, h));
    i.touch();
    Ok(Value::Undefined)
}

/// Named access on `window`: an element by `id` (or a named `<form>`, `<img>`,
/// `<iframe>`, `<embed>`, `<object>`) when no global of that name exists.
fn global_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let Key::Str(s) = k else { return Ok(None) };
    if k.array_index().is_some() || o.borrow().props.find_str(s).is_some() || s.is_empty() {
        return Ok(None);
    }
    let found = {
        let rc = inner(vm);
        let i = rc.borrow();
        if i.doc.len() <= 1 {
            None
        } else {
            let by_id = i
                .doc
                .by_id(s)
                .iter()
                .copied()
                .find(|n| is_connected(&i.doc, *n));
            by_id.or_else(|| {
                i.doc.descendants(Document::ROOT).find(|n| {
                    matches!(
                        i.doc.tag(*n),
                        Some("form" | "img" | "iframe" | "embed" | "object")
                    ) && i.doc.attr(*n, "name") == Some(s.as_str())
                })
            })
        }
    };
    Ok(found.map(|n| wrap_node(vm, n)))
}
fn global_set(_vm: &mut Vm, _o: &Obj, _k: &Key, _v: &Value) -> JsResult<Option<bool>> {
    Ok(None)
}
fn global_delete(_vm: &mut Vm, _o: &Obj, _k: &Key) -> JsResult<Option<bool>> {
    Ok(None)
}
fn global_keys(_vm: &mut Vm, _o: &Obj) -> JsResult<Vec<Key>> {
    Ok(Vec::new())
}
pub static GLOBAL_HOOKS: HostHooks = HostHooks {
    class: "Window",
    get: global_get,
    set: global_set,
    delete: global_delete,
    keys: global_keys,
    plain: false,
};

pub fn install(vm: &mut Vm, w: &Obj) {
    vm.global.borrow_mut().kind = cw_jsvm::value::Kind::Host(Box::new(cw_jsvm::value::HostData {
        hooks: &GLOBAL_HOOKS,
        data: Vec::new(),
    }));
    vm.method(w, "querySelector", 2, query_selector);
    vm.method(w, "querySelectorAll", 2, query_selector_all);
    vm.method(w, "matches", 2, matches);
    vm.method(w, "closest", 2, closest);
    vm.method(w, "getElementById", 2, get_element_by_id);
    vm.method(w, "collection", 4, collection);
    vm.method(w, "collect", 3, collect_now);
    vm.method(w, "staticList", 2, static_list);
    vm.method(w, "listItems", 1, list_items);
    vm.method(w, "innerHTML", 1, inner_html);
    vm.method(w, "outerHTML", 1, outer_html);
    vm.method(w, "setInnerHTML", 2, set_inner_html);
    vm.method(w, "parseFragment", 2, parse_fragment);
    vm.method(w, "parseDocument", 1, parse_document);
    vm.method(w, "innerText", 1, inner_text);
    vm.method(w, "document", 0, document_node);
    vm.method(w, "docPart", 1, doc_part);
    vm.method(w, "docInfo", 1, doc_info);
    vm.method(w, "doctypeInfo", 1, doctype_info);
    vm.method(w, "setCookie", 1, set_cookie);
    vm.method(w, "docWrite", 1, doc_write);
    vm.method(w, "docReplace", 1, doc_replace);
    vm.method(w, "runScript", 1, run_script);
    vm.method(w, "defineCustom", 1, define_custom);
    vm.method(w, "setFlag", 2, set_flag);
    vm.method(w, "subtreeElements", 1, subtree_elements);
    vm.method(w, "templateContent", 1, template_content);
    vm.method(w, "insertAdjacent", 3, insert_adjacent);
    vm.method(w, "elementIndex", 3, element_index);
    vm.method(w, "generation", 0, generation);
    vm.method(w, "setImageSize", 3, set_image_size);
}
