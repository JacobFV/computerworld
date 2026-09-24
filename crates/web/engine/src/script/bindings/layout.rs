//! Geometry reads that flush layout: `getBoundingClientRect`, `offset*`, `client*`,
//! `scroll*`, `elementFromPoint`, window scrolling and the viewport.

use cw_jsvm::value::{Args, JsResult, Obj, Value};
use cw_jsvm::vm::Vm;

use super::{arg_node, arg_num, arg_str, inner, node_array, opt_node, this_node};
use crate::dom::{Document, NodeId};
use crate::geom::{Au, Rect};
use crate::layout::FragmentKind;
use crate::script::inner::{fragment_of, Inner};
use crate::style::Position;

fn px(a: Au) -> f64 {
    a.to_f64_px()
}

/// The union of the element's fragment rects in viewport coordinates, or None
/// when it has no box.
fn client_rect(i: &mut Inner, n: NodeId) -> Option<Rect> {
    let rects = i.rects_of(n);
    let mut it = rects.into_iter();
    let first = it.next()?;
    let u = it.fold(first, |acc, r| acc.union(r));
    let (sx, sy) = i.window_scroll();
    Some(u.translate(-sx, -sy))
}

fn rect_array(vm: &mut Vm, r: Option<Rect>) -> Value {
    match r {
        Some(r) => vm.arr(vec![
            Value::Num(px(r.origin.x)),
            Value::Num(px(r.origin.y)),
            Value::Num(px(r.size.width)),
            Value::Num(px(r.size.height)),
        ]),
        None => vm.arr(vec![
            Value::Num(0.0),
            Value::Num(0.0),
            Value::Num(0.0),
            Value::Num(0.0),
        ]),
    }
}

fn bounding_rect(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let r = client_rect(&mut inner(vm).borrow_mut(), n);
    Ok(rect_array(vm, r))
}

fn client_rects(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let rects = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let (sx, sy) = i.window_scroll();
        i.rects_of(n)
            .into_iter()
            .map(|r| r.translate(-sx, -sy))
            .collect::<Vec<_>>()
    };
    let items: Vec<Value> = rects.into_iter().map(|r| rect_array(vm, Some(r))).collect();
    Ok(vm.arr(items))
}

/// Box metrics of an element: `[offsetLeft, offsetTop, offsetWidth, offsetHeight,
/// clientLeft, clientTop, clientWidth, clientHeight, scrollWidth, scrollHeight]`.
fn metrics(i: &mut Inner, n: NodeId) -> [f64; 10] {
    i.ensure_layout();
    let Some(tree) = i.tree.as_ref() else {
        return [0.0; 10];
    };
    if Some(n) == i.doc.document_element() {
        // The root element's client box is the viewport; its scroll size the
        // document's.
        let vw = px(tree.viewport_width);
        let vh = px(tree.viewport_height);
        let rects = tree.rects_of(n);
        let (w, h) = rects
            .first()
            .map(|r| (px(r.size.width), px(r.size.height)))
            .unwrap_or((vw, vh));
        return [
            0.0,
            0.0,
            w.round(),
            h.round(),
            0.0,
            0.0,
            vw.round(),
            vh.round(),
            px(tree.content_width.max(tree.viewport_width)).round(),
            px(tree.content_height.max(tree.viewport_height)).round(),
        ];
    }
    let Some((f, abs)) = fragment_of(tree, n) else {
        // Inline elements: use the union of inline fragments for offsets.
        let rects = tree.rects_of(n);
        if rects.is_empty() {
            return [0.0; 10];
        }
        let u = rects.iter().skip(1).fold(rects[0], |a, r| a.union(*r));
        let (ol, ot) = offset_origin(i, n);
        return [
            px(u.origin.x - ol),
            px(u.origin.y - ot),
            u.size.width.to_px_round() as f64,
            u.size.height.to_px_round() as f64,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
    };
    let (padding, border, scroll) = match &f.kind {
        FragmentKind::Box {
            padding,
            border,
            scroll,
            ..
        } => (*padding, *border, *scroll),
        _ => (crate::geom::Edges::ZERO, crate::geom::Edges::ZERO, None),
    };
    let w = abs.size.width;
    let h = abs.size.height;
    let bar_w = scroll
        .map(|s| {
            if s.shows_y_bar {
                Au::from_px_i32(15)
            } else {
                Au::ZERO
            }
        })
        .unwrap_or(Au::ZERO);
    let bar_h = scroll
        .map(|s| {
            if s.shows_x_bar {
                Au::from_px_i32(15)
            } else {
                Au::ZERO
            }
        })
        .unwrap_or(Au::ZERO);
    let client_w = (w - border.horizontal() - bar_w).max(Au::ZERO);
    let client_h = (h - border.vertical() - bar_h).max(Au::ZERO);
    let (scroll_w, scroll_h) = match scroll {
        Some(s) => (
            s.content_width.max(client_w),
            s.content_height.max(client_h),
        ),
        None => {
            let ov = f.overflow;
            let inner_w = (w - border.horizontal()).max(Au::ZERO);
            let inner_h = (h - border.vertical()).max(Au::ZERO);
            (
                (ov.right() - border.left).max(inner_w),
                (ov.bottom() - border.top).max(inner_h),
            )
        }
    };
    let _ = padding;
    let is_inline = i
        .styles
        .get(n)
        .map(|s| {
            s.display.is_inline_level() && !matches!(s.display, crate::style::Display::InlineBlock)
        })
        .unwrap_or(false);
    let (ol, ot) = offset_origin(i, n);
    let round = |a: Au| a.to_px_round() as f64;
    if is_inline {
        return [
            round(abs.origin.x - ol),
            round(abs.origin.y - ot),
            round(w),
            round(h),
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
    }
    [
        round(abs.origin.x - ol),
        round(abs.origin.y - ot),
        round(w),
        round(h),
        round(border.left),
        round(border.top),
        round(client_w),
        round(client_h),
        round(scroll_w),
        round(scroll_h),
    ]
}

/// The origin `offsetTop/Left` are relative to: the offsetParent's padding edge.
fn offset_origin(i: &mut Inner, n: NodeId) -> (Au, Au) {
    match offset_parent(i, n) {
        Some(p)
            if i.doc.is(p, "body")
                && !i.styles.get(p).map(|s| s.is_positioned()).unwrap_or(false) =>
        {
            // Offsets against a static body are relative to the initial containing
            // block, as browsers report them.
            let _ = p;
            (Au::ZERO, Au::ZERO)
        }
        Some(p) => {
            let Some(tree) = i.tree.as_ref() else {
                return (Au::ZERO, Au::ZERO);
            };
            match fragment_of(tree, p) {
                Some((f, abs)) => {
                    let border = match &f.kind {
                        FragmentKind::Box { border, .. } => *border,
                        _ => crate::geom::Edges::ZERO,
                    };
                    (abs.origin.x + border.left, abs.origin.y + border.top)
                }
                None => (Au::ZERO, Au::ZERO),
            }
        }
        None => (Au::ZERO, Au::ZERO),
    }
}

fn offset_parent(i: &Inner, n: NodeId) -> Option<NodeId> {
    let style = i.styles.get(n)?;
    if style.display.is_none()
        || matches!(style.position, Position::Fixed)
        || i.doc.is(n, "body")
        || i.doc.is(n, "html")
    {
        return None;
    }
    let mut cur = i.doc.parent(n);
    while let Some(p) = cur {
        if !i.doc.is_element(p) {
            return None;
        }
        if i.doc.is(p, "body") {
            return Some(p);
        }
        if let Some(s) = i.styles.get(p) {
            if s.is_positioned() || i.doc.is(p, "td") || i.doc.is(p, "th") || i.doc.is(p, "table") {
                return Some(p);
            }
        }
        cur = i.doc.parent(p);
    }
    None
}

fn box_metrics(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let m = metrics(&mut inner(vm).borrow_mut(), n);
    let v: Vec<Value> = m.iter().map(|x| Value::Num(*x)).collect();
    Ok(vm.arr(v))
}

macro_rules! metric_getter {
    ($name:ident, $idx:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
            let n = this_node(vm, a)?;
            let m = metrics(&mut inner(vm).borrow_mut(), n);
            Ok(Value::Num(m[$idx]))
        }
    };
}
metric_getter!(offset_left, 0);
metric_getter!(offset_top, 1);
metric_getter!(offset_width, 2);
metric_getter!(offset_height, 3);
metric_getter!(client_left, 4);
metric_getter!(client_top, 5);
metric_getter!(client_width, 6);
metric_getter!(client_height, 7);
metric_getter!(scroll_width, 8);
metric_getter!(scroll_height, 9);

fn offset_parent_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let p = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        i.ensure_styles();
        offset_parent(&i, n)
    };
    Ok(opt_node(vm, p))
}

fn scroll_get(i: &Inner, n: NodeId) -> (Au, Au) {
    i.scroll.get(&n).copied().unwrap_or((Au::ZERO, Au::ZERO))
}

fn scroll_top_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let n = scrolling_node(&i, n);
    i.ensure_layout();
    Ok(Value::Num(px(scroll_get(&i, n).1)))
}
fn scroll_left_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let n = scrolling_node(&i, n);
    i.ensure_layout();
    Ok(Value::Num(px(scroll_get(&i, n).0)))
}

/// `document.documentElement.scrollTop` scrolls the viewport.
fn scrolling_node(i: &Inner, n: NodeId) -> NodeId {
    if Some(n) == i.doc.document_element() {
        Document::ROOT
    } else {
        n
    }
}

fn set_scroll(vm: &mut Vm, n: NodeId, x: Option<f64>, y: Option<f64>) {
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let n = scrolling_node(&i, n);
    let (cx, cy) = scroll_get(&i, n);
    let nx = x.map(Au::from_f64_px).unwrap_or(cx);
    let ny = y.map(Au::from_f64_px).unwrap_or(cy);
    i.set_scroll(n, nx, ny);
    i.ensure_layout();
}

fn scroll_top_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let v = arg_num(vm, a, 0)?;
    set_scroll(vm, n, None, Some(v));
    Ok(Value::Undefined)
}
fn scroll_left_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = this_node(vm, a)?;
    let v = arg_num(vm, a, 0)?;
    set_scroll(vm, n, Some(v), None);
    Ok(Value::Undefined)
}

/// `W.scrollTo(node|null, x, y)`; null is the window.
fn scroll_to(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = super::node_of(&a.arg(0)).unwrap_or(Document::ROOT);
    let x = if a.arg(1).is_nullish() {
        None
    } else {
        Some(arg_num(vm, a, 1)?)
    };
    let y = if a.arg(2).is_nullish() {
        None
    } else {
        Some(arg_num(vm, a, 2)?)
    };
    set_scroll(vm, n, x, y);
    Ok(Value::Undefined)
}

fn scroll_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = super::node_of(&a.arg(0)).unwrap_or(Document::ROOT);
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let n = scrolling_node(&i, n);
    i.ensure_layout();
    let (x, y) = scroll_get(&i, n);
    drop(i);
    Ok(vm.arr(vec![Value::Num(px(x)), Value::Num(px(y))]))
}

/// `W.windowScroll()`: the window's scroll offset as it stands, without flushing
/// layout. The page coordinates of a UI event come from the scroll position at the
/// time the browser hit-tested it, as in Chromium; reading `scrollX` instead would
/// force a layout for every event while an update is pending.
fn window_scroll(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let (x, y) = inner(vm).borrow().window_scroll();
    Ok(vm.arr(vec![Value::Num(px(x)), Value::Num(px(y))]))
}

/// `W.scrollIntoView(node, alignToTop)`: scrolls the window (and scroll
/// container ancestors) so the element is visible.
fn scroll_into_view(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let top = !matches!(a.arg(1), Value::Bool(false));
    let center = matches!(&a.arg(2), Value::Str(s) if s.as_str() == "center");
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let rects = i.rects_of(n);
    let Some(r) = rects.first().copied() else {
        return Ok(Value::Undefined);
    };
    let vh = Au::from_px_i32(i.viewport.height as i32);
    let (sx, sy) = i.window_scroll();
    let ny = if center {
        r.origin.y - (vh - r.size.height).scale(1, 2)
    } else if top {
        r.origin.y
    } else {
        r.origin.y + r.size.height - vh
    };
    let ny = if !top && !center && r.origin.y >= sy && r.bottom() <= sy + vh {
        sy
    } else {
        ny
    };
    i.set_scroll(Document::ROOT, sx, ny.max(Au::ZERO));
    i.ensure_layout();
    Ok(Value::Undefined)
}

fn element_from_point(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let x = arg_num(vm, a, 0)? as i32;
    let y = arg_num(vm, a, 1)? as i32;
    let n = inner(vm).borrow_mut().element_from_point(x, y);
    Ok(opt_node(vm, n))
}

fn elements_from_point(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let x = arg_num(vm, a, 0)? as i32;
    let y = arg_num(vm, a, 1)? as i32;
    let nodes = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        match i.element_from_point(x, y) {
            Some(n) => {
                let mut out = vec![n];
                out.extend(i.doc.ancestors(n).filter(|a| i.doc.is_element(*a)));
                out
            }
            None => Vec::new(),
        }
    };
    Ok(node_array(vm, &nodes))
}

/// `[width, height, dpr, documentWidth, documentHeight]`.
fn viewport(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let rc = inner(vm);
    let i = rc.borrow();
    let v = i.viewport;
    let (dw, dh) = match &i.tree {
        Some(t) => (px(t.content_width), px(t.content_height)),
        None => (v.width as f64, v.height as f64),
    };
    drop(i);
    Ok(vm.arr(vec![
        Value::Num(v.width as f64),
        Value::Num(v.height as f64),
        Value::Num(v.scale as f64),
        Value::Num(dw),
        Value::Num(dh),
    ]))
}

fn document_size(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    i.ensure_layout();
    let (w, h) = i
        .tree
        .as_ref()
        .map(|t| {
            (
                px(t.content_width.max(t.viewport_width)),
                px(t.content_height.max(t.viewport_height)),
            )
        })
        .unwrap_or((0.0, 0.0));
    drop(i);
    Ok(vm.arr(vec![Value::Num(w), Value::Num(h)]))
}

fn set_viewport(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let w = arg_num(vm, a, 0)? as u32;
    let h = arg_num(vm, a, 1)? as u32;
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    i.viewport.width = w;
    i.viewport.height = h;
    i.sheet_changed();
    Ok(Value::Undefined)
}

/// Whether the element is rendered (has a box) — `checkVisibility`.
fn is_rendered(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let r = !inner(vm).borrow_mut().rects_of(n).is_empty();
    Ok(Value::Bool(r))
}

/// The nearest scroll container of a node (for wheel scrolling), or null.
fn scroll_container(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let found = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        i.ensure_layout();
        let mut cur = Some(n);
        let mut out = None;
        while let Some(c) = cur {
            if let Some(tree) = i.tree.as_ref() {
                if let Some((f, _)) = fragment_of(tree, c) {
                    if let FragmentKind::Box {
                        scroll: Some(s), ..
                    } = &f.kind
                    {
                        let inner_h = f.rect.size.height;
                        if s.content_height > inner_h || s.content_width > f.rect.size.width {
                            out = Some(c);
                            break;
                        }
                    }
                }
            }
            cur = i.doc.parent(c);
        }
        out
    };
    Ok(opt_node(vm, found))
}

fn layout_generation(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    i.ensure_layout();
    Ok(Value::Num(i.generation as f64))
}

fn ensure_layout(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let _ = arg_str(vm, a, 0);
    inner(vm).borrow_mut().ensure_layout();
    Ok(Value::Undefined)
}

pub fn install_element_accessors(vm: &mut Vm, p: &Obj) {
    vm.accessor(p, "clientLeft", client_left, None);
    vm.accessor(p, "clientTop", client_top, None);
    vm.accessor(p, "clientWidth", client_width, None);
    vm.accessor(p, "clientHeight", client_height, None);
    vm.accessor(p, "scrollWidth", scroll_width, None);
    vm.accessor(p, "scrollHeight", scroll_height, None);
    vm.accessor(p, "scrollTop", scroll_top_get, Some(scroll_top_set));
    vm.accessor(p, "scrollLeft", scroll_left_get, Some(scroll_left_set));
}

pub fn install_html_element_accessors(vm: &mut Vm, p: &Obj) {
    vm.accessor(p, "offsetLeft", offset_left, None);
    vm.accessor(p, "offsetTop", offset_top, None);
    vm.accessor(p, "offsetWidth", offset_width, None);
    vm.accessor(p, "offsetHeight", offset_height, None);
    vm.accessor(p, "offsetParent", offset_parent_get, None);
}

pub fn install(vm: &mut Vm, w: &Obj) {
    vm.method(w, "boundingRect", 1, bounding_rect);
    vm.method(w, "clientRects", 1, client_rects);
    vm.method(w, "boxMetrics", 1, box_metrics);
    vm.method(w, "scrollTo", 3, scroll_to);
    vm.method(w, "scrollOf", 1, scroll_of);
    vm.method(w, "windowScroll", 0, window_scroll);
    vm.method(w, "scrollIntoView", 3, scroll_into_view);
    vm.method(w, "elementFromPoint", 2, element_from_point);
    vm.method(w, "elementsFromPoint", 2, elements_from_point);
    vm.method(w, "viewport", 0, viewport);
    vm.method(w, "documentSize", 0, document_size);
    vm.method(w, "setViewport", 2, set_viewport);
    vm.method(w, "isRendered", 1, is_rendered);
    vm.method(w, "scrollContainer", 1, scroll_container);
    vm.method(w, "layoutGeneration", 0, layout_generation);
    vm.method(w, "ensureLayout", 0, ensure_layout);
}
